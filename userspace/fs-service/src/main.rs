//! The filesystem server, in ring 3.
//!
//! `kernel/src/fs/` and `kernel/src/syscall/files.rs` do this job inside the
//! kernel today: mount FAT32 from the block layer, keep a descriptor table, and
//! answer `open`/`read`/`lseek`/`stat`/`getdents` for the one ring-3 process.
//! This is the same thing as a program. It holds no capability at all — not even
//! the I/O port grant the ATA driver holds — and reaches the disk only by asking
//! another unprivileged process for sectors.
//!
//! What the move cost, exactly:
//!
//!   * every sector read is an IPC round trip instead of a function call, which
//!     is why `block.rs` caches a sector;
//!   * a message is capped at 4096 bytes, so a client read is capped at 4000 and
//!     a directory listing at 40 entries per call, and clients loop;
//!   * `receive_message` is one queue with no notion of "the reply to *this*
//!     request", so a client message that arrives while we are mid-sector-read
//!     has to be set aside by hand — see [`stash_client_message`].
//!
//! What it did not cost: the filesystem itself. `fat32.rs` is the kernel's file
//! with the three sector reads redirected.
//!
//! ## What replaced what
//!
//! The previous contents of this crate — `main.rs`, `lib.rs`, `vfs.rs`,
//! `ext4.rs` — are deleted. They described an ext4 driver whose `read_block`
//! filled the buffer with zeroes and whose superblock was a literal in the
//! source. Nothing in them had ever touched a disk.

#![no_std]
#![no_main]

extern crate alloc;

use core::panic::PanicInfo;

use linked_list_allocator::LockedHeap;

/// `print` with formatting. Allocates, so it must not be used before the heap is
/// initialised, nor from the panic handler.
macro_rules! say {
    ($($arg:tt)*) => {
        crate::print(&::alloc::format!($($arg)*))
    };
}

mod block;
mod fat32;

use fat32::{DirEntry, Fat32, FsError};

// --- heap ------------------------------------------------------------------

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// 256 KiB of heap, in .bss — the same arrangement `userspace/shell` uses.
///
/// This costs nothing until it is touched, because the kernel demand-pages
/// `.bss`: the range is mapped without frames behind it and a physical page is
/// allocated on the first fault against each one. A server that only ever lists
/// small directories therefore uses a few pages, and the ceiling is there for
/// the cases that need it — a long cluster chain, or a 32 KiB cluster size,
/// which `read_dir` and `read_at` allocate a whole buffer for.
const HEAP_SIZE: usize = 256 * 1024;
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

// --- syscalls --------------------------------------------------------------

const SYS_EXIT: u64 = 1;
const SYS_WRITE: u64 = 23;
const SYS_SEND_MESSAGE: u64 = 30;
const SYS_RECEIVE_MESSAGE: u64 = 31;
const SYS_REGISTER_SERVICE: u64 = 46;
const SYS_LOOKUP_SERVICE: u64 = 47;

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

fn exit(code: u64) -> ! {
    unsafe { syscall3(SYS_EXIT, code, 0, 0) };
    loop {
        core::hint::spin_loop();
    }
}

pub(crate) fn send_message(to: u32, bytes: &[u8]) -> i64 {
    unsafe {
        syscall3(
            SYS_SEND_MESSAGE,
            to as u64,
            bytes.as_ptr() as u64,
            bytes.len() as u64,
        )
    }
}

/// Blocking receive. Returns `(sender << 32) | len`, or negative.
pub(crate) fn receive_message(buf: &mut [u8]) -> i64 {
    unsafe {
        syscall3(
            SYS_RECEIVE_MESSAGE,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
            1, // blocking: park the thread, do not spin on an empty queue
        )
    }
}

/// Resolve a service name to a pid. This also grants the two processes mutual
/// permission to message each other, which is why `block::connect` may only be
/// given a pid that this returned.
fn lookup_service(name: &str) -> i64 {
    unsafe {
        syscall3(
            SYS_LOOKUP_SERVICE,
            name.as_ptr() as u64,
            name.len() as u64,
            0,
        )
    }
}

