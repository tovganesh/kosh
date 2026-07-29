//! PS/2 keyboard: IRQ1 -> scancode -> decoded key -> ring buffer.
//!
//! `drivers/keyboard` already had a correct 100-entry scancode table, but its
//! port I/O was stubbed — `read_data()` returned 0 unconditionally, so the only
//! way to inject a keystroke was a "simulate key press" control command. This
//! is the real thing: an interrupt handler reading port 0x60.
//!
//! The handler does the minimum possible work — read the port, decode, push a
//! byte — and never allocates or blocks. Everything else happens in
//! `read_char()`, called from normal kernel context.

use core::sync::atomic::{AtomicUsize, Ordering};

use lazy_static::lazy_static;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use spin::Mutex;
use x86_64::instructions::port::Port;
use x86_64::structures::idt::InterruptStackFrame;

use crate::interrupts::{pic, InterruptIndex};
use crate::serial_println;

const PS2_DATA_PORT: u16 = 0x60;

/// Capacity of the decoded-character ring. A power of two so the modulo is a
/// mask. 128 characters is far more than a human can type between polls.
const BUFFER_SIZE: usize = 128;

lazy_static! {
    static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(
        Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::Ignore)
    );
}

/// Lock-free-ish SPSC ring: the IRQ handler is the only producer, kernel
/// context is the only consumer.
struct KeyBuffer {
    data: [u8; BUFFER_SIZE],
    read: AtomicUsize,
    write: AtomicUsize,
}

impl KeyBuffer {
    const fn new() -> Self {
        Self {
            data: [0; BUFFER_SIZE],
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
        }
    }

    fn push(&mut self, byte: u8) {
        let write = self.write.load(Ordering::Relaxed);
        let next = (write + 1) % BUFFER_SIZE;

        // Drop on overflow rather than overwrite unread input — losing the
        // newest keystroke is less confusing than losing the oldest.
        if next == self.read.load(Ordering::Acquire) {
            return;
        }

        self.data[write] = byte;
        self.write.store(next, Ordering::Release);
    }

    fn pop(&self) -> Option<u8> {
        let read = self.read.load(Ordering::Relaxed);
        if read == self.write.load(Ordering::Acquire) {
            return None;
        }

        let byte = self.data[read];
        self.read.store((read + 1) % BUFFER_SIZE, Ordering::Release);
        Some(byte)
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

/// Take one decoded character, if any is waiting. Non-blocking.
pub fn read_char() -> Option<char> {
    crate::interrupts::without_interrupts(|| BUFFER.lock().pop().map(|b| b as char))
}

/// Block until a character is available.
///
/// Uses `hlt` rather than a spin so an idle wait does not burn the CPU. This
/// is the primitive the shell's line editor will sit on in Phase 7.
pub fn read_char_blocking() -> char {
    loop {
        if let Some(c) = read_char() {
            return c;
        }
        x86_64::instructions::hlt();
    }
}

/// Whether any input is pending.
pub fn has_input() -> bool {
    crate::interrupts::without_interrupts(|| BUFFER.lock().is_empty()) == false
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
            if let Some(key) = keyboard.process_keyevent(key_event) {
                let byte = match key {
                    DecodedKey::Unicode(c) => {
                        if c.is_ascii() {
                            Some(c as u8)
                        } else {
                            None
                        }
                    }
                    // Arrow keys, function keys and friends have no ASCII
                    // encoding. Phase 7 will map these to editor commands.
                    DecodedKey::RawKey(_) => None,
                };

                if let Some(byte) = byte {
                    if let Some(mut buffer) = BUFFER.try_lock() {
                        buffer.push(byte);
                    }
                }
            }
        }
    }

    unsafe {
        pic::notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}
