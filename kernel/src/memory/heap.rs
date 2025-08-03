use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};
use spin::Mutex;
use alloc::vec::Vec;
use crate::memory::{PAGE_SIZE, align_up};
use crate::memory::physical::allocate_frames;
use crate::{serial_println, println};

/// Minimum allocation size (to reduce fragmentation)
const MIN_ALLOC_SIZE: usize = 16;

/// Maximum allocation size that can be handled by the heap
const MAX_ALLOC_SIZE: usize = 1024 * 1024; // 1MB

/// Magic number for heap corruption detection
const HEAP_MAGIC: u32 = 0xDEADBEEF;

/// Header for each allocated block
#[repr(C)]
struct BlockHeader {
    magic: u32,
    size: usize,
    next: Option<NonNull<BlockHeader>>,
    prev: Option<NonNull<BlockHeader>>,
    is_free: bool,
    #[cfg(debug_assertions)]
    alloc_id: u64,
}

impl BlockHeader {
    fn new(size: usize) -> Self {
        Self {
            magic: HEAP_MAGIC,
            size,
            next: None,
            prev: None,
            is_free: true,
            #[cfg(debug_assertions)]
            alloc_id: 0,
        }
    }

    /// Get the data pointer for this block
    fn data_ptr(&self) -> *mut u8 {
        unsafe {
            (self as *const Self as *mut u8).add(core::mem::size_of::<BlockHeader>())
        }
    }

    /// Get the block header from a data pointer
    unsafe fn from_data_ptr(ptr: *mut u8) -> *mut Self {
        ptr.sub(core::mem::size_of::<BlockHeader>()) as *mut Self
    }

    /// Check if the block header is valid (corruption detection)
    fn is_valid(&self) -> bool {
        self.magic == HEAP_MAGIC
    }

    /// Get the total size including header
    fn total_size(&self) -> usize {
        core::mem::size_of::<BlockHeader>() + self.size
    }
}

/// Allocation tracking for debugging
#[derive(Debug, Clone, Copy)]
pub struct AllocationStats {
    pub total_allocations: u64,
    pub total_deallocations: u64,
    pub current_allocations: u64,
    pub bytes_allocated: usize,
    pub bytes_deallocated: usize,
    pub current_bytes: usize,
    pub peak_bytes: usize,
    pub heap_size: usize,
    pub free_bytes: usize,
}

impl AllocationStats {
    const fn new() -> Self {
        Self {
            total_allocations: 0,
            total_deallocations: 0,
            current_allocations: 0,
            bytes_allocated: 0,
            bytes_deallocated: 0,
            current_bytes: 0,
            peak_bytes: 0,
            heap_size: 0,
            free_bytes: 0,
        }
    }
}

/// Linked-list based kernel heap allocator
pub struct KernelHeapAllocator {
    /// Start of the heap memory region
    heap_start: *mut u8,
    /// Size of the heap in bytes
    heap_size: usize,
    /// Head of the free block list
    free_list_head: Option<NonNull<BlockHeader>>,
    /// Allocation statistics for debugging
    stats: AllocationStats,
    /// Next allocation ID for debugging
    #[cfg(debug_assertions)]
    next_alloc_id: u64,
}

// SAFETY: KernelHeapAllocator is only used in kernel context where we control access
unsafe impl Send for KernelHeapAllocator {}
unsafe impl Sync for KernelHeapAllocator {}

impl KernelHeapAllocator {
    /// Create a new uninitialized heap allocator
    pub const fn new() -> Self {
        Self {
            heap_start: ptr::null_mut(),
            heap_size: 0,
            free_list_head: None,
            stats: AllocationStats::new(),
            #[cfg(debug_assertions)]
            next_alloc_id: 1,
        }
    }

    /// Initialize the heap with the given size
    pub fn init(&mut self, heap_size_pages: usize) -> Result<(), &'static str> {
        if heap_size_pages == 0 {
            return Err("Heap size cannot be zero");
        }

        // Allocate physical pages for the heap
        let start_frame = allocate_frames(heap_size_pages)
            .ok_or("Failed to allocate physical memory for heap")?;