fn register_service(name: &str) -> i64 {
    unsafe {
        syscall3(
            SYS_REGISTER_SERVICE,
            name.as_ptr() as u64,
            name.len() as u64,
            0,
        )
    }
}

// --- little-endian field access --------------------------------------------

pub(crate) fn read_u32(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

pub(crate) fn read_u64(buf: &[u8], at: usize) -> u64 {
    let mut v = [0u8; 8];
    v.copy_from_slice(&buf[at..at + 8]);
    u64::from_le_bytes(v)
}

pub(crate) fn write_u32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub(crate) fn write_u64(buf: &mut [u8], at: usize, value: u64) {
    buf[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

// --- the filesystem protocol -----------------------------------------------
//
// Fixed-layout little-endian records, like the block protocol next door. Both
// ends live in this repository; a self-describing encoding would only add a
// parser, and a parser is a thing that can be wrong.
//
// Request header, 40 bytes, then `path_len` bytes of path:
//     @0  u32 magic     @4  u32 op       @8  u32 handle    @12 u32 whence
//     @16 i64 offset    @24 u64 len      @32 u32 path_len  @36 u32 reserved
//
// Reply header, 24 bytes, then the payload:
//     @0  u32 magic     @4  i32 status   @8  u32 len       @12 u32 count
//     @16 i64 value

const REQ_MAGIC: u32 = 0x4B46_5330; // "KFS0"
const REP_MAGIC: u32 = 0x4B46_5231; // "KFR1"

const OP_OPEN: u32 = 0;
const OP_CLOSE: u32 = 1;
const OP_READ: u32 = 2;
const OP_LSEEK: u32 = 3;
const OP_STAT: u32 = 4;
const OP_GETDENTS: u32 = 5;
const OP_SHUTDOWN: u32 = 6;
/// `STATFS` -> a human-readable description of the mounted volume and the disk
/// under it, as UTF-8 in the payload.
///
/// A string rather than a struct on purpose. `df` prints it and nothing parses
/// it, so a struct would be an ABI to keep in step for no gain — and the moment
/// something *does* want the numbers, a struct is the right answer and this is
/// the wrong one. Written down so that trade is a decision rather than an
/// accident.
const OP_STATFS: u32 = 7;

const REQ_HEADER: usize = 40;
const REP_HEADER: usize = 24;

/// The kernel caps a message at 4096 bytes, and that cap is behind every "the
/// client loops" in this file.
const MAX_MESSAGE: usize = 4096;
const MAX_PAYLOAD: usize = MAX_MESSAGE - REP_HEADER; // 4072

/// The most a single READ answers with. 4000 rather than 4072 because a round
/// number is easier to reason about at the client end, and a short read is legal
/// anyway — the client is already looping.
const MAX_READ: usize = 4000;

/// 72 bytes: `RawDirEntry` in `userspace/shell/src/syscall.rs`, which is
/// `UserDirEntry` in `kernel/src/syscall/files.rs`. Same layout deliberately —
/// a client written against the kernel's `getdents` should not be able to tell
/// that the answer now comes from a different process.
const DIRENT_SIZE: usize = 72;
const NAME_BYTES: usize = 64;

/// 40 * 72 = 2880, inside the payload cap with room to spare.
const MAX_DIRENTS: usize = 40;

const SEEK_SET: u32 = 0;
const SEEK_CUR: u32 = 1;
const SEEK_END: u32 = 2;

// Status codes, chosen to be the errno values the kernel's syscall layer already
// maps these failures to.
const ST_OK: i32 = 0;
const ST_NOT_FOUND: i32 = -2;
const ST_IO: i32 = -5;
const ST_NOT_A_DIRECTORY: i32 = -20;
const ST_IS_A_DIRECTORY: i32 = -21;
const ST_INVALID: i32 = -22;

fn fs_error_to_status(e: FsError) -> i32 {
    match e {
        FsError::NotFound => ST_NOT_FOUND,
        FsError::NotADirectory => ST_NOT_A_DIRECTORY,
        FsError::IsADirectory => ST_IS_A_DIRECTORY,
        // Everything else is the disk, or the structures on it, being wrong.
        // From the client's side those are indistinguishable from I/O failure,
        // and the detail has already gone to the console.
        FsError::Block(_)
        | FsError::NotFat32
        | FsError::BadGeometry(_)
        | FsError::CorruptChain => ST_IO,
        FsError::NameTooLong => ST_INVALID,
    }
}

// --- state -----------------------------------------------------------------
//
// All `static mut`, all single-threaded by construction: this process has one
// thread, and the server loop finishes one request before taking the next. A
// lock here would protect against nothing.

static mut VOLUME: Option<Fat32> = None;

/// What the block service said about the disk, kept for `STATFS`.
///
/// Cached rather than re-asked: `df` should describe the disk this filesystem
/// is mounted on, and asking the driver again would describe whatever disk it
/// has *now* — the same answer today and a lie the moment a driver can be
/// restarted under a service that stays up.
static mut DISK_BLOCKS: u64 = 0;
static mut DISK_MODEL: [u8; 41] = [0; 41];

static mut REQUEST: [u8; MAX_MESSAGE] = [0; MAX_MESSAGE];
static mut REPLY: [u8; MAX_MESSAGE] = [0; MAX_MESSAGE];

/// One message set aside because it arrived while we were waiting for the block
/// service. See [`stash_client_message`].
static mut STASH: [u8; MAX_MESSAGE] = [0; MAX_MESSAGE];
static mut STASH_LEN: usize = 0;
static mut STASH_FROM: u32 = 0;

fn volume() -> &'static Fat32 {
    match unsafe { (*core::ptr::addr_of!(VOLUME)).as_ref() } {
        Some(fs) => fs,
        // Unreachable: `fs_service_main` exits before registering the service if
        // the mount failed, so nothing can ask for a volume that is not there.
        None => {
            print("  fs-service: no volume mounted\n");
            exit(5)
        }
    }
}

// --- open files ------------------------------------------------------------

/// One open file. Read-only, because the filesystem is.
struct OpenFile {
    entry: DirEntry,
    offset: u32,
    /// The pid that opened it. A handle is an index, not a capability, so this
    /// check is the only thing stopping one client from reading another's file
    /// by guessing a small number.
    owner: u32,
    /// Value of [`TICK`] at last use, for eviction.
    used: u64,
}

const MAX_OPEN: usize = 16;

static mut OPEN: [Option<OpenFile>; MAX_OPEN] = [const { None }; MAX_OPEN];
static mut TICK: u64 = 0;

fn tick() -> u64 {
    unsafe {
        TICK = TICK.wrapping_add(1);
        TICK
    }
}

/// Take a slot for a newly opened file, evicting if the table is full.
///
/// ## Why eviction rather than "table full"
///
/// There is no process-exit notification in this system. When a client dies, or
/// simply forgets to CLOSE, this server never learns of it, and the slot stays
/// held by a pid that no longer exists. Sixteen of those and the filesystem
/// stops working for everybody, permanently, with no way back short of
/// restarting the server. A fixed table that leaks is a table that fills up.
///
/// So OPEN never fails for want of a slot. It evicts, preferring the requesting
/// client's own stalest handle — a client that leaks then mostly hurts itself —
/// and falling back to the globally stalest. The failure mode being traded for
/// should be named too: a client that opens a file, waits while sixteen other
/// opens happen, and then reads gets `-22` on a handle it believes is good. That
/// is visible and recoverable by re-opening, where a permanently full table is
/// neither.
///
/// The real fix is for the kernel to tell servers when a process dies. Until
/// there is such a message this is the least bad approximation, and it is an
/// approximation.
fn allocate_slot(entry: DirEntry, owner: u32) -> u32 {
    let table = unsafe { &mut *core::ptr::addr_of_mut!(OPEN) };

    let mut chosen = None;
    for (i, slot) in table.iter().enumerate() {
        if slot.is_none() {
            chosen = Some(i);
            break;
        }
    }

    let index = match chosen {
        Some(i) => i,
        None => {
            let mut victim = 0usize;
            // (belongs to the requester, last used) — ordered so that "belongs
            // to the requester" wins outright and ties break to the stalest.
            let mut best: Option<(bool, u64)> = None;
            for (i, slot) in table.iter().enumerate() {
                let file = match slot.as_ref() {
                    Some(f) => f,
                    None => continue,
                };
                let key = (file.owner == owner, file.used);
                let better = match best {
                    None => true,
                    Some((best_owned, best_used)) => {
                        (key.0 && !best_owned) || (key.0 == best_owned && key.1 < best_used)
                    }
                };
                if better {
                    best = Some(key);
                    victim = i;
                }
            }
            say!(
                "  fs-service: open table full, dropping handle {} held by pid {}\n",
                victim + 1,
                table[victim].as_ref().map(|f| f.owner).unwrap_or(0)
            );
            victim
        }
    };

    table[index] = Some(OpenFile {
        entry,
        offset: 0,
        owner,
        used: tick(),
    });

    // Handles are slot + 1 so that 0 is never valid: a client that sends a
    // zeroed request gets an error rather than whatever is in slot zero.
    (index + 1) as u32
}

/// Resolve a handle belonging to `owner`, refreshing its place in the LRU order.
fn slot_index(handle: u32, owner: u32) -> Option<usize> {
    if handle == 0 || handle as usize > MAX_OPEN {
        return None;
    }
    let index = handle as usize - 1;
    let now = tick();
    let table = unsafe { &mut *core::ptr::addr_of_mut!(OPEN) };

    match table[index].as_mut() {
        Some(file) if file.owner == owner => {
            file.used = now;
            Some(index)
        }
        // Either nothing is open there or it belongs to someone else. Both
        // answer the same way: as far as this client is concerned the handle
        // does not exist, and distinguishing the two would tell it that another
        // process has a file open.
        _ => None,
    }
}

// --- serving ---------------------------------------------------------------

/// Set aside a message that arrived while `block.rs` was waiting for a sector.
///
/// `receive_message` hands over whatever reached the queue first, from anybody,
/// and nothing in a message says "this is the reply to your read". So the block
/// client checks the sender and hands anything else here, rather than parsing an
/// fs request as sector data.
///
/// One slot, because the server answers one request at a time and the client it
/// is currently serving is waiting on a reply: in practice only a *second*
/// client can produce a message during a read. A third one's request is dropped
/// with a warning and that client will wait forever for a reply that is not
/// coming. That is the honest limit of this design. Fixing it means either a
/// per-request reply port in the kernel or a real request queue here, and
/// neither is worth building before there is a second filesystem client.
pub(crate) fn stash_client_message(sender: u32, bytes: &[u8]) {
    unsafe {
        if STASH_LEN != 0 {
            print("  fs-service: message arrived during a disk read with the stash full; dropped\n");
            return;
        }
        if bytes.is_empty() {
            // A zero-length message cannot be a valid request, and storing one
            // would be indistinguishable from an empty stash.
            return;
        }
        let stash = &mut *core::ptr::addr_of_mut!(STASH);
        let n = core::cmp::min(bytes.len(), stash.len());
        stash[..n].copy_from_slice(&bytes[..n]);
        STASH_LEN = n;
        STASH_FROM = sender;
    }
}

/// The next request, from the stash if one is waiting there. `None` means the
/// receive failed and the server should stop.
fn next_message() -> Option<(u32, usize)> {
    unsafe {
        if STASH_LEN != 0 {
            let len = STASH_LEN;
            let from = STASH_FROM;
            let stash = &*core::ptr::addr_of!(STASH);
            let request = &mut *core::ptr::addr_of_mut!(REQUEST);
            request[..len].copy_from_slice(&stash[..len]);
            STASH_LEN = 0;
            return Some((from, len));
        }
    }

    let request = unsafe { &mut *core::ptr::addr_of_mut!(REQUEST) };
    let received = receive_message(request);
    if received < 0 {
        print("  fs-service: receive failed, stopping\n");
        return None;
    }

    let sender = (received as u64 >> 32) as u32;
    let len = (received as u64 & 0xFFFF_FFFF) as usize;
    // The kernel should never report more than it copied, but the length is used
    // to bound slicing below, so clamp it here rather than trust it there.
    Some((sender, core::cmp::min(len, request.len())))
}

/// The path bytes of the request currently in [`REQUEST`].
///
/// The borrow outlives the disk reads that follow it, which is safe for a reason
/// worth stating: a message that arrives during those reads goes to [`STASH`],
/// never to `REQUEST`, so nothing overwrites the path while it is in use.
fn request_path(len: usize) -> Result<&'static str, i32> {
    let request = unsafe { &*core::ptr::addr_of!(REQUEST) };
    let path_len = read_u32(request, 32) as usize;

    if path_len == 0 || path_len > len - REQ_HEADER {
        return Err(ST_INVALID);
    }

    core::str::from_utf8(&request[REQ_HEADER..REQ_HEADER + path_len]).map_err(|_| ST_INVALID)
}

/// Write one 72-byte `RawDirEntry` at `at` bytes into the payload.
///
/// The name is truncated to 63 bytes and NUL-padded, which is exactly what
/// `kernel/src/syscall/files.rs` does. It is worth naming what that means: a
/// long filename over 63 bytes can be cut in the middle of a UTF-8 sequence, and
/// the client's `name_str()` then renders the entry as `<invalid utf-8>`. The
/// kernel has always behaved this way and clients are written against it, so
/// this copies it rather than quietly fixing it in one of the two places.
fn write_dirent(payload: &mut [u8], at: usize, name: &str, size: u32, is_dir: bool) {
    let record = &mut payload[at..at + DIRENT_SIZE];
    for b in record.iter_mut() {
        *b = 0;
    }

    let bytes = name.as_bytes();
    let n = core::cmp::min(bytes.len(), NAME_BYTES - 1);
    record[..n].copy_from_slice(&bytes[..n]);

    write_u32(record, NAME_BYTES, size);
    record[NAME_BYTES + 4] = is_dir as u8;
    // The three reserved bytes are already zero from the wipe above. This buffer
    // is sent to another process, so they must be zero rather than whatever the
    // previous reply left there.
}

/// Fill the reply header and send it, along with the `payload_len` bytes the
/// handler has already written after it.
fn reply(to: u32, status: i32, payload_len: usize, count: u32, value: i64) {
    unsafe {
        let buffer = &mut *core::ptr::addr_of_mut!(REPLY);
        write_u32(buffer, 0, REP_MAGIC);
        write_u32(buffer, 4, status as u32);
        write_u32(buffer, 8, payload_len as u32);
        write_u32(buffer, 12, count);
        write_u64(buffer, 16, value as u64);

        let total = REP_HEADER + payload_len;
        if send_message(to, &buffer[..total]) < 0 {
            // The client is gone, or never had permission to hear from us.
            // Nothing can be done about it, but it should not be silent: a
            // client waiting on a reply it will never get looks exactly like a
            // hung filesystem.
            say!("  fs-service: reply to pid {} could not be delivered\n", to);
        }
    }
}

/// Handle one request body. Returns `(status, payload_len, count, value)`; the
/// payload itself goes straight into [`REPLY`] past the header.
fn handle(op: u32, sender: u32, len: usize) -> (i32, usize, u32, i64) {
    let request = unsafe { &*core::ptr::addr_of!(REQUEST) };
    let handle = read_u32(request, 8);
    let whence = read_u32(request, 12);
    let offset = read_u64(request, 16) as i64;
    let want = read_u64(request, 24);

    match op {
        OP_STATFS => {
            let text = {
                use core::fmt::Write;
                let mut out = alloc::string::String::new();
                let (blocks, model) = unsafe {
                    (
                        *core::ptr::addr_of!(DISK_BLOCKS),
                        *core::ptr::addr_of!(DISK_MODEL),
                    )
                };
                let end = model.iter().position(|&b| b == 0).unwrap_or(model.len());
                let name = core::str::from_utf8(&model[..end]).unwrap_or("<unreadable>");
                let _ = write!(
                    out,
                    "{}\n{}  {} blocks x 512 bytes  ({} MB)",
                    volume().describe(),
                    name,
                    blocks,
                    blocks * 512 / (1024 * 1024)
                );
                out
            };

            let bytes = text.as_bytes();
            let n = core::cmp::min(bytes.len(), MAX_PAYLOAD);
            unsafe {
                let reply = &mut *core::ptr::addr_of_mut!(REPLY);
                reply[REP_HEADER..REP_HEADER + n].copy_from_slice(&bytes[..n]);
            }
            (ST_OK, n, 0, 0)
        }
        OP_OPEN => {
            let path = match request_path(len) {
                Ok(p) => p,
                Err(status) => return (status, 0, 0, 0),
            };
            let entry = match volume().lookup(path) {
                Ok(e) => e,
                Err(e) => return (fs_error_to_status(e), 0, 0, 0),
            };
            if entry.is_dir {
                // Directories are listed with GETDENTS, not read as byte
                // streams. The kernel's `sys_open` refused this too, though it
                // said "invalid argument" where this says "is a directory".
                return (ST_IS_A_DIRECTORY, 0, 0, 0);
            }
            (ST_OK, 0, 0, allocate_slot(entry, sender) as i64)
        }

        OP_CLOSE => match slot_index(handle, sender) {
            Some(index) => {
                let table = unsafe { &mut *core::ptr::addr_of_mut!(OPEN) };
                table[index] = None;
                (ST_OK, 0, 0, 0)
            }
            None => (ST_INVALID, 0, 0, 0),
        },

        OP_READ => {
            let index = match slot_index(handle, sender) {
                Some(i) => i,
                None => return (ST_INVALID, 0, 0, 0),
            };

            let want = core::cmp::min(want as usize, MAX_READ);
            if want == 0 {
                return (ST_OK, 0, 0, 0);
            }

            let table = unsafe { &mut *core::ptr::addr_of_mut!(OPEN) };
            let file = match table[index].as_mut() {
                Some(f) => f,
                None => return (ST_INVALID, 0, 0, 0),
            };

            // The reply buffer is the read buffer: the sectors are assembled
            // where they will be sent from, rather than copied through a
            // scratch buffer on the way out.
            let reply_buffer = unsafe { &mut *core::ptr::addr_of_mut!(REPLY) };
            let read = volume().read_at(
                &file.entry,
                file.offset,
                &mut reply_buffer[REP_HEADER..REP_HEADER + want],
            );

            match read {
                // Reading at or past the end returns 0 bytes and status ok,
                // which is how a client's read loop terminates.
                Ok(n) => {
                    file.offset += n as u32;
                    (ST_OK, n, 0, 0)
                }
                Err(e) => (fs_error_to_status(e), 0, 0, 0),
            }
        }

        OP_LSEEK => {
            let index = match slot_index(handle, sender) {
                Some(i) => i,
                None => return (ST_INVALID, 0, 0, 0),
            };
            let table = unsafe { &mut *core::ptr::addr_of_mut!(OPEN) };
            let file = match table[index].as_mut() {
                Some(f) => f,
                None => return (ST_INVALID, 0, 0, 0),
            };

            let base = match whence {
                SEEK_SET => 0i64,
                SEEK_CUR => file.offset as i64,
                SEEK_END => file.entry.size as i64,
                _ => return (ST_INVALID, 0, 0, 0),
            };

            let target = match base.checked_add(offset) {
                Some(t) => t,
                None => return (ST_INVALID, 0, 0, 0),
            };
            // Seeking past the end is legal — reads there simply return nothing
            // — but the offset is a u32 because a FAT32 file size is. The
            // kernel's `sys_lseek` truncated, so a seek to 4 GiB + 1 silently
            // became a seek to 1. Refusing is identical behaviour for every
            // offset a file can actually have, and an error rather than a wrong
            // answer for the rest.
            if target < 0 || target > u32::MAX as i64 {
                return (ST_INVALID, 0, 0, 0);
            }

            file.offset = target as u32;
            (ST_OK, 0, 0, target)
        }

        OP_STAT => {
            let path = match request_path(len) {
                Ok(p) => p,
                Err(status) => return (status, 0, 0, 0),
            };
            let entry = match volume().lookup(path) {
                Ok(e) => e,
                Err(e) => return (fs_error_to_status(e), 0, 0, 0),
            };

            let reply_buffer = unsafe { &mut *core::ptr::addr_of_mut!(REPLY) };
            // `/` stats as name "/", size 0, is_dir 1 — the synthetic root entry
            // `lookup` returns, since the root has no directory entry of its own
            // on disk. That is what the kernel's `sys_stat` reported, so clients
            // that special-case it keep working.
            write_dirent(
                &mut reply_buffer[REP_HEADER..],
                0,
                &entry.name,
                entry.size,
                entry.is_dir,
            );
            (ST_OK, DIRENT_SIZE, 0, 0)
        }

        OP_GETDENTS => {
            let path = match request_path(len) {
                Ok(p) => p,
                Err(status) => return (status, 0, 0, 0),
            };
            if offset < 0 {
                return (ST_INVALID, 0, 0, 0);
            }
            let start = offset as usize;
            let capacity = core::cmp::min(want as usize, MAX_DIRENTS);
            if capacity == 0 {
                // The kernel refused a zero-capacity buffer the same way. A
                // client that asks for no entries has a bug, and answering "ok,
                // zero entries" would look to it like the end of the directory.
                return (ST_INVALID, 0, 0, 0);
            }

            let entry = match volume().lookup(path) {
                Ok(e) => e,
                Err(e) => return (fs_error_to_status(e), 0, 0, 0),
            };
            if !entry.is_dir {
                return (ST_NOT_A_DIRECTORY, 0, 0, 0);
            }

            let entries = match volume().read_dir(entry.first_cluster) {
                Ok(e) => e,
                Err(e) => return (fs_error_to_status(e), 0, 0, 0),
            };

            // `.` and `..` are real entries in a FAT subdirectory and are
            // returned as they are on disk, which is what the kernel's
            // `getdents` did. The root has neither, also as before.
            //
            // The whole directory is re-read for every windowful, which is
            // O(n^2) in directory size across a client's paging loop. The price
            // is deliberate: the alternative is per-client iteration state that
            // nothing would ever free — the same argument as the open table
            // above, reaching the opposite conclusion because here there is a
            // stateless option and there it was that or nothing.
            let reply_buffer = unsafe { &mut *core::ptr::addr_of_mut!(REPLY) };
            let mut count = 0usize;
            for e in entries.iter().skip(start).take(capacity) {
                write_dirent(
                    &mut reply_buffer[REP_HEADER..],
                    count * DIRENT_SIZE,
                    &e.name,
                    e.size,
                    e.is_dir,
                );
                count += 1;
            }

            // A start index past the end returns zero entries and status ok,
            // which is how a client's listing loop terminates.
            (ST_OK, count * DIRENT_SIZE, count as u32, 0)
        }

        _ => (ST_INVALID, 0, 0, 0),
    }
}

/// Handle one request. Returns false when told to shut down.
fn serve_one() -> bool {
    let (sender, len) = match next_message() {
        Some(v) => v,
        None => return false,
    };

    let request = unsafe { &*core::ptr::addr_of!(REQUEST) };

    if len < REQ_HEADER || read_u32(request, 0) != REQ_MAGIC {
        reply(sender, ST_INVALID, 0, 0, 0);
        return true;
    }

    let op = read_u32(request, 4);

    if op == OP_SHUTDOWN {
        reply(sender, ST_OK, 0, 0, 0);
        return false;
    }

    let (status, payload_len, count, value) = handle(op, sender, len);
    debug_assert!(payload_len <= MAX_PAYLOAD);
    reply(sender, status, payload_len, count, value);
    true
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
    call    fs_service_main
1:
    jmp     1b
"#,
    options(att_syntax)
);

/// Find the block service, retrying while it starts.
///
/// There is no sleep in the syscall set this program uses, so the wait is a
/// spin — bounded, because a block service that is never going to appear should
/// be a loud failure rather than a process that looks busy forever. If the two
/// are started in the wrong order this covers it; if the driver is absent, the
/// log says so and the exit code says which step failed.
fn find_block_service() -> u32 {
    const ATTEMPTS: u32 = 100;
    const SPINS: u32 = 200_000;

    for attempt in 0..ATTEMPTS {
        let pid = lookup_service("block");
        if pid > 0 {
            return pid as u32;
        }
        if attempt == 0 {
            print("  fs-service: waiting for the 'block' service\n");
        }
        for _ in 0..SPINS {
            core::hint::spin_loop();
        }
    }

    print("  fs-service: no 'block' service registered; is the ATA driver running?\n");
    exit(1)
}

#[no_mangle]
pub extern "C" fn fs_service_main() -> ! {
    // Before anything that allocates, which is nearly everything below.
    unsafe {
        ALLOCATOR
            .lock()
            .init(core::ptr::addr_of_mut!(HEAP) as *mut u8, HEAP_SIZE);
    }

    print("  fs-service: starting in ring 3\n");

    let block_pid = find_block_service();
    block::connect(block_pid);
    say!("  fs-service: block service is pid {}\n", block_pid);

    // INFO before mounting, so that a disk which is not there is reported as a
    // disk which is not there, rather than as a filesystem that will not mount.
    match block::info() {
        Ok((sectors, model)) => {
            unsafe {
                DISK_BLOCKS = sectors;
                DISK_MODEL = model;
            }
            let end = model.iter().position(|&b| b == 0).unwrap_or(model.len());
            let name = core::str::from_utf8(&model[..end]).unwrap_or("<not utf-8>");
            say!(
                "  fs-service: disk '{}', {} sectors, {} MB\n",
                name,
                sectors,
                sectors * 512 / (1024 * 1024)
            );
        }
        Err(e) => {
            say!(
                "  fs-service: the block service will not answer INFO: {:?}\n",
                e
            );
            exit(2);
        }
    }

    // The image is a bare filesystem, not a partitioned disk, so the BPB is at
    // LBA 0. A partition table would mean reading the MBR first and mounting at
    // the partition's start sector.
    match Fat32::mount(0) {
        Ok(fs) => {
            say!(
                "  fs-service: mounted '{}' — {}\n",
                fs.label(),
                fs.describe()
            );
            unsafe { VOLUME = Some(fs) };
        }
        Err(e) => {
            say!("  fs-service: no FAT32 filesystem at LBA 0: {:?}\n", e);
            exit(3);
        }
    }

    // Only now. A client that looks us up must find a server that can already
    // answer, not one still deciding whether it has a disk.
    if register_service("fs") < 0 {
        print("  fs-service: register_service('fs') was refused\n");
        exit(4);
    }
    print("  fs-service: registered as 'fs'\n");

    while serve_one() {}

    // The block service is deliberately left running. It is not ours to stop:
    // the kernel's own block layer and any other client are entitled to it, and
    // shutting it down from here would take the disk away from them.
    let (hits, misses) = block::cache_stats();
    say!(
        "  fs-service: shutting down, {} sector cache hits / {} misses\n",
        hits,
        misses
    );
    exit(0)
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // No formatting here: a panic may well be the allocator failing, and `say!`
    // allocates.
    print("  fs-service panic\n");
    exit(6)
}
