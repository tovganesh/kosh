use crate::memory::{PAGE_SIZE, physical::PageFrame};
use alloc::{vec, vec::Vec, boxed::Box};
use alloc::collections::BTreeMap;
use spin::Mutex;
use crate::{serial_println, println};

// Re-export swap modules that are in the same directory
pub use crate::memory::swap_file;
pub use crate::memory::swap_config;
pub use crate::memory::swap_algorithm;

/// Swap slot identifier - represents a location in swap space
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SwapSlot(pub usize);

impl SwapSlot {
    /// Create a new swap slot
    pub fn new(slot: usize) -> Self {
        SwapSlot(slot)
    }
    
    /// Get the slot number
    pub fn slot(&self) -> usize {
        self.0
    }
    
    /// Calculate the offset in the swap device
    pub fn offset(&self) -> usize {
        self.0 * PAGE_SIZE
    }
}

/// Swap device types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapDeviceType {
    /// File-based swap
    File,
    /// Partition-based swap
    Partition,
}

/// Swap device interface trait
pub trait SwapDevice: Send + Sync {
    /// Get the device type
    fn device_type(&self) -> SwapDeviceType;
    
    /// Get the total size in bytes
    fn size(&self) -> usize;
    
    /// Get the number of available swap slots
    fn slot_count(&self) -> usize {
        self.size() / PAGE_SIZE
    }
    
    /// Read a page from swap
    fn read_page(&mut self, slot: SwapSlot, buffer: &mut [u8; PAGE_SIZE]) -> Result<(), SwapError>;
    
    /// Write a page to swap
    fn write_page(&mut self, slot: SwapSlot, buffer: &[u8; PAGE_SIZE]) -> Result<(), SwapError>;
    
    /// Check if the device is available
    fn is_available(&self) -> bool;
    
    /// Get device name/identifier
    fn name(&self) -> &str;
}

/// Swap operation errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapError {
    /// Device not available
    DeviceUnavailable,
    /// Invalid swap slot
    InvalidSlot,
    /// I/O error during read/write
    IoError,
    /// No space available
    NoSpace,
    /// Slot already in use
    SlotInUse,
    /// Slot not in use
    SlotNotInUse,
}

/// Swap space allocation tracking
#[derive(Debug)]
struct SwapAllocator {
    /// Bitmap tracking allocated slots (true = allocated, false = free)
    bitmap: Vec<bool>,
    /// Number of free slots
    free_slots: usize,
    /// Total number of slots
    total_slots: usize,
    /// Next slot to check for allocation (for faster allocation)
    next_free_hint: usize,
}

impl SwapAllocator {
    /// Create a new swap allocator
    fn new(total_slots: usize) -> Self {
        Self {
            bitmap: vec![false; total_slots],
            free_slots: total_slots,
            total_slots,
            next_free_hint: 0,
        }
    }
    
    /// Allocate a swap slot
    fn allocate_slot(&mut self) -> Option<SwapSlot> {
        if self.free_slots == 0 {
            return None;
        }
        
        // Start searching from the hint
        for i in 0..self.total_slots {
            let slot_idx = (self.next_free_hint + i) % self.total_slots;
            
            if !self.bitmap[slot_idx] {
                // Found free slot
                self.bitmap[slot_idx] = true;
                self.free_slots -= 1;
                self.next_free_hint = (slot_idx + 1) % self.total_slots;
                return Some(SwapSlot(slot_idx));
            }
        }
        
        None
    }
    
    /// Deallocate a swap slot
    fn deallocate_slot(&mut self, slot: SwapSlot) -> Result<(), SwapError> {
        if slot.0 >= self.total_slots {
            return Err(SwapError::InvalidSlot);
        }
        
        if !self.bitmap[slot.0] {
            return Err(SwapError::SlotNotInUse);
        }
        
        self.bitmap[slot.0] = false;
        self.free_slots += 1;
        
        // Update hint if this slot is before current hint
        if slot.0 < self.next_free_hint {
            self.next_free_hint = slot.0;
        }
        
        Ok(())
    }
    