        // Map the physical pages to virtual memory
        // For now, we'll use identity mapping (virtual = physical)
        // In a full implementation, this would use the VMM
        let heap_start = start_frame.address() as *mut u8;
        let heap_size = heap_size_pages * PAGE_SIZE;

        // Initialize the heap memory
        unsafe {
            ptr::write_bytes(heap_start, 0, heap_size);
        }

        // Create the initial free block that spans the entire heap
        let initial_block = heap_start as *mut BlockHeader;
        let initial_size = heap_size - core::mem::size_of::<BlockHeader>();

        unsafe {
            ptr::write(initial_block, BlockHeader::new(initial_size));
        }

        self.heap_start = heap_start;
        self.heap_size = heap_size;
        self.free_list_head = NonNull::new(initial_block);
        self.stats.heap_size = heap_size;
        self.stats.free_bytes = initial_size;

        serial_println!("Kernel heap initialized: {} KB at 0x{:x}", 
                       heap_size / 1024, heap_start as usize);

        Ok(())
    }

    /// Allocate memory with the given layout
    pub fn allocate(&mut self, layout: Layout) -> Result<NonNull<u8>, &'static str> {
        let size = layout.size().max(MIN_ALLOC_SIZE);
        let align = layout.align();

        if size > MAX_ALLOC_SIZE {
            return Err("Allocation too large");
        }

        // Find a suitable free block
        let block = self.find_free_block(size, align)?;

        // Split the block if it's significantly larger than needed
        self.split_block(block, size);

        // Mark the block as used
        unsafe {
            (*block.as_ptr()).is_free = false;
            #[cfg(debug_assertions)]
            {
                (*block.as_ptr()).alloc_id = self.next_alloc_id;
                self.next_alloc_id += 1;
            }
        }

        // Remove from free list
        self.remove_from_free_list(block);

        // Update statistics
        self.stats.total_allocations += 1;
        self.stats.current_allocations += 1;
        self.stats.bytes_allocated += size;
        self.stats.current_bytes += size;
        self.stats.free_bytes -= size;

        if self.stats.current_bytes > self.stats.peak_bytes {
            self.stats.peak_bytes = self.stats.current_bytes;
        }

        let data_ptr = unsafe { (*block.as_ptr()).data_ptr() };

        #[cfg(debug_assertions)]
        {
            // Fill allocated memory with a pattern for debugging
            unsafe {
                ptr::write_bytes(data_ptr, 0xAA, size);
            }
        }

        Ok(NonNull::new(data_ptr).unwrap())
    }

    /// Deallocate memory
    pub fn deallocate(&mut self, ptr: NonNull<u8>) -> Result<(), &'static str> {
        let data_ptr = ptr.as_ptr();

        // Get the block header
        let block_ptr = unsafe { BlockHeader::from_data_ptr(data_ptr) };
        let block = NonNull::new(block_ptr).ok_or("Invalid pointer")?;

        // Validate the block
        unsafe {
            if !(*block.as_ptr()).is_valid() {
                return Err("Heap corruption detected: invalid magic number");
            }

            if (*block.as_ptr()).is_free {
                return Err("Double free detected");
            }

            let size = (*block.as_ptr()).size;

            // Mark as free
            (*block.as_ptr()).is_free = true;

            #[cfg(debug_assertions)]
            {
                // Fill freed memory with a pattern for debugging
                ptr::write_bytes(data_ptr, 0xDD, size);
            }

            // Update statistics
            self.stats.total_deallocations += 1;
            self.stats.current_allocations -= 1;
            self.stats.bytes_deallocated += size;
            self.stats.current_bytes -= size;
            self.stats.free_bytes += size;

            // Add back to free list
            self.add_to_free_list(block);

            // Try to coalesce with adjacent free blocks
            self.coalesce_free_blocks(block);
        }

        Ok(())
    }

    /// Find a free block that can satisfy the allocation
    fn find_free_block(&mut self, size: usize, _align: usize) -> Result<NonNull<BlockHeader>, &'static str> {
        let mut current = self.free_list_head;

        while let Some(block) = current {
            unsafe {
                let block_ptr = block.as_ptr();
                if (*block_ptr).is_free && (*block_ptr).size >= size {
                    // Check alignment
                    let data_ptr = (*block_ptr).data_ptr();
                    let aligned_ptr = align_up(data_ptr as usize);
                    let alignment_offset = aligned_ptr - data_ptr as usize;

                    if (*block_ptr).size >= size + alignment_offset {
                        return Ok(block);
                    }
                }
                current = (*block_ptr).next;
            }
        }

        Err("Out of memory")
    }

    /// Split a block if it's significantly larger than needed
    fn split_block(&mut self, block: NonNull<BlockHeader>, needed_size: usize) {
        unsafe {
            let block_ptr = block.as_ptr();
            let total_size = (*block_ptr).size;
            let header_size = core::mem::size_of::<BlockHeader>();

            // Only split if the remaining part would be large enough for another allocation
            if total_size >= needed_size + header_size + MIN_ALLOC_SIZE {
                let remaining_size = total_size - needed_size - header_size;

                // Create new block for the remaining space
                let new_block_ptr = ((*block_ptr).data_ptr() as usize + needed_size) as *mut BlockHeader;
                ptr::write(new_block_ptr, BlockHeader::new(remaining_size));

                // Update the original block size
                (*block_ptr).size = needed_size;

                // Link the new block into the free list
                let new_block = NonNull::new(new_block_ptr).unwrap();
                self.add_to_free_list(new_block);
            }
        }
    }

    /// Add a block to the free list
    fn add_to_free_list(&mut self, block: NonNull<BlockHeader>) {
        unsafe {
            let block_ptr = block.as_ptr();
            (*block_ptr).next = self.free_list_head;
            (*block_ptr).prev = None;

            if let Some(head) = self.free_list_head {
                (*head.as_ptr()).prev = Some(block);
            }

            self.free_list_head = Some(block);
        }
    }

    /// Remove a block from the free list
    fn remove_from_free_list(&mut self, block: NonNull<BlockHeader>) {
        unsafe {
            let block_ptr = block.as_ptr();

            if let Some(prev) = (*block_ptr).prev {
                (*prev.as_ptr()).next = (*block_ptr).next;
            } else {
                self.free_list_head = (*block_ptr).next;
            }

            if let Some(next) = (*block_ptr).next {
                (*next.as_ptr()).prev = (*block_ptr).prev;
            }

            (*block_ptr).next = None;
            (*block_ptr).prev = None;
        }
    }

    /// Coalesce adjacent free blocks
    fn coalesce_free_blocks(&mut self, block: NonNull<BlockHeader>) {
        // This is a simplified version - in a full implementation,
        // we would check for physically adjacent blocks and merge them
        // For now, we just ensure the block is properly linked
        unsafe {
            let block_ptr = block.as_ptr();
            if !(*block_ptr).is_free {
                return;
            }

            // In a more sophisticated implementation, we would:
            // 1. Check if the next physical block is free and merge
            // 2. Check if the previous physical block is free and merge
            // 3. Update the free list accordingly
        }
    }

    /// Get allocation statistics
    pub fn stats(&self) -> AllocationStats {
        self.stats
    }

    /// Print heap statistics
    pub fn print_stats(&self) {
        serial_println!("Kernel Heap Statistics:");
        serial_println!("  Heap size: {} KB", self.stats.heap_size / 1024);
        serial_println!("  Current allocations: {}", self.stats.current_allocations);
        serial_println!("  Total allocations: {}", self.stats.total_allocations);
        serial_println!("  Total deallocations: {}", self.stats.total_deallocations);
        serial_println!("  Current bytes: {} KB", self.stats.current_bytes / 1024);
        serial_println!("  Peak bytes: {} KB", self.stats.peak_bytes / 1024);
        serial_println!("  Free bytes: {} KB", self.stats.free_bytes / 1024);

        println!("Heap: {} KB total, {} KB used, {} KB free",
                self.stats.heap_size / 1024,
                (self.stats.heap_size - self.stats.free_bytes) / 1024,
                self.stats.free_bytes / 1024);
    }

    /// Validate heap integrity (corruption detection)
    pub fn validate_heap(&self) -> Result<(), &'static str> {
        let mut current = self.free_list_head;
        let mut blocks_checked = 0;

        while let Some(block) = current {
            unsafe {
                let block_ptr = block.as_ptr();

                // Check magic number
                if !(*block_ptr).is_valid() {
                    return Err("Heap corruption: invalid magic number");
                }

                // Check that free blocks are actually marked as free
                if !(*block_ptr).is_free {
                    return Err("Heap corruption: non-free block in free list");
                }

                // Check bounds
                let block_start = block_ptr as usize;
                let block_end = block_start + (*block_ptr).total_size();
                let heap_start = self.heap_start as usize;
                let heap_end = heap_start + self.heap_size;

                if block_start < heap_start || block_end > heap_end {
                    return Err("Heap corruption: block outside heap bounds");
                }

                current = (*block_ptr).next;
                blocks_checked += 1;

                // Prevent infinite loops
                if blocks_checked > 10000 {
                    return Err("Heap corruption: possible circular free list");
                }
            }
        }

        Ok(())
    }
}

