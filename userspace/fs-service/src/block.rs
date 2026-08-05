//! The block device, reached over IPC.
//!
//! This is the piece the move to ring 3 actually changed. In the kernel,
//! `crate::block::read_block` was a function call: the ATA driver ran in the
//! same address space, and the sector landed straight in the caller's buffer.
//! Here the driver is `userspace/ata-driver`, a separate process with the I/O
//! port capability, and every sector costs a `send_message`, a block, a wake and
//! a `receive_message`. The FAT32 code above this file does not know that — it
//! calls `read_block` and `read_blocks` with the same signatures the kernel's
//! block layer had, which is the point.
//!
//! ## The cache
//!
//! One sector, and it earns its place. `next_cluster` reads a FAT sector to
//! follow every single link in a chain, and a FAT32 sector holds 128 entries, so
//! walking a 128-cluster run of a file re-read the *same* sector 128 times. In
//! the kernel that was 128 PIO transfers, which was already bad; here it would
//! be 128 IPC round trips. With the cache it is one.
//!
//! Caching is safe here only because nothing writes: this server implements no
//! write path, and the block protocol it speaks has no write operation, so no
//! sector can change underneath the cached copy. The first time either of those
//! stops being true, this cache needs an invalidation story.
//!
//! Multi-sector reads deliberately bypass the cache in both directions. They are
//! file data being streamed through once; letting them fill the cache would
//! evict the FAT sector that the very next `next_cluster` call is about to want.

use crate::{read_u32, write_u32, write_u64};

pub const BLOCK_SIZE: usize = 512;

// --- the block protocol ----------------------------------------------------
//
// The client side of what `userspace/ata-driver/src/main.rs` serves. Kept as
// separate constants rather than a shared crate: two copies of six numbers that
// are checked against each other by the magic field beat a dependency.

const REQ_MAGIC: u32 = 0x4B42_4C4B; // "KBLK"
const REP_MAGIC: u32 = 0x4B52_504C; // "KRPL"

const OP_READ: u32 = 0;
const OP_INFO: u32 = 1;

/// Request: magic, op, lba, count, reserved. 24 bytes.
const REQ_BYTES: usize = 24;
/// Reply header: magic, status, len, reserved. 16 bytes, then the payload.
const REP_HEADER: usize = 16;

/// The driver refuses more than four sectors per request, so a read of a whole
/// cluster has to be split into runs. 4 * 512 + 16 is comfortably inside the
/// kernel's 4096-byte message cap.
const MAX_SECTORS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockError {
    /// `connect` was never called, or the block service was not found.
    NotConnected,
    /// `send_message` or `receive_message` failed. The service is gone, or the
    /// kernel never granted us permission to talk to it.
    Ipc,
    /// A reply arrived that is not a reply: wrong magic, short header, or a
    /// payload length that disagrees with the message length.
    Protocol,
    /// The driver answered with a failure status. The code is the driver's.
    Device(i32),
    /// We asked for something the protocol cannot express — a zero-length or
    /// non-sector-multiple read. A bug on this side, not the driver's.
    BadRequest,
}

// All of this is `static mut` and single-threaded by construction: the server
// loop handles one request at a time and there is no second thread in this
// process. Making it a Mutex would buy nothing and cost a dependency.

static mut SERVICE_PID: u32 = 0;
static mut REQUEST: [u8; REQ_BYTES] = [0; REQ_BYTES];
static mut REPLY: [u8; REP_HEADER + MAX_SECTORS * BLOCK_SIZE] =
    [0; REP_HEADER + MAX_SECTORS * BLOCK_SIZE];

/// Sentinel for "the cache holds nothing". The driver speaks 28-bit LBA, so
/// `u64::MAX` can never be a real sector and does not need a separate flag.
const NO_SECTOR: u64 = u64::MAX;

static mut CACHE: [u8; BLOCK_SIZE] = [0; BLOCK_SIZE];
static mut CACHE_LBA: u64 = NO_SECTOR;
static mut CACHE_HITS: u64 = 0;
static mut CACHE_MISSES: u64 = 0;

/// Point the client at the block service's pid.
///
/// Must be called after a successful `lookup_service("block")` — that lookup is
/// what grants the two processes permission to message each other, so sending
/// before it fails with `Ipc` rather than going somewhere unexpected.
pub fn connect(pid: u32) {
    unsafe {
        SERVICE_PID = pid;
        CACHE_LBA = NO_SECTOR;
    }
}

/// Cache hits and misses, for the shutdown line.
pub fn cache_stats() -> (u64, u64) {
    unsafe { (CACHE_HITS, CACHE_MISSES) }
}

