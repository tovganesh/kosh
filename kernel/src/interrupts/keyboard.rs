//! PS/2 keyboard: IRQ1 -> scancode -> decoded key -> ring buffer.
//!
//! `drivers/keyboard` already had a correct 100-entry scancode table, but its
//! port I/O was stubbed — `read_data()` returned 0 unconditionally, so the only
//! way to inject a keystroke was a "simulate key press" control command. This
//! is the real thing: an interrupt handler reading port 0x60.
//!
//! The handler does the minimum possible work — read the port, decode, push a
//! key — and never allocates or blocks. Everything else happens in
//! [`read_key`], called from normal kernel context.
//!
//! The buffer carries a [`Key`] rather than a byte, because a line editor needs
//! to tell an arrow key from the character it would otherwise be conflated
//! with. Squeezing cursor keys into spare ASCII control codes works right up
//! until something wants to type one of those control codes.

use core::sync::atomic::{AtomicUsize, Ordering};

use lazy_static::lazy_static;
use pc_keyboard::{layouts, DecodedKey, HandleControl, KeyCode, Keyboard, ScancodeSet1};
use spin::Mutex;
use x86_64::instructions::port::Port;
use x86_64::structures::idt::InterruptStackFrame;

use crate::interrupts::{pic, InterruptIndex};
use crate::serial_println;

const PS2_DATA_PORT: u16 = 0x60;

/// Capacity of the key ring. A power of two so the modulo is a mask. 128 keys
/// is far more than a human can type between polls.
const BUFFER_SIZE: usize = 128;

/// A decoded keypress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Delete,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
}

lazy_static! {
    static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(
        Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::Ignore)
    );
}

/// Lock-free-ish SPSC ring: the IRQ handler is the only producer, kernel
/// context is the only consumer.
struct KeyBuffer {
    data: [Key; BUFFER_SIZE],
    read: AtomicUsize,
    write: AtomicUsize,
}

impl KeyBuffer {
    const fn new() -> Self {
        Self {
            data: [Key::Char('\0'); BUFFER_SIZE],
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
        }
    }

    fn push(&mut self, key: Key) {
        let write = self.write.load(Ordering::Relaxed);
        let next = (write + 1) % BUFFER_SIZE;

        // Drop on overflow rather than overwrite unread input — losing the
        // newest keystroke is less confusing than losing the oldest.
        if next == self.read.load(Ordering::Acquire) {
            return;
        }

        self.data[write] = key;
        self.write.store(next, Ordering::Release);
    }

    fn pop(&self) -> Option<Key> {
        let read = self.read.load(Ordering::Relaxed);
        if read == self.write.load(Ordering::Acquire) {
            return None;
        }

        let key = self.data[read];
        self.read.store((read + 1) % BUFFER_SIZE, Ordering::Release);
        Some(key)
    }

    fn is_empty(&self) -> bool {
        self.read.load(Ordering::Relaxed) == self.write.load(Ordering::Acquire)
    }
}

static BUFFER: Mutex<KeyBuffer> = Mutex::new(KeyBuffer::new());

pub fn init() {
    // Drain anything the BIOS left in the controller's output buffer,
    // otherwise the first real keystroke never generates an IRQ.
    unsafe {
        let mut port: Port<u8> = Port::new(PS2_DATA_PORT);
        let _ = port.read();
    }
    serial_println!("  PS/2 keyboard: IRQ1 handler installed (US layout, set 1)");
}

/// Take one key, if any is waiting. Non-blocking.
pub fn read_key() -> Option<Key> {
    crate::interrupts::without_interrupts(|| BUFFER.lock().pop())
}

/// Block until a key is available.
///
/// Uses `hlt` rather than a spin so an idle wait does not burn the CPU — and,
/// now that the kernel is preemptive, so other threads keep running while the
/// console waits for a human.
pub fn read_key_blocking() -> Key {
    loop {
        if let Some(key) = read_key() {
            return key;
        }
        x86_64::instructions::hlt();
    }
}

/// Take one character, ignoring keys that have no character. Non-blocking.
pub fn read_char() -> Option<char> {
    match read_key() {
        Some(Key::Char(c)) => Some(c),
        Some(Key::Enter) => Some('\n'),
        Some(Key::Backspace) => Some('\x08'),
        _ => None,
    }
}

/// Whether any input is pending.
pub fn has_input() -> bool {
    !crate::interrupts::without_interrupts(|| BUFFER.lock().is_empty())
}

fn translate(key: DecodedKey) -> Option<Key> {
    match key {
        DecodedKey::Unicode('\n') | DecodedKey::Unicode('\r') => Some(Key::Enter),
        DecodedKey::Unicode('\u{8}') | DecodedKey::Unicode('\u{7f}') => Some(Key::Backspace),
        DecodedKey::Unicode('\t') => Some(Key::Tab),
        DecodedKey::Unicode('\u{1b}') => Some(Key::Escape),
        DecodedKey::Unicode(c) if c.is_ascii() => Some(Key::Char(c)),
        DecodedKey::Unicode(_) => None,

        DecodedKey::RawKey(KeyCode::ArrowUp) => Some(Key::Up),
        DecodedKey::RawKey(KeyCode::ArrowDown) => Some(Key::Down),
        DecodedKey::RawKey(KeyCode::ArrowLeft) => Some(Key::Left),
        DecodedKey::RawKey(KeyCode::ArrowRight) => Some(Key::Right),
        DecodedKey::RawKey(KeyCode::Home) => Some(Key::Home),
        DecodedKey::RawKey(KeyCode::End) => Some(Key::End),
        DecodedKey::RawKey(KeyCode::Delete) => Some(Key::Delete),
        DecodedKey::RawKey(KeyCode::Backspace) => Some(Key::Backspace),
        DecodedKey::RawKey(_) => None,
    }
}

pub extern "x86-interrupt" fn keyboard_interrupt_handler(_frame: InterruptStackFrame) {
    let scancode: u8 = unsafe {
        let mut port: Port<u8> = Port::new(PS2_DATA_PORT);
        port.read()
    };

    // `try_lock` rather than `lock`: this runs in interrupt context, and
    // blocking on a lock held by interrupted kernel code would deadlock the
    // machine. Dropping a keystroke is the correct trade.
    if let Some(mut keyboard) = KEYBOARD.try_lock() {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(decoded) = keyboard.process_keyevent(key_event) {
                if let Some(key) = translate(decoded) {
                    if let Some(mut buffer) = BUFFER.try_lock() {
                        buffer.push(key);
                    }
                }
            }
        }
    }

    unsafe {
        pic::notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}