/// Global kernel heap allocator instance
static KERNEL_HEAP: Mutex<KernelHeapAllocator> = Mutex::new(KernelHeapAllocator::new());

/// Global allocator implementation for Rust's allocator interface
pub struct GlobalKernelAllocator;

unsafe impl GlobalAlloc for GlobalKernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match KERNEL_HEAP.lock().allocate(layout) {
            Ok(ptr) => ptr.as_ptr(),
            Err(_) => ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if let Some(non_null_ptr) = NonNull::new(ptr) {
            if let Err(e) = KERNEL_HEAP.lock().deallocate(non_null_ptr) {
                serial_println!("Deallocation error: {}", e);
                // In a real kernel, we might want to panic here
            }
        }
    }
}

/// Initialize the kernel heap
pub fn init_kernel_heap(heap_size_pages: usize) -> Result<(), &'static str> {
    KERNEL_HEAP.lock().init(heap_size_pages)?;
    serial_println!("Kernel heap allocator initialized successfully");
    Ok(())
}

/// Get heap statistics
pub fn heap_stats() -> AllocationStats {
    KERNEL_HEAP.lock().stats()
}

/// Print heap statistics
pub fn print_heap_stats() {
    KERNEL_HEAP.lock().print_stats();
}

/// Validate heap integrity
pub fn validate_heap() -> Result<(), &'static str> {
    KERNEL_HEAP.lock().validate_heap()
}

