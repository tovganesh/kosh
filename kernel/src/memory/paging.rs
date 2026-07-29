//! Kernel page tables.
//!
//! The bootstrap trampoline in `boot32.rs` sets up a flat identity map so the
//! CPU can reach long mode. That map is deliberately crude: everything is
//! writable and executable, and it only covers the first 1 GiB.
//!
//! This module replaces it with page tables the kernel builds itself:
//!
//!   * **W^X on the kernel image** — `.text` is read+execute, `.rodata` is
//!     read-only+NX, everything else is read+write+NX. Nothing is both
//!     writable and executable.
//!   * **A physical memory map** at [`PHYSMAP_BASE`] so the kernel can reach
//!     any physical frame by adding a constant, without editing page tables.
//!     This is what `vmm.rs`'s `PHYSICAL_MEMORY_OFFSET` always assumed existed
//!     — nothing had ever created it.
//!   * **A dedicated heap window** at [`KERNEL_HEAP_BASE`], so the heap lives
//!     at proper virtual addresses instead of using raw physical addresses as
//!     pointers.
//!   * **Page 0 left unmapped**, preserving the null guard.
//!
//! Until this module ran, `Cr3::write` appeared nowhere in the kernel — the
//! "virtual memory manager" was reading the bootloader's CR3 and describing it.

use spin::Mutex;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr3, Cr3Flags};
use x86_64::registers::model_specific::{Efer, EferFlags};
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size2MiB,
    Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

use crate::memory::physical::{allocate_frame, bitmap_extent, PageFrame};
use crate::memory::PAGE_SIZE;
use crate::serial_println;

/// All of physical memory is mapped here, read+write, never executable.
/// PML4 entry 256 — the first entry of the higher half.
pub const PHYSMAP_BASE: u64 = 0xFFFF_8000_0000_0000;

/// Virtual base of the kernel heap window. PML4 entry 288.
pub const KERNEL_HEAP_BASE: u64 = 0xFFFF_9000_0000_0000;

extern "C" {
    static __kernel_start: u8;
    static __text_start: u8;
    static __text_end: u8;
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __kernel_end: u8;
}

fn sym(s: &u8) -> u64 {
    s as *const u8 as u64
}

/// Adapter so the `x86_64` crate's mapper can pull frames from our bitmap
/// allocator.
struct KoshFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for KoshFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        allocate_frame().map(|f: PageFrame| {
            PhysFrame::containing_address(PhysAddr::new(f.address() as u64))
        })
    }
}

/// Set once `init` has switched CR3. Before that, physical addresses must be
/// reached through the bootstrap identity map instead.
static PHYSMAP_ACTIVE: Mutex<bool> = Mutex::new(false);

/// Translate a physical address to a kernel-virtual one.
///
/// Before [`init`] this is the identity map (offset 0); afterwards it is the
/// physmap. Callers do not need to care which.
pub fn phys_to_virt(phys: u64) -> u64 {
    if *PHYSMAP_ACTIVE.lock() {
        PHYSMAP_BASE + phys
    } else {
        phys
    }
}

/// Build the kernel's own page tables and install them.
///
/// `phys_mem_end` is the highest physical address that needs to appear in the
/// physmap — normally the end of the last usable memory region.
pub fn init(phys_mem_end: u64) -> Result<(), &'static str> {
    serial_println!("Building kernel page tables...");

    // NO_EXECUTE is a reserved bit unless EFER.NXE is set; setting it without
    // enabling the feature turns every NX mapping into a reserved-bit page
    // fault. Enable it before we write a single entry.
    unsafe {
        Efer::update(|flags| flags.insert(EferFlags::NO_EXECUTE_ENABLE));
    }
    serial_println!("  EFER.NXE enabled");

    // CR0.WP makes read-only mappings apply to ring 0 as well. Without it the
    // kernel can happily write through a read-only PTE and the .rodata/.text
    // protections below are decorative — which is exactly what the first
    // version of this code shipped: a deliberate write to .text succeeded.
    unsafe {
        Cr0::update(|flags| flags.insert(Cr0Flags::WRITE_PROTECT));
    }
    serial_println!("  CR0.WP enabled (read-only pages enforced in ring 0)");

    // We are still running on the bootstrap identity map, so a physical
    // address is also a valid virtual address. That is what makes it possible
    // to build a fresh table hierarchy at all.
    let pml4_frame = allocate_frame().ok_or("no frame for PML4")?;
    let pml4_phys = pml4_frame.address() as u64;
    let pml4: &mut PageTable = unsafe {
        let ptr = pml4_phys as *mut PageTable;
        ptr.write(PageTable::new());
        &mut *ptr
    };

    // Offset 0: during construction, physical == virtual.
    let mut mapper = unsafe { OffsetPageTable::new(pml4, VirtAddr::new(0)) };
    let mut frames = KoshFrameAllocator;

    map_kernel_image(&mut mapper, &mut frames)?;
    let identity_end = map_low_identity(&mut mapper, &mut frames)?;
    map_physical_memory(&mut mapper, &mut frames, phys_mem_end)?;

    // Install. From the next instruction onwards the kernel is running on
    // tables it built, with W^X enforced and a real physmap.
    unsafe {
        Cr3::write(
            PhysFrame::containing_address(PhysAddr::new(pml4_phys)),
            Cr3Flags::empty(),
        );
    }

    *PHYSMAP_ACTIVE.lock() = true;

    serial_println!("  CR3 -> 0x{:x} (kernel page tables active)", pml4_phys);
    serial_println!(
        "  identity window : 0x{:x}..0x{:x} (4 KiB pages, W^X)",
        PAGE_SIZE,
        identity_end
    );
    serial_println!(
        "  physmap         : 0x{:x} -> 0x0..0x{:x} (2 MiB pages, RW+NX)",
        PHYSMAP_BASE,
        phys_mem_end
    );
    serial_println!("  page 0          : unmapped (null guard)");

    Ok(())
}

