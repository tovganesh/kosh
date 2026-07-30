//! Per-process address spaces.
//!
//! ## What this replaces
//!
//! Every program the kernel has ever loaded shared one set of page tables. They
//! were protected from each other by page permissions — `USER_ACCESSIBLE`, NX,
//! read-only text — but not by *separation*: `ksh` could not read `hello`'s
//! memory, yet both had to be linked at different fixed addresses so their
//! images would not land on top of each other, and `spawn` had to refuse an
//! image overlapping one already resident. Every userspace program carrying a
//! hand-picked link address in its own `user.ld` was the visible symptom.
//!
//! An address space here is a PML4 of its own. The lower half — entries 0..256,
//! the whole of userspace — belongs to it alone. The upper half is the kernel's
//! and is shared, which is only possible because the kernel moved out of
//! PML4[0] in the previous phase.
//!
//! ## Sharing the upper half
//!
//! [`AddressSpace::new_user`] copies PML4 entries 256..512 from the kernel's
//! table. It copies the *entries*, not the tables they point at, so every
//! address space walks the same PDPTs for the physmap, the heap and the kernel
//! image. A page mapped into the kernel heap after a process was created is
//! therefore visible to it — which is what makes it safe for a syscall to
//! allocate.
//!
//! The one thing that would break that is the kernel populating a *new* PML4
//! entry after processes exist, because the copy already happened. Nothing does:
//! the physmap, the heap window and the kernel image are one entry each and are
//! all created by `paging::init`. [`check_kernel_half`] asserts it rather than
//! trusting it.
//!
//! ## Which frames belong to a space
//!
//! Teardown must free the frames a process was given without freeing frames it
//! merely borrowed — the built-in ring-3 payload lives in the kernel image and
//! is mapped into user space directly. Leaf entries created by
//! `map_user_pages_in` carry [`OWNED_BY_ADDRESS_SPACE`]; those from
//! `map_user_range_in` do not. Teardown frees exactly the tagged ones, plus the
//! page tables it walked.

use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::{PageTable, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

use crate::memory::paging::{
    kernel_pml4_phys, mapper_for, KERNEL_PML4_FIRST, OWNED_BY_ADDRESS_SPACE, PHYSMAP_BASE,
};
use crate::memory::physical::{allocate_frame, deallocate_frame, PageFrame};
use crate::serial_println;

/// A process's page tables.
///
/// Deliberately not `Clone`: two handles to one PML4 would mean two teardowns.
#[derive(Debug)]
pub struct AddressSpace {
    pml4_phys: u64,
}

impl AddressSpace {
    /// A fresh address space: empty lower half, the kernel's upper half.
    pub fn new_user() -> Result<Self, &'static str> {
        let frame = allocate_frame().ok_or("out of memory allocating a PML4")?;
        let pml4_phys = frame.address() as u64;

        let kernel_pml4 = kernel_pml4_phys();
        if kernel_pml4 == 0 {
            return Err("paging::init has not run");
        }

        unsafe {
            let table = &mut *((PHYSMAP_BASE + pml4_phys) as *mut PageTable);
            table.zero();

            let kernel = &*((PHYSMAP_BASE + kernel_pml4) as *const PageTable);
            for i in KERNEL_PML4_FIRST..512 {
                table[i] = kernel[i].clone();
            }
        }

        Ok(Self { pml4_phys })
    }

    pub fn pml4_phys(&self) -> u64 {
        self.pml4_phys
    }

    /// Load this space into CR3.
    ///
    /// # Safety
    /// Every address the caller is currently using — RIP, RSP, the GDT, the IDT,
    /// the per-CPU block — must be mapped identically in the new space. That
    /// holds for anything in the kernel's higher half, which is where all of
    /// those live, and is why this is safe to call from the scheduler.
    pub unsafe fn activate(&self) {
        Cr3::write(
            PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(self.pml4_phys)),
            Cr3Flags::empty(),
        );
    }

    /// Free the lower half and the PML4 itself.
    ///
    /// # Safety
    /// This space must not be in CR3. The exiting thread switches back to the
    /// kernel's tables before calling this, because freeing the page tables you
    /// are executing on is only funny once.
    pub unsafe fn free(self) -> usize {
        let mut freed = 0;
        let table = &mut *((PHYSMAP_BASE + self.pml4_phys) as *mut PageTable);

        for i in 0..KERNEL_PML4_FIRST {
            if table[i].is_unused() {
                continue;
            }
            freed += free_level(table[i].addr().as_u64(), 3);
            table[i].set_unused();
        }

        deallocate_frame(PageFrame::from_address(self.pml4_phys as usize));
        freed + 1
    }
}

