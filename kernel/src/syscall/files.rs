//! File descriptors, and the syscalls that use them.
//!
//! The dispatcher already had `sys_open`, `sys_read`, `sys_close` and
//! `sys_lseek`. What they did: `sys_open` returned the literal `3` for every
//! path ("for demonstration, return a dummy file descriptor"), `sys_read`
//! returned `min(count, 1024)` without touching the buffer ("simulate reading
//! some data"), and `sys_close` and `sys_lseek` returned `NotSupported`. There
//! was no descriptor table, because there was nothing to put in it.
//!
//! Now there is a disk and a filesystem, so this connects them to ring 3.
//!
//! ## One table, not one per process
//!
//! A descriptor table belongs to a process. There is exactly one user process
//! at a time here — the syscall path still uses a single static kernel stack,
//! which is only safe for that reason — so the table is global and the limit is
//! honest rather than hidden. It moves into the process control block when
//! `fork` arrives.

use alloc::string::String;
use spin::Mutex;

use crate::fs;
use crate::fs::fat32::{DirEntry, FsError};
use crate::interrupts::keyboard::Key;
use crate::syscall::uaccess::{copy_str_from_user, copy_to_user};
use crate::syscall::{SyscallError, SyscallResult};

/// Reserved descriptors, by convention: 0 stdin, 1 stdout, 2 stderr.
const FIRST_FILE_FD: usize = 3;
const MAX_FDS: usize = 16;

/// Longest path the kernel will accept from userspace.
const MAX_PATH: usize = 255;

/// An open file. Read-only, because the filesystem is.
struct OpenFile {
    path: String,
    entry: DirEntry,
    offset: u32,
}

static OPEN_FILES: Mutex<[Option<OpenFile>; MAX_FDS]> = Mutex::new([const { None }; MAX_FDS]);

fn fs_error_to_syscall(e: FsError) -> SyscallError {
    match e {
        FsError::NotFound => SyscallError::NotFound,
        FsError::NotADirectory | FsError::IsADirectory => SyscallError::InvalidArgument,
        FsError::Block(_) => SyscallError::InternalError,
        FsError::NameTooLong => SyscallError::InvalidArgument,
        _ => SyscallError::InternalError,
    }
}

