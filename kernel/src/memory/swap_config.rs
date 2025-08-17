use crate::memory::swap::{SwapDevice, SwapError, add_swap_device};
use crate::memory::swap::swap_file::{FileSwapDevice, PartitionSwapDevice};
use alloc::vec::Vec;
use alloc::string::String;
use alloc::boxed::Box;
use crate::{serial_println, println};

/// Swap configuration entry
#[derive(Debug, Clone)]
pub struct SwapConfig {
    /// Device type and configuration
    pub device_type: SwapDeviceConfig,
    /// Priority (higher numbers = higher priority)
    pub priority: u32,
    /// Whether to enable this swap device
    pub enabled: bool,
}

/// Swap device configuration types
#[derive(Debug, Clone)]
pub enum SwapDeviceConfig {
    /// File-based swap configuration
    File {
        /// File path
        path: String,
        /// Size in MB
        size_mb: usize,
    },
    /// Partition-based swap configuration
    Partition {
        /// Device path (e.g., "/dev/sda1")
        device_path: String,
        /// Partition ID
        partition_id: u32,
        /// Size in MB
        size_mb: usize,
    },
}

/// Swap configuration manager
pub struct SwapConfigManager {
    /// List of configured swap devices
    configs: Vec<SwapConfig>,
    /// Active device indices (sorted by priority)
    active_devices: Vec<usize>,
}

impl SwapConfigManager {
    /// Create a new swap configuration manager
    pub fn new() -> Self {
        Self {
            configs: Vec::new(),
            active_devices: Vec::new(),
        }
    }
    
    /// Add a swap configuration
    pub fn add_config(&mut self, config: SwapConfig) -> usize {
        let config_index = self.configs.len();
        self.configs.push(config);
        config_index
    }
    
    /// Remove a swap configuration
    pub fn remove_config(&mut self, config_index: usize) -> Result<(), SwapError> {
        if config_index >= self.configs.len() {
            return Err(SwapError::InvalidSlot);
        }
        
        // Remove from active devices if present
        self.active_devices.retain(|&idx| idx != config_index);
        
        // Remove the configuration
        self.configs.remove(config_index);
        
        // Update indices in active_devices
        for device_idx in &mut self.active_devices {
            if *device_idx > config_index {
                *device_idx -= 1;
            }
        }
        
        Ok(())
    }
    
    /// Enable a swap configuration
    pub fn enable_config(&mut self, config_index: usize) -> Result<(), SwapError> {
        if config_index >= self.configs.len() {
            return Err(SwapError::InvalidSlot);
        }
        
        self.configs[config_index].enabled = true;
        
        // Try to activate the device
        self.activate_device(config_index)
    }
    
    /// Disable a swap configuration
    pub fn disable_config(&mut self, config_index: usize) -> Result<(), SwapError> {
        if config_index >= self.configs.len() {
            return Err(SwapError::InvalidSlot);
        }
        
        self.configs[config_index].enabled = false;
        
        // Remove from active devices
        self.active_devices.retain(|&idx| idx != config_index);
        
        Ok(())
    }
    
    /// Activate a swap device from configuration
    fn activate_device(&mut self, config_index: usize) -> Result<(), SwapError> {
        if config_index >= self.configs.len() {
            return Err(SwapError::InvalidSlot);
        }
        
        let config = &self.configs[config_index];
        if !config.enabled {
            return Err(SwapError::DeviceUnavailable);
        }
        
        // Create the swap device based on configuration
        let device: Box<dyn SwapDevice> = match &config.device_type {
            SwapDeviceConfig::File { path, size_mb } => {
                serial_println!("Creating file-based swap device: {} ({} MB)", path, size_mb);
                Box::new(FileSwapDevice::new(path.clone(), *size_mb)?)
            }
            SwapDeviceConfig::Partition { device_path, partition_id, size_mb } => {
                serial_println!("Creating partition-based swap device: {} (partition {}, {} MB)", 
                               device_path, partition_id, size_mb);
                Box::new(PartitionSwapDevice::new(device_path.clone(), *partition_id, *size_mb)?)
            }
        };
        
        // Add device to the global swap manager
        let device_index = add_swap_device(device)?;
        
        // Add to active devices and sort by priority
        self.active_devices.push(config_index);
        self.active_devices.sort_by(|&a, &b| {
            self.configs[b].priority.cmp(&self.configs[a].priority)
        });
        
        serial_println!("Activated swap device {} with global index {}", config_index, device_index);
        
        Ok(())
    }
    
    /// Initialize all enabled swap devices
    pub fn initialize_all(&mut self) -> Result<usize, SwapError> {
        let mut activated_count = 0;
        
        serial_println!("Initializing swap devices from configuration...");
        
        for config_index in 0..self.configs.len() {
            if self.configs[config_index].enabled {
                match self.activate_device(config_index) {
                    Ok(()) => {
                        activated_count += 1;
                    }
                    Err(err) => {
                        serial_println!("Failed to activate swap device {}: {:?}", config_index, err);
                        // Continue with other devices
                    }
                }
            }
        }
        
        serial_println!("Activated {} swap devices", activated_count);
        println!("Swap: {} devices configured and activated", activated_count);
        
        Ok(activated_count)
    }
    
    /// Get configuration count
    pub fn config_count(&self) -> usize {
        self.configs.len()
    }
    
