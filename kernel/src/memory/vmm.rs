use crate::memory::{PAGE_SIZE, align_down, align_up};
use crate::memory::physical::{PageFrame, allocate_frame, deallocate_frame};
use crate::{serial_println, println};
use spin::Mutex;
use x86_64::structures::paging::{
    PageTable, PageTableFlags, PhysFrame, Page, Size4KiB,
    FrameAllocator, FrameDeallocator, Mapper, OffsetPageTable, Translate,
    mapper::{MapToError, UnmapError}
};
use x86_64::{VirtAddr, PhysAddr};
use alloc::vec::Vec;

/// Virtual address type for better type safety
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtualAddress(pub usize);

impl VirtualAddress {
    /// Create a new virtual address
    pub fn new(addr: usize) -> Self {
        VirtualAddress(addr)
    }
    
    /// Get the raw address value
    pub fn as_usize(&self) -> usize {
        self.0
    }
    
    /// Convert to x86_64 VirtAddr
    pub fn as_virt_addr(&self) -> VirtAddr {
        VirtAddr::new(self.0 as u64)
    }
    
    /// Align address up to page boundary
    pub fn align_up(&self) -> Self {
        VirtualAddress(align_up(self.0))
    }
    
    /// Align address down to page boundary
    pub fn align_down(&self) -> Self {
        VirtualAddress(align_down(self.0))
    }
    
    /// Check if address is page-aligned
    pub fn is_aligned(&self) -> bool {
        self.0 & (PAGE_SIZE - 1) == 0
    }
}

/// Memory protection flags for virtual memory regions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryProtection {
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    pub user_accessible: bool,
}

impl MemoryProtection {
    /// Create read-only protection
    pub const fn read_only() -> Self {
        Self {
            readable: true,
            writable: false,
            executable: false,
            user_accessible: false,
        }
    }
    
    /// Create read-write protection
    pub const fn read_write() -> Self {
        Self {
            readable: true,
            writable: true,
            executable: false,
            user_accessible: false,
        }
    }
    
    /// Create read-execute protection
    pub const fn read_execute() -> Self {
        Self {
            readable: true,
            writable: false,
            executable: true,
            user_accessible: false,
        }
    }
    
    /// Create user-accessible read-write protection
    pub const fn user_read_write() -> Self {
        Self {
            readable: true,
            writable: true,
            executable: false,
            user_accessible: true,
        }
    }
    
    /// Convert to x86_64 page table flags
    pub fn to_page_table_flags(&self) -> PageTableFlags {
        let mut flags = PageTableFlags::PRESENT;
        
        if self.writable {
            flags |= PageTableFlags::WRITABLE;
        }
        
        if !self.executable {
            flags |= PageTableFlags::NO_EXECUTE;
        }
        
        if self.user_accessible {
            flags |= PageTableFlags::USER_ACCESSIBLE;
        }
        
        flags
    }
}

/// Virtual memory region descriptor
#[derive(Debug, Clone)]
pub struct VirtualMemoryRegion {
    pub start: VirtualAddress,
    pub size: usize,
    pub protection: MemoryProtection,
    pub name: &'static str,
}

impl VirtualMemoryRegion {
    /// Create a new virtual memory region
    pub fn new(start: VirtualAddress, size: usize, protection: MemoryProtection, name: &'static str) -> Self {
        Self {
            start,
            size,
            protection,
            name,
        }
    }
    
    /// Get the end address of the region
    pub fn end(&self) -> VirtualAddress {
        VirtualAddress(self.start.0 + self.size)
    }
    
    /// Check if an address is within this region
    pub fn contains(&self, addr: VirtualAddress) -> bool {
        addr.0 >= self.start.0 && addr.0 < self.start.0 + self.size
    }
    
    /// Get the number of pages in this region
    pub fn page_count(&self) -> usize {
        (self.size + PAGE_SIZE - 1) / PAGE_SIZE
    }
}

/// Frame allocator wrapper for x86_64 crate
#[derive(Debug)]
pub struct KoshFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for KoshFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        allocate_frame().map(|frame| {
            let phys_addr = PhysAddr::new(frame.address() as u64);
            PhysFrame::containing_address(phys_addr)
        })
    }
}