/// Send one request and wait for its reply. Returns the payload length.
///
/// The awkward part is the wait. `receive_message` hands over whatever arrived
/// first from anyone, and a filesystem client can perfectly well send a request
/// while we are halfway through reading a directory for the previous one. Such a
/// message is set aside via [`crate::stash_client_message`] and the wait
/// continues; treating it as the driver's reply would mean parsing an fs request
/// as sector data.
fn transact(op: u32, lba: u64, count: u32) -> Result<usize, BlockError> {
    let pid = unsafe { SERVICE_PID };
    if pid == 0 {
        return Err(BlockError::NotConnected);
    }

    unsafe {
        let request = &mut *core::ptr::addr_of_mut!(REQUEST);
        write_u32(request, 0, REQ_MAGIC);
        write_u32(request, 4, op);
        write_u64(request, 8, lba);
        write_u32(request, 16, count);
        write_u32(request, 20, 0);

        if crate::send_message(pid, request) < 0 {
            return Err(BlockError::Ipc);
        }
    }

    loop {
        let reply = unsafe { &mut *core::ptr::addr_of_mut!(REPLY) };
        let received = crate::receive_message(reply);
        if received < 0 {
            return Err(BlockError::Ipc);
        }

        let sender = (received as u64 >> 32) as u32;
        let len = (received as u64 & 0xFFFF_FFFF) as usize;

        if sender != pid {
            crate::stash_client_message(sender, &reply[..core::cmp::min(len, reply.len())]);
            continue;
        }

        if len < REP_HEADER || read_u32(reply, 0) != REP_MAGIC {
            return Err(BlockError::Protocol);
        }

        let status = read_u32(reply, 4) as i32;
        let payload = read_u32(reply, 8) as usize;

        if status != 0 {
            return Err(BlockError::Device(status));
        }
        // A payload length the message cannot contain means the header is lying;
        // trusting it would read past what the kernel copied in.
        if payload > len - REP_HEADER {
            return Err(BlockError::Protocol);
        }

        return Ok(payload);
    }
}

/// Total sectors and the drive's model string, as the driver reports them.
pub fn info() -> Result<(u64, [u8; 41]), BlockError> {
    let payload = transact(OP_INFO, 0, 0)?;
    if payload < 8 + 41 {
        return Err(BlockError::Protocol);
    }

    let reply = unsafe { &*core::ptr::addr_of!(REPLY) };
    let mut count = [0u8; 8];
    count.copy_from_slice(&reply[REP_HEADER..REP_HEADER + 8]);
    let mut model = [0u8; 41];
    model.copy_from_slice(&reply[REP_HEADER + 8..REP_HEADER + 8 + 41]);

    Ok((u64::from_le_bytes(count), model))
}

/// Read a run of at most [`MAX_SECTORS`] sectors, straight into `out`.
fn read_run(lba: u64, out: &mut [u8]) -> Result<(), BlockError> {
    let count = out.len() / BLOCK_SIZE;
    if count == 0 || count > MAX_SECTORS || out.len() % BLOCK_SIZE != 0 {
        return Err(BlockError::BadRequest);
    }

    let payload = transact(OP_READ, lba, count as u32)?;
    // Short reads are not a thing in this protocol: the driver either fills the
    // whole request or fails it. Accepting fewer bytes would leave the tail of
    // `out` holding whatever was there before, silently.
    if payload != out.len() {
        return Err(BlockError::Protocol);
    }

    let reply = unsafe { &*core::ptr::addr_of!(REPLY) };
    out.copy_from_slice(&reply[REP_HEADER..REP_HEADER + payload]);
    Ok(())
}

/// Read one sector, through the cache.
pub fn read_block(lba: u64, out: &mut [u8; BLOCK_SIZE]) -> Result<(), BlockError> {
    unsafe {
        if CACHE_LBA == lba {
            CACHE_HITS += 1;
            out.copy_from_slice(&*core::ptr::addr_of!(CACHE));
            return Ok(());
        }
        CACHE_MISSES += 1;

        // Invalidate *before* the read, not after. If the read fails partway,
        // the cache must not still claim to hold `lba` — nor the sector it held
        // before, since `read_run` may have overwritten part of it.
        CACHE_LBA = NO_SECTOR;
        let cache = &mut *core::ptr::addr_of_mut!(CACHE);
        read_run(lba, cache)?;
        CACHE_LBA = lba;
        out.copy_from_slice(cache);
    }
    Ok(())
}

/// Read `buf.len() / 512` consecutive sectors starting at `lba`.
///
/// Split into runs of at most four, because that is all one message holds. The
/// kernel's version of this function did the whole transfer in one call; here
/// the loop is the visible cost of the boundary.
pub fn read_blocks(lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
    if buf.is_empty() || buf.len() % BLOCK_SIZE != 0 {
        return Err(BlockError::BadRequest);
    }

    let mut done = 0usize;
    while done < buf.len() {
        let sectors = core::cmp::min(MAX_SECTORS, (buf.len() - done) / BLOCK_SIZE);
        let bytes = sectors * BLOCK_SIZE;
        read_run(lba + (done / BLOCK_SIZE) as u64, &mut buf[done..done + bytes])?;
        done += bytes;
    }

    Ok(())
}
