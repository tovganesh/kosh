//! The PS/2 keyboard driver, in ring 3.
//!
//! The keyboard was the last device driver inside the kernel. Moving it here
//! completes the microkernel goal: the kernel owns no device drivers at all.
//!
//! It talks to port 0x60 directly with `inb` — permitted by the TSS I/O
//! permission bitmap — and waits for keypresses on IRQ 1 using `SYS_WAIT_IRQ`.
//! It translates Scancode Set 1 into ASCII characters and ANSI escape sequences,
//! and answers read requests from the shell (`ksh`) over IPC as the "input" service.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

const SYS_EXIT: u64 = 1;
const SYS_WRITE: u64 = 23;
const SYS_SEND_MESSAGE: u64 = 30;
const SYS_RECEIVE_MESSAGE: u64 = 31;
const SYS_REQUEST_DEVICE: u64 = 44;
const SYS_REGISTER_SERVICE: u64 = 46;
const SYS_WAIT_IRQ: u64 = 48;

const KBD_IRQ: u64 = 1;
const KBD_DATA_PORT: u16 = 0x60;

// --- the input protocol ----------------------------------------------------

const REQ_MAGIC: u32 = 0x4B49_4E50; // "KINP"
const REP_MAGIC: u32 = 0x4B49_5250; // "KIRP"

const OP_READ: u32 = 0;
const OP_SHUTDOWN: u32 = 1;

const REQ_BYTES: usize = 16;
const REP_HEADER: usize = 16;

const STATUS_OK: i32 = 0;
const STATUS_BAD_REQUEST: i32 = -1;

// --- syscalls --------------------------------------------------------------

#[inline(always)]
unsafe fn syscall3(number: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") number => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

fn print(s: &str) {
    unsafe { syscall3(SYS_WRITE, 1, s.as_ptr() as u64, s.len() as u64) };
}

fn print_u64(mut value: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    if value == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while value > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    unsafe { syscall3(SYS_WRITE, 1, buf[i..].as_ptr() as u64, (buf.len() - i) as u64) };
}

fn exit(code: u64) -> ! {
    unsafe { syscall3(SYS_EXIT, code, 0, 0) };
    loop {
        core::hint::spin_loop();
    }
}

// --- port I/O --------------------------------------------------------------

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack, preserves_flags));
    value
}

// --- IRQ waiting -----------------------------------------------------------

static mut IRQ_WAITS: u64 = 0;
static mut IRQ_TIMEOUTS: u64 = 0;
static mut IRQ_SEEN: u64 = 0;

fn irq_count() -> u64 {
    wait_irq(u64::MAX, 0)
}

fn wait_irq(seen: u64, timeout_ms: u64) -> u64 {
    let got = unsafe { syscall3(SYS_WAIT_IRQ, KBD_IRQ, seen, timeout_ms) };
    if got < 0 {
        seen
    } else {
        got as u64
    }
}

// --- scancode decoding -----------------------------------------------------

static mut SHIFT_DOWN: bool = false;
static mut CAPS_LOCK: bool = false;
static mut CTRL_DOWN: bool = false;
static mut ALT_DOWN: bool = false;
static mut EXTENDED: bool = false;

/// Internal ring buffer of decoded key bytes waiting to be sent to clients.
const KEY_RING_CAP: usize = 256;
static mut KEY_RING: [u8; KEY_RING_CAP] = [0; KEY_RING_CAP];
static mut KEY_HEAD: usize = 0;
static mut KEY_TAIL: usize = 0;

fn ring_push(byte: u8) {
    unsafe {
        let next = (KEY_HEAD + 1) % KEY_RING_CAP;
        if next != KEY_TAIL {
            KEY_RING[KEY_HEAD] = byte;
            KEY_HEAD = next;
        }
    }
}

fn ring_push_slice(bytes: &[u8]) {
    for &b in bytes {
        ring_push(b);
    }
}

fn ring_pop() -> Option<u8> {
    unsafe {
        if KEY_TAIL == KEY_HEAD {
            None
        } else {
            let b = KEY_RING[KEY_TAIL];
            KEY_TAIL = (KEY_TAIL + 1) % KEY_RING_CAP;
            Some(b)
        }
    }
}

fn ring_available() -> usize {
    unsafe {
        if KEY_HEAD >= KEY_TAIL {
            KEY_HEAD - KEY_TAIL
        } else {
            KEY_RING_CAP - KEY_TAIL + KEY_HEAD
        }
    }
}