impl FrameDeallocator<Size4KiB> for KoshFrameAllocator {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        let phys_addr = frame.start_address().as_u64() as usize;
        let page_frame = PageFrame::from_address(phys_addr);
        deallocate_frame(page_frame);
    }
}

/// Virtual address space abstraction
pub struct VirtualAddressSpace {
    /// Page table mapper
    mapper: OffsetPageTable<'static>,
    /// Frame allocator
    frame_allocator: KoshFrameAllocator,
    /// Virtual memory regions
    regions: Vec<VirtualMemoryRegion>,
    /// Physical memory offset for higher half kernel
    physical_memory_offset: VirtAddr,
}

impl core::fmt::Debug for VirtualAddressSpace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VirtualAddressSpace")
            .field("frame_allocator", &self.frame_allocator)
            .field("regions", &self.regions)
            .field("physical_memory_offset", &self.physical_memory_offset)
            .finish()
    }
}

impl VirtualAddressSpace {
    /// Create a new virtual address space
    pub unsafe fn new(level_4_table: &'static mut PageTable, physical_memory_offset: VirtAddr) -> Self {
        let mapper = OffsetPageTable::new(level_4_table, physical_memory_offset);
        
        Self {
            mapper,
            frame_allocator: KoshFrameAllocator,
            regions: Vec::new(),
            physical_memory_offset,
        }
    }
    
    /// Map a virtual page to a physical frame
    pub fn map_page(&mut self, virt_addr: VirtualAddress, phys_frame: PageFrame, protection: MemoryProtection) -> Result<(), MapToError<Size4KiB>> {
        let page: Page<Size4KiB> = Page::containing_address(virt_addr.as_virt_addr());
        let frame = PhysFrame::containing_address(PhysAddr::new(phys_frame.address() as u64));
        let flags = protection.to_page_table_flags();
        
        unsafe {
            self.mapper.map_to(page, frame, flags, &mut self.frame_allocator)?.flush();
        }
        
        Ok(())
    }
    
    /// Map a virtual address range to physical frames
    pub fn map_range(&mut self, virt_start: VirtualAddress, phys_start: usize, size: usize, protection: MemoryProtection) -> Result<(), MapToError<Size4KiB>> {
        let page_count = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        
        for i in 0..page_count {
            let virt_addr = VirtualAddress(virt_start.0 + i * PAGE_SIZE);
            let phys_addr = phys_start + i * PAGE_SIZE;
            let phys_frame = PageFrame::from_address(phys_addr);
            
            self.map_page(virt_addr, phys_frame, protection)?;
        }
        
        Ok(())
    }
    
    /// Unmap a virtual page
    pub fn unmap_page(&mut self, virt_addr: VirtualAddress) -> Result<(), UnmapError> {
        let page: Page<Size4KiB> = Page::containing_address(virt_addr.as_virt_addr());
        let (_, flush) = self.mapper.unmap(page)?;
        flush.flush();
        Ok(())
    }
    
    /// Unmap a virtual address range
    pub fn unmap_range(&mut self, virt_start: VirtualAddress, size: usize) -> Result<(), UnmapError> {
        let page_count = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        
        for i in 0..page_count {
            let virt_addr = VirtualAddress(virt_start.0 + i * PAGE_SIZE);
            self.unmap_page(virt_addr)?;
        }
        
        Ok(())
    }
    
    /// Add a virtual memory region
    pub fn add_region(&mut self, region: VirtualMemoryRegion) {
        self.regions.push(region);
    }
    
    /// Find a region containing the given address
    pub fn find_region(&self, addr: VirtualAddress) -> Option<&VirtualMemoryRegion> {
        self.regions.iter().find(|region| region.contains(addr))
    }
    
    /// Get all regions
    pub fn regions(&self) -> &[VirtualMemoryRegion] {
        &self.regions
    }
    
