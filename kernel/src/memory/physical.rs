use multiboot2::{BootInformation, MemoryAreaType};
use spin::Mutex;
use crate::memory::{PAGE_SIZE, align_down, align_up};
use crate::{serial_println, println};

extern "C" {
    /// Start of the loaded kernel image (defined by linker.ld).
    static __kernel_start: u8;
    /// End of the loaded kernel image, including .bss (defined by linker.ld).
    static __kernel_end: u8;
}

/// Physical extent of the kernel image as actually loaded by the bootloader.
/// These frames must never be handed out by the allocator — the boot page
/// tables and the boot stack live in .bss, inside this range.
///
/// The linker symbols are higher-half addresses (the kernel is linked at
/// `KERNEL_VMA + 1 MiB` and loaded at 1 MiB), so each one is converted. Before
/// the migration this function returned the symbols unchanged and was correct
/// only because the two numbers were the same. Getting it wrong now means the
/// comparison at the bottom of `mark_available_frames` never matches and the
/// allocator hands out the running kernel's own frames.
fn kernel_image_range() -> (usize, usize) {
    use crate::memory::paging::kernel_phys;
    unsafe {
        (
            kernel_phys(&__kernel_start as *const u8 as u64) as usize,
            kernel_phys(&__kernel_end as *const u8 as u64) as usize,
        )
    }
}

/// Physical page frame number
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PageFrame(pub usize);

impl PageFrame {
    /// Create a new page frame from a physical address
    pub fn from_address(addr: usize) -> Self {
        PageFrame(align_down(addr) / PAGE_SIZE)
    }
    
    /// Get the physical address of this page frame
    pub fn address(&self) -> usize {
        self.0 * PAGE_SIZE
    }
    
    /// Get the next page frame
    #[allow(dead_code)]
    pub fn next(&self) -> Self {
        PageFrame(self.0 + 1)
    }
}

/// Memory statistics for tracking allocation
#[derive(Debug, Clone, Copy)]
pub struct MemoryStats {
    pub total_pages: usize,
    pub used_pages: usize,
    pub free_pages: usize,
    pub reserved_pages: usize,
}

impl MemoryStats {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            total_pages: 0,
            used_pages: 0,
            free_pages: 0,
            reserved_pages: 0,
        }
    }
    
    pub fn total_memory_mb(&self) -> usize {
        (self.total_pages * PAGE_SIZE) / (1024 * 1024)
    }
    
    pub fn used_memory_mb(&self) -> usize {
        (self.used_pages * PAGE_SIZE) / (1024 * 1024)
    }
    
    pub fn free_memory_mb(&self) -> usize {
        (self.free_pages * PAGE_SIZE) / (1024 * 1024)
    }
}

/// Bitmap-based physical memory allocator
pub struct PhysicalMemoryManager {
    /// Bitmap where each bit represents a page frame
    bitmap: &'static mut [u8],
    /// Total number of page frames
    total_frames: usize,
    /// Number of free frames
    free_frames: usize,
    /// Number of used frames
    used_frames: usize,
    /// Number of reserved frames (kernel, bootloader, etc.)
    reserved_frames: usize,
    /// Start of the bitmap in memory
    bitmap_start: usize,
    /// How many address spaces are using each frame.
    ///
    /// Copy-on-write is what makes this necessary: after a `fork` the same
    /// frame appears in two sets of page tables, and whichever process exits
    /// first must not hand it back to the allocator. `deallocate_frame`
    /// decrements; the frame is only really freed at zero.
    ///
    /// `u8` rather than `u16`: the sharers of a frame are address spaces, and
    /// the thread table holds 16. 255 is a long way past that, and an overflow
    /// is reported rather than wrapped.
    refcounts: &'static mut [u8],
}

impl PhysicalMemoryManager {
    /// Initialize the physical memory manager from multiboot2 information
    pub fn new(boot_info: &BootInformation) -> Result<Self, &'static str> {
        let memory_map = boot_info.memory_map_tag()
            .ok_or("No memory map found in multiboot2 info")?;
        
        // Find the largest usable memory area to place our bitmap
        let mut max_end_addr = 0;
        let mut _total_memory = 0;
        
        for area in memory_map.memory_areas() {
            // Check if memory area is available
            if area.typ() == MemoryAreaType::Available {
                max_end_addr = max_end_addr.max(area.end_address() as usize);
                _total_memory += area.size() as usize;
            }
        }
        
        if max_end_addr == 0 {
            return Err("No usable memory found");
        }
        
