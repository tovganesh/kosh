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

/// Where the kernel image is linked. PML4 entry 511, PDPT entry 510 — the last
/// 2 GiB, which is the window `-C code-model=kernel` assumes.
///
/// The kernel is *loaded* at 1 MiB physical and *runs* at `KERNEL_VMA + 1 MiB`.
/// Anything that turns a linker symbol into a physical address goes through
/// [`kernel_phys`]; getting that wrong used to be impossible, because the two
/// were the same number.
pub const KERNEL_VMA: u64 = 0xFFFF_FFFF_8000_0000;

/// How much physical memory is reachable through the `KERNEL_VMA` window.
///
/// The trampoline maps a full 1 GiB there and [`init`] keeps a smaller slice, so
/// this is the bound during construction — before the physmap exists and while
/// `KERNEL_VMA + phys` is the only way to reach a fresh page-table frame.
pub const KERNEL_WINDOW_SIZE: u64 = 1024 * 1024 * 1024;

/// Physical address of a kernel-image virtual address.
///
/// Only valid for addresses inside the kernel window — linker symbols, the frame
/// bitmap, VGA. Use [`translate`] for anything else.
pub const fn kernel_phys(virt: u64) -> u64 {
    virt - KERNEL_VMA
}

/// Kernel-virtual address of a physical address in the low window.
///
/// The inverse of [`kernel_phys`], and usable *before* the physmap exists —
/// which is what makes it the right tool for the frame bitmap and for the VGA
/// buffer, both of which are touched before `paging::init` runs.
pub const fn kernel_virt(phys: u64) -> u64 {
    phys + KERNEL_VMA
}

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


/// How much physical memory `init` actually mapped into the kernel window.
///
/// The trampoline maps a full 1 GiB there; the real tables map only as far as
/// the frame bitmap needs, so reporting `KERNEL_WINDOW_SIZE` as the window would
/// overstate it by two orders of magnitude.
static KERNEL_WINDOW_END: Mutex<u64> = Mutex::new(0);

pub fn kernel_window_end() -> u64 {
    *KERNEL_WINDOW_END.lock()
}

/// The PML4 `init` built. Every user address space starts as a copy of its
/// higher half.
static KERNEL_PML4: Mutex<u64> = Mutex::new(0);

pub fn kernel_pml4_phys() -> u64 {
    *KERNEL_PML4.lock()
}

/// First PML4 index belonging to the kernel.
///
/// Entries 0..256 are the lower canonical half and belong entirely to whichever
/// process is running; 256..512 are the kernel's and are shared by every address
/// space. The split is the whole reason the higher-half migration happened.
pub const KERNEL_PML4_FIRST: usize = 256;

/// Borrow an arbitrary PML4 as a mapper, whatever CR3 currently holds.
///
/// This is what lets one process load a program into *another* process's
/// address space — `sys_spawn` runs in the parent and has to populate the
/// child. It works because the physmap is in the shared higher half, so every
/// address space can reach every physical frame, including another space's
/// page tables.
///
/// # Safety
/// `pml4_phys` must be a live PML4, and the caller must not hold two aliasing
/// mappers over it.
pub unsafe fn mapper_for(pml4_phys: u64) -> OffsetPageTable<'static> {
    let table: &'static mut PageTable = &mut *((PHYSMAP_BASE + pml4_phys) as *mut PageTable);
    OffsetPageTable::new(table, VirtAddr::new(PHYSMAP_BASE))
}

/// Marks a leaf mapping whose frame belongs to the address space and must be
/// freed when it is torn down.
///
/// `map_user_pages` allocates fresh frames and sets this; `map_user_range` maps
/// frames it does not own — the `.user` blob lives inside the kernel image — and
/// does not. Without the distinction, tearing down an address space that had run
/// the built-in payload would hand the kernel's own text back to the frame
/// allocator.
pub const OWNED_BY_ADDRESS_SPACE: PageTableFlags = PageTableFlags::BIT_9;

/// Marks a leaf that is shared copy-on-write.
///
/// The page is present and readable but **not** writable, and the frame's
/// reference count is above one. A write faults; `resolve_cow` gives the writer
/// a private copy (or, if it turns out to be the last holder, simply makes the
/// page writable again) and the instruction retries.
///
/// A separate bit from `WRITABLE` being clear, because a genuinely read-only
/// page — `.text`, `.rodata` — must keep faulting rather than being handed a
/// writable copy.
pub const COPY_ON_WRITE: PageTableFlags = PageTableFlags::BIT_10;

