use crate::memory::{PAGE_SIZE, swap::{SwapDevice, SwapDeviceType, SwapSlot, SwapError}};
use alloc::{vec, vec::Vec};
use alloc::string::{String, ToString};

/// File-based swap device implementation
pub struct FileSwapDevice {
    /// Device name/path
    name: String,
    /// Total size in bytes
    size: usize,
    /// Storage buffer (simulating file storage)
    storage: Vec<u8>,
    /// Whether the device is available
    available: bool,
}

impl FileSwapDevice {
    /// Create a new file-based swap device
    pub fn new(name: String, size_mb: usize) -> Result<Self, SwapError> {
        if size_mb == 0 {
            return Err(SwapError::InvalidSlot);
        }
        
        let size = size_mb * 1024 * 1024;
        
        // Ensure size is page-aligned
        let aligned_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        
        // Initialize storage buffer
        let storage = vec![0u8; aligned_size];
        
        Ok(Self {
            name,
            size: aligned_size,
            storage,
            available: true,
        })
    }
    
    /// Create a swap device from existing storage
    pub fn from_storage(name: String, storage: Vec<u8>) -> Result<Self, SwapError> {
        if storage.is_empty() {
            return Err(SwapError::InvalidSlot);
        }
        
        // Ensure size is page-aligned
        let size = storage.len();
        if size % PAGE_SIZE != 0 {
            return Err(SwapError::InvalidSlot);
        }
        
        Ok(Self {
            name,
            size,
            storage,
            available: true,
        })
    }
    
    /// Set device availability
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }
    
    /// Get the storage buffer (for testing)
    #[cfg(test)]
    pub fn storage(&self) -> &[u8] {
        &self.storage
    }
}

impl SwapDevice for FileSwapDevice {
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
        
        let offset = slot.offset();
        if offset + PAGE_SIZE > self.size {
            return Err(SwapError::InvalidSlot);
        }
        
        // Copy data from storage to buffer
        buffer.copy_from_slice(&self.storage[offset..offset + PAGE_SIZE]);
        
        Ok(())
    }
    
    fn write_page(&mut self, slot: SwapSlot, buffer: &[u8; PAGE_SIZE]) -> Result<(), SwapError> {
        if !self.available {
            return Err(SwapError::DeviceUnavailable);
        }
        
        let offset = slot.offset();
        if offset + PAGE_SIZE > self.size {
            return Err(SwapError::InvalidSlot);
        }
        
        // Copy data from buffer to storage
        self.storage[offset..offset + PAGE_SIZE].copy_from_slice(buffer);
        
        Ok(())
    }
    
    fn is_available(&self) -> bool {
        self.available
    }
    
    fn name(&self) -> &str {
        &self.name
    }
}

/// Partition-based swap device implementation (placeholder)
pub struct PartitionSwapDevice {
    /// Device name/path
    name: String,
    /// Total size in bytes
    size: usize,
    /// Partition identifier
    partition_id: u32,
    /// Whether the device is available
    available: bool,
}

impl PartitionSwapDevice {
    /// Create a new partition-based swap device
    pub fn new(name: String, partition_id: u32, size_mb: usize) -> Result<Self, SwapError> {
        if size_mb == 0 {
            return Err(SwapError::InvalidSlot);
        }
        
        let size = size_mb * 1024 * 1024;
        
        // Ensure size is page-aligned
        let aligned_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        
        Ok(Self {
            name,
            size: aligned_size,
            partition_id,
            available: true,
        })
    }
    
    /// Set device availability
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }
    
    /// Get partition ID
    pub fn partition_id(&self) -> u32 {
        self.partition_id
    }
}

impl SwapDevice for PartitionSwapDevice {
    fn device_type(&self) -> SwapDeviceType {
        SwapDeviceType::Partition
    }
    
    fn size(&self) -> usize {
        self.size
    }
    
    fn read_page(&mut self, _slot: SwapSlot, _buffer: &mut [u8; PAGE_SIZE]) -> Result<(), SwapError> {
        if !self.available {
            return Err(SwapError::DeviceUnavailable);
        }
        
        // TODO: Implement actual partition I/O
        // For now, this is a placeholder that would interface with storage drivers
        Err(SwapError::IoError)
    }
    
