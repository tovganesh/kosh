use crate::memory::{PAGE_SIZE, physical::PageFrame, vmm::VirtualAddress};
use crate::memory::swap::{SwapSlot, SwapError, swap_out_page, swap_in_page, is_page_swapped};
use alloc::vec::Vec;
use alloc::collections::{BTreeMap, VecDeque};
use spin::Mutex;
use crate::{serial_println, println};

/// Page replacement algorithm types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageReplacementAlgorithm {
    /// Least Recently Used
    LRU,
    /// First In First Out
    FIFO,
    /// Clock (Second Chance)
    Clock,
    /// Least Frequently Used
    LFU,
}

/// Page access information for tracking usage
#[derive(Debug, Clone, Copy)]
pub struct PageAccessInfo {
    /// Virtual address of the page
    pub virtual_address: VirtualAddress,
    /// Physical page frame
    pub page_frame: PageFrame,
    /// Last access timestamp
    pub last_access: u64,
    /// Access count
    pub access_count: u64,
    /// Whether the page has been referenced recently
    pub referenced: bool,
    /// Whether the page has been modified
    pub dirty: bool,
}

impl PageAccessInfo {
    /// Create new page access info
    pub fn new(virtual_address: VirtualAddress, page_frame: PageFrame) -> Self {
        Self {
            virtual_address,
            page_frame,
            last_access: 0,
            access_count: 0,
            referenced: false,
            dirty: false,
        }
    }
    
    /// Update access information
    pub fn update_access(&mut self, timestamp: u64, is_write: bool) {
        self.last_access = timestamp;
        self.access_count += 1;
        self.referenced = true;
        if is_write {
            self.dirty = true;
        }
    }
    
    /// Reset reference bit (used by clock algorithm)
    pub fn clear_reference(&mut self) {
        self.referenced = false;
    }
}

/// LRU page replacement algorithm implementation
pub struct LRUReplacer {
    /// Pages ordered by access time (most recent first)
    pages: VecDeque<PageFrame>,
    /// Map from page frame to position in queue
    page_positions: BTreeMap<PageFrame, usize>,
    /// Maximum number of pages to track
    max_pages: usize,
}

impl LRUReplacer {
    /// Create a new LRU replacer
    pub fn new(max_pages: usize) -> Self {
        Self {
            pages: VecDeque::new(),
            page_positions: BTreeMap::new(),
            max_pages,
        }
    }
    
    /// Record page access
    pub fn access_page(&mut self, page_frame: PageFrame) {
        // Remove page from current position if it exists
        if let Some(&position) = self.page_positions.get(&page_frame) {
            self.pages.remove(position);
            // Update positions for pages after the removed one
            for (frame, pos) in self.page_positions.iter_mut() {
                if *pos > position {
                    *pos -= 1;
                }
            }
        }
        
        // Add page to front (most recently used)
        self.pages.push_front(page_frame);
        
        // Update positions
        for (i, &frame) in self.pages.iter().enumerate() {
            self.page_positions.insert(frame, i);
        }
        
        // Remove oldest pages if we exceed the limit
        while self.pages.len() > self.max_pages {
            if let Some(oldest_page) = self.pages.pop_back() {
                self.page_positions.remove(&oldest_page);
            }
        }
    }
    
    /// Get the least recently used page
    pub fn get_victim_page(&self) -> Option<PageFrame> {
        self.pages.back().copied()
    }
    
    /// Remove a page from tracking
    pub fn remove_page(&mut self, page_frame: PageFrame) {
        if let Some(&position) = self.page_positions.get(&page_frame) {
            self.pages.remove(position);
            self.page_positions.remove(&page_frame);
            
            // Update positions for pages after the removed one
            for (frame, pos) in self.page_positions.iter_mut() {
                if *pos > position {
                    *pos -= 1;
                }
            }
        }
    }
    
    /// Get current page count
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

/// FIFO page replacement algorithm implementation
pub struct FIFOReplacer {
    /// Pages in order of arrival
    pages: VecDeque<PageFrame>,
    /// Maximum number of pages to track
    max_pages: usize,
}

impl FIFOReplacer {
    /// Create a new FIFO replacer
    pub fn new(max_pages: usize) -> Self {
        Self {
            pages: VecDeque::new(),
            max_pages,
        }
    }
    
    /// Add a page
    pub fn add_page(&mut self, page_frame: PageFrame) {
        // Don't add if already present
        if !self.pages.contains(&page_frame) {
            self.pages.push_back(page_frame);
            
            // Remove oldest if we exceed the limit
            while self.pages.len() > self.max_pages {
                self.pages.pop_front();
            }
        }
    }
    