/// Give the current address space a private, writable copy of a shared page.
///
/// Returns `Ok(true)` if it did something, `Ok(false)` if the page was not
/// copy-on-write and the fault is real.
///
/// Two cases:
///
/// * **The frame has other holders.** Allocate one, copy 4 KiB through the
///   physmap, point the entry at the copy and drop a reference to the original.
/// * **This is the last holder.** Nothing to copy: clear the marker and restore
///   `WRITABLE`. Without this case a process that forks and whose child exits
///   would keep paying a copy for every page it writes, forever.
///
/// The TLB entry for a page that was cached read-only has to be invalidated
/// explicitly — the CPU will not notice the table changed underneath it.
/// Copy-on-write faults resolved since boot, and how many needed a real copy.
///
/// Logged because a copy-on-write implementation that never fires and one that
/// works look identical from outside: both produce a correct program. The
/// counters are the only evidence the mechanism is on the path at all.
static COW_RESOLVED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static COW_COPIED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn cow_stats() -> (u64, u64) {
    use core::sync::atomic::Ordering;
    (
        COW_RESOLVED.load(Ordering::Relaxed),
        COW_COPIED.load(Ordering::Relaxed),
    )
}

pub fn resolve_cow(virt: u64) -> Result<bool, &'static str> {
    use x86_64::instructions::tlb;
    use x86_64::structures::paging::mapper::TranslateResult;
    use x86_64::structures::paging::Translate;

    let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(virt));

    let (frame_phys, flags) = {
        let mapper = unsafe { active_mapper() };
        match mapper.translate(page.start_address()) {
            TranslateResult::Mapped { frame, flags, .. } => {
                (frame.start_address().as_u64(), flags)
            }
            _ => return Ok(false),
        }
    };

    if !flags.contains(COPY_ON_WRITE) {
        return Ok(false);
    }

    let old = PageFrame::from_address(frame_phys as usize);
    let writable_flags = (flags | PageTableFlags::WRITABLE) - COPY_ON_WRITE;

    // The count is read once and acted on with interrupts disabled, because two
    // threads faulting on the same shared page must not both conclude they are
    // the last holder.
    crate::interrupts::without_interrupts(|| {
        let refs = crate::memory::physical::frame_refs(old);

        if refs <= 1 {
            unsafe {
                let mut mapper = active_mapper();
                mapper
                    .update_flags(page, writable_flags)
                    .map_err(|_| "could not make the last copy writable")?
                    .flush();
            }
            COW_RESOLVED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            return Ok(true);
        }

        let copy = allocate_frame().ok_or("out of memory resolving a copy-on-write fault")?;
        let copy_phys = copy.address() as u64;

        unsafe {
            core::ptr::copy_nonoverlapping(
                (PHYSMAP_BASE + frame_phys) as *const u8,
                (PHYSMAP_BASE + copy_phys) as *mut u8,
                PAGE_SIZE,
            );

            let mut mapper = active_mapper();
            // `unmap` then `map_to` rather than editing in place: the crate has
            // no "repoint this entry" operation, and doing it by hand would mean
            // reimplementing the table walk.
            let (_, flush) = mapper.unmap(page).map_err(|_| "could not unmap a CoW page")?;
            flush.ignore();

            let table_flags = PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER_ACCESSIBLE;
            mapper
                .map_to_with_table_flags(
                    page,
                    PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(copy_phys)),
                    writable_flags,
                    table_flags,
                    &mut KoshFrameAllocator,
                )
                .map_err(|_| "could not map a CoW copy")?
                .flush();
        }

        // Only now: the original has one fewer holder.
        crate::memory::physical::deallocate_frame(old);
        tlb::flush(page.start_address());

        COW_RESOLVED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        COW_COPIED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        Ok(true)
    })
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

    // The physmap does not exist yet, so a freshly-allocated table frame has to
    // be reached through the window the trampoline set up: `KERNEL_VMA + phys`,
    // valid for the low 1 GiB. `BootstrapFrameAllocator` refuses anything above
    // that rather than handing back a pointer that faults on first write.
    let pml4_frame = allocate_frame().ok_or("no frame for PML4")?;
    let pml4_phys = pml4_frame.address() as u64;
    check_bootstrap_reachable(pml4_phys)?;
    let pml4: &mut PageTable = unsafe {
        let ptr = kernel_virt(pml4_phys) as *mut PageTable;
        ptr.write(PageTable::new());
        &mut *ptr
    };

    // The offset the mapper uses to reach the tables it is editing. During
    // construction that is the kernel window, not the physmap — the physmap is
    // one of the things being built.
    let mut mapper = unsafe { OffsetPageTable::new(pml4, VirtAddr::new(KERNEL_VMA)) };
    let mut frames = BootstrapFrameAllocator;

    map_kernel_image(&mut mapper, &mut frames)?;
    let window_end = map_kernel_window(&mut mapper, &mut frames)?;
    map_physical_memory(&mut mapper, &mut frames, phys_mem_end)?;

    // Install. From the next instruction onwards the kernel is running on
    // tables it built, with W^X enforced and a real physmap.
    //
    // Nothing about this instruction is delicate any more: RIP, RSP and every
    // static are higher-half addresses that the new tables map, so the switch is
    // invisible. Before the migration it worked only because the new tables
    // reproduced the bootstrap identity map exactly.
    unsafe {
        Cr3::write(
            PhysFrame::containing_address(PhysAddr::new(pml4_phys)),
            Cr3Flags::empty(),
        );
    }

    *KERNEL_WINDOW_END.lock() = window_end;
    *KERNEL_PML4.lock() = pml4_phys;

    serial_println!("  CR3 -> 0x{:x} (kernel page tables active)", pml4_phys);
    serial_println!(
        "  kernel window   : 0x{:x} -> 0x0..0x{:x} (4 KiB pages, W^X)",
        KERNEL_VMA,
        window_end
    );
    serial_println!(
        "  physmap         : 0x{:x} -> 0x0..0x{:x} (2 MiB pages, RW+NX)",
        PHYSMAP_BASE,
        phys_mem_end
    );
    serial_println!("  page 0          : unmapped (null guard)");
    serial_println!("  PML4[0]         : empty — reserved for user address spaces");

    Ok(())
}