/// Process one scancode byte from port 0x60.
fn handle_scancode(sc: u8) {
    unsafe {
        if sc == 0xE0 {
            EXTENDED = true;
            return;
        }

        let is_release = (sc & 0x80) != 0;
        let code = sc & 0x7F;

        if EXTENDED {
            EXTENDED = false;
            if !is_release {
                match code {
                    0x48 => ring_push_slice(b"\x1b[A"),  // Up
                    0x50 => ring_push_slice(b"\x1b[B"),  // Down
                    0x4D => ring_push_slice(b"\x1b[C"),  // Right
                    0x4B => ring_push_slice(b"\x1b[D"),  // Left
                    0x47 => ring_push_slice(b"\x1b[H"),  // Home
                    0x4F => ring_push_slice(b"\x1b[F"),  // End
                    0x53 => ring_push_slice(b"\x1b[3~"), // Delete
                    _ => {}
                }
            }
            return;
        }

        // Modifiers
        match code {
            0x2A | 0x36 => {
                SHIFT_DOWN = !is_release;
                return;
            }
            0x1D => {
                CTRL_DOWN = !is_release;
                return;
            }
            0x38 => {
                ALT_DOWN = !is_release;
                return;
            }
            0x3A if !is_release => {
                CAPS_LOCK = !CAPS_LOCK;
                return;
            }
            _ => {}
        }

        if is_release {
            return;
        }

        let shift = SHIFT_DOWN;
        let upper = shift ^ CAPS_LOCK;

        if CTRL_DOWN {
            match code {
                0x2E => ring_push(0x03), // Ctrl-C
                0x20 => ring_push(0x04), // Ctrl-D
                _ => {}
            }
            return;
        }

        let ch = match code {
            0x01 => Some(b'\x1b'),
            0x02 => Some(if shift { b'!' } else { b'1' }),
            0x03 => Some(if shift { b'@' } else { b'2' }),
            0x04 => Some(if shift { b'#' } else { b'3' }),
            0x05 => Some(if shift { b'$' } else { b'4' }),
            0x06 => Some(if shift { b'%' } else { b'5' }),
            0x07 => Some(if shift { b'^' } else { b'6' }),
            0x08 => Some(if shift { b'&' } else { b'7' }),
            0x09 => Some(if shift { b'*' } else { b'8' }),
            0x0A => Some(if shift { b'(' } else { b'9' }),
            0x0B => Some(if shift { b')' } else { b'0' }),
            0x0C => Some(if shift { b'_' } else { b'-' }),
            0x0D => Some(if shift { b'+' } else { b'=' }),
            0x0E => Some(b'\x08'), // Backspace
            0x0F => Some(b'\t'),   // Tab
            0x10 => Some(if upper { b'Q' } else { b'q' }),
            0x11 => Some(if upper { b'W' } else { b'w' }),
            0x12 => Some(if upper { b'E' } else { b'e' }),
            0x13 => Some(if upper { b'R' } else { b'r' }),
            0x14 => Some(if upper { b'T' } else { b't' }),
            0x15 => Some(if upper { b'Y' } else { b'y' }),
            0x16 => Some(if upper { b'U' } else { b'u' }),
            0x17 => Some(if upper { b'I' } else { b'i' }),
            0x18 => Some(if upper { b'O' } else { b'o' }),
            0x19 => Some(if upper { b'P' } else { b'p' }),
            0x1A => Some(if shift { b'{' } else { b'[' }),
            0x1B => Some(if shift { b'}' } else { b']' }),
            0x1C => Some(b'\n'), // Enter
            0x1E => Some(if upper { b'A' } else { b'a' }),
            0x1F => Some(if upper { b'S' } else { b's' }),
            0x20 => Some(if upper { b'D' } else { b'd' }),
            0x21 => Some(if upper { b'F' } else { b'f' }),
            0x22 => Some(if upper { b'G' } else { b'g' }),
            0x23 => Some(if upper { b'H' } else { b'h' }),
            0x24 => Some(if upper { b'J' } else { b'j' }),
            0x25 => Some(if upper { b'K' } else { b'k' }),
            0x26 => Some(if upper { b'L' } else { b'l' }),
            0x27 => Some(if shift { b':' } else { b';' }),
            0x28 => Some(if shift { b'"' } else { b'\'' }),
            0x29 => Some(if shift { b'~' } else { b'`' }),
            0x2B => Some(if shift { b'|' } else { b'\\' }),
            0x2C => Some(if upper { b'Z' } else { b'z' }),
            0x2D => Some(if upper { b'X' } else { b'x' }),
            0x2E => Some(if upper { b'C' } else { b'c' }),
            0x2F => Some(if upper { b'V' } else { b'v' }),
            0x30 => Some(if upper { b'B' } else { b'b' }),
            0x31 => Some(if upper { b'N' } else { b'n' }),
            0x32 => Some(if upper { b'M' } else { b'm' }),
            0x33 => Some(if shift { b'<' } else { b',' }),
            0x34 => Some(if shift { b'>' } else { b'.' }),
            0x35 => Some(if shift { b'?' } else { b'/' }),
            0x39 => Some(b' '), // Space
            _ => None,
        };

        if let Some(b) = ch {
            ring_push(b);
        }
    }
}

// --- serving ---------------------------------------------------------------

static mut REQUEST: [u8; 64] = [0; 64];
static mut REPLY: [u8; REP_HEADER + 128] = [0; REP_HEADER + 128];

