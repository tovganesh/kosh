use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};
use spin::Mutex;
use alloc::vec::Vec;
use crate::memory::PAGE_SIZE;
use crate::memory::paging::{map_kernel_pages, KERNEL_HEAP_BASE};
use crate::{serial_println, println};

/// Minimum allocation size (to reduce fragmentation)
const MIN_ALLOC_SIZE: usize = 16;

/// Round `addr` up to a multiple of `align` (which must be a power of two).
const fn align_up_to(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

/// Maximum allocation size that can be handled by the heap
const MAX_ALLOC_SIZE: usize = 1024 * 1024; // 1MB

/// Magic number for heap corruption detection
const HEAP_MAGIC: u32 = 0xDEADBEEF;

/// Header for each allocated block.
///
/// `align(16)` matters: it rounds `size_of::<BlockHeader>()` up to a multiple
/// of 16, so a 16-aligned block base gives a 16-aligned payload. Combined with
/// rounding every allocation size up to 16, that keeps *every* block base
/// 16-aligned for the life of the heap. Without it the header was 40 bytes,
/// payloads landed on odd offsets, and even an 8-byte alignment request failed
/// after the first split.
#[repr(C, align(16))]
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

    /// Initialize the heap with the given size.
    ///
    /// The heap lives at its own virtual address range ([`KERNEL_HEAP_BASE`]),
    /// backed by frames the page tables map in. It previously took the raw
    /// physical address of a contiguous frame run and used it as a pointer —
    /// which happened to work only because paging was effectively off, and
    /// silently required physical contiguity it did not need.
    pub fn init(&mut self, heap_size_pages: usize) -> Result<(), &'static str> {
        if heap_size_pages == 0 {
            return Err("Heap size cannot be zero");
        }

        // Allocate and map `heap_size_pages` frames at the heap window. The
        // frames do not have to be contiguous — that is the whole point of
        // having page tables.
        map_kernel_pages(KERNEL_HEAP_BASE, heap_size_pages)?;

        let heap_start = KERNEL_HEAP_BASE as *mut u8;
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
        // Round up to 16 so the *next* block's base stays 16-aligned.
        let size = align_up_to(layout.size().max(MIN_ALLOC_SIZE), MIN_ALLOC_SIZE);
        let align = layout.align();

        if size > MAX_ALLOC_SIZE {
            return Err("Allocation too large");
        }

        // Find a suitable free block and, if the requested alignment forces
        // the payload away from the natural header boundary, carve a leading
        // free block off the front so the returned pointer really is aligned.
        let block = self.find_free_block(size, align)?;
        let block = self.align_block(block, size, align)?;

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

        // Account for what the block actually holds, not what was asked for.
        //
        // `deallocate` credits back `(*block).size`, and after the alignment
        // carve and the split that is not the same as the requested `size`.
        // Charging one and crediting the other made `current_bytes` drift
        // downwards until it underflowed, and `peak_bytes` then latched onto
        // the wrapped value — which is how the console ended up reporting a
        // peak of 18446744073709551568 bytes.
        let charged = unsafe { (*block.as_ptr()).size };

        self.stats.total_allocations += 1;
        self.stats.current_allocations += 1;
        self.stats.bytes_allocated += charged;
        self.stats.current_bytes += charged;

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

            // Add back to free list
            self.add_to_free_list(block);

            // Try to coalesce with adjacent free blocks
            self.coalesce_free_blocks(block);
        }

        Ok(())
    }

    /// Find a free block that can satisfy `size` bytes at `align`.
    ///
    /// The previous version ignored `layout.align()` entirely: it aligned to
    /// PAGE_SIZE regardless of what was asked for (rejecting almost everything
    /// by demanding up to 4095 bytes of slack) and then returned the
    /// *unaligned* pointer anyway. So it both over-rejected and under-delivered.
    fn find_free_block(
        &mut self,
        size: usize,
        align: usize,
    ) -> Result<NonNull<BlockHeader>, &'static str> {
        let mut current = self.free_list_head;

        while let Some(block) = current {
            unsafe {
                let block_ptr = block.as_ptr();
                if (*block_ptr).is_free && Self::block_fits(block, size, align) {
                    return Ok(block);
                }
                current = (*block_ptr).next;
            }
        }

        Err("Out of memory")
    }

    /// Where the payload would have to sit inside a block starting at `base`
    /// to satisfy `align`.
    ///
    /// If the natural payload position is already aligned, that is the answer.
    /// Otherwise we must carve a leading free block off the front, and that
    /// leading block needs a header plus at least `MIN_ALLOC_SIZE` of payload —
    /// so the answer is the first aligned address at or beyond
    /// `base + 2*header + MIN_ALLOC_SIZE`.
    ///
    /// Returning the *first* aligned address without that floor was wrong: for
    /// any alignment above 16 the gap was a few bytes, too small to be a legal
    /// block, and the allocation was rejected outright.
    fn aligned_payload(base: usize, data: usize, align: usize) -> Option<usize> {
        if data % align == 0 {
            return Some(data);
        }

        let header = core::mem::size_of::<BlockHeader>();
        let floor = base.checked_add(2 * header)?.checked_add(MIN_ALLOC_SIZE)?;
        Some(align_up_to(core::cmp::max(data, floor), align))
    }

    /// Whether `block` can serve `size` bytes at `align`.
    ///
    /// Two ways to satisfy it: the payload is already aligned, or there is
    /// enough room to carve a leading free block off the front (which needs a
    /// whole header plus a minimum-sized payload, or it would not be a valid
    /// block).
    fn block_fits(block: NonNull<BlockHeader>, size: usize, align: usize) -> bool {
        unsafe {
            let b = block.as_ptr();
            let header = core::mem::size_of::<BlockHeader>();
            let base = b as usize;
            let data = (*b).data_ptr() as usize;
            let avail = (*b).size;

            let Some(aligned) = Self::aligned_payload(base, data, align) else {
                return false;
            };

            if aligned == data {
                return avail >= size;
            }

            let lead_size = aligned - base - 2 * header;
            let new_size = (base + header + avail).saturating_sub(aligned);

            lead_size >= MIN_ALLOC_SIZE && new_size >= size
        }
    }

    /// Carve a leading free block off `block` if needed so the payload lands on
    /// `align`. Returns the block that should actually serve the allocation.
    fn align_block(
        &mut self,
        block: NonNull<BlockHeader>,
        _size: usize,
        align: usize,
    ) -> Result<NonNull<BlockHeader>, &'static str> {
        unsafe {
            let b = block.as_ptr();
            let header = core::mem::size_of::<BlockHeader>();
            let base = b as usize;
            let data = (*b).data_ptr() as usize;
            let avail = (*b).size;

            let aligned =
                Self::aligned_payload(base, data, align).ok_or("alignment not satisfiable")?;
            if aligned == data {
                return Ok(block);
            }

            let lead_size = aligned - base - 2 * header;
            let new_header = (aligned - header) as *mut BlockHeader;
            let new_size = base + header + avail - aligned;

            // Shrink the original into the leading free block...
            (*b).size = lead_size;

            // ...and write a fresh header for the aligned remainder.
            ptr::write(new_header, BlockHeader::new(new_size));
            let new_block = NonNull::new(new_header).ok_or("alignment split produced null")?;
            self.add_to_free_list(new_block);

            debug_assert_eq!((*new_block.as_ptr()).data_ptr() as usize % align, 0);
            Ok(new_block)
        }
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

    /// Merge every run of physically-adjacent free blocks.
    ///
    /// This used to be an empty function with a comment describing what it
    /// would do, so the heap fragmented monotonically and never recovered: free
    /// a 64-byte block next to another 64-byte block and you still could not
    /// serve a 128-byte request.
    ///
    /// The heap is one contiguous run of blocks — `header + payload`, back to
    /// back, from `heap_start` to `heap_start + heap_size` — so a linear walk
    /// visits them in address order and adjacency is just pointer arithmetic.
    ///
    /// O(number of blocks) per call. Fine at this scale; if it ever shows up in
    /// a profile, the fix is a boundary tag holding the previous block's size,
    /// which makes merging O(1).
    fn coalesce_free_blocks(&mut self, _hint: NonNull<BlockHeader>) {
        if self.heap_start.is_null() {
            return;
        }

        let header_size = core::mem::size_of::<BlockHeader>();
        let heap_end = self.heap_start as usize + self.heap_size;
        let mut cursor = self.heap_start as usize;

        while cursor < heap_end {
            let block = cursor as *mut BlockHeader;

            unsafe {
                if !(*block).is_valid() {
                    // Corruption: stop rather than walk off into nothing.
                    serial_println!("Heap corruption during coalesce at 0x{:x}", cursor);
                    return;
                }

                let mut total = (*block).total_size();

                if (*block).is_free {
                    // Absorb every free block immediately following this one.
                    loop {
                        let next_addr = cursor + total;
                        if next_addr >= heap_end {
                            break;
                        }

                        let next = next_addr as *mut BlockHeader;
                        if !(*next).is_valid() || !(*next).is_free {
                            break;
                        }

                        let next_total = (*next).total_size();
                        if let Some(nn) = NonNull::new(next) {
                            self.remove_from_free_list(nn);
                        }

                        // The absorbed header becomes payload.
                        (*block).size += next_total;
                        (*next).magic = 0;
                        total = (*block).total_size();
                    }
                }

                cursor += total;
            }
        }
    }

    /// Get allocation statistics.
    ///
    /// `free_bytes` is recomputed from the block chain rather than tracked
    /// incrementally: the incremental version ignored block headers and was not
    /// updated by `split_block`, so it drifted and eventually underflowed.
    pub fn stats(&self) -> AllocationStats {
        let mut stats = self.stats;
        stats.free_bytes = self.free_bytes();
        stats
    }

    /// Sum of the payload sizes of all free blocks.
    fn free_bytes(&self) -> usize {
        if self.heap_start.is_null() {
            return 0;
        }

        let heap_end = self.heap_start as usize + self.heap_size;
        let mut cursor = self.heap_start as usize;
        let mut free = 0usize;

        while cursor < heap_end {
            unsafe {
                let block = cursor as *mut BlockHeader;
                if !(*block).is_valid() {
                    break;
                }
                if (*block).is_free {
                    free += (*block).size;
                }
                cursor += (*block).total_size();
            }
        }

        free
    }

    /// Number of blocks in the heap, and how many of them are free.
    ///
    /// Used by the stress test to prove coalescing actually works: after
    /// freeing everything the heap must collapse back to a single free block.
    pub fn block_census(&self) -> (usize, usize) {
        if self.heap_start.is_null() {
            return (0, 0);
        }

        let heap_end = self.heap_start as usize + self.heap_size;
        let mut cursor = self.heap_start as usize;
        let (mut total, mut free) = (0usize, 0usize);

        while cursor < heap_end {
            unsafe {
                let block = cursor as *mut BlockHeader;
                if !(*block).is_valid() {
                    break;
                }
                total += 1;
                if (*block).is_free {
                    free += 1;
                }
                cursor += (*block).total_size();
            }
        }

        (total, free)
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

/// Blocks in the heap, and how many are free.
pub fn block_census() -> (usize, usize) {
    KERNEL_HEAP.lock().block_census()
}

/// Stress the allocator: many mixed-size, mixed-alignment allocations, freed in
/// a scrambled order, then assert the heap collapses back to a single free
/// block of its original size.
///
/// This is the test that proves coalescing works. Before Phase 3
/// `coalesce_free_blocks` was an empty function, so this would have finished
/// with hundreds of stranded free blocks instead of one.
pub fn stress_test() {
    use alloc::vec::Vec;

    const ROUNDS: usize = 2000;

    serial_println!("Heap stress test: {} mixed alloc/free rounds...", ROUNDS);

    let (blocks_before, _) = block_census();
    let free_before = heap_stats().free_bytes;

    let mut live: Vec<(NonNull<u8>, usize)> = Vec::new();
    let mut misaligned = 0usize;
    let mut failures = 0usize;

    for i in 0..ROUNDS {
        // Sizes 16..~2 KiB, alignments 8/16/32/64/128 — cycled so we exercise
        // the alignment-split path rather than only the happy case.
        let size = 16 + (i * 37) % 2048;
        let align = 1usize << (3 + (i % 5));

        let layout = match Layout::from_size_align(size, align) {
            Ok(l) => l,
            Err(_) => continue,
        };

        let result = KERNEL_HEAP.lock().allocate(layout);

        match result {
            Ok(ptr) => {
                if ptr.as_ptr() as usize % align != 0 {
                    misaligned += 1;
                }
                // Touch the whole allocation so a bad mapping or an overlapping
                // block shows up as corruption rather than passing silently.
                unsafe { ptr::write_bytes(ptr.as_ptr(), (i & 0xFF) as u8, size) };
                live.push((ptr, size));
            }
            Err(_) => failures += 1,
        }

        // Free roughly two thirds of what we allocate, out of order, to churn
        // the free list and force merges of non-adjacent-in-time blocks.
        if live.len() > 8 && i % 3 != 0 {
            let victim = live.remove((i * 7) % live.len());
            if KERNEL_HEAP.lock().deallocate(victim.0).is_err() {
                failures += 1;
            }
        }
    }

    // Drain the rest.
    while let Some((ptr, _)) = live.pop() {
        if KERNEL_HEAP.lock().deallocate(ptr).is_err() {
            failures += 1;
        }
    }
    drop(live);

    let (blocks_after, free_blocks_after) = block_census();
    let free_after = heap_stats().free_bytes;

    serial_println!("  allocation failures : {}", failures);
    serial_println!("  misaligned pointers : {}", misaligned);
    serial_println!(
        "  blocks: {} before -> {} after ({} free)",
        blocks_before,
        blocks_after,
        free_blocks_after
    );
    serial_println!("  free bytes: {} before -> {} after", free_before, free_after);

    if misaligned != 0 {
        serial_println!("  FAIL: allocator returned {} misaligned pointers", misaligned);
    } else if failures != 0 {
        serial_println!("  FAIL: {} allocation/deallocation errors", failures);
    } else if blocks_after != 1 {
        serial_println!(
            "  FAIL: heap did not coalesce back to one block ({} remain)",
            blocks_after
        );
    } else if free_after != free_before {
        serial_println!(
            "  FAIL: leaked {} bytes",
            free_before.saturating_sub(free_after)
        );
    } else {
        serial_println!("  PASS: heap fully reclaimed, coalesced to a single free block");
    }

    match validate_heap() {
        Ok(()) => serial_println!("  PASS: heap integrity validated"),
        Err(e) => serial_println!("  FAIL: heap integrity: {}", e),
    }
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
        
        // NOTE: the lock MUST be released before `ptrs.push`. A temporary in a
        // `match` scrutinee lives until the end of the match, so writing
        // `match KERNEL_HEAP.lock().allocate(..)` holds the spinlock while the
        // Vec grows — and Vec growth calls the global allocator, which tries to
        // take the same lock. That is a hard deadlock, and it was hanging the
        // boot right here.
        let result = KERNEL_HEAP.lock().allocate(layout);

        match result {
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