/// Test the heap allocator
pub fn test_heap_allocator() {
    serial_println!("Testing kernel heap allocator...");

    // Test basic allocation and deallocation
    {
        let layout = Layout::from_size_align(64, 8).unwrap();
        let ptr = KERNEL_HEAP.lock().allocate(layout);
        
        match ptr {
            Ok(ptr) => {
                serial_println!("Allocated 64 bytes at 0x{:x}", ptr.as_ptr() as usize);
                
                // Write some data to test
                unsafe {
                    ptr::write_bytes(ptr.as_ptr(), 0x42, 64);
                }
                
                // Deallocate
                if let Err(e) = KERNEL_HEAP.lock().deallocate(ptr) {
                    serial_println!("Deallocation failed: {}", e);
                } else {
                    serial_println!("Successfully deallocated 64 bytes");
                }
            }
            Err(e) => {
                serial_println!("Allocation failed: {}", e);
            }
        }
    }

    // Test multiple allocations
    let mut ptrs = Vec::new();
    for i in 0..10 {
        let size = 32 + i * 16;
        let layout = Layout::from_size_align(size, 8).unwrap();
        
        match KERNEL_HEAP.lock().allocate(layout) {
            Ok(ptr) => {
                serial_println!("Allocated {} bytes at 0x{:x}", size, ptr.as_ptr() as usize);
                ptrs.push((ptr, layout));
            }
            Err(e) => {
                serial_println!("Allocation {} failed: {}", i, e);
                break;
            }
        }
    }

    // Deallocate all
    for (ptr, _layout) in ptrs {
        if let Err(e) = KERNEL_HEAP.lock().deallocate(ptr) {
            serial_println!("Deallocation failed: {}", e);
        }
    }

    // Validate heap integrity
    match validate_heap() {
        Ok(()) => serial_println!("Heap validation passed"),
        Err(e) => serial_println!("Heap validation failed: {}", e),
    }

    // Print final statistics
    print_heap_stats();

    serial_println!("Heap allocator test complete");
}