/// Frame allocator for the window between "no page tables of our own" and "a
/// physmap". Every frame it hands out is about to be written through
/// `KERNEL_VMA + phys`, so a frame outside that window is not a subtle problem.
struct BootstrapFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for BootstrapFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let frame = allocate_frame()?;
        let phys = frame.address() as u64;
        if phys >= KERNEL_WINDOW_SIZE {
            // Returning None would surface as a vague "failed to map" further
            // up, so say what actually happened.
            serial_println!(
                "  FATAL: page-table frame at 0x{:x} is outside the 0x{:x} bootstrap window",
                phys,
                KERNEL_WINDOW_SIZE
            );
            return None;
        }
        Some(PhysFrame::containing_address(PhysAddr::new(phys)))
    }
}

fn check_bootstrap_reachable(phys: u64) -> Result<(), &'static str> {
    if phys >= KERNEL_WINDOW_SIZE {
        return Err("PML4 frame is outside the bootstrap kernel window");
    }
    Ok(())
}

/// Map the kernel image at [`KERNEL_VMA`] with per-section permissions.
///
/// `.text` loses write, `.rodata` loses execute and write, everything else
/// loses execute. The addresses here are already higher-half — they are the
/// linker symbols — and [`kernel_phys`] turns each one back into the frame GRUB
/// actually loaded it into.
fn map_kernel_image(
    mapper: &mut OffsetPageTable,
    frames: &mut BootstrapFrameAllocator,
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
        map_window_4k(mapper, frames, addr, flags)?;
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

/// Map the low physical region the kernel reaches by `KERNEL_VMA + phys`:
/// VGA at 0xB8000, the multiboot info GRUB left below 1 MiB, and the physical
/// frame bitmap.
///
/// This used to be `map_low_identity`, and it mapped the same frames at the same
/// numeric addresses in PML4[0]. The frames are identical; the virtual addresses
/// moved out of the half that belongs to userspace. That move is the whole
/// point of the migration — everything else here is unchanged.
///
/// Physical page 0 is skipped so that `KERNEL_VMA` itself is unmapped, the same
/// null guard the identity map had at address 0.
fn map_kernel_window(
    mapper: &mut OffsetPageTable,
    frames: &mut BootstrapFrameAllocator,
) -> Result<u64, &'static str> {
    let (_bitmap_start, bitmap_end) = bitmap_extent();
    let kernel_start = align_down_page(sym(unsafe { &__kernel_start }));
    let kernel_end = align_up_page(sym(unsafe { &__kernel_end }));

    // One 2 MiB of slack past the bitmap covers early page-table frames, which
    // the allocator hands out from just above it.
    let end = align_up_2mib(bitmap_end as u64 + 2 * 1024 * 1024);
    if end > KERNEL_WINDOW_SIZE {
        return Err("kernel window is not big enough for the frame bitmap");
    }

    let rw = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

    let mut phys = PAGE_SIZE as u64; // skip physical page 0: null guard
    while phys < end {
        let virt = kernel_virt(phys);
        // The kernel image was already mapped with tighter permissions.
        if virt >= kernel_start && virt < kernel_end {
            phys += PAGE_SIZE as u64;
            continue;
        }
        map_window_4k(mapper, frames, virt, rw)?;
        phys += PAGE_SIZE as u64;
    }

    Ok(end)
}

