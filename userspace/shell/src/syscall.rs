//! Raw system calls.
//!
//! The kernel's ABI, as `syscall/entry.rs` implements it: number in RAX,
//! arguments in RDI, RSI, RDX, R10, R8, R9, and a return value in RAX that is
//! negative on failure. Argument four is in R10 rather than RCX because the
//! `syscall` instruction destroys RCX.
//!
//! This replaces `service_client.rs`, which had zero `asm!` in 1,399 lines and
//! whose `wait_for_response` returned a fabricated success.

#![allow(dead_code)]

pub const SYS_EXIT: u64 = 1;
pub const SYS_WAIT: u64 = 4;
pub const SYS_GETPID: u64 = 5;
pub const SYS_YIELD: u64 = 8;
pub const SYS_SPAWN: u64 = 9;
pub const SYS_OPEN: u64 = 20;
pub const SYS_CLOSE: u64 = 21;
pub const SYS_READ: u64 = 22;
pub const SYS_WRITE: u64 = 23;
pub const SYS_LSEEK: u64 = 24;
pub const SYS_STAT: u64 = 25;
pub const SYS_GETDENTS: u64 = 70;

pub const STDIN: u64 = 0;
pub const STDOUT: u64 = 1;

pub const SEEK_SET: u64 = 0;
pub const SEEK_CUR: u64 = 1;
pub const SEEK_END: u64 = 2;

/// A directory entry as the kernel hands it over: fixed size, so there is
/// nothing to parse. Must match `kernel/src/syscall/files.rs`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawDirEntry {
    pub name: [u8; 64],
    pub size: u32,
    pub is_dir: u8,
    pub _reserved: [u8; 3],
}

impl RawDirEntry {
    pub const fn zeroed() -> Self {
        Self {
            name: [0; 64],
            size: 0,
            is_dir: 0,
            _reserved: [0; 3],
        }
    }

    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..end]).unwrap_or("<invalid utf-8>")
    }
}

#[inline(always)]
unsafe fn syscall(number: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") number => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

pub fn write(fd: u64, bytes: &[u8]) -> i64 {
    unsafe { syscall(SYS_WRITE, fd, bytes.as_ptr() as u64, bytes.len() as u64, 0) }
}

pub fn read(fd: u64, buf: &mut [u8]) -> i64 {
    unsafe { syscall(SYS_READ, fd, buf.as_mut_ptr() as u64, buf.len() as u64, 0) }
}

pub fn open(path: &str) -> i64 {
    unsafe { syscall(SYS_OPEN, path.as_ptr() as u64, path.len() as u64, 0, 0) }
}

pub fn close(fd: u64) -> i64 {
    unsafe { syscall(SYS_CLOSE, fd, 0, 0, 0) }
}

pub fn lseek(fd: u64, offset: i64, whence: u64) -> i64 {
    unsafe { syscall(SYS_LSEEK, fd, offset as u64, whence, 0) }
}

pub fn getdents(path: &str, out: &mut [RawDirEntry]) -> i64 {
    unsafe {
        syscall(
            SYS_GETDENTS,
            path.as_ptr() as u64,
            path.len() as u64,
            out.as_mut_ptr() as u64,
            (out.len() * core::mem::size_of::<RawDirEntry>()) as u64,
        )
    }
}

pub fn stat(path: &str, out: &mut RawDirEntry) -> i64 {
    unsafe {
        syscall(
            SYS_STAT,
            path.as_ptr() as u64,
            path.len() as u64,
            out as *mut RawDirEntry as u64,
            0,
        )
    }
}

pub fn getpid() -> i64 {
    unsafe { syscall(SYS_GETPID, 0, 0, 0, 0) }
}

pub fn yield_now() -> i64 {
    unsafe { syscall(SYS_YIELD, 0, 0, 0, 0) }
}

/// ENOENT, as `SyscallError::NotFound` encodes it. The one error `spawn` returns
/// that means "no such program" rather than "something went wrong", so it is the
/// only one a shell should turn into "command not found".
pub const ENOENT: i64 = -2;

/// Load a program by name and run it on its own task. Returns the task id.
///
/// Not `fork`/`exec` — the kernel has one address space, so there is nothing to
/// duplicate. The name is a boot-module name, not a filesystem path.
pub fn spawn(name: &str) -> i64 {
    unsafe { syscall(SYS_SPAWN, name.as_ptr() as u64, name.len() as u64, 0, 0) }
}

/// Block until `task` finishes; returns its exit code through `status`.
pub fn wait(task: i64, status: &mut i32) -> i64 {
    unsafe {
        syscall(
            SYS_WAIT,
            task as u64,
            status as *mut i32 as u64,
            0,
            0,
        )
    }
}

pub fn exit(code: u64) -> ! {
    unsafe { syscall(SYS_EXIT, code, 0, 0, 0) };
    loop {
        core::hint::spin_loop();
    }
}
