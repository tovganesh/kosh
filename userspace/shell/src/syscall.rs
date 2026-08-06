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
pub const SYS_READ: u64 = 22;
pub const SYS_WRITE: u64 = 23;
pub const SYS_SEND_MESSAGE: u64 = 30;
pub const SYS_RECEIVE_MESSAGE: u64 = 31;
pub const SYS_TIME: u64 = 52;
pub const SYS_LOOKUP_SERVICE: u64 = 47;

pub const STDIN: u64 = 0;
pub const STDOUT: u64 = 1;

pub const SEEK_SET: u64 = 0;
pub const SEEK_CUR: u64 = 1;
pub const SEEK_END: u64 = 2;

/// A directory entry as the filesystem hands it over: fixed size, so there is
/// nothing to parse. Must match `userspace/fs-service/src/main.rs`.
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

/// Read from the console.
///
/// Still a system call, and deliberately a separate function from [`read`]:
/// standard input is the kernel's keyboard ring, not a file, and routing fd 0
/// through the filesystem service would ask it about a descriptor it never
/// handed out. The two used to share a number because the kernel owned both.
pub fn read_stdin(buf: &mut [u8]) -> i64 {
    unsafe { syscall(SYS_READ, STDIN, buf.as_mut_ptr() as u64, buf.len() as u64, 0) }
}

pub fn write(fd: u64, bytes: &[u8]) -> i64 {
    unsafe { syscall(SYS_WRITE, fd, bytes.as_ptr() as u64, bytes.len() as u64, 0) }
}

// --- files, over IPC ---------------------------------------------------------
//
// These six functions used to be system calls: `sys_open` and friends, answered
// by `kernel/src/syscall/files.rs` reading `kernel/src/fs/fat32.rs`. The
// filesystem now runs in ring 3 as the `fs` service, so they are messages
// instead — same six signatures, same semantics, and nothing above this module
// changed. `cmd_ls`, `cmd_cat`, `cmd_cd` and `cmd_stat` are byte-for-byte what
// they were.
//
// That is the case for putting the seam here rather than in the commands: a
// filesystem that moved out of the kernel and forced a rewrite of every caller
// would be a filesystem whose interface was the kernel, not the operation.

const FS_REQ_MAGIC: u32 = 0x4B46_5330; // "KFS0"
const FS_REP_MAGIC: u32 = 0x4B46_5231; // "KFR1"

const FS_OP_OPEN: u32 = 0;
const FS_OP_CLOSE: u32 = 1;
const FS_OP_READ: u32 = 2;
const FS_OP_LSEEK: u32 = 3;
const FS_OP_STAT: u32 = 4;
const FS_OP_GETDENTS: u32 = 5;

const FS_REQ_HEADER: usize = 40;
const FS_REP_HEADER: usize = 24;
const FS_MAX_PATH: usize = 256;
/// Entries the service returns per `GETDENTS`. It is a message, and a message is
/// 4096 bytes, so a directory larger than this needs more than one round trip —
/// which `getdents` below does, invisibly to its caller.
const FS_MAX_DIRENTS: usize = 40;

/// Reply buffer. One, static, because this shell is single-threaded and a 4 KiB
/// buffer on the stack of every filesystem call is 4 KiB of stack this program
/// does not have to spare.
static mut FS_REPLY: [u8; 4096] = [0; 4096];
static mut FS_REQUEST: [u8; FS_REQ_HEADER + FS_MAX_PATH] = [0; FS_REQ_HEADER + FS_MAX_PATH];

/// pid of the `fs` service, looked up once.
static mut FS_PID: i64 = 0;

pub fn lookup_service(name: &str) -> i64 {
    unsafe { syscall(SYS_LOOKUP_SERVICE, name.as_ptr() as u64, name.len() as u64, 0, 0) }
}

/// Find the filesystem. Called once at startup; every file operation after that
/// assumes it succeeded and fails cleanly if it did not.
pub fn connect_fs() -> i64 {
    let pid = lookup_service("fs");
    if pid >= 0 {
        unsafe { FS_PID = pid };
    }
    pid
}

pub fn fs_connected() -> bool {
    unsafe { FS_PID > 0 }
}