/// Map all of physical memory at [`PHYSMAP_BASE`] using 2 MiB pages.
fn map_physical_memory(
    mapper: &mut OffsetPageTable,
    frames: &mut BootstrapFrameAllocator,
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

/// Map one page of the `KERNEL_VMA` window to the frame it corresponds to.
///
/// `virt` is a higher-half address; the frame is `virt - KERNEL_VMA`. Before the
/// migration this function was `identity_map_4k` and the frame was `virt`
/// itself, which is the single line that made the kernel unmovable.
fn map_window_4k(
    mapper: &mut OffsetPageTable,
    frames: &mut BootstrapFrameAllocator,
    virt: u64,
    flags: PageTableFlags,
) -> Result<(), &'static str> {
    let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(virt));
    let frame: PhysFrame<Size4KiB> =
        PhysFrame::containing_address(PhysAddr::new(kernel_phys(virt)));

    unsafe {
        mapper
            .map_to(page, frame, flags, frames)
            .map_err(|_| "failed to map a kernel window page")?
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

/// Map `pages` of existing physical memory starting at `phys` to `virt`, with
/// `flags`, for ring-3 access.
///
/// Every table on the walk also needs `USER_ACCESSIBLE`, or the CPU stops at
/// the first supervisor-only level and the leaf flags never get consulted. That
/// is why this uses `map_to_with_table_flags` rather than plain `map_to`:
/// permissive parent entries, restrictive leaves. Kernel pages sharing those
/// tables stay private because *their* leaf entries lack the bit.
pub fn map_user_range(
    virt: u64,
    phys: u64,
    pages: usize,
    flags: PageTableFlags,
) -> Result<(), &'static str> {
    map_user_range_in(unsafe { active_mapper() }, virt, phys, pages, flags)
}

/// As [`map_user_range`], but into a named address space rather than the live
/// one. The frames are borrowed, not owned — see [`OWNED_BY_ADDRESS_SPACE`].
pub fn map_user_range_in(
    mut mapper: OffsetPageTable<'static>,
    virt: u64,
    phys: u64,
    pages: usize,
    flags: PageTableFlags,
) -> Result<(), &'static str> {
    let mut frames = KoshFrameAllocator;

    let table_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

    for i in 0..pages {
        let offset = (i * PAGE_SIZE) as u64;
        let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(virt + offset));
        let frame: PhysFrame<Size4KiB> =
            PhysFrame::containing_address(PhysAddr::new(phys + offset));

        unsafe {
            mapper
                .map_to_with_table_flags(page, frame, flags, table_flags, &mut frames)
                .map_err(|_| "failed to map user range")?
                .flush();
        }
    }

    Ok(())
}

/// Allocate `pages` fresh frames and map them at `virt` for ring-3 access.
pub fn map_user_pages(
    virt: u64,
    pages: usize,
    flags: PageTableFlags,
) -> Result<(), &'static str> {
    map_user_pages_in(unsafe { active_mapper() }, virt, pages, flags)
}

/// As [`map_user_pages`], but into a named address space.
///
/// The frames are freshly allocated, so the leaf entries are tagged
/// [`OWNED_BY_ADDRESS_SPACE`] and tearing the space down returns them.
pub fn map_user_pages_in(
    mut mapper: OffsetPageTable<'static>,
    virt: u64,
    pages: usize,
    flags: PageTableFlags,
) -> Result<(), &'static str> {
    let mut frames = KoshFrameAllocator;
    let flags = flags | OWNED_BY_ADDRESS_SPACE;

    let table_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

    for i in 0..pages {
        let frame = allocate_frame().ok_or("out of physical memory mapping user pages")?;

        // Zero it through the physmap before it is reachable from ring 3 —
        // handing a user process a page of somebody else's data is a textbook
        // information leak.
        unsafe {
            core::ptr::write_bytes(
                (PHYSMAP_BASE + frame.address() as u64) as *mut u8,
                0,
                PAGE_SIZE,
            );
        }

        let page: Page<Size4KiB> =
            Page::containing_address(VirtAddr::new(virt + (i * PAGE_SIZE) as u64));
        let phys: PhysFrame<Size4KiB> =
            PhysFrame::containing_address(PhysAddr::new(frame.address() as u64));

        unsafe {
            mapper
                .map_to_with_table_flags(page, phys, flags, table_flags, &mut frames)
                .map_err(|_| "failed to map user page")?
                .flush();
        }
    }

    Ok(())
}