    /// Get the oldest page (victim for replacement)
    pub fn get_victim_page(&self) -> Option<PageFrame> {
        self.pages.front().copied()
    }
    
    /// Remove a page
    pub fn remove_page(&mut self, page_frame: PageFrame) {
        if let Some(pos) = self.pages.iter().position(|&p| p == page_frame) {
            self.pages.remove(pos);
        }
    }
    
    /// Get current page count
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

/// Clock (Second Chance) page replacement algorithm implementation
pub struct ClockReplacer {
    /// Pages with their reference bits
    pages: Vec<(PageFrame, bool)>,
    /// Current clock hand position
    clock_hand: usize,
    /// Maximum number of pages to track
    max_pages: usize,
}

impl ClockReplacer {
    /// Create a new Clock replacer
    pub fn new(max_pages: usize) -> Self {
        Self {
            pages: Vec::new(),
            clock_hand: 0,
            max_pages,
        }
    }
    
    /// Add or update a page
    pub fn access_page(&mut self, page_frame: PageFrame) {
        // Find existing page and set reference bit
        for (frame, referenced) in &mut self.pages {
            if *frame == page_frame {
                *referenced = true;
                return;
            }
        }
        
        // Add new page if not found
        if self.pages.len() < self.max_pages {
            self.pages.push((page_frame, true));
        } else {
            // Replace using clock algorithm
            self.replace_page(page_frame);
        }
    }
    
    /// Replace a page using clock algorithm
    fn replace_page(&mut self, new_page: PageFrame) {
        loop {
            if self.clock_hand >= self.pages.len() {
                self.clock_hand = 0;
            }
            
            let (_, referenced) = &mut self.pages[self.clock_hand];
            
            if *referenced {
                // Give second chance - clear reference bit and move to next
                *referenced = false;
                self.clock_hand += 1;
            } else {
                // Replace this page
                self.pages[self.clock_hand] = (new_page, true);
                self.clock_hand += 1;
                break;
            }
        }
    }
    
    /// Get victim page using clock algorithm
    pub fn get_victim_page(&mut self) -> Option<PageFrame> {
        if self.pages.is_empty() {
            return None;
        }
        
        let start_position = self.clock_hand;
        
        loop {
            if self.clock_hand >= self.pages.len() {
                self.clock_hand = 0;
            }
            
            let (frame, referenced) = &mut self.pages[self.clock_hand];
            
            if *referenced {
                // Give second chance
                *referenced = false;
                self.clock_hand += 1;
                
                // Prevent infinite loop
                if self.clock_hand == start_position {
                    break;
                }
            } else {
                // Found victim
                let victim = *frame;
                self.clock_hand += 1;
                return Some(victim);
            }
        }
        
        // If all pages have reference bit set, return the current one
        Some(self.pages[self.clock_hand].0)
    }
    
    /// Remove a page
    pub fn remove_page(&mut self, page_frame: PageFrame) {
        if let Some(pos) = self.pages.iter().position(|(frame, _)| *frame == page_frame) {
            self.pages.remove(pos);
            if self.clock_hand > pos {
                self.clock_hand -= 1;
            } else if self.clock_hand >= self.pages.len() && !self.pages.is_empty() {
                self.clock_hand = 0;
            }
        }
    }
    
    /// Get current page count
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

/// Page swapping manager that coordinates page replacement algorithms
pub struct PageSwapper {
    /// Current replacement algorithm
    algorithm: PageReplacementAlgorithm,
    /// LRU replacer
    lru_replacer: LRUReplacer,
    /// FIFO replacer
    fifo_replacer: FIFOReplacer,
    /// Clock replacer
    clock_replacer: ClockReplacer,
    /// Page access information
    page_info: BTreeMap<PageFrame, PageAccessInfo>,
    /// Current timestamp for LRU tracking
    current_timestamp: u64,
    /// Maximum number of pages in memory before swapping
    memory_pressure_threshold: usize,
    /// Statistics
    stats: SwapperStats,
}

/// Swapper statistics
#[derive(Debug, Clone, Copy)]
pub struct SwapperStats {
    pub pages_swapped_out: u64,
    pub pages_swapped_in: u64,
    pub page_faults: u64,
    pub algorithm_switches: u64,
}

impl SwapperStats {
    pub fn new() -> Self {
        Self {
            pages_swapped_out: 0,
            pages_swapped_in: 0,
            page_faults: 0,
            algorithm_switches: 0,
        }
    }
}

impl PageSwapper {
    /// Create a new page swapper
    pub fn new(algorithm: PageReplacementAlgorithm, max_pages: usize) -> Self {
        Self {
            algorithm,
            lru_replacer: LRUReplacer::new(max_pages),
            fifo_replacer: FIFOReplacer::new(max_pages),
            clock_replacer: ClockReplacer::new(max_pages),
            page_info: BTreeMap::new(),
            current_timestamp: 0,
            memory_pressure_threshold: max_pages,
            stats: SwapperStats::new(),
        }
    }
    
