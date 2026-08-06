//! Console input, and the one path helper the loader still needs.
//!
//! ## What used to be here
//!
//! A descriptor table, `sys_open`, `sys_read`, `sys_close`, `sys_lseek`,
//! `sys_stat` and `sys_getdents` — 405 lines connecting `kernel/src/fs/fat32.rs`
//! to ring 3. The filesystem is `userspace/fs-service` now, so `ksh` opens files
//! by sending it a message, and the kernel is not on that path at all. Those
//! system calls and the code behind them are gone rather than kept refusing
//! politely: a syscall number that exists and always fails is a worse answer
//! than one that does not exist.
//!
//! What is left is the part that was never about files. `read(0, ...)` reads the
//! keyboard, and the keyboard is the kernel's — it is one of the two devices
//! still inside (the timer is the other). `path_from_user` stays because `spawn`
//! and `exec` take a boot-module name, which is a string from userspace and has
//! nothing to do with a filesystem.

use alloc::string::String;

use crate::interrupts::keyboard::Key;
use crate::syscall::uaccess::{copy_str_from_user, copy_to_user};
use crate::syscall::{SyscallError, SyscallResult};

/// Longest path the kernel will accept from userspace.
const MAX_PATH: usize = 255;

/// Read a path out of user memory and validate it.
pub fn path_from_user(ptr: u64, len: u64) -> Result<String, SyscallError> {
    let len = len as usize;
    if len == 0 || len > MAX_PATH {
        return Err(SyscallError::InvalidArgument);
    }

    let mut buf = [0u8; MAX_PATH];
    let n = copy_str_from_user(ptr, &mut buf, len).map_err(|_| SyscallError::InvalidArgument)?;

    core::str::from_utf8(&buf[..n])
        .map(String::from)
        .map_err(|_| SyscallError::InvalidArgument)
}

/// `read(0, buf, count)` — the keyboard.
///
/// The only `read` the kernel implements. Everything else a program might read
/// is a message to a service.
pub fn sys_read(args: [u64; 6]) -> SyscallResult {
    let fd = args[0] as usize;
    let buf_ptr = args[1];
    let count = args[2] as usize;

    if count == 0 {
        return Ok(0);
    }
    if fd != 0 {
        return Err(SyscallError::BadFileDescriptor);
    }

    read_stdin(buf_ptr, count)
}

/// Wait for a key from inside a system call.
///
/// This cannot use `keyboard::read_key_blocking`, and the reason is worth
/// spelling out: `SFMASK` clears the interrupt flag on syscall entry, so a
/// syscall runs with interrupts *masked*. Halting in that state means the
/// keyboard IRQ that would wake us can never fire — the machine simply stops,
/// with the shell's prompt on screen looking like it is waiting for input.
///
/// So this enables interrupts while it waits, and masks them again before
/// returning so the rest of the syscall runs in the state `SFMASK` established.
/// `enable_and_hlt` does the sti/hlt atomically, avoiding the window where an
/// interrupt lands between the two and the halt then waits for the next one.
///
/// This is safe only because exactly one thread is ever in ring 3 — the same
/// reason the single static syscall stack works. A second user thread entering a
/// syscall while this one is parked would reuse that stack from the top and
/// corrupt this frame.
fn wait_for_key() -> Key {
    use crate::interrupts::keyboard;

    loop {
        if let Some(key) = keyboard::read_key() {
            return key;
        }
        x86_64::instructions::interrupts::enable_and_hlt();
        x86_64::instructions::interrupts::disable();
    }
}

/// Blocking read from the keyboard.
///
/// Blocks for the first key, then drains whatever else is already buffered. That
/// is what a line editor wants: it should not spin, and it should not need one
/// syscall per character when someone pastes a line.
fn read_stdin(buf_ptr: u64, count: usize) -> SyscallResult {
    use crate::interrupts::keyboard;

    let mut out = [0u8; 64];
    let limit = core::cmp::min(count, out.len());
    let mut n = 0usize;

    // First key: block.
    let first = wait_for_key();
    n += encode_key(first, &mut out[n..limit]);

    // Anything else already waiting.
    while n < limit {
        match keyboard::read_key() {
            Some(key) => {
                let written = encode_key(key, &mut out[n..limit]);
                if written == 0 {
                    break;
                }
                n += written;
            }
            None => break,
        }
    }

    if n > 0 {
        copy_to_user(buf_ptr, &out[..n]).map_err(|_| SyscallError::InvalidArgument)?;
    }

    Ok(n as u64)
}

/// Turn a key into bytes a userspace line editor can act on.
///
/// Cursor keys become the usual ANSI escape sequences rather than invented
/// control codes, so a userspace editor written against any terminal convention
/// already understands them.
fn encode_key(key: Key, out: &mut [u8]) -> usize {
    let bytes: &[u8] = match key {
        Key::Char(c) if c.is_ascii() => {
            if out.is_empty() {
                return 0;
            }
            out[0] = c as u8;
            return 1;
        }
        Key::Char(_) => return 0,
        Key::Enter => b"\n",
        Key::Backspace => b"\x08",
        Key::Tab => b"\t",
        Key::Escape => b"\x1b",
        Key::Delete => b"\x1b[3~",
        Key::Up => b"\x1b[A",
        Key::Down => b"\x1b[B",
        Key::Right => b"\x1b[C",
        Key::Left => b"\x1b[D",
        Key::Home => b"\x1b[H",
        Key::End => b"\x1b[F",
    };

    if out.len() < bytes.len() {
        return 0;
    }
    out[..bytes.len()].copy_from_slice(bytes);
    bytes.len()
}