/// Read a path out of user memory and validate it.
fn path_from_user(ptr: u64, len: u64) -> Result<String, SyscallError> {
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

/// `open(path, path_len)` -> fd
///
/// The length is passed explicitly rather than relying on a NUL terminator: a
/// caller-supplied length is something the kernel can bound-check before it
/// starts walking user memory.
pub fn sys_open(args: [u64; 6]) -> SyscallResult {
    let path = path_from_user(args[0], args[1])?;

    let entry = fs::lookup(&path).map_err(fs_error_to_syscall)?;
    if entry.is_dir {
        // Directories are listed with getdents, not read as byte streams.
        return Err(SyscallError::InvalidArgument);
    }

    let mut table = OPEN_FILES.lock();
    for fd in FIRST_FILE_FD..MAX_FDS {
        if table[fd].is_none() {
            table[fd] = Some(OpenFile {
                path,
                entry,
                offset: 0,
            });
            return Ok(fd as u64);
        }
    }

    Err(SyscallError::ResourceExhausted)
}

/// `close(fd)`
pub fn sys_close(args: [u64; 6]) -> SyscallResult {
    let fd = args[0] as usize;
    if fd < FIRST_FILE_FD || fd >= MAX_FDS {
        return Err(SyscallError::BadFileDescriptor);
    }

    let mut table = OPEN_FILES.lock();
    match table[fd].take() {
        Some(_) => Ok(0),
        None => Err(SyscallError::BadFileDescriptor),
    }
}

/// `read(fd, buf, count)` -> bytes read
///
/// fd 0 is the keyboard; anything else is a file.
pub fn sys_read(args: [u64; 6]) -> SyscallResult {
    let fd = args[0] as usize;
    let buf_ptr = args[1];
    let count = args[2] as usize;

    if count == 0 {
        return Ok(0);
    }

    if fd == 0 {
        return read_stdin(buf_ptr, count);
    }
    if fd < FIRST_FILE_FD || fd >= MAX_FDS {
        return Err(SyscallError::BadFileDescriptor);
    }

    // Copy under the lock into a bounded kernel buffer, then release the lock
    // before touching user memory — `copy_to_user` walks page tables, and doing
    // that while holding the descriptor table serialises every other file
    // operation behind it.
    const CHUNK: usize = 512;
    let mut buffer = [0u8; CHUNK];

    let (read, new_offset) = {
        let mut table = OPEN_FILES.lock();
        let file = table[fd].as_mut().ok_or(SyscallError::BadFileDescriptor)?;

        let want = core::cmp::min(count, CHUNK);
        let n = fs::read_at(&file.entry, file.offset, &mut buffer[..want])
            .map_err(fs_error_to_syscall)?;

        file.offset += n as u32;
        (n, file.offset)
    };

    let _ = new_offset;

    if read > 0 {
        copy_to_user(buf_ptr, &buffer[..read]).map_err(|_| SyscallError::InvalidArgument)?;
    }

    Ok(read as u64)
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

const SEEK_SET: u64 = 0;
const SEEK_CUR: u64 = 1;
const SEEK_END: u64 = 2;

/// `lseek(fd, offset, whence)` -> new offset
pub fn sys_lseek(args: [u64; 6]) -> SyscallResult {
    let fd = args[0] as usize;
    let offset = args[1] as i64;
    let whence = args[2];

    if fd < FIRST_FILE_FD || fd >= MAX_FDS {
        return Err(SyscallError::BadFileDescriptor);
    }

    let mut table = OPEN_FILES.lock();
    let file = table[fd].as_mut().ok_or(SyscallError::BadFileDescriptor)?;

    let base = match whence {
        SEEK_SET => 0i64,
        SEEK_CUR => file.offset as i64,
        SEEK_END => file.entry.size as i64,
        _ => return Err(SyscallError::InvalidArgument),
    };

    let target = base.checked_add(offset).ok_or(SyscallError::InvalidArgument)?;
    if target < 0 {
        return Err(SyscallError::InvalidArgument);
    }

    // Seeking past the end is legal; reads there simply return nothing.
    file.offset = target as u32;
    Ok(file.offset as u64)
}

/// One directory entry as handed to userspace. Fixed size, so the caller can
/// walk the buffer without parsing anything.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UserDirEntry {
    pub name: [u8; 64],
    pub size: u32,
    pub is_dir: u8,
    pub _reserved: [u8; 3],
}

pub const DIRENT_SIZE: usize = core::mem::size_of::<UserDirEntry>();

/// `getdents(path, path_len, buf, buf_len)` -> entries written
///
/// Path-based rather than fd-based, because opening a directory as a byte stream
/// is a category error the `open` above already refuses.
pub fn sys_getdents(args: [u64; 6]) -> SyscallResult {
    let path = path_from_user(args[0], args[1])?;
    let buf_ptr = args[2];
    let capacity = (args[3] as usize) / DIRENT_SIZE;

    if capacity == 0 {
        return Err(SyscallError::InvalidArgument);
    }

    let entries = fs::read_dir(&path).map_err(fs_error_to_syscall)?;

    let mut written = 0usize;
    for entry in entries.iter() {
        if written >= capacity {
            break;
        }

        let mut record = UserDirEntry {
            name: [0u8; 64],
            size: entry.size,
            is_dir: entry.is_dir as u8,
            _reserved: [0; 3],
        };

        let bytes = entry.name.as_bytes();
        let n = core::cmp::min(bytes.len(), record.name.len() - 1);
        record.name[..n].copy_from_slice(&bytes[..n]);

        // SAFETY: UserDirEntry is repr(C) and contains no padding that could
        // expose uninitialised kernel memory — every field is written above.
        let raw = unsafe {
            core::slice::from_raw_parts(
                &record as *const UserDirEntry as *const u8,
                DIRENT_SIZE,
            )
        };

        copy_to_user(buf_ptr + (written * DIRENT_SIZE) as u64, raw)
            .map_err(|_| SyscallError::InvalidArgument)?;

        written += 1;
    }

    Ok(written as u64)
}

/// `stat(path, path_len, statbuf)` — size and type only.
pub fn sys_stat(args: [u64; 6]) -> SyscallResult {
    let path = path_from_user(args[0], args[1])?;
    let out_ptr = args[2];

    let entry = fs::lookup(&path).map_err(fs_error_to_syscall)?;

    let mut record = UserDirEntry {
        name: [0u8; 64],
        size: entry.size,
        is_dir: entry.is_dir as u8,
        _reserved: [0; 3],
    };
    let bytes = entry.name.as_bytes();
    let n = core::cmp::min(bytes.len(), record.name.len() - 1);
    record.name[..n].copy_from_slice(&bytes[..n]);

    let raw = unsafe {
        core::slice::from_raw_parts(&record as *const UserDirEntry as *const u8, DIRENT_SIZE)
    };
    copy_to_user(out_ptr, raw).map_err(|_| SyscallError::InvalidArgument)?;

    Ok(0)
}

/// How many descriptors are open, for diagnostics.
pub fn open_count() -> usize {
    OPEN_FILES.lock().iter().filter(|f| f.is_some()).count()
}

/// Close everything. Called when the user process exits, since there is no
/// process teardown path yet that would do it.
pub fn close_all() {
    let mut table = OPEN_FILES.lock();
    for slot in table.iter_mut() {
        *slot = None;
    }
}

/// List open descriptors, for the console.
pub fn describe_open(mut f: impl FnMut(usize, &str, u32, u32)) {
    let table = OPEN_FILES.lock();
    for (fd, slot) in table.iter().enumerate() {
        if let Some(file) = slot {
            f(fd, &file.path, file.offset, file.entry.size);
        }
    }
}