/// Recursively free one level of page tables and everything under it.
///
/// `level` counts down: 3 = PDPT, 2 = PD, 1 = PT. A huge-page entry at level 3
/// or 2 is a leaf; the kernel never creates one in the lower half, but checking
/// costs nothing and misreading one as a table pointer would free 512 frames of
/// somebody else's memory.
unsafe fn free_level(table_phys: u64, level: u8) -> usize {
    let mut freed = 0;
    let table = &mut *((PHYSMAP_BASE + table_phys) as *mut PageTable);

    for i in 0..512 {
        let entry = &mut table[i];
        if entry.is_unused() {
            continue;
        }

        let flags = entry.flags();
        let addr = entry.addr().as_u64();

        if level == 1 || flags.contains(PageTableFlags::HUGE_PAGE) {
            // A leaf. Free the frame only if this space owns it — the built-in
            // ring-3 payload is mapped straight out of the kernel image.
            if flags.contains(OWNED_BY_ADDRESS_SPACE) {
                deallocate_frame(PageFrame::from_address(addr as usize));
                freed += 1;
            }
        } else {
            freed += free_level(addr, level - 1);
        }

        entry.set_unused();
    }

    // The table itself.
    deallocate_frame(PageFrame::from_address(table_phys as usize));
    freed + 1
}

/// Check that every address space still agrees with the kernel about the upper
/// half.
///
/// `new_user` copies PML4 entries 256..512 once, at creation. If the kernel ever
/// populated a *new* top-level entry afterwards, existing processes would not
/// see it and a syscall running in one of them would fault on a perfectly valid
/// kernel address — intermittently, depending on which process was running.
///
/// Nothing creates one today. This says so out loud instead of assuming.
pub fn check_kernel_half(space: &AddressSpace) -> bool {
    let kernel_pml4 = kernel_pml4_phys();
    unsafe {
        let theirs = &*((PHYSMAP_BASE + space.pml4_phys()) as *const PageTable);
        let kernel = &*((PHYSMAP_BASE + kernel_pml4) as *const PageTable);

        for i in KERNEL_PML4_FIRST..512 {
            if theirs[i].addr() != kernel[i].addr() || theirs[i].flags() != kernel[i].flags() {
                serial_println!(
                    "  address space diverges from the kernel at PML4[{}]: 0x{:x} vs 0x{:x}",
                    i,
                    theirs[i].addr().as_u64(),
                    kernel[i].addr().as_u64()
                );
                return false;
            }
        }
    }
    true
}

/// How many PML4 entries the kernel occupies. Diagnostics.
pub fn kernel_half_entries() -> usize {
    let kernel_pml4 = kernel_pml4_phys();
    if kernel_pml4 == 0 {
        return 0;
    }
    unsafe {
        let kernel = &*((PHYSMAP_BASE + kernel_pml4) as *const PageTable);
        (KERNEL_PML4_FIRST..512).filter(|&i| !kernel[i].is_unused()).count()
    }
}