    /// Check if a slot is allocated
    fn is_slot_allocated(&self, slot: SwapSlot) -> bool {
        slot.0 < self.total_slots && self.bitmap[slot.0]
    }
    
    /// Get allocation statistics
    fn stats(&self) -> SwapStats {
        SwapStats {
            total_slots: self.total_slots,
            used_slots: self.total_slots - self.free_slots,
            free_slots: self.free_slots,
        }
    }
}

/// Page-to-swap mapping entry
#[derive(Debug, Clone, Copy)]
struct SwapEntry {
    /// The swap device index
    device_index: usize,
    /// The swap slot in the device
    slot: SwapSlot,
}

/// Swap space manager
pub struct SwapManager {
    /// List of swap devices
    devices: Vec<Box<dyn SwapDevice>>,
    /// Allocators for each device
    allocators: Vec<SwapAllocator>,
    /// Mapping from page frame to swap entry
    page_to_swap: BTreeMap<PageFrame, SwapEntry>,
    /// Mapping from swap entry to page frame (for reverse lookup)
    swap_to_page: BTreeMap<(usize, SwapSlot), PageFrame>,
    /// Total swap space statistics
    total_stats: SwapStats,
}

/// Swap space statistics
#[derive(Debug, Clone, Copy)]
pub struct SwapStats {
    pub total_slots: usize,
    pub used_slots: usize,
    pub free_slots: usize,
}

impl SwapStats {
    /// Get total swap space in bytes
    pub fn total_bytes(&self) -> usize {
        self.total_slots * PAGE_SIZE
    }
    
    /// Get used swap space in bytes
    pub fn used_bytes(&self) -> usize {
        self.used_slots * PAGE_SIZE
    }
    
    /// Get free swap space in bytes
    pub fn free_bytes(&self) -> usize {
        self.free_slots * PAGE_SIZE
    }
    
    /// Get total swap space in MB
    pub fn total_mb(&self) -> usize {
        self.total_bytes() / (1024 * 1024)
    }
    
    /// Get used swap space in MB
    pub fn used_mb(&self) -> usize {
        self.used_bytes() / (1024 * 1024)
    }
    
    /// Get free swap space in MB
    pub fn free_mb(&self) -> usize {
        self.free_bytes() / (1024 * 1024)
    }
    
    /// Get usage percentage
    pub fn usage_percent(&self) -> f32 {
        if self.total_slots == 0 {
            0.0
        } else {
            (self.used_slots as f32 / self.total_slots as f32) * 100.0
        }
    }
}