    fn write_page(&mut self, _slot: SwapSlot, _buffer: &[u8; PAGE_SIZE]) -> Result<(), SwapError> {
        if !self.available {
            return Err(SwapError::DeviceUnavailable);
        }
        
        // TODO: Implement actual partition I/O
        // For now, this is a placeholder that would interface with storage drivers
        Err(SwapError::IoError)
    }
    
    fn is_available(&self) -> bool {
        self.available
    }
    
    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    
    #[test_case]
    fn test_file_swap_device_creation() {
        let device = FileSwapDevice::new("test_swap".to_string(), 1).unwrap();
        
        assert_eq!(device.name(), "test_swap");
        assert_eq!(device.size(), 1024 * 1024); // 1MB
        assert_eq!(device.slot_count(), 256); // 1MB / 4KB
        assert_eq!(device.device_type(), SwapDeviceType::File);
        assert!(device.is_available());
    }
    
    #[test_case]
    fn test_file_swap_device_io() {
        let mut device = FileSwapDevice::new("test_swap".to_string(), 1).unwrap();
        
        // Test write and read
        let slot = SwapSlot::new(0);
        let write_data = [0x42u8; PAGE_SIZE];
        let mut read_data = [0u8; PAGE_SIZE];
        
        // Write page
        device.write_page(slot, &write_data).unwrap();
        
        // Read page back
        device.read_page(slot, &mut read_data).unwrap();
        
        // Verify data matches
        assert_eq!(write_data, read_data);
    }
    
    #[test_case]
    fn test_file_swap_device_invalid_slot() {
        let mut device = FileSwapDevice::new("test_swap".to_string(), 1).unwrap();
        
        // Try to access slot beyond device capacity
        let invalid_slot = SwapSlot::new(device.slot_count());
        let data = [0u8; PAGE_SIZE];
        let mut buffer = [0u8; PAGE_SIZE];
        
        assert_eq!(device.write_page(invalid_slot, &data), Err(SwapError::InvalidSlot));
        assert_eq!(device.read_page(invalid_slot, &mut buffer), Err(SwapError::InvalidSlot));
    }
    
    #[test_case]
    fn test_file_swap_device_unavailable() {
        let mut device = FileSwapDevice::new("test_swap".to_string(), 1).unwrap();
        device.set_available(false);
        
        let slot = SwapSlot::new(0);
        let data = [0u8; PAGE_SIZE];
        let mut buffer = [0u8; PAGE_SIZE];
        
        assert_eq!(device.write_page(slot, &data), Err(SwapError::DeviceUnavailable));
        assert_eq!(device.read_page(slot, &mut buffer), Err(SwapError::DeviceUnavailable));
        assert!(!device.is_available());
    }
    
    #[test_case]
    fn test_partition_swap_device_creation() {
        let device = PartitionSwapDevice::new("sda1".to_string(), 1, 2).unwrap();
        
        assert_eq!(device.name(), "sda1");
        assert_eq!(device.size(), 2 * 1024 * 1024); // 2MB
        assert_eq!(device.slot_count(), 512); // 2MB / 4KB
        assert_eq!(device.device_type(), SwapDeviceType::Partition);
        assert_eq!(device.partition_id(), 1);
        assert!(device.is_available());
    }
    
    #[test_case]
    fn test_from_storage() {
        let storage = vec![0u8; PAGE_SIZE * 2]; // 2 pages
        let device = FileSwapDevice::from_storage("test".to_string(), storage).unwrap();
        
        assert_eq!(device.size(), PAGE_SIZE * 2);
        assert_eq!(device.slot_count(), 2);
    }
    
    #[test_case]
    fn test_from_storage_invalid_size() {
        let storage = vec![0u8; PAGE_SIZE + 1]; // Not page-aligned
        let result = FileSwapDevice::from_storage("test".to_string(), storage);
        
        assert_eq!(result, Err(SwapError::InvalidSlot));
    }
}