    /// Set the page replacement algorithm
    pub fn set_algorithm(&mut self, algorithm: PageReplacementAlgorithm) {
        if self.algorithm != algorithm {
            self.algorithm = algorithm;
            self.stats.algorithm_switches += 1;
            serial_println!("Switched page replacement algorithm to {:?}", algorithm);
        }
    }
    
    /// Record page access
    pub fn access_page(&mut self, virtual_address: VirtualAddress, page_frame: PageFrame, is_write: bool) {
        self.current_timestamp += 1;
        
        // Update page access information
        let page_info = self.page_info.entry(page_frame).or_insert_with(|| {
            PageAccessInfo::new(virtual_address, page_frame)
        });
        page_info.update_access(self.current_timestamp, is_write);
        
        // Update algorithm-specific tracking
        match self.algorithm {
            PageReplacementAlgorithm::LRU => {
                self.lru_replacer.access_page(page_frame);
            }
            PageReplacementAlgorithm::FIFO => {
                self.fifo_replacer.add_page(page_frame);
            }
            PageReplacementAlgorithm::Clock => {
                self.clock_replacer.access_page(page_frame);
            }
            PageReplacementAlgorithm::LFU => {
                // LFU uses the access count in page_info
            }
        }
    }
    
    /// Handle page fault - swap in a page if needed
    pub fn handle_page_fault(&mut self, virtual_address: VirtualAddress, page_frame: PageFrame) -> Result<(), SwapError> {
        self.stats.page_faults += 1;
        
        // Check if page is swapped out
        if is_page_swapped(page_frame) {
            serial_println!("Page fault: swapping in page {} for virtual address 0x{:x}", 
                           page_frame.0, virtual_address.as_usize());
            
            // Allocate buffer for page data
            let mut page_data = [0u8; PAGE_SIZE];
            
            // Swap in the page
            swap_in_page(page_frame, &mut page_data)?;
            
            self.stats.pages_swapped_in += 1;
            
            // Record access
            self.access_page(virtual_address, page_frame, false);
            
            serial_println!("Successfully swapped in page {}", page_frame.0);
        }
        
        Ok(())
    }
    
    /// Check if memory pressure requires swapping out pages
    pub fn check_memory_pressure(&mut self) -> Result<usize, SwapError> {
        let current_page_count = self.get_current_page_count();
        
        if current_page_count > self.memory_pressure_threshold {
            let pages_to_swap = current_page_count - self.memory_pressure_threshold;
            serial_println!("Memory pressure detected: {} pages over threshold, swapping out {} pages", 
                           current_page_count - self.memory_pressure_threshold, pages_to_swap);
            
            self.swap_out_pages(pages_to_swap)
        } else {
            Ok(0)
        }
    }
    
    /// Swap out a specified number of pages
    pub fn swap_out_pages(&mut self, count: usize) -> Result<usize, SwapError> {
        let mut swapped_count = 0;
        
        for _ in 0..count {
            if let Some(victim_page) = self.select_victim_page() {
                if let Some(page_info) = self.page_info.get(&victim_page) {
                    // Create page data buffer
                    let page_data = [0u8; PAGE_SIZE]; // In real implementation, read from memory
                    
                    // Swap out the page
                    match swap_out_page(victim_page, &page_data) {
                        Ok(swap_slot) => {
                            serial_println!("Swapped out page {} to slot {}", victim_page.0, swap_slot.slot());
                            
                            // Remove from algorithm tracking
                            self.remove_page_from_tracking(victim_page);
                            
                            // Remove page info
                            self.page_info.remove(&victim_page);
                            
                            self.stats.pages_swapped_out += 1;
                            swapped_count += 1;
                        }
                        Err(e) => {
                            serial_println!("Failed to swap out page {}: {:?}", victim_page.0, e);
                            break;
                        }
                    }
                }
            } else {
                break; // No more pages to swap
            }
        }
        
        if swapped_count > 0 {
            serial_println!("Successfully swapped out {} pages", swapped_count);
        }
        
        Ok(swapped_count)
    }
    