/// Prove that two address spaces really are separate.
///
/// Maps one page at the *same user virtual address* in two different spaces,
/// writes a different value into each through the physmap, then activates each
/// space in turn and reads that address. Under the single shared address space
/// this kernel had until now, the second mapping would either fail with
/// `PageAlreadyMapped` or silently replace the first, and both reads would
/// return the same value.
///
/// Deliberately not a test of "can a process read another process's memory" —
/// that question was already answered by page permissions. The claim being
/// checked here is narrower and newer: the same number means different memory
/// depending on which PML4 is loaded.
pub fn self_test() {
    use crate::memory::paging::{map_user_pages_in, translate_in};
    use crate::memory::PAGE_SIZE;

    // Somewhere in the lower half nothing else uses. Above the `mmap` region, so
    // the two are not confusable in a log even though these spaces are private
    // and thrown away.
    const PROBE: u64 = 0x0000_0000_2800_0000;
    const VALUE_A: u64 = 0xAAAA_AAAA_AAAA_AAAA;
    const VALUE_B: u64 = 0xBBBB_BBBB_BBBB_BBBB;

    serial_println!("Verifying address-space isolation...");

    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;

    let (a, b) = match (AddressSpace::new_user(), AddressSpace::new_user()) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            serial_println!("  Address spaces: FAIL — could not allocate two PML4s");
            return;
        }
    };

    let mut mapped_ok = true;
    for pml4 in [a.pml4_phys(), b.pml4_phys()] {
        if let Err(e) = map_user_pages_in(unsafe { mapper_for(pml4) }, PROBE, 1, flags) {
            serial_println!("  Address spaces: FAIL — could not map the probe page: {}", e);
            mapped_ok = false;
        }
    }
    if !mapped_ok {
        unsafe {
            a.free();
            b.free();
        }
        return;
    }

    // Distinct frames is the first thing to check: if the two spaces shared a
    // lower half, this is where it shows.
    let phys_a = translate_in(a.pml4_phys(), PROBE);
    let phys_b = translate_in(b.pml4_phys(), PROBE);

    match (phys_a, phys_b) {
        (Some(pa), Some(pb)) if pa != pb => serial_println!(
            "  0x{:x} -> frame 0x{:x} in one space, 0x{:x} in the other",
            PROBE,
            pa,
            pb
        ),
        (Some(pa), Some(pb)) => {
            serial_println!(
                "  Address spaces: FAIL — 0x{:x} resolves to the same frame 0x{:x}/0x{:x}",
                PROBE,
                pa,
                pb
            );
            unsafe {
                a.free();
                b.free();
            }
            return;
        }
        _ => {
            serial_println!("  Address spaces: FAIL — the probe page did not map");
            unsafe {
                a.free();
                b.free();
            }
            return;
        }
    }

    // Write through the physmap, which is shared, so neither write needs the
    // space it targets to be active.
    unsafe {
        core::ptr::write_volatile((PHYSMAP_BASE + phys_a.unwrap()) as *mut u64, VALUE_A);
        core::ptr::write_volatile((PHYSMAP_BASE + phys_b.unwrap()) as *mut u64, VALUE_B);
    }

    // Now read the *same address* with each space loaded. Interrupts off: a
    // context switch in the middle would reload CR3 from under us.
    let (kernel_cr3, _) = Cr3::read();
    let (seen_a, seen_b) = crate::interrupts::without_interrupts(|| unsafe {
        a.activate();
        let sa = core::ptr::read_volatile(PROBE as *const u64);
        b.activate();
        let sb = core::ptr::read_volatile(PROBE as *const u64);
        Cr3::write(kernel_cr3, Cr3Flags::empty());
        (sa, sb)
    });

    if seen_a == VALUE_A && seen_b == VALUE_B {
        serial_println!(
            "  same address 0x{:x}: 0x{:x} in one space, 0x{:x} in the other",
            PROBE,
            seen_a,
            seen_b
        );
        serial_println!(
            "  Address spaces: PASS — {} kernel PML4 entries shared, lower half private",
            kernel_half_entries()
        );
    } else {
        serial_println!(
            "  Address spaces: FAIL — read 0x{:x} and 0x{:x}, expected 0x{:x} and 0x{:x}",
            seen_a,
            seen_b,
            VALUE_A,
            VALUE_B
        );
    }

    // Both spaces should also still agree with the kernel about the upper half.
    if !check_kernel_half(&a) || !check_kernel_half(&b) {
        serial_println!("  Address spaces: FAIL — upper half diverged from the kernel");
    }

    let freed = unsafe { a.free() + b.free() };
    serial_println!("  teardown returned {} frame(s), including {} probe pages", freed, 2);
    let _ = PAGE_SIZE;
}