impl SwapManager {
    /// Create a new swap manager
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            allocators: Vec::new(),
            page_to_swap: BTreeMap::new(),
            swap_to_page: BTreeMap::new(),
            total_stats: SwapStats {
                total_slots: 0,
                used_slots: 0,
                free_slots: 0,
            },
        }
    }
    
    /// Add a swap device
    pub fn add_device(&mut self, device: Box<dyn SwapDevice>) -> Result<usize, SwapError> {
        if !device.is_available() {
            return Err(SwapError::DeviceUnavailable);
        }
        
        let device_index = self.devices.len();
        let slot_count = device.slot_count();
        
        serial_println!("Adding swap device '{}' with {} slots ({} MB)", 
                       device.name(), slot_count, (slot_count * PAGE_SIZE) / (1024 * 1024));
        
        // Create allocator for this device
        let allocator = SwapAllocator::new(slot_count);
        
        self.devices.push(device);
        self.allocators.push(allocator);
        
        // Update total statistics
        self.total_stats.total_slots += slot_count;
        self.total_stats.free_slots += slot_count;
        
        Ok(device_index)
    }
    
    /// Remove a swap device (must be empty)
    pub fn remove_device(&mut self, device_index: usize) -> Result<(), SwapError> {
        if device_index >= self.devices.len() {
            return Err(SwapError::InvalidSlot);
        }
        
        // Check if device has any allocated slots
        let allocator = &self.allocators[device_index];
        if allocator.stats().used_slots > 0 {
            return Err(SwapError::SlotInUse);
        }
        
        // Remove mappings for this device
        self.swap_to_page.retain(|(dev_idx, _), _| *dev_idx != device_index);
        
        // Update total statistics
        let device_stats = allocator.stats();
        self.total_stats.total_slots -= device_stats.total_slots;
        self.total_stats.free_slots -= device_stats.free_slots;
        
        // Remove device and allocator
        self.devices.remove(device_index);
        self.allocators.remove(device_index);
        
        // Update device indices in mappings
        let mut new_swap_to_page = BTreeMap::new();
        for ((dev_idx, slot), page) in self.swap_to_page.iter() {
            let new_dev_idx = if *dev_idx > device_index {
                *dev_idx - 1
            } else {
                *dev_idx
            };
            new_swap_to_page.insert((new_dev_idx, *slot), *page);
        }
        self.swap_to_page = new_swap_to_page;
        
        Ok(())
    }
    
    /// Swap out a page to swap space
    pub fn swap_out_page(&mut self, page_frame: PageFrame, page_data: &[u8; PAGE_SIZE]) -> Result<SwapSlot, SwapError> {
        // Check if page is already swapped out
        if self.page_to_swap.contains_key(&page_frame) {
            return Err(SwapError::SlotInUse);
        }
        
        // Find a device with free space
        for (device_index, allocator) in self.allocators.iter_mut().enumerate() {
            if let Some(slot) = allocator.allocate_slot() {
                // Try to write to the device
                match self.devices[device_index].write_page(slot, page_data) {
                    Ok(()) => {
                        // Success - update mappings
                        let swap_entry = SwapEntry {
                            device_index,
                            slot,
                        };
                        
                        self.page_to_swap.insert(page_frame, swap_entry);
                        self.swap_to_page.insert((device_index, slot), page_frame);
                        
                        // Update statistics
                        self.total_stats.used_slots += 1;
                        self.total_stats.free_slots -= 1;
                        
                        return Ok(slot);
                    }
                    Err(err) => {
                        // Failed to write - deallocate the slot
                        let _ = allocator.deallocate_slot(slot);
                        serial_println!("Failed to write page to swap device {}: {:?}", device_index, err);
                        continue;
                    }
                }
            }
        }
        
        Err(SwapError::NoSpace)
    }
    
    /// Swap in a page from swap space
    pub fn swap_in_page(&mut self, page_frame: PageFrame, page_data: &mut [u8; PAGE_SIZE]) -> Result<(), SwapError> {
        // Find the swap entry for this page
        let swap_entry = self.page_to_swap.get(&page_frame)
            .ok_or(SwapError::SlotNotInUse)?;
        
        let device_index = swap_entry.device_index;
        let slot = swap_entry.slot;
        
        // Read from the device
        self.devices[device_index].read_page(slot, page_data)?;
        
        // Deallocate the swap slot
        self.allocators[device_index].deallocate_slot(slot)?;
        
        // Remove mappings
        self.page_to_swap.remove(&page_frame);
        self.swap_to_page.remove(&(device_index, slot));
        
        // Update statistics
        self.total_stats.used_slots -= 1;
        self.total_stats.free_slots += 1;
        
        Ok(())
    }
    
    /// Check if a page is swapped out
    pub fn is_page_swapped(&self, page_frame: PageFrame) -> bool {
        self.page_to_swap.contains_key(&page_frame)
    }
    
    /// Get swap statistics
    pub fn stats(&self) -> SwapStats {
        self.total_stats
    }
    
    /// Get device count
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
    
    /// Get device statistics
    pub fn device_stats(&self, device_index: usize) -> Option<SwapStats> {
        self.allocators.get(device_index).map(|a| a.stats())
    }
    
    /// Print swap statistics
    pub fn print_stats(&self) {
        let stats = self.stats();
        
        serial_println!("Swap Space Statistics:");
        serial_println!("  Total: {} MB ({} slots)", stats.total_mb(), stats.total_slots);
        serial_println!("  Used:  {} MB ({} slots)", stats.used_mb(), stats.used_slots);
        serial_println!("  Free:  {} MB ({} slots)", stats.free_mb(), stats.free_slots);
        serial_println!("  Usage: {:.1}%", stats.usage_percent());
        serial_println!("  Devices: {}", self.device_count());
        
        for (i, device) in self.devices.iter().enumerate() {
            if let Some(device_stats) = self.device_stats(i) {
                serial_println!("    Device {}: '{}' - {} MB total, {} MB used", 
                               i, device.name(), device_stats.total_mb(), device_stats.used_mb());
            }
        }
        
        println!("Swap: {} MB total, {} MB free, {:.1}% used", 
                stats.total_mb(), stats.free_mb(), stats.usage_percent());
    }
}