    /// Select a victim page for swapping based on the current algorithm
    fn select_victim_page(&mut self) -> Option<PageFrame> {
        match self.algorithm {
            PageReplacementAlgorithm::LRU => {
                self.lru_replacer.get_victim_page()
            }
            PageReplacementAlgorithm::FIFO => {
                self.fifo_replacer.get_victim_page()
            }
            PageReplacementAlgorithm::Clock => {
                self.clock_replacer.get_victim_page()
            }
            PageReplacementAlgorithm::LFU => {
                // Find page with lowest access count
                self.page_info.iter()
                    .min_by_key(|(_, info)| info.access_count)
                    .map(|(frame, _)| *frame)
            }
        }
    }
    
    /// Remove page from algorithm-specific tracking
    fn remove_page_from_tracking(&mut self, page_frame: PageFrame) {
        self.lru_replacer.remove_page(page_frame);
        self.fifo_replacer.remove_page(page_frame);
        self.clock_replacer.remove_page(page_frame);
    }
    
    /// Get current number of pages being tracked
    fn get_current_page_count(&self) -> usize {
        match self.algorithm {
            PageReplacementAlgorithm::LRU => self.lru_replacer.page_count(),
            PageReplacementAlgorithm::FIFO => self.fifo_replacer.page_count(),
            PageReplacementAlgorithm::Clock => self.clock_replacer.page_count(),
            PageReplacementAlgorithm::LFU => self.page_info.len(),
        }
    }
    
    /// Set memory pressure threshold
    pub fn set_memory_pressure_threshold(&mut self, threshold: usize) {
        self.memory_pressure_threshold = threshold;
        serial_println!("Set memory pressure threshold to {} pages", threshold);
    }
    
    /// Get statistics
    pub fn stats(&self) -> SwapperStats {
        self.stats
    }
    
