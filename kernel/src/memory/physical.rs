use multiboot2::{BootInformation, MemoryAreaType};
use spin::Mutex;
use crate::memory::{PAGE_SIZE, align_down};
use crate::{serial_println, println};

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
        
        // Find a suitable location for the bitmap after the kernel
        // We'll place it at 2MB to avoid low memory areas
        let bitmap_start = 0x200000; // 2MB
        let bitmap_end = bitmap_start + bitmap_size;
        
        // Ensure bitmap doesn't overlap with any reserved areas
        for area in memory_map.memory_areas() {
            if area.typ() != MemoryAreaType::Available {
                let area_start = area.start_address() as usize;
                let area_end = area.end_address() as usize;
                
                if bitmap_start < area_end && bitmap_end > area_start {
                    return Err("Cannot place bitmap - overlaps with reserved memory");
                }
            }
        }
        
        // Initialize bitmap memory
        let bitmap = unsafe {
            core::slice::from_raw_parts_mut(bitmap_start as *mut u8, bitmap_size)
        };
        
        // Clear the bitmap (all pages initially marked as used)
        bitmap.fill(0xFF);
        
        let mut manager = Self {
            bitmap,
            total_frames,
            free_frames: 0,
            used_frames: 0,
            reserved_frames: 0,
            bitmap_start,
        };
        
        // Mark available memory areas as free
        manager.parse_memory_map(&memory_map)?;
        
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
            
            if area.typ() == MemoryAreaType::Available {
                // Mark pages as free, but avoid the bitmap area and low memory
                for frame_num in start_frame.0..=end_frame.0 {
                    let frame_addr = frame_num * PAGE_SIZE;
                    
                    // Skip low memory (first 1MB) and bitmap area
                    if frame_addr < 0x100000 || 
                       (frame_addr >= self.bitmap_start && 
                        frame_addr < self.bitmap_start + self.bitmap.len()) {
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
    pub fn deallocate_frame(&mut self, frame: PageFrame) {
        if frame.0 >= self.total_frames {
            return;
        }
        
        if self.is_frame_free(frame) {
            // Already free, this might indicate a double-free bug
            serial_println!("Warning: Attempted to free already free frame {}", frame.0);
            return;
        }
        
        self.mark_frame_free(frame);
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