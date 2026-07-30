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

use spin::Mutex;
use x86_64::structures::paging::PageTableFlags;

use crate::memory::paging::{self, PHYSMAP_BASE};
use crate::memory::PAGE_SIZE;
use crate::serial_println;

/// A boot module as GRUB placed it, copied out of the multiboot2 structure
/// before that borrow ends.
#[derive(Debug, Clone, Copy)]
pub struct BootModule {
    pub start: u64,
    pub end: u64,
}

impl BootModule {
    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    /// The module's bytes, viewed through the physmap.
    ///
    /// # Safety
    /// Only valid after `paging::init`, and only while the frames remain
    /// reserved — which `physical::reserve_boot_modules` guarantees.
    pub unsafe fn bytes(&self) -> &'static [u8] {
        core::slice::from_raw_parts((PHYSMAP_BASE + self.start) as *const u8, self.len())
    }
}

const MAX_BOOT_MODULES: usize = 4;
static BOOT_MODULES: Mutex<[Option<BootModule>; MAX_BOOT_MODULES]> =
    Mutex::new([None; MAX_BOOT_MODULES]);

/// Copy the module table out of the multiboot2 info.
pub fn record_boot_modules(boot_info: &multiboot2::BootInformation) {
    let mut slots = BOOT_MODULES.lock();
    let mut n = 0;

    for module in boot_info.module_tags() {
        if n >= MAX_BOOT_MODULES {
            serial_println!("  (ignoring boot modules beyond {})", MAX_BOOT_MODULES);
            break;
        }

        let name = module.cmdline().unwrap_or("<no cmdline>");
        slots[n] = Some(BootModule {
            start: module.start_address() as u64,
            end: module.end_address() as u64,
        });

        serial_println!(
            "Boot module {}: 0x{:x}..0x{:x} ({} bytes) '{}'",
            n,
            module.start_address(),
            module.end_address(),
            module.end_address() - module.start_address(),
            name
        );
        n += 1;
    }

    if n == 0 {
        serial_println!("No boot modules supplied by the bootloader");
    }
}

/// The nth boot module, if present.
pub fn boot_module(index: usize) -> Option<BootModule> {
    BOOT_MODULES.lock().get(index).copied().flatten()
}

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

/// Load boot module 0 as an ELF and run it in ring 3.
///
/// This is the real path: a program the kernel was not compiled with, parsed
/// out of an ELF, mapped at the addresses it was linked for, and entered.
pub fn run_boot_module(_arg: usize) {
    let Some(module) = boot_module(0) else {
        serial_println!("No boot module to load — skipping ELF loader demo.");
        serial_println!("  (add `module2 /boot/init` to grub.cfg)");
        return;
    };

    serial_println!("Loading boot module 0 as ELF:");
    let image = unsafe { module.bytes() };
    crate::elf::describe(image);

    let loaded = match unsafe { crate::elf::load(image) } {
        Ok(l) => l,
        Err(e) => {
            serial_println!("  ELF load FAILED: {:?}", e);
            return;
        }
    };

    serial_println!(
        "  loaded {} segment(s), {} bytes, entry 0x{:x}",
        loaded.segments,
        loaded.bytes_mapped,
        loaded.entry
    );

    // A fresh stack, well clear of the image's own segments.
    let stack_top = ELF_USER_STACK_TOP;
    let stack_bottom = stack_top - (USER_STACK_PAGES * PAGE_SIZE) as u64;
    let stack_flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;

    if let Err(e) = paging::map_user_pages(stack_bottom, USER_STACK_PAGES, stack_flags) {
        serial_println!("  failed to map stack for loaded image: {}", e);
        return;
    }
    serial_println!(
        "  stack          : 0x{:x}..0x{:x} (RW-, user, NX)",
        stack_bottom,
        stack_top
    );

    serial_println!("Entering loaded ELF in ring 3...");
    serial_println!("--- loaded program output ---");

    unsafe { enter_ring3(loaded.entry, stack_top) }
}

/// Stack for the ELF-loaded program. Separate from the built-in payload's, so
/// the two demos cannot interfere.
const ELF_USER_STACK_TOP: u64 = 0x0000_0000_6000_0000;

/// Drop to ring 3 at `entry` with `stack_top`.
///
/// `stack_top` is passed through as RSP unchanged, so the contract with
/// userspace is the System V one: **RSP is 16-byte aligned at process entry**.
/// It is the program's `_start` that must then establish the call-boundary
/// alignment its compiled code expects — which is exactly what a real crt0
/// does, and what `userspace/hello` does. Getting this wrong shows up as a #GP
/// on the first `movaps` spill, not as anything obviously stack-related.
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