/// Map the kernel image with per-section permissions.
///
/// The identity mapping is kept — the kernel is linked at 1 MiB and is
/// executing there, so it cannot move without a jump trampoline. What changes
/// is the permissions: `.text` loses write, everything else loses execute.
fn map_kernel_image(
    mapper: &mut OffsetPageTable,
    frames: &mut KoshFrameAllocator,
) -> Result<(), &'static str> {
    let text_start = align_down_page(sym(unsafe { &__text_start }));
    let text_end = align_up_page(sym(unsafe { &__text_end }));
    let rodata_start = align_down_page(sym(unsafe { &__rodata_start }));
    let rodata_end = align_up_page(sym(unsafe { &__rodata_end }));
    let kernel_start = align_down_page(sym(unsafe { &__kernel_start }));
    let kernel_end = align_up_page(sym(unsafe { &__kernel_end }));

    const PRESENT: PageTableFlags = PageTableFlags::PRESENT;
    let rx = PRESENT;
    let ro = PRESENT | PageTableFlags::NO_EXECUTE;
    let rw = PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

    let mut addr = kernel_start;
    while addr < kernel_end {
        let flags = if addr >= text_start && addr < text_end {
            rx
        } else if addr >= rodata_start && addr < rodata_end {
            ro
        } else {
            rw
        };
        identity_map_4k(mapper, frames, addr, flags)?;
        addr += PAGE_SIZE as u64;
    }

    serial_println!(
        "  kernel image    : .text 0x{:x}..0x{:x} RX, .rodata 0x{:x}..0x{:x} RO+NX, rest RW+NX",
        text_start,
        text_end,
        rodata_start,
        rodata_end
    );

    Ok(())
}

/// Identity-map the low region the kernel still touches by physical address:
/// VGA at 0xB8000, and the physical frame bitmap.
///
/// Page 0 is skipped so a null dereference still faults.
fn map_low_identity(
    mapper: &mut OffsetPageTable,
    frames: &mut KoshFrameAllocator,
) -> Result<u64, &'static str> {
    let (_bitmap_start, bitmap_end) = bitmap_extent();
    let kernel_start = align_down_page(sym(unsafe { &__kernel_start }));
    let kernel_end = align_up_page(sym(unsafe { &__kernel_end }));

    // One 2 MiB of slack past the bitmap covers early page-table frames, which
    // the allocator hands out from just above it.
    let end = align_up_2mib(bitmap_end as u64 + 2 * 1024 * 1024);

    let rw = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

    let mut addr = PAGE_SIZE as u64; // skip page 0: null guard
    while addr < end {
        // The kernel image was already mapped with tighter permissions.
        if addr >= kernel_start && addr < kernel_end {
            addr += PAGE_SIZE as u64;
            continue;
        }
        identity_map_4k(mapper, frames, addr, rw)?;
        addr += PAGE_SIZE as u64;
    }

    Ok(end)
}

/// Map all of physical memory at [`PHYSMAP_BASE`] using 2 MiB pages.
fn map_physical_memory(
    mapper: &mut OffsetPageTable,
    frames: &mut KoshFrameAllocator,
    phys_mem_end: u64,
) -> Result<(), &'static str> {
    const HUGE: u64 = 2 * 1024 * 1024;
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

    let end = align_up_2mib(phys_mem_end);
    let mut phys = 0u64;

    while phys < end {
        let page: Page<Size2MiB> =
            Page::containing_address(VirtAddr::new(PHYSMAP_BASE + phys));
        let frame: PhysFrame<Size2MiB> =
            PhysFrame::containing_address(PhysAddr::new(phys));

        unsafe {
            mapper
                .map_to(page, frame, flags, frames)
                .map_err(|_| "failed to map physmap page")?
                .ignore(); // not live yet; no TLB entry can exist
        }

        phys += HUGE;
    }

    Ok(())
}