    /// Translate virtual address to physical address
    pub fn translate(&self, virt_addr: VirtualAddress) -> Option<PhysAddr> {
        self.mapper.translate_addr(virt_addr.as_virt_addr())
    }
    
    /// Check if a virtual address is mapped
    pub fn is_mapped(&self, virt_addr: VirtualAddress) -> bool {
        self.translate(virt_addr).is_some()
    }
}

/// Kernel virtual memory layout constants.
///
/// These used to be three invented numbers — code at `KERNEL_VMA`, data 16 MiB
/// above it, heap 16 MiB above that — describing a layout nothing had ever
/// built. They were printed in the boot log next to the real one, which is
/// worse than not printing them at all.
///
/// They are derived from `memory::paging` now, so the layout this module reports
/// is the layout the page tables actually have. `KERNEL_CODE_START` finally
/// being correct is not a coincidence: the higher-half migration moved the
/// kernel to the address this constant had been claiming all along.
pub mod kernel_layout {
    use super::VirtualAddress;

    /// Base of the kernel image window. The image itself starts 1 MiB in;
    /// the first page is left unmapped as the null guard.
    pub const KERNEL_CODE_START: VirtualAddress =
        VirtualAddress(crate::memory::paging::KERNEL_VMA as usize);

    /// Upper bound on the kernel window. The amount `paging::init` actually
    /// maps is smaller and only known at run time — see
    /// `paging::kernel_window_end`, which is what the layout dump reports.
    pub const KERNEL_CODE_SIZE: usize = crate::memory::paging::KERNEL_WINDOW_SIZE as usize;

    /// Kernel heap start address — the window `memory::heap` actually maps.
    pub const KERNEL_HEAP_START: VirtualAddress =
        VirtualAddress(crate::memory::paging::KERNEL_HEAP_BASE as usize);

    /// Kernel heap size. Matches `boot::HEAP_SIZE_PAGES` worth of 4 KiB pages.
    pub const KERNEL_HEAP_SIZE: usize = 1024 * 4096;
    
    /// Physical memory mapping start (for higher half kernel)
    /// Offset at which all of physical memory is visible in the virtual
    /// address space.
    ///
    /// Phase 3: `memory::paging` maps all of physical memory here when it
    /// builds the kernel page tables, so this offset is now backed by real
    /// mappings. (Through Phase 2 it had to be 0 — the constant claimed a
    /// higher-half physmap that nothing had ever created, so using it was an
    /// immediate page fault.)
    pub const PHYSICAL_MEMORY_OFFSET: VirtualAddress =
        VirtualAddress(crate::memory::paging::PHYSMAP_BASE as usize);
}

/// Global virtual memory manager
static VIRTUAL_MEMORY_MANAGER: Mutex<Option<VirtualAddressSpace>> = Mutex::new(None);

/// Initialize virtual memory management
pub unsafe fn init_virtual_memory() -> Result<(), &'static str> {
    serial_println!("Initializing virtual memory management...");
    
    // Get the current level 4 page table
    let level_4_table = get_level_4_page_table();
    
    // Create virtual address space with physical memory offset
    let physical_memory_offset = kernel_layout::PHYSICAL_MEMORY_OFFSET.as_virt_addr();
    let mut vas = VirtualAddressSpace::new(level_4_table, physical_memory_offset);
    
    // Set up kernel virtual memory regions
    setup_kernel_memory_layout(&mut vas)?;
    
    // Store the virtual address space globally
    *VIRTUAL_MEMORY_MANAGER.lock() = Some(vas);
    
    serial_println!("Virtual memory management initialized successfully");
    Ok(())
}

/// Get the current level 4 page table
unsafe fn get_level_4_page_table() -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;
    
    let (level_4_table_frame, _) = Cr3::read();
    let phys_addr = level_4_table_frame.start_address();
    let virt_addr = kernel_layout::PHYSICAL_MEMORY_OFFSET.0 + phys_addr.as_u64() as usize;
    let page_table_ptr: *mut PageTable = virt_addr as *mut PageTable;
    
    &mut *page_table_ptr
}

