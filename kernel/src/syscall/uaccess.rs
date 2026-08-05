//! Copying across the user/kernel boundary.
//!
//! A syscall argument that is a pointer is *untrusted input*. The kernel must
//! never dereference it without first establishing that it points where the
//! caller is allowed to point.
//!
//! This did not exist before Phase 5, and the consequences were visible in the
//! dispatcher: `sys_write` returned the byte count without reading the buffer,
//! `sys_read` returned a length without writing one, and `sys_send_message`
//! discarded the user pointer entirely and sent a `format!` string in its
//! place. Those all "worked" because nothing could call them.
//!
//! Two checks, both necessary:
//!
//! 1. **Range.** The whole span must lie in the lower canonical half. A caller
//!    passing `0xFFFF_8000_0000_0000` is asking the kernel to read its own
//!    physmap on their behalf.
//! 2. **Mapping.** Every page in the span must be present *and* carry
//!    `USER_ACCESSIBLE`. Range checking alone is not enough — a kernel-only
//!    page can sit at a low address.

use x86_64::structures::paging::mapper::TranslateResult;
use x86_64::structures::paging::{PageTableFlags, Translate};
use x86_64::VirtAddr;

use crate::memory::PAGE_SIZE;

/// First address of the higher (kernel) half. Anything at or above this is
/// never a valid user pointer.
pub const USER_ADDRESS_LIMIT: u64 = 0x0000_8000_0000_0000;

/// Refuse absurdly large transfers outright rather than walking millions of
/// page-table entries on behalf of a hostile caller.
const MAX_TRANSFER: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAccessError {
    /// Pointer or length puts part of the span outside the user half.
    OutOfRange,
    /// A page in the span is not mapped.
    NotMapped,
    /// A page in the span is mapped, but not for user access.
    NotUserAccessible,
    /// A page in the span is mapped read-only and this is a write.
    NotWritable,
    /// Length exceeds `MAX_TRANSFER`.
    TooLarge,
}

/// Check that `[ptr, ptr + len)` is a valid user span.
///
/// `writable` additionally requires every page to be writable by the user.
pub fn validate_user_range(ptr: u64, len: usize, writable: bool) -> Result<(), UserAccessError> {
    if len == 0 {
        return Ok(());
    }
    if len > MAX_TRANSFER {
        return Err(UserAccessError::TooLarge);
    }

    // Overflow here is itself an attack: `ptr = u64::MAX, len = 2` would
    // otherwise wrap into a "valid looking" range.
    let end = ptr.checked_add(len as u64).ok_or(UserAccessError::OutOfRange)?;
    if end > USER_ADDRESS_LIMIT {
        return Err(UserAccessError::OutOfRange);
    }

    let mapper = unsafe { crate::memory::paging::active_mapper() };

    let first_page = ptr & !(PAGE_SIZE as u64 - 1);
    let mut page = first_page;

    while page < end {
        // A reserved page is not mapped *yet*. The kernel is about to read or
        // write it at the user address, which would fault from ring 0 in the
        // middle of a syscall — the same hazard copy-on-write has — so
        // materialise it before the check rather than after.
        //
        // Unconditional and idempotent: `resolve_demand` returns `Ok(false)`
        // immediately for anything that is not a reservation.
        let _ = crate::memory::paging::resolve_demand(page);

        match mapper.translate(VirtAddr::new(page)) {
            TranslateResult::Mapped { flags, .. } => {
                if !flags.contains(PageTableFlags::USER_ACCESSIBLE) {
                    return Err(UserAccessError::NotUserAccessible);
                }
                if writable && !flags.contains(PageTableFlags::WRITABLE) {
                    // A copy-on-write page is writable — it just needs the copy
                    // doing first. Resolve it here rather than letting the write
                    // below fault: `copy_to_user` writes at the *user* address,
                    // so with CR0.WP set a shared page would trap into the page
                    // fault handler from ring 0, in the middle of a syscall that
                    // may be holding locks the handler wants.
                    //
                    // Doing it up front turns that into an ordinary function
                    // call. It also means a failure is reported as an error
                    // return rather than a fault.
                    if flags.contains(crate::memory::paging::COPY_ON_WRITE) {
                        crate::memory::paging::resolve_cow(page)
                            .map_err(|_| UserAccessError::NotWritable)?;
                    } else {
                        return Err(UserAccessError::NotWritable);
                    }
                }
            }
            _ => return Err(UserAccessError::NotMapped),
        }

        page += PAGE_SIZE as u64;
    }

    Ok(())
}

/// Copy `dst.len()` bytes from the user address `src` into `dst`.
pub fn copy_from_user(src: u64, dst: &mut [u8]) -> Result<(), UserAccessError> {
    validate_user_range(src, dst.len(), false)?;

    // The span is validated and the kernel shares the address space, so a
    // straight copy is sound. Volatile so the compiler cannot decide the
    // user's memory is unobservable and elide it.
    for (i, byte) in dst.iter_mut().enumerate() {
        *byte = unsafe { core::ptr::read_volatile((src + i as u64) as *const u8) };
    }

    Ok(())
}

/// Copy `src` into the user address `dst`.
pub fn copy_to_user(dst: u64, src: &[u8]) -> Result<(), UserAccessError> {
    validate_user_range(dst, src.len(), true)?;

    for (i, byte) in src.iter().enumerate() {
        unsafe { core::ptr::write_volatile((dst + i as u64) as *mut u8, *byte) };
    }

    Ok(())
}

/// Read a NUL-terminated user string, up to `max` bytes, into `buf`.
/// Returns the number of bytes read, excluding the terminator.
pub fn copy_str_from_user(
    src: u64,
    buf: &mut [u8],
    max: usize,
) -> Result<usize, UserAccessError> {
    let limit = core::cmp::min(max, buf.len());

    for i in 0..limit {
        let addr = src + i as u64;
        validate_user_range(addr, 1, false)?;

        let byte = unsafe { core::ptr::read_volatile(addr as *const u8) };
        if byte == 0 {
            return Ok(i);
        }
        buf[i] = byte;
    }

    Ok(limit)
}

/// Prove the checks reject what they should.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("Verifying user-pointer validation...");

    let kernel_addr = crate::memory::paging::PHYSMAP_BASE;
    let checks: [(&str, Result<(), UserAccessError>, bool); 4] = [
        (
            "kernel physmap address rejected",
            validate_user_range(kernel_addr, 8, false),
            false,
        ),
        (
            "null pointer rejected",
            validate_user_range(0, 8, false),
            false,
        ),
        (
            "length overflow rejected",
            validate_user_range(u64::MAX - 1, 16, false),
            false,
        ),
        (
            "oversized transfer rejected",
            validate_user_range(0x4000_0000, MAX_TRANSFER + 1, false),
            false,
        ),
    ];

    for (name, result, should_succeed) in checks {
        let ok = result.is_ok() == should_succeed;
        serial_println!("  {} {}", if ok { "PASS" } else { "FAIL" }, name);
    }
}