fn put_u32(buf: &mut [u8], at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_u64(buf: &mut [u8], at: usize, v: u64) {
    buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn get_u32(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

fn get_i64(buf: &[u8], at: usize) -> i64 {
    let mut v = [0u8; 8];
    v.copy_from_slice(&buf[at..at + 8]);
    i64::from_le_bytes(v)
}

/// One request/reply round trip with the filesystem.
///
/// Returns `(status, payload_len, count, value)` from the reply header, with the
/// payload left in `FS_REPLY` at `FS_REP_HEADER`.
fn fs_call(
    op: u32,
    handle: u32,
    whence: u32,
    offset: i64,
    len: u64,
    path: &str,
) -> (i32, usize, u32, i64) {
    unsafe {
        if FS_PID <= 0 {
            return (-5, 0, 0, 0);
        }
        if path.len() > FS_MAX_PATH {
            return (-22, 0, 0, 0);
        }

        let request = &mut *core::ptr::addr_of_mut!(FS_REQUEST);
        put_u32(request, 0, FS_REQ_MAGIC);
        put_u32(request, 4, op);
        put_u32(request, 8, handle);
        put_u32(request, 12, whence);
        put_u64(request, 16, offset as u64);
        put_u64(request, 24, len);
        put_u32(request, 32, path.len() as u32);
        put_u32(request, 36, 0);
        request[FS_REQ_HEADER..FS_REQ_HEADER + path.len()].copy_from_slice(path.as_bytes());

        let sent = syscall(
            SYS_SEND_MESSAGE,
            FS_PID as u64,
            request.as_ptr() as u64,
            (FS_REQ_HEADER + path.len()) as u64,
            0,
        );
        if sent < 0 {
            return (-5, 0, 0, 0);
        }

        let reply = &mut *core::ptr::addr_of_mut!(FS_REPLY);
        let got = syscall(
            SYS_RECEIVE_MESSAGE,
            reply.as_mut_ptr() as u64,
            reply.len() as u64,
            1,
            0,
        );
        if got < 0 {
            return (-5, 0, 0, 0);
        }

        let received = (got & 0xFFFF_FFFF) as usize;
        if received < FS_REP_HEADER || get_u32(reply, 0) != FS_REP_MAGIC {
            return (-5, 0, 0, 0);
        }

        (
            get_u32(reply, 4) as i32,
            get_u32(reply, 8) as usize,
            get_u32(reply, 12),
            get_i64(reply, 16),
        )
    }
}

pub fn open(path: &str) -> i64 {
    let (status, _, _, value) = fs_call(FS_OP_OPEN, 0, 0, 0, 0, path);
    if status < 0 {
        status as i64
    } else {
        value
    }
}

pub fn close(fd: u64) -> i64 {
    let (status, _, _, _) = fs_call(FS_OP_CLOSE, fd as u32, 0, 0, 0, "");
    status as i64
}

/// Read at the descriptor's current offset. Short reads are normal — the service
/// caps a reply at what fits in one message — so callers loop, which `cmd_cat`
/// already did when the kernel was the one imposing a limit.
pub fn read(fd: u64, buf: &mut [u8]) -> i64 {
    let (status, len, _, _) = fs_call(FS_OP_READ, fd as u32, 0, 0, buf.len() as u64, "");
    if status < 0 {
        return status as i64;
    }
    let n = core::cmp::min(len, buf.len());
    unsafe {
        let reply = &*core::ptr::addr_of!(FS_REPLY);
        buf[..n].copy_from_slice(&reply[FS_REP_HEADER..FS_REP_HEADER + n]);
    }
    n as i64
}

pub fn lseek(fd: u64, offset: i64, whence: u64) -> i64 {
    let (status, _, _, value) = fs_call(FS_OP_LSEEK, fd as u32, whence as u32, offset, 0, "");
    if status < 0 {
        status as i64
    } else {
        value
    }
}

/// Fill `out` with directory entries, looping until the service runs out or the
/// caller's buffer is full.
///
/// The loop is the whole difference from the syscall version. `getdents` used to
/// hand the kernel a buffer and get it filled in one go; a message has a size, so
/// a directory of more than `FS_MAX_DIRENTS` entries arrives in pieces. The
/// caller never sees that.
pub fn getdents(path: &str, out: &mut [RawDirEntry]) -> i64 {
    let mut filled = 0usize;

    while filled < out.len() {
        let want = core::cmp::min(out.len() - filled, FS_MAX_DIRENTS);
        let (status, len, count, _) =
            fs_call(FS_OP_GETDENTS, 0, 0, filled as i64, want as u64, path);

        if status < 0 {
            return status as i64;
        }
        if count == 0 {
            break;
        }

        let entry_size = core::mem::size_of::<RawDirEntry>();
        let usable = core::cmp::min(count as usize, len / entry_size);
        unsafe {
            let reply = &*core::ptr::addr_of!(FS_REPLY);
            let src = reply.as_ptr().add(FS_REP_HEADER) as *const RawDirEntry;
            core::ptr::copy_nonoverlapping(src, out.as_mut_ptr().add(filled), usable);
        }
        filled += usable;

        if usable < want {
            break;
        }
    }

    filled as i64
}

pub fn stat(path: &str, out: &mut RawDirEntry) -> i64 {
    let (status, len, _, _) = fs_call(FS_OP_STAT, 0, 0, 0, 0, path);
    if status < 0 {
        return status as i64;
    }
    if len < core::mem::size_of::<RawDirEntry>() {
        return -5;
    }
    unsafe {
        let reply = &*core::ptr::addr_of!(FS_REPLY);
        core::ptr::copy_nonoverlapping(
            reply.as_ptr().add(FS_REP_HEADER) as *const RawDirEntry,
            out as *mut RawDirEntry,
            1,
        );
    }
    0
}

/// Seconds since the Unix epoch, from the CMOS RTC. Negative on failure.
pub fn time() -> i64 {
    unsafe { syscall(SYS_TIME, 0, 0, 0, 0) }
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
/// Not `fork`/`exec`: the child starts from an ELF rather than as a copy of the
/// caller. It does get its own address space, so it can be — and `hello` is —
/// linked at the same address as this shell. The name is a boot-module name, not
/// a filesystem path.
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