/// Set up kernel virtual memory layout
fn setup_kernel_memory_layout(vas: &mut VirtualAddressSpace) -> Result<(), &'static str> {
    serial_println!("Setting up kernel virtual memory layout...");
    
    // The kernel image window. One region rather than separate code and data
    // ones: the image is a single contiguous span whose per-section permissions
    // live in the page tables, not in a table of constants.
    let kernel_code_region = VirtualMemoryRegion::new(
        kernel_layout::KERNEL_CODE_START,
        crate::memory::paging::kernel_window_end() as usize,
        MemoryProtection::read_execute(),
        "Kernel Window"
    );
    vas.add_region(kernel_code_region);

    // The physmap, which was missing from this table entirely even though it is
    // the largest mapping the kernel has.
    let physmap_region = VirtualMemoryRegion::new(
        kernel_layout::PHYSICAL_MEMORY_OFFSET,
        crate::memory::physical::physical_memory_end() as usize,
        MemoryProtection::read_write(),
        "Physmap"
    );
    vas.add_region(physmap_region);

    // Add kernel heap region
    let kernel_heap_region = VirtualMemoryRegion::new(
        kernel_layout::KERNEL_HEAP_START,
        kernel_layout::KERNEL_HEAP_SIZE,
        MemoryProtection::read_write(),
        "Kernel Heap"
    );
    vas.add_region(kernel_heap_region);
    
    serial_println!("Kernel virtual memory layout configured");
    print_memory_layout(vas);
    
    Ok(())
}

/// Print the current virtual memory layout
pub fn print_memory_layout(vas: &VirtualAddressSpace) {
    serial_println!("Virtual Memory Layout:");
    
    for region in vas.regions() {
        serial_println!(
            "  {}: 0x{:016x} - 0x{:016x} ({} KB) [{}{}{}{}]",
            region.name,
            region.start.0,
            region.end().0,
            region.size / 1024,
            if region.protection.readable { "R" } else { "-" },
            if region.protection.writable { "W" } else { "-" },
            if region.protection.executable { "X" } else { "-" },
            if region.protection.user_accessible { "U" } else { "K" }
        );
    }
    
    println!("Virtual memory layout configured with {} regions", vas.regions().len());
}

/// Map a virtual address to a physical address with protection
pub fn map_virtual_to_physical(virt_addr: VirtualAddress, phys_addr: usize, protection: MemoryProtection) -> Result<(), &'static str> {
    let mut manager = VIRTUAL_MEMORY_MANAGER.lock();
    let vas = manager.as_mut().ok_or("Virtual memory manager not initialized")?;
    
    let phys_frame = PageFrame::from_address(phys_addr);
    vas.map_page(virt_addr, phys_frame, protection)
        .map_err(|_| "Failed to map virtual page")?;
    
    Ok(())
}

/// Map a virtual address range to physical addresses
pub fn map_virtual_range(virt_start: VirtualAddress, phys_start: usize, size: usize, protection: MemoryProtection) -> Result<(), &'static str> {
    let mut manager = VIRTUAL_MEMORY_MANAGER.lock();
    let vas = manager.as_mut().ok_or("Virtual memory manager not initialized")?;
    
    vas.map_range(virt_start, phys_start, size, protection)
        .map_err(|_| "Failed to map virtual range")?;
    
    Ok(())
}

/// Unmap a virtual address
pub fn unmap_virtual_address(virt_addr: VirtualAddress) -> Result<(), &'static str> {
    let mut manager = VIRTUAL_MEMORY_MANAGER.lock();
    let vas = manager.as_mut().ok_or("Virtual memory manager not initialized")?;
    
    vas.unmap_page(virt_addr)
        .map_err(|_| "Failed to unmap virtual page")?;
    
    Ok(())
}

/// Translate virtual address to physical address
pub fn translate_virtual_address(virt_addr: VirtualAddress) -> Option<usize> {
    let manager = VIRTUAL_MEMORY_MANAGER.lock();
    let vas = manager.as_ref()?;
    
    vas.translate(virt_addr).map(|phys_addr| phys_addr.as_u64() as usize)
}