fn read_u32(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

fn write_u32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn reply(to: u32, status: i32, payload_len: usize) {
    unsafe {
        let reply_buf = &mut *core::ptr::addr_of_mut!(REPLY);
        write_u32(reply_buf, 0, REP_MAGIC);
        write_u32(reply_buf, 4, status as u32);
        write_u32(reply_buf, 8, payload_len as u32);
        write_u32(reply_buf, 12, 0);

        syscall3(
            SYS_SEND_MESSAGE,
            to as u64,
            reply_buf.as_ptr() as u64,
            (REP_HEADER + payload_len) as u64,
        );
    }
}

/// Wait for a key event from IRQ 1, reading port 0x60 and buffering bytes.
fn await_key() {
    let seen = unsafe { IRQ_SEEN };
    let now = wait_irq(seen, 200);

    unsafe {
        IRQ_WAITS += 1;
        if now == seen {
            IRQ_TIMEOUTS += 1;
        } else {
            IRQ_SEEN = now;
            let sc = inb(KBD_DATA_PORT);
            handle_scancode(sc);
        }
    }
}

/// Handle one request. Returns false when told to shut down.
fn serve_one() -> bool {
    let received = unsafe {
        let req = &mut *core::ptr::addr_of_mut!(REQUEST);
        syscall3(
            SYS_RECEIVE_MESSAGE,
            req.as_mut_ptr() as u64,
            req.len() as u64,
            1, // blocking
        )
    };

    if received < 0 {
        print("  kbd-driver: receive failed, stopping\n");
        return false;
    }

    let sender = (received as u64 >> 32) as u32;
    let len = (received as u64 & 0xFFFF_FFFF) as usize;

    let req = unsafe { &*core::ptr::addr_of!(REQUEST) };

    if len < REQ_BYTES || read_u32(req, 0) != REQ_MAGIC {
        reply(sender, STATUS_BAD_REQUEST, 0);
        return true;
    }

    let op = read_u32(req, 4);

    match op {
        OP_SHUTDOWN => {
            reply(sender, STATUS_OK, 0);
            false
        }
        OP_READ => {
            let max_bytes = read_u32(req, 8) as usize;
            let max_bytes = max_bytes.min(128).max(1);

            // Block on IRQ 1 until we have at least one byte to give.
            while ring_available() == 0 {
                await_key();
            }

            // Drain available bytes into the reply payload
            let mut drained = 0usize;
            unsafe {
                let reply_buf = &mut *core::ptr::addr_of_mut!(REPLY);
                while drained < max_bytes {
                    if let Some(b) = ring_pop() {
                        reply_buf[REP_HEADER + drained] = b;
                        drained += 1;
                    } else {
                        break;
                    }
                }
            }

            reply(sender, STATUS_OK, drained);
            true
        }
        _ => {
            reply(sender, STATUS_BAD_REQUEST, 0);
            true
        }
    }
}

// --- entry -----------------------------------------------------------------

core::arch::global_asm!(
    r#"
.section .text._start, "ax"
.global _start
.type _start, @function
_start:
    xorq    %rbp, %rbp
    andq    $-16, %rsp
    call    kbd_driver_main
1:
    jmp     1b
"#,
    options(att_syntax)
);

#[no_mangle]
pub extern "C" fn kbd_driver_main(_argc: u64, _argv: *const *const u8) -> ! {
    print("  kbd-driver: starting in ring 3\n");

    let device = "kbd0";
    let granted = unsafe {
        syscall3(
            SYS_REQUEST_DEVICE,
            device.as_ptr() as u64,
            device.len() as u64,
            0,
        )
    };

    if granted < 0 {
        print("  kbd-driver: request_device('kbd0') was refused\n");
        exit(1);
    }
    print("  kbd-driver: got the kbd0 ports\n");

    // Initialize IRQ seen count and drain any stale scancode from controller
    unsafe {
        IRQ_SEEN = irq_count();
        let _ = inb(KBD_DATA_PORT);
    }

    let service = "input";
    if unsafe {
        syscall3(
            SYS_REGISTER_SERVICE,
            service.as_ptr() as u64,
            service.len() as u64,
            0,
        )
    } < 0
    {
        print("  kbd-driver: could not register as 'input'\n");
        exit(2);
    }
    print("  kbd-driver: registered as the 'input' service\n");

    while serve_one() {}

    unsafe {
        print("  kbd-driver: ");
        print_u64(IRQ_WAITS - IRQ_TIMEOUTS);
        print(" of ");
        print_u64(IRQ_WAITS);
        print(" waits were woken by IRQ 1\n");

        if IRQ_WAITS > 0 && IRQ_TIMEOUTS < IRQ_WAITS {
            print("  kbd-driver: the keyboard woke this process from ring 3\n");
        }
    }

    // Drain any remaining scancodes before exiting so the controller's output buffer
    // is empty when returning to the kernel console
    unsafe {
        while inb(0x64) & 1 != 0 {
            let _ = inb(KBD_DATA_PORT);
        }
    }

    print("  kbd-driver: shutting down, releasing kbd0\n");
    exit(0)
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print("  kbd-driver panic\n");
    exit(3)
}
