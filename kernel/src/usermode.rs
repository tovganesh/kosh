//! Ring 3.
//!
//! `iretq`, `sysretq`, `swapgs` and `user_code_segment` had zero occurrences in
//! this repository before Phase 5. For a microkernel — an architecture whose
//! entire premise is that services run *outside* the kernel — that was the
//! single biggest gap in the project.
//!
//! ## Getting to ring 3
//!
//! There is no "jump to user mode" instruction. You fake a return from one:
//! push the five words `iretq` expects (SS, RSP, RFLAGS, CS, RIP) with ring-3
//! selectors, and execute it. The CPU cannot tell the difference between that
//! and returning from a genuine interrupt.
//!
//! ## The payload
//!
//! `user_program.rs` assembles a small position-independent blob into its own
//! `.user` linker section. Phase 5 maps that section's existing frames at a
//! user virtual address rather than allocating and copying — the copying
//! version is an ELF loader, which is Phase 6. The blob is written in assembly
//! with every reference RIP-relative, so it runs correctly at an address it was
//! not linked for.

use x86_64::structures::paging::PageTableFlags;

use crate::memory::paging;
use crate::memory::PAGE_SIZE;
use crate::serial_println;

/// Where the user blob is mapped. 1 GiB — clear of the kernel's low identity
/// window and nowhere near the higher half.
pub const USER_CODE_BASE: u64 = 0x0000_0000_4000_0000;

/// Top of the user stack, growing down.
pub const USER_STACK_TOP: u64 = 0x0000_0000_5000_0000;

/// Pages of user stack.
const USER_STACK_PAGES: usize = 4;

extern "C" {
    static __user_start: u8;
    static __user_end: u8;
    fn kosh_user_entry();
    fn kosh_user_fault_entry();
}

/// Which payload to run.
#[derive(Debug, Clone, Copy)]
pub enum Demo {
    /// Syscalls, including one the kernel must refuse.
    Syscalls,
    /// Dereference a kernel address directly — must be killed, not survived.
    Fault,
}

static CODE_MAPPED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

fn sym(s: &u8) -> u64 {
    s as *const u8 as u64
}

/// Map the user blob and its stack, then drop to ring 3.
///
/// Runs as a kernel thread, so `sys_exit` can retire it through
/// `task::exit_current()` and hand control back to the rest of the kernel —
/// no unwinding required.
pub fn run_user_demo(which: usize) {
    use core::sync::atomic::Ordering;

    let demo = if which == 0 { Demo::Syscalls } else { Demo::Fault };

    let blob_start = sym(unsafe { &__user_start });
    let blob_end = sym(unsafe { &__user_end });
    let blob_pages = ((blob_end - blob_start) as usize).div_ceil(PAGE_SIZE).max(1);

    // Offset of the entry point within the section, so we can find it again at
    // the address the blob is actually mapped to.
    let entry_fn = match demo {
        Demo::Syscalls => kosh_user_entry as usize as u64,
        Demo::Fault => kosh_user_fault_entry as usize as u64,
    };
    let user_entry = USER_CODE_BASE + (entry_fn - blob_start);

    serial_println!("Preparing ring 3 ({:?}):", demo);

    // Read + execute for the user, and NOT writable — W^X applies to userspace
    // too.
    let code_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if !CODE_MAPPED.swap(true, Ordering::SeqCst) {
        serial_println!(
            "  .user section  : 0x{:x}..0x{:x} ({} page(s))",
            blob_start,
            blob_end,
            blob_pages
        );
        if let Err(e) = paging::map_user_range(USER_CODE_BASE, blob_start, blob_pages, code_flags) {
            serial_println!("  failed to map user code: {}", e);
            return;
        }
        serial_println!("  code mapped at : 0x{:x} (R-X, user)", USER_CODE_BASE);
    }
    serial_println!("  entry          : 0x{:x}", user_entry);

    // Each run gets its own stack region so the second demo cannot observe the
    // first one's leftovers.
    let stack_top = USER_STACK_TOP - (which as u64 * 0x10_0000);
    let stack_bottom = stack_top - (USER_STACK_PAGES * PAGE_SIZE) as u64;
    let stack_flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;
    if let Err(e) = paging::map_user_pages(stack_bottom, USER_STACK_PAGES, stack_flags) {
        serial_println!("  failed to map user stack: {}", e);
        return;
    }
    serial_println!(
        "  stack          : 0x{:x}..0x{:x} (RW-, user, NX)",
        stack_bottom,
        stack_top
    );

    serial_println!("Entering ring 3 via iretq...");
    serial_println!("--- ring 3 output ---");

    unsafe { enter_ring3(user_entry, stack_top) }
}

/// Drop to ring 3 at `entry` with `stack_top`.
///
/// # Safety
/// `entry` and `stack_top` must be mapped `USER_ACCESSIBLE`, and the GDT must
/// carry ring-3 code and data descriptors.
unsafe fn enter_ring3(entry: u64, stack_top: u64) -> ! {
    let sel = crate::gdt::selectors();
    let user_cs = (sel.user_code.0 | 3) as u64;
    let user_ss = (sel.user_data.0 | 3) as u64;

    // Reserved bit 1 is always set; IF on so the timer can still preempt the
    // user program. A ring-3 thread that cannot be preempted is a ring-3 thread
    // that can hang the machine with `for {}`.
    let rflags: u64 = 0x202;

    core::arch::asm!(
        "push {ss}",
        "push {rsp}",
        "push {rflags}",
        "push {cs}",
        "push {rip}",
        "iretq",
        ss = in(reg) user_ss,
        rsp = in(reg) stack_top,
        rflags = in(reg) rflags,
        cs = in(reg) user_cs,
        rip = in(reg) entry,
        options(noreturn)
    )
}