/// Check if a virtual address is mapped
pub fn is_virtual_address_mapped(virt_addr: VirtualAddress) -> bool {
    let manager = VIRTUAL_MEMORY_MANAGER.lock();
    if let Some(vas) = manager.as_ref() {
        vas.is_mapped(virt_addr)
    } else {
        false
    }
}

/// Get virtual memory statistics
pub fn print_virtual_memory_stats() {
    let manager = VIRTUAL_MEMORY_MANAGER.lock();
    if let Some(vas) = manager.as_ref() {
        print_memory_layout(vas);
        
        let total_virtual_size: usize = vas.regions().iter().map(|r| r.size).sum();
        serial_println!("Total virtual address space: {} MB", total_virtual_size / (1024 * 1024));
        println!("Virtual memory: {} MB address space configured", total_virtual_size / (1024 * 1024));
    } else {
        serial_println!("Virtual memory manager not initialized");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test_case]
    fn test_virtual_address_creation() {
        let addr = VirtualAddress::new(0x1000);
        assert_eq!(addr.as_usize(), 0x1000);
    }
    
    #[test_case]
    fn test_virtual_address_alignment() {
        let addr = VirtualAddress::new(0x1234);
        let aligned_up = addr.align_up();
        let aligned_down = addr.align_down();
        
        assert_eq!(aligned_up.as_usize(), 0x2000);
        assert_eq!(aligned_down.as_usize(), 0x1000);
        
        let page_aligned = VirtualAddress::new(0x2000);
        assert!(page_aligned.is_aligned());
        assert!(!addr.is_aligned());
    }
    
    #[test_case]
    fn test_memory_protection_flags() {
        let read_only = MemoryProtection::read_only();
        assert!(read_only.readable);
        assert!(!read_only.writable);
        assert!(!read_only.executable);
        assert!(!read_only.user_accessible);
        
        let read_write = MemoryProtection::read_write();
        assert!(read_write.readable);
        assert!(read_write.writable);
        assert!(!read_write.executable);
        assert!(!read_write.user_accessible);
        
        let read_execute = MemoryProtection::read_execute();
        assert!(read_execute.readable);
        assert!(!read_execute.writable);
        assert!(read_execute.executable);
        assert!(!read_execute.user_accessible);
    }
    
    #[test_case]
    fn test_virtual_memory_region() {
        let region = VirtualMemoryRegion::new(
            VirtualAddress::new(0x1000),
            0x2000,
            MemoryProtection::read_write(),
            "Test Region"
        );
        
        assert_eq!(region.start.as_usize(), 0x1000);
        assert_eq!(region.size, 0x2000);
        assert_eq!(region.end().as_usize(), 0x3000);
        assert_eq!(region.name, "Test Region");
        
        // Test address containment
        assert!(region.contains(VirtualAddress::new(0x1500)));
        assert!(region.contains(VirtualAddress::new(0x1000)));
        assert!(!region.contains(VirtualAddress::new(0x3000)));
        assert!(!region.contains(VirtualAddress::new(0x500)));
        
        // Test page count calculation
        assert_eq!(region.page_count(), 2);
    }
    
    #[test_case]
    fn test_kernel_layout_constants() {
        // These now assert against `memory::paging`, so the test fails if the two
        // modules ever disagree about where the kernel lives — which is the only
        // thing worth asserting about a constant.
        assert_eq!(
            kernel_layout::KERNEL_CODE_START.as_usize(),
            crate::memory::paging::KERNEL_VMA as usize
        );
        assert_eq!(
            kernel_layout::KERNEL_HEAP_START.as_usize(),
            crate::memory::paging::KERNEL_HEAP_BASE as usize
        );
        assert_eq!(
            kernel_layout::PHYSICAL_MEMORY_OFFSET.as_usize(),
            crate::memory::paging::PHYSMAP_BASE as usize
        );
        // The kernel window has to start below the heap window, or the two
        // descriptions overlap and the layout dump is nonsense.
        assert!(kernel_layout::KERNEL_HEAP_START.as_usize() < kernel_layout::KERNEL_CODE_START.as_usize());
    }
}