/// Global swap manager instance
static SWAP_MANAGER: Mutex<Option<SwapManager>> = Mutex::new(None);

/// Initialize the global swap manager
pub fn init_swap_manager() -> Result<(), SwapError> {
    let manager = SwapManager::new();
    *SWAP_MANAGER.lock() = Some(manager);
    
    serial_println!("Swap manager initialized");
    Ok(())
}

/// Add a swap device to the global manager
pub fn add_swap_device(device: Box<dyn SwapDevice>) -> Result<usize, SwapError> {
    let mut manager_guard = SWAP_MANAGER.lock();
    let manager = manager_guard.as_mut().ok_or(SwapError::DeviceUnavailable)?;
    manager.add_device(device)
}

/// Remove a swap device from the global manager
pub fn remove_swap_device(device_index: usize) -> Result<(), SwapError> {
    let mut manager_guard = SWAP_MANAGER.lock();
    let manager = manager_guard.as_mut().ok_or(SwapError::DeviceUnavailable)?;
    manager.remove_device(device_index)
}

/// Swap out a page
pub fn swap_out_page(page_frame: PageFrame, page_data: &[u8; PAGE_SIZE]) -> Result<SwapSlot, SwapError> {
    let mut manager_guard = SWAP_MANAGER.lock();
    let manager = manager_guard.as_mut().ok_or(SwapError::DeviceUnavailable)?;
    manager.swap_out_page(page_frame, page_data)
}

/// Swap in a page
pub fn swap_in_page(page_frame: PageFrame, page_data: &mut [u8; PAGE_SIZE]) -> Result<(), SwapError> {
    let mut manager_guard = SWAP_MANAGER.lock();
    let manager = manager_guard.as_mut().ok_or(SwapError::DeviceUnavailable)?;
    manager.swap_in_page(page_frame, page_data)
}

/// Check if a page is swapped out
pub fn is_page_swapped(page_frame: PageFrame) -> bool {
    let manager_guard = SWAP_MANAGER.lock();
    if let Some(manager) = manager_guard.as_ref() {
        manager.is_page_swapped(page_frame)
    } else {
        false
    }
}

/// Get swap statistics
pub fn swap_stats() -> Option<SwapStats> {
    let manager_guard = SWAP_MANAGER.lock();
    manager_guard.as_ref().map(|m| m.stats())
}