fn identity_map_4k(
    mapper: &mut OffsetPageTable,
    frames: &mut KoshFrameAllocator,
    addr: u64,
    flags: PageTableFlags,
) -> Result<(), &'static str> {
    let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(addr));
    let frame: PhysFrame<Size4KiB> = PhysFrame::containing_address(PhysAddr::new(addr));

    unsafe {
        mapper
            .map_to(page, frame, flags, frames)
            .map_err(|_| "failed to identity-map page")?
            .ignore();
    }
    Ok(())
}

/// Map `pages` freshly-allocated frames at `virt`, read+write, no-execute.
///
/// Used for the kernel heap. Frames do not need to be contiguous — that is the
/// entire point of having page tables.
pub fn map_kernel_pages(virt: u64, pages: usize) -> Result<(), &'static str> {
    let mut mapper = unsafe { active_mapper() };
    let mut frames = KoshFrameAllocator;
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

    for i in 0..pages {
        let frame = allocate_frame().ok_or("out of physical memory mapping kernel pages")?;
        let page: Page<Size4KiB> =
            Page::containing_address(VirtAddr::new(virt + (i * PAGE_SIZE) as u64));
        let phys: PhysFrame<Size4KiB> =
            PhysFrame::containing_address(PhysAddr::new(frame.address() as u64));

        unsafe {
            mapper
                .map_to(page, phys, flags, &mut frames)
                .map_err(|_| "failed to map kernel page")?
                .flush();
        }
    }

    Ok(())
}

/// Borrow the live page tables through the physmap.
///
/// # Safety
/// Only valid after [`init`] has installed the tables and enabled the physmap.
/// The caller must not create two aliasing mappers at once.
pub unsafe fn active_mapper() -> OffsetPageTable<'static> {
    let (frame, _) = Cr3::read();
    let virt = PHYSMAP_BASE + frame.start_address().as_u64();
    let table: &'static mut PageTable = &mut *(virt as *mut PageTable);
    OffsetPageTable::new(table, VirtAddr::new(PHYSMAP_BASE))
}

/// Translate a kernel-virtual address using the live tables. `None` if unmapped.
pub fn translate(virt: u64) -> Option<u64> {
    use x86_64::structures::paging::Translate;
    let mapper = unsafe { active_mapper() };
    mapper.translate_addr(VirtAddr::new(virt)).map(|p| p.as_u64())
}

fn align_down_page(addr: u64) -> u64 {
    addr & !(PAGE_SIZE as u64 - 1)
}

fn align_up_page(addr: u64) -> u64 {
    align_down_page(addr + PAGE_SIZE as u64 - 1)
}

fn align_up_2mib(addr: u64) -> u64 {
    const HUGE: u64 = 2 * 1024 * 1024;
    (addr + HUGE - 1) & !(HUGE - 1)
}

/// Verify the tables we just installed behave as advertised.
pub fn self_test() {
    serial_println!("Verifying kernel page tables...");

    // The physmap must alias the identity region. Reading the kernel's first
    // bytes through both windows must give the same value.
    let kernel_start = align_down_page(sym(unsafe { &__kernel_start }));
    let via_identity = unsafe { core::ptr::read_volatile(kernel_start as *const u64) };
    let via_physmap =
        unsafe { core::ptr::read_volatile((PHYSMAP_BASE + kernel_start) as *const u64) };

    if via_identity == via_physmap {
        serial_println!("  physmap aliases identity map: OK (0x{:016x})", via_identity);
    } else {
        serial_println!(
            "  physmap MISMATCH: identity 0x{:x} vs physmap 0x{:x}",
            via_identity,
            via_physmap
        );
    }

    match translate(kernel_start) {
        Some(phys) if phys == kernel_start => {
            serial_println!("  translate(0x{:x}) -> 0x{:x}: OK", kernel_start, phys)
        }
        Some(phys) => serial_println!("  translate mismatch: 0x{:x}", phys),
        None => serial_println!("  translate FAILED: kernel not mapped?"),
    }

    if translate(0).is_none() {
        serial_println!("  page 0 unmapped: OK (null dereferences will fault)");
    } else {
        serial_println!("  WARNING: page 0 is mapped, null dereferences will NOT fault");
    }
}