/// Unmap `pages` user pages in a named address space and free their frames.
///
/// Kept as the surgical counterpart to [`map_user_pages_in`]. Whole-process
/// teardown does not come through here any more — `AddressSpace::free` walks the
/// lower half and frees everything tagged [`OWNED_BY_ADDRESS_SPACE`], which is
/// both cheaper and, unlike a recorded list of ranges, cannot go out of date.
///
/// Returns how many pages were actually unmapped; a page that was not mapped is
/// skipped rather than treated as an error.
///
/// # Safety
/// Nothing may still be using these addresses.
pub unsafe fn unmap_user_pages_in(
    mut mapper: OffsetPageTable<'static>,
    virt: u64,
    pages: usize,
) -> usize {
    let mut freed = 0;

    for i in 0..pages {
        let page: Page<Size4KiB> =
            Page::containing_address(VirtAddr::new(virt + (i * PAGE_SIZE) as u64));

        match mapper.unmap(page) {
            Ok((frame, flush)) => {
                flush.flush();
                crate::memory::physical::deallocate_frame(PageFrame::from_address(
                    frame.start_address().as_u64() as usize,
                ));
                freed += 1;
            }
            Err(_) => {}
        }
    }

    freed
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

/// Translate an address in a *named* address space rather than the live one.
///
/// The ELF loader needs this: it runs in the parent's address space and writes
/// segment bytes into the child's, so "which frame is this vaddr" has to be
/// asked of the child's tables.
pub fn translate_in(pml4_phys: u64, virt: u64) -> Option<u64> {
    use x86_64::structures::paging::Translate;
    let mapper = unsafe { mapper_for(pml4_phys) };
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

    // The physmap must alias the kernel window. Reading the kernel's first bytes
    // through both must give the same value.
    let kernel_start = align_down_page(sym(unsafe { &__kernel_start }));
    let kernel_phys_start = kernel_phys(kernel_start);

    let via_window = unsafe { core::ptr::read_volatile(kernel_start as *const u64) };
    let via_physmap =
        unsafe { core::ptr::read_volatile((PHYSMAP_BASE + kernel_phys_start) as *const u64) };

    if via_window == via_physmap {
        serial_println!("  physmap aliases identity map: OK (0x{:016x})", via_window);
    } else {
        serial_println!(
            "  physmap MISMATCH: window 0x{:x} vs physmap 0x{:x}",
            via_window,
            via_physmap
        );
    }

    match translate(kernel_start) {
        Some(phys) if phys == kernel_phys_start => serial_println!(
            "  translate(0x{:x}) -> 0x{:x}: OK",
            kernel_start,
            phys
        ),
        Some(phys) => serial_println!(
            "  translate mismatch: 0x{:x}, expected 0x{:x}",
            phys,
            kernel_phys_start
        ),
        None => serial_println!("  translate FAILED: kernel not mapped?"),
    }

    if translate(0).is_none() {
        serial_println!("  page 0 unmapped: OK (null dereferences will fault)");
    } else {
        serial_println!("  WARNING: page 0 is mapped, null dereferences will NOT fault");
    }

    // The point of the whole migration: the kernel is no longer anywhere in the
    // half of the address space a user process needs.
    //
    // Checking the *image* address specifically, rather than PML4[0] being
    // empty, because PML4[0] is exactly where user mappings live — it is
    // populated, just not by us.
    let low_kernel = translate(kernel_phys_start);
    if low_kernel.is_none() {
        serial_println!(
            "  kernel out of PML4[0]: OK (0x{:x} is unmapped, was the kernel image)",
            kernel_phys_start
        );
    } else {
        serial_println!(
            "  WARNING: 0x{:x} still maps to 0x{:x} — the low identity map survived",
            kernel_phys_start,
            low_kernel.unwrap()
        );
    }
}