    /// Get active device count
    pub fn active_count(&self) -> usize {
        self.active_devices.len()
    }
    
    /// Get configuration by index
    pub fn get_config(&self, config_index: usize) -> Option<&SwapConfig> {
        self.configs.get(config_index)
    }
    
    /// Print configuration summary
    pub fn print_config(&self) {
        serial_println!("Swap Configuration:");
        serial_println!("  Total configs: {}", self.config_count());
        serial_println!("  Active devices: {}", self.active_count());
        
        for (i, config) in self.configs.iter().enumerate() {
            let status = if config.enabled { "enabled" } else { "disabled" };
            let active = if self.active_devices.contains(&i) { " (active)" } else { "" };
            
            match &config.device_type {
                SwapDeviceConfig::File { path, size_mb } => {
                    serial_println!("    {}: File '{}' - {} MB, priority {}, {}{}", 
                                   i, path, size_mb, config.priority, status, active);
                }
                SwapDeviceConfig::Partition { device_path, partition_id, size_mb } => {
                    serial_println!("    {}: Partition '{}' (ID {}) - {} MB, priority {}, {}{}", 
                                   i, device_path, partition_id, size_mb, config.priority, status, active);
                }
            }
        }
    }
}

/// Create a default swap configuration
pub fn create_default_config() -> SwapConfigManager {
    let mut manager = SwapConfigManager::new();
    
    // Add a default file-based swap (16MB for testing)
    let default_file_config = SwapConfig {
        device_type: SwapDeviceConfig::File {
            path: "/swap/swapfile".to_string(),
            size_mb: 16,
        },
        priority: 10,
        enabled: true,
    };
    
    manager.add_config(default_file_config);
    
    serial_println!("Created default swap configuration");
    
    manager
}

/// Create swap configuration from system detection
pub fn detect_swap_devices() -> SwapConfigManager {
    let mut manager = SwapConfigManager::new();
    
    // TODO: Implement actual device detection
    // For now, create some example configurations
    
    // Example file-based swap
    let file_config = SwapConfig {
        device_type: SwapDeviceConfig::File {
            path: "/var/swap/swapfile".to_string(),
            size_mb: 64,
        },
        priority: 5,
        enabled: false, // Disabled by default until file system is ready
    };
    manager.add_config(file_config);
    
    // Example partition-based swap (would be detected from partition table)
    let partition_config = SwapConfig {
        device_type: SwapDeviceConfig::Partition {
            device_path: "/dev/sda2".to_string(),
            partition_id: 2,
            size_mb: 128,
        },
        priority: 10, // Higher priority than file-based
        enabled: false, // Disabled until storage driver is ready
    };
    manager.add_config(partition_config);
    
    serial_println!("Detected {} potential swap devices", manager.config_count());
    
    manager
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test_case]
    fn test_swap_config_creation() {
        let config = SwapConfig {
            device_type: SwapDeviceConfig::File {
                path: "/test/swap".to_string(),
                size_mb: 32,
            },
            priority: 5,
            enabled: true,
        };
        
        assert!(config.enabled);
        assert_eq!(config.priority, 5);
        
        match config.device_type {
            SwapDeviceConfig::File { ref path, size_mb } => {
                assert_eq!(path, "/test/swap");
                assert_eq!(size_mb, 32);
            }
            _ => panic!("Wrong device type"),
        }
    }
    
    #[test_case]
    fn test_swap_config_manager() {
        let mut manager = SwapConfigManager::new();
        
        // Add configurations
        let config1 = SwapConfig {
            device_type: SwapDeviceConfig::File {
                path: "/swap1".to_string(),
                size_mb: 16,
            },
            priority: 5,
            enabled: true,
        };
        
        let config2 = SwapConfig {
            device_type: SwapDeviceConfig::Partition {
                device_path: "/dev/sda1".to_string(),
                partition_id: 1,
                size_mb: 32,
            },
            priority: 10,
            enabled: false,
        };
        
        let idx1 = manager.add_config(config1);
        let idx2 = manager.add_config(config2);
        
        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(manager.config_count(), 2);
        
        // Test enable/disable
        assert!(manager.get_config(idx1).unwrap().enabled);
        assert!(!manager.get_config(idx2).unwrap().enabled);
        
        manager.disable_config(idx1).unwrap();
        assert!(!manager.get_config(idx1).unwrap().enabled);
        
        manager.enable_config(idx2).unwrap();
        assert!(manager.get_config(idx2).unwrap().enabled);
    }
    
    #[test_case]
    fn test_default_config() {
        let manager = create_default_config();
        
        assert_eq!(manager.config_count(), 1);
        
        let config = manager.get_config(0).unwrap();
        assert!(config.enabled);
        assert_eq!(config.priority, 10);
        
        match &config.device_type {
            SwapDeviceConfig::File { path, size_mb } => {
                assert_eq!(path, "/swap/swapfile");
                assert_eq!(*size_mb, 16);
            }
            _ => panic!("Expected file config"),
        }
    }
    
    #[test_case]
    fn test_detect_swap_devices() {
        let manager = detect_swap_devices();
        
        assert_eq!(manager.config_count(), 2);
        
        // Both should be disabled by default
        assert!(!manager.get_config(0).unwrap().enabled);
        assert!(!manager.get_config(1).unwrap().enabled);
    }
}