        // Calculate total number of page frames
        let total_frames = max_end_addr / PAGE_SIZE;
        
        // Calculate bitmap size (1 bit per page frame)
        let bitmap_size = (total_frames + 7) / 8; // Round up to nearest byte
        // ...and the refcount table, one byte per frame, immediately after it.
        let refcount_size = total_frames;
        let metadata_size = bitmap_size + refcount_size;
        
        // Place the metadata after the kernel image *and* after every boot
        // module.
        //
        // "After the kernel image" was the rule until the refcount table
        // arrived, and it was correct only because the bitmap was small: 16 KiB
        // fits in the gap GRUB leaves before the first module. The refcount
        // table is one byte per frame — 128 KiB for 512 MiB of RAM — which
        // reaches straight into the module GRUB loaded at 0x195000. The symptom
        // was both modules reading as all zeros and the ELF loader reporting
        // `BadMagic`, several subsystems away from the cause.
        //
        // A hardcoded 2 MiB was the rule before *that*, and failed the same way
        // once the kernel grew. Computing the bound is the only version that
        // does not have a size at which it silently breaks.
        let (_kernel_start, kernel_end) = kernel_image_range();
        let mut metadata_after = kernel_end;
        for module in boot_info.module_tags() {
            metadata_after = metadata_after.max(module.end_address() as usize);
        }
        let bitmap_start = align_up(metadata_after);
        let bitmap_end = bitmap_start + metadata_size;
        
        // Ensure the metadata doesn't overlap with any reserved areas
        for area in memory_map.memory_areas() {
            if area.typ() != MemoryAreaType::Available {
                let area_start = area.start_address() as usize;
                let area_end = area.end_address() as usize;
                
                if bitmap_start < area_end && bitmap_end > area_start {
                    return Err("Cannot place the frame metadata - overlaps with reserved memory");
                }
            }
        }
        
        // The bitmap lives at a physical address, but it has to be *written*
        // through a virtual one — and this runs before `paging::init`, so the
        // physmap does not exist yet. `KERNEL_VMA + phys` is the window the boot
        // trampoline set up, and `paging::init` deliberately keeps it, so this
        // pointer stays valid for the life of the kernel and needs no rebasing
        // when the low identity map goes away.
        let bitmap = unsafe {
            core::slice::from_raw_parts_mut(
                crate::memory::paging::kernel_virt(bitmap_start as u64) as *mut u8,
                bitmap_size,
            )
        };
        let refcounts = unsafe {
            core::slice::from_raw_parts_mut(
                crate::memory::paging::kernel_virt((bitmap_start + bitmap_size) as u64) as *mut u8,
                refcount_size,
            )
        };
        
        // Clear the bitmap (all pages initially marked as used)
        bitmap.fill(0xFF);
        // Nobody holds a reference to anything yet. Frames the allocator never
        // hands out — the kernel image, this table, low memory — stay at zero
        // for the life of the system, which is what makes an accidental
        // `deallocate_frame` on one of them detectable rather than silent.
        refcounts.fill(0);
        
        serial_println!(
            "  frame metadata  : bitmap {} bytes + refcounts {} bytes at 0x{:x}",
            bitmap_size,
            refcount_size,
            bitmap_start
        );
        
        // The bitmap is filled with 0xFF above, i.e. every frame starts out
        // marked used. The counters have to agree with that, otherwise the
        // first `mark_frame_free` underflows `used_frames` (panicking in debug,
        // wrapping to ~u64::MAX in release and corrupting every memory stat).
        let mut manager = Self {
            bitmap,
            total_frames,
            free_frames: 0,
            used_frames: total_frames,
            reserved_frames: 0,
            bitmap_start,
            refcounts,
        };
        
        // Mark available memory areas as free
        manager.parse_memory_map(&memory_map)?;

        // Boot modules live in RAM that the memory map reports as *available*,
        // because from the firmware's point of view it is. If we do not claim
        // them here, the first allocation lands on top of the init binary we
        // are about to load.
        manager.reserve_boot_modules(boot_info);
        
        serial_println!("Physical memory manager initialized:");
        serial_println!("  Total frames: {}", manager.total_frames);
        serial_println!("  Free frames: {}", manager.free_frames);
        serial_println!("  Used frames: {}", manager.used_frames);
        serial_println!("  Reserved frames: {}", manager.reserved_frames);
        serial_println!("  Bitmap at: 0x{:x} (size: {} bytes)", bitmap_start, bitmap_size);
        