/// Print swap statistics
pub fn print_swap_stats() {
    let manager_guard = SWAP_MANAGER.lock();
    if let Some(manager) = manager_guard.as_ref() {
        manager.print_stats();
    } else {
        serial_println!("Swap manager not initialized");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    
    /// Mock swap device for testing
    struct MockSwapDevice {
        name: &'static str,
        size: usize,
        available: bool,
        storage: Vec<[u8; PAGE_SIZE]>,
    }
    
    impl MockSwapDevice {
        fn new(name: &'static str, size_mb: usize) -> Self {
            let size = size_mb * 1024 * 1024;
            let slot_count = size / PAGE_SIZE;
            
            Self {
                name,
                size,
                available: true,
                storage: vec![[0u8; PAGE_SIZE]; slot_count],
            }
        }
    }
    
    impl SwapDevice for MockSwapDevice {
        fn device_type(&self) -> SwapDeviceType {
            SwapDeviceType::File
        }
        
        fn size(&self) -> usize {
            self.size
        }
        
        fn read_page(&mut self, slot: SwapSlot, buffer: &mut [u8; PAGE_SIZE]) -> Result<(), SwapError> {
            if !self.available {
                return Err(SwapError::DeviceUnavailable);
            }
            
            if slot.0 >= self.storage.len() {
                return Err(SwapError::InvalidSlot);
            }
            
            buffer.copy_from_slice(&self.storage[slot.0]);
            Ok(())
        }
        
        fn write_page(&mut self, slot: SwapSlot, buffer: &[u8; PAGE_SIZE]) -> Result<(), SwapError> {
            if !self.available {
                return Err(SwapError::DeviceUnavailable);
            }
            
            if slot.0 >= self.storage.len() {
                return Err(SwapError::InvalidSlot);
            }
            
            self.storage[slot.0].copy_from_slice(buffer);
            Ok(())
        }
        
        fn is_available(&self) -> bool {
            self.available
        }
        
        fn name(&self) -> &str {
            self.name
        }
    }
    
    #[test_case]
    fn test_swap_slot() {
        let slot = SwapSlot::new(42);
        assert_eq!(slot.slot(), 42);
        assert_eq!(slot.offset(), 42 * PAGE_SIZE);
    }
    
    #[test_case]
    fn test_swap_allocator() {
        let mut allocator = SwapAllocator::new(10);
        
        // Test initial state
        let stats = allocator.stats();
        assert_eq!(stats.total_slots, 10);
        assert_eq!(stats.free_slots, 10);
        assert_eq!(stats.used_slots, 0);
        
        // Test allocation
        let slot1 = allocator.allocate_slot().unwrap();
        assert_eq!(slot1.slot(), 0);
        assert!(allocator.is_slot_allocated(slot1));
        
        let slot2 = allocator.allocate_slot().unwrap();
        assert_eq!(slot2.slot(), 1);
        
        // Test deallocation
        allocator.deallocate_slot(slot1).unwrap();
        assert!(!allocator.is_slot_allocated(slot1));
        
        // Test allocation after deallocation
        let slot3 = allocator.allocate_slot().unwrap();
        assert_eq!(slot3.slot(), 0); // Should reuse the freed slot
    }
    
    #[test_case]
    fn test_swap_manager() {
        let mut manager = SwapManager::new();
        
        // Add a mock device
        let device = Box::new(MockSwapDevice::new("test_swap", 1)); // 1MB
        let device_index = manager.add_device(device).unwrap();
        assert_eq!(device_index, 0);
        
        // Test swap out
        let page_frame = PageFrame(100);
        let page_data = [0x42u8; PAGE_SIZE];
        let slot = manager.swap_out_page(page_frame, &page_data).unwrap();
        
        assert!(manager.is_page_swapped(page_frame));
        
        // Test swap in
        let mut read_data = [0u8; PAGE_SIZE];
        manager.swap_in_page(page_frame, &mut read_data).unwrap();
        
        assert_eq!(read_data, page_data);
        assert!(!manager.is_page_swapped(page_frame));
    }
    
    #[test_case]
    fn test_swap_stats() {
        let stats = SwapStats {
            total_slots: 1024,
            used_slots: 256,
            free_slots: 768,
        };
        
        assert_eq!(stats.total_bytes(), 1024 * PAGE_SIZE);
        assert_eq!(stats.used_bytes(), 256 * PAGE_SIZE);
        assert_eq!(stats.free_bytes(), 768 * PAGE_SIZE);
        assert_eq!(stats.usage_percent(), 25.0);
    }
}