    /// Print statistics
    pub fn print_stats(&self) {
        serial_println!("Page Swapper Statistics:");
        serial_println!("  Algorithm: {:?}", self.algorithm);
        serial_println!("  Pages swapped out: {}", self.stats.pages_swapped_out);
        serial_println!("  Pages swapped in: {}", self.stats.pages_swapped_in);
        serial_println!("  Page faults handled: {}", self.stats.page_faults);
        serial_println!("  Algorithm switches: {}", self.stats.algorithm_switches);
        serial_println!("  Current pages tracked: {}", self.get_current_page_count());
        serial_println!("  Memory pressure threshold: {}", self.memory_pressure_threshold);
        
        println!("Swap: {} out, {} in, {} faults, {:?} algorithm", 
                self.stats.pages_swapped_out, self.stats.pages_swapped_in, 
                self.stats.page_faults, self.algorithm);
    }
}

/// Global page swapper instance
static PAGE_SWAPPER: Mutex<Option<PageSwapper>> = Mutex::new(None);

/// Initialize the global page swapper
pub fn init_page_swapper(algorithm: PageReplacementAlgorithm, max_pages: usize) -> Result<(), SwapError> {
    let swapper = PageSwapper::new(algorithm, max_pages);
    *PAGE_SWAPPER.lock() = Some(swapper);
    
    serial_println!("Page swapper initialized with {:?} algorithm, {} max pages", algorithm, max_pages);
    Ok(())
}

/// Record page access in the global swapper
pub fn record_page_access(virtual_address: VirtualAddress, page_frame: PageFrame, is_write: bool) {
    if let Some(swapper) = PAGE_SWAPPER.lock().as_mut() {
        swapper.access_page(virtual_address, page_frame, is_write);
    }
}

/// Handle page fault in the global swapper
pub fn handle_page_fault(virtual_address: VirtualAddress, page_frame: PageFrame) -> Result<(), SwapError> {
    let mut swapper_guard = PAGE_SWAPPER.lock();
    if let Some(swapper) = swapper_guard.as_mut() {
        swapper.handle_page_fault(virtual_address, page_frame)
    } else {
        Err(SwapError::DeviceUnavailable)
    }
}

/// Check memory pressure and swap out pages if needed
pub fn check_memory_pressure() -> Result<usize, SwapError> {
    let mut swapper_guard = PAGE_SWAPPER.lock();
    if let Some(swapper) = swapper_guard.as_mut() {
        swapper.check_memory_pressure()
    } else {
        Err(SwapError::DeviceUnavailable)
    }
}

/// Manually swap out pages
pub fn swap_out_pages(count: usize) -> Result<usize, SwapError> {
    let mut swapper_guard = PAGE_SWAPPER.lock();
    if let Some(swapper) = swapper_guard.as_mut() {
        swapper.swap_out_pages(count)
    } else {
        Err(SwapError::DeviceUnavailable)
    }
}

/// Set page replacement algorithm
pub fn set_page_replacement_algorithm(algorithm: PageReplacementAlgorithm) -> Result<(), SwapError> {
    let mut swapper_guard = PAGE_SWAPPER.lock();
    if let Some(swapper) = swapper_guard.as_mut() {
        swapper.set_algorithm(algorithm);
        Ok(())
    } else {
        Err(SwapError::DeviceUnavailable)
    }
}

/// Set memory pressure threshold
pub fn set_memory_pressure_threshold(threshold: usize) -> Result<(), SwapError> {
    let mut swapper_guard = PAGE_SWAPPER.lock();
    if let Some(swapper) = swapper_guard.as_mut() {
        swapper.set_memory_pressure_threshold(threshold);
        Ok(())
    } else {
        Err(SwapError::DeviceUnavailable)
    }
}

/// Get swapper statistics
pub fn get_swapper_stats() -> Option<SwapperStats> {
    let swapper_guard = PAGE_SWAPPER.lock();
    swapper_guard.as_ref().map(|s| s.stats())
}

/// Print swapper statistics
pub fn print_swapper_stats() {
    let swapper_guard = PAGE_SWAPPER.lock();
    if let Some(swapper) = swapper_guard.as_ref() {
        swapper.print_stats();
    } else {
        serial_println!("Page swapper not initialized");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test_case]
    fn test_lru_replacer() {
        let mut lru = LRUReplacer::new(3);
        
        // Add pages
        lru.access_page(PageFrame(1));
        lru.access_page(PageFrame(2));
        lru.access_page(PageFrame(3));
        
        assert_eq!(lru.page_count(), 3);
        
        // Access page 1 again (should move to front)
        lru.access_page(PageFrame(1));
        
        // Page 2 should be the LRU now
        assert_eq!(lru.get_victim_page(), Some(PageFrame(2)));
        
        // Add another page (should evict page 2)
        lru.access_page(PageFrame(4));
        assert_eq!(lru.page_count(), 3);
        assert_eq!(lru.get_victim_page(), Some(PageFrame(3)));
    }
    
    #[test_case]
    fn test_fifo_replacer() {
        let mut fifo = FIFOReplacer::new(3);
        
        // Add pages
        fifo.add_page(PageFrame(1));
        fifo.add_page(PageFrame(2));
        fifo.add_page(PageFrame(3));
        
        assert_eq!(fifo.page_count(), 3);
        
        // First page should be victim
        assert_eq!(fifo.get_victim_page(), Some(PageFrame(1)));
        
        // Add another page
        fifo.add_page(PageFrame(4));
        assert_eq!(fifo.page_count(), 3);
        assert_eq!(fifo.get_victim_page(), Some(PageFrame(2)));
    }
    
    #[test_case]
    fn test_clock_replacer() {
        let mut clock = ClockReplacer::new(3);
        
        // Add pages
        clock.access_page(PageFrame(1));
        clock.access_page(PageFrame(2));
        clock.access_page(PageFrame(3));
        
        assert_eq!(clock.page_count(), 3);
        
        // All pages have reference bit set, so first one should be victim after clearing
        let victim = clock.get_victim_page();
        assert!(victim.is_some());
    }
    
    #[test_case]
    fn test_page_access_info() {
        let mut info = PageAccessInfo::new(
            VirtualAddress::new(0x1000),
            PageFrame(1)
        );
        
        assert_eq!(info.access_count, 0);
        assert!(!info.referenced);
        assert!(!info.dirty);
        
        info.update_access(100, true);
        
        assert_eq!(info.access_count, 1);
        assert_eq!(info.last_access, 100);
        assert!(info.referenced);
        assert!(info.dirty);
    }
    
    #[test_case]
    fn test_swapper_stats() {
        let mut stats = SwapperStats::new();
        
        assert_eq!(stats.pages_swapped_out, 0);
        assert_eq!(stats.pages_swapped_in, 0);
        assert_eq!(stats.page_faults, 0);
        
        stats.pages_swapped_out += 1;
        stats.page_faults += 1;
        
        assert_eq!(stats.pages_swapped_out, 1);
        assert_eq!(stats.page_faults, 1);
    }
}