        Ok(manager)
    }
    
    /// Parse memory map and mark available areas as free
    fn parse_memory_map(&mut self, memory_map: &multiboot2::MemoryMapTag) -> Result<(), &'static str> {
        for area in memory_map.memory_areas() {
            let start_addr = area.start_address() as usize;
            let end_addr = area.end_address() as usize;
            let start_frame = PageFrame::from_address(start_addr);
            let end_frame = PageFrame::from_address(end_addr - 1);
            
            let (kernel_start, kernel_end) = kernel_image_range();

            if area.typ() == MemoryAreaType::Available {
                // Mark pages as free, but avoid low memory, the kernel image
                // and the bitmap itself.
                for frame_num in start_frame.0..=end_frame.0 {
                    let frame_addr = frame_num * PAGE_SIZE;
                    
                    // Skip low memory (first 1MB), the kernel image (which
                    // includes the boot page tables and boot stack in .bss),
                    // and the frame bitmap.
                    if frame_addr < 0x100000
                        || (frame_addr >= kernel_start && frame_addr < kernel_end)
                        || (frame_addr >= self.bitmap_start
                            && frame_addr < self.metadata_end())
                    {
                        self.reserved_frames += 1;
                        continue;
                    }
                    
                    if frame_num < self.total_frames {
                        self.mark_frame_free(PageFrame(frame_num));
                    }
                }
            } else {
                // Mark non-available areas as reserved
                for frame_num in start_frame.0..=end_frame.0 {
                    if frame_num < self.total_frames {
                        self.reserved_frames += 1;
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Mark every frame holding a multiboot2 boot module as used.
    fn reserve_boot_modules(&mut self, boot_info: &BootInformation) {
        for module in boot_info.module_tags() {
            let start = module.start_address() as usize;
            let end = module.end_address() as usize;

            let first = PageFrame::from_address(start);
            let last = PageFrame::from_address(end.saturating_sub(1));

            let mut count = 0usize;
            for frame_num in first.0..=last.0 {
                if frame_num < self.total_frames {
                    self.mark_frame_used(PageFrame(frame_num));
                    count += 1;
                }
            }

            serial_println!(
                "  reserved boot module 0x{:x}..0x{:x} ({} frames)",
                start,
                end,
                count
            );
        }
    }

    /// Allocate a single page frame
    pub fn allocate_frame(&mut self) -> Option<PageFrame> {
        if self.free_frames == 0 {
            return None;
        }
        
        // Find first free frame
        for byte_idx in 0..self.bitmap.len() {
            let byte = self.bitmap[byte_idx];
            if byte != 0xFF {
                // Found a byte with at least one free bit
                for bit_idx in 0..8 {
                    if (byte & (1 << bit_idx)) == 0 {
                        // Found free frame
                        let frame_num = byte_idx * 8 + bit_idx;
                        if frame_num < self.total_frames {
                            let frame = PageFrame(frame_num);
                            self.mark_frame_used(frame);
                            // One owner: whoever asked for it.
                            self.refcounts[frame_num] = 1;
                            return Some(frame);
                        }
                    }
                }
            }
        }
        
        None
    }
    
    /// Allocate multiple contiguous page frames
    pub fn allocate_frames(&mut self, count: usize) -> Option<PageFrame> {
        if count == 0 || self.free_frames < count {
            return None;
        }
        
        if count == 1 {
            return self.allocate_frame();
        }
        
        // Find contiguous free frames
        let mut consecutive_free = 0;
        let mut start_frame = None;
        
        for frame_num in 0..self.total_frames {
            if self.is_frame_free(PageFrame(frame_num)) {
                if consecutive_free == 0 {
                    start_frame = Some(PageFrame(frame_num));
                }
                consecutive_free += 1;
                
                if consecutive_free == count {
                    // Found enough contiguous frames
                    let start = start_frame.unwrap();
                    for i in 0..count {
                        self.mark_frame_used(PageFrame(start.0 + i));
                    }
                    return Some(start);
                }
            } else {
                consecutive_free = 0;
                start_frame = None;
            }
        }
        
        None
    }
    
    /// Deallocate a page frame
    /// Drop one reference to a frame, freeing it when the last one goes.
    ///
    /// Before copy-on-write this unconditionally marked the frame free, which
    /// was correct because a frame had exactly one owner. After `fork`, two
    /// address spaces can hold the same frame, and whichever process exits
    /// first would otherwise hand a page the other is still reading back to the
    /// allocator.
    pub fn deallocate_frame(&mut self, frame: PageFrame) {
        if frame.0 >= self.total_frames {
            return;
        }
        
        if self.is_frame_free(frame) {
            // Already free, this might indicate a double-free bug
            serial_println!("Warning: Attempted to free already free frame {}", frame.0);
            return;
        }

        match self.refcounts[frame.0] {
            0 => {
                // A frame the allocator never handed out: the kernel image, the
                // metadata tables, low memory. Freeing one is a bug in the
                // caller, and marking it free would put the running kernel's own
                // pages on the free list.
                serial_println!(
                    "Warning: refusing to free frame {} (0x{:x}) — it was never allocated",
                    frame.0,
                    frame.address()
                );
                return;
            }
            1 => {
                self.refcounts[frame.0] = 0;
                self.mark_frame_free(frame);
            }
            n => {
                self.refcounts[frame.0] = n - 1;
            }
        }
    }

    /// Take another reference to an already-allocated frame.
    ///
    /// Returns the new count, or `None` if the frame was not allocated or the
    /// count would overflow.
    pub fn share_frame(&mut self, frame: PageFrame) -> Option<u8> {
        if frame.0 >= self.total_frames {
            return None;
        }
        let current = self.refcounts[frame.0];
        if current == 0 || current == u8::MAX {
            return None;
        }
        self.refcounts[frame.0] = current + 1;
        Some(current + 1)
    }

    /// End of the frame metadata: the bitmap *and* the refcount table.
    ///
    /// The reservation loop used `bitmap_start + bitmap.len()`, which was right
    /// while the bitmap was the only metadata. Adding the refcount table without
    /// this made the allocator hand out the frames holding the refcount table,
    /// which then filled with page data — and `shared_frames()` reported 112,969
    /// frames shared after a fork of 20 pages, which is how it was noticed.
    fn metadata_end(&self) -> usize {
        self.bitmap_start + self.bitmap.len() + self.refcounts.len()
    }

    /// How many address spaces hold this frame. 0 means the allocator does not
    /// own it.
    pub fn frame_refs(&self, frame: PageFrame) -> u8 {
        if frame.0 >= self.total_frames {
            return 0;
        }
        self.refcounts[frame.0]
    }

    /// How many frames currently have more than one holder. Diagnostics — it is
    /// the number that shows copy-on-write is doing anything.
    pub fn shared_frames(&self) -> usize {
        self.refcounts.iter().filter(|&&c| c > 1).count()
    }
    
    /// Deallocate multiple contiguous page frames
    pub fn deallocate_frames(&mut self, start_frame: PageFrame, count: usize) {
        for i in 0..count {
            self.deallocate_frame(PageFrame(start_frame.0 + i));
        }
    }
    
    /// Check if a frame is free
    pub fn is_frame_free(&self, frame: PageFrame) -> bool {
        if frame.0 >= self.total_frames {
            return false;
        }
        
        let byte_idx = frame.0 / 8;
        let bit_idx = frame.0 % 8;
        
        if byte_idx >= self.bitmap.len() {
            return false;
        }
        
        (self.bitmap[byte_idx] & (1 << bit_idx)) == 0
    }
    
    /// Mark a frame as used
    fn mark_frame_used(&mut self, frame: PageFrame) {
        if frame.0 >= self.total_frames {
            return;
        }
        
        let byte_idx = frame.0 / 8;
        let bit_idx = frame.0 % 8;
        
        if byte_idx >= self.bitmap.len() {
            return;
        }
        
        if (self.bitmap[byte_idx] & (1 << bit_idx)) == 0 {
            // Frame was free, now marking as used
            self.bitmap[byte_idx] |= 1 << bit_idx;
            self.free_frames -= 1;
            self.used_frames += 1;
        }
    }
    
    /// Mark a frame as free
    fn mark_frame_free(&mut self, frame: PageFrame) {
        if frame.0 >= self.total_frames {
            return;
        }
        
        let byte_idx = frame.0 / 8;
        let bit_idx = frame.0 % 8;
        
        if byte_idx >= self.bitmap.len() {
            return;
        }
        
        if (self.bitmap[byte_idx] & (1 << bit_idx)) != 0 {
            // Frame was used, now marking as free
            self.bitmap[byte_idx] &= !(1 << bit_idx);
            self.used_frames -= 1;
            self.free_frames += 1;
        }
    }
    
    /// Get memory statistics
    pub fn stats(&self) -> MemoryStats {
        MemoryStats {
            total_pages: self.total_frames,
            used_pages: self.used_frames,
            free_pages: self.free_frames,
            reserved_pages: self.reserved_frames,
        }
    }
    
    /// Print memory statistics
    pub fn print_stats(&self) {
        let stats = self.stats();
        serial_println!("Memory Statistics:");
        serial_println!("  Total: {} MB ({} pages)", stats.total_memory_mb(), stats.total_pages);
        serial_println!("  Used:  {} MB ({} pages)", stats.used_memory_mb(), stats.used_pages);
        serial_println!("  Free:  {} MB ({} pages)", stats.free_memory_mb(), stats.free_pages);
        serial_println!("  Reserved: {} pages", stats.reserved_pages);
        
        println!("Memory: {} MB total, {} MB free, {} MB used", 
                stats.total_memory_mb(), stats.free_memory_mb(), stats.used_memory_mb());
    }
}

/// Global physical memory manager instance
static PHYSICAL_MEMORY_MANAGER: Mutex<Option<PhysicalMemoryManager>> = Mutex::new(None);

/// Initialize the global physical memory manager
pub fn init_physical_memory(boot_info: &BootInformation) -> Result<(), &'static str> {
    let manager = PhysicalMemoryManager::new(boot_info)?;
    manager.print_stats();
    
    *PHYSICAL_MEMORY_MANAGER.lock() = Some(manager);
    
    serial_println!("Physical memory manager initialized successfully");
    Ok(())
}

/// Allocate a single page frame
pub fn allocate_frame() -> Option<PageFrame> {
    PHYSICAL_MEMORY_MANAGER.lock().as_mut()?.allocate_frame()
}

/// Allocate multiple contiguous page frames
pub fn allocate_frames(count: usize) -> Option<PageFrame> {
    PHYSICAL_MEMORY_MANAGER.lock().as_mut()?.allocate_frames(count)
}

/// Deallocate a page frame
/// Take another reference to a frame. See
/// [`PhysicalMemoryManager::share_frame`].
pub fn share_frame(frame: PageFrame) -> Option<u8> {
    PHYSICAL_MEMORY_MANAGER.lock().as_mut()?.share_frame(frame)
}

/// How many address spaces hold this frame.
pub fn frame_refs(frame: PageFrame) -> u8 {
    PHYSICAL_MEMORY_MANAGER
        .lock()
        .as_ref()
        .map(|m| m.frame_refs(frame))
        .unwrap_or(0)
}

/// Frames held by more than one address space.
pub fn shared_frames() -> usize {
    PHYSICAL_MEMORY_MANAGER
        .lock()
        .as_ref()
        .map(|m| m.shared_frames())
        .unwrap_or(0)
}

pub fn deallocate_frame(frame: PageFrame) {
    if let Some(manager) = PHYSICAL_MEMORY_MANAGER.lock().as_mut() {
        manager.deallocate_frame(frame);
    }
}

/// Deallocate multiple contiguous page frames
pub fn deallocate_frames(start_frame: PageFrame, count: usize) {
    if let Some(manager) = PHYSICAL_MEMORY_MANAGER.lock().as_mut() {
        manager.deallocate_frames(start_frame, count);
    }
}

/// Physical extent of the frame bitmap, as (start, end).
///
/// The bitmap is accessed by physical address, so whoever builds the kernel
/// page tables has to keep it reachable.
pub fn bitmap_extent() -> (usize, usize) {
    let guard = PHYSICAL_MEMORY_MANAGER.lock();
    match guard.as_ref() {
        Some(m) => (
            m.bitmap_start,
            m.bitmap_start + m.bitmap.len() + m.refcounts.len(),
        ),
        None => (0, 0),
    }
}

/// End of usable physical memory as reported by the bootloader.
pub fn physical_memory_end() -> u64 {
    let guard = PHYSICAL_MEMORY_MANAGER.lock();
    match guard.as_ref() {
        Some(m) => (m.total_frames * PAGE_SIZE) as u64,
        None => 0,
    }
}

/// Get memory statistics
#[allow(dead_code)]
pub fn memory_stats() -> Option<MemoryStats> {
    PHYSICAL_MEMORY_MANAGER.lock().as_ref().map(|m| m.stats())
}

/// Print memory statistics
pub fn print_memory_stats() {
    if let Some(manager) = PHYSICAL_MEMORY_MANAGER.lock().as_ref() {
        manager.print_stats();
    }
}