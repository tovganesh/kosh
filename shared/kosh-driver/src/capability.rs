use alloc::{vec::Vec, string::String};
use kosh_types::{Capability, CapabilityFlags};

/// Types of capabilities that drivers can require or provide
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverCapabilityType {
    /// Hardware access capabilities
    Hardware(HardwareCapability),
    /// Memory access capabilities
    Memory(MemoryCapability),
    /// IPC capabilities
    Ipc(IpcCapability),
    /// File system capabilities
    FileSystem(FileSystemCapability),
    /// Network capabilities
    Network(NetworkCapability),
    /// Direct memory access capability
    MemoryAccess,
    /// Hardware access capability
    HardwareAccess,
    /// Text output capability
    TextOutput,
    /// Graphics output capability
    GraphicsOutput,
    /// Custom capability
    Custom(String),
}

/// Hardware access capabilities
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareCapability {
    /// Access to specific I/O ports
    IoPort { start: u16, end: u16 },
    /// Access to memory-mapped I/O regions
    MemoryMappedIo { start: u64, size: u64 },
    /// Access to specific IRQ lines
    Interrupt { irq: u32 },
    /// DMA access
    Dma { channel: u32 },
    /// PCI device access
    PciDevice { vendor_id: u32, device_id: u32 },
    /// Generic hardware access
    GenericHardware,
}

/// Memory access capabilities
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryCapability {
    /// Allocate physical memory
    PhysicalMemoryAlloc,
    /// Map physical memory to virtual addresses
    MemoryMapping,
    /// Access to specific memory regions
    MemoryRegion { start: u64, size: u64 },
    /// DMA-coherent memory allocation
    DmaMemory,
}

/// IPC capabilities
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcCapability {
    /// Send messages to specific process types
    SendTo(ProcessType),
    /// Receive messages from specific process types
    ReceiveFrom(ProcessType),
    /// Create IPC channels
    CreateChannel,
    /// Share memory regions
    SharedMemory,
}

/// File system capabilities
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSystemCapability {
    /// Read files from specific paths
    ReadPath(String),
    /// Write files to specific paths
    WritePath(String),
    /// Create files and directories
    Create,
    /// Delete files and directories
    Delete,
    /// Mount file systems
    Mount,
}

/// Network capabilities
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkCapability {
    /// Raw socket access
    RawSocket,
    /// Specific protocol access
    Protocol(NetworkProtocol),
    /// Network interface access
    Interface(String),
    /// Packet filtering
    PacketFilter,
}

/// Process types for IPC capabilities
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessType {
    Kernel,
    Driver,
    System,
    User,
    Specific(String),
}

/// Network protocols
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkProtocol {
    Tcp,
    Udp,
    Icmp,
    Raw,
    Custom(u16),
}

/// Capability manager for drivers
pub struct DriverCapabilityManager {
    granted_capabilities: Vec<DriverCapabilityType>,
}

impl DriverCapabilityManager {
    pub fn new() -> Self {
        Self {
            granted_capabilities: Vec::new(),
        }
    }

    /// Grant a capability to the driver
    pub fn grant_capability(&mut self, capability: DriverCapabilityType) {
        if !self.granted_capabilities.contains(&capability) {
            self.granted_capabilities.push(capability);
        }
    }

    /// Revoke a capability from the driver
    pub fn revoke_capability(&mut self, capability: &DriverCapabilityType) {
        self.granted_capabilities.retain(|c| c != capability);
    }

    /// Check if a capability is granted
    pub fn has_capability(&self, capability: &DriverCapabilityType) -> bool {
        self.granted_capabilities.contains(capability)
    }

    /// Get all granted capabilities
    pub fn get_capabilities(&self) -> &Vec<DriverCapabilityType> {
        &self.granted_capabilities
    }

    /// Convert driver capabilities to kernel capabilities
    pub fn to_kernel_capabilities(&self) -> Vec<Capability> {
        self.granted_capabilities
            .iter()
            .map(|cap| self.driver_capability_to_kernel(cap))
            .collect()
    }

    fn driver_capability_to_kernel(&self, capability: &DriverCapabilityType) -> Capability {
        let flags = match capability {
            DriverCapabilityType::Hardware(_) => CapabilityFlags::HARDWARE_ACCESS,
            DriverCapabilityType::Memory(_) => CapabilityFlags::READ_MEMORY | CapabilityFlags::WRITE_MEMORY,
            DriverCapabilityType::Ipc(_) => CapabilityFlags::IPC_SEND | CapabilityFlags::IPC_RECEIVE,
            DriverCapabilityType::FileSystem(_) => CapabilityFlags::FILE_READ | CapabilityFlags::FILE_WRITE,
            DriverCapabilityType::Network(_) => CapabilityFlags::NETWORK_ACCESS,
            DriverCapabilityType::MemoryAccess => CapabilityFlags::READ_MEMORY | CapabilityFlags::WRITE_MEMORY,
            DriverCapabilityType::HardwareAccess => CapabilityFlags::HARDWARE_ACCESS,
            DriverCapabilityType::TextOutput => CapabilityFlags::HARDWARE_ACCESS, // VGA buffer access
            DriverCapabilityType::GraphicsOutput => CapabilityFlags::HARDWARE_ACCESS,
            DriverCapabilityType::Custom(_) => CapabilityFlags::empty(),
        };

        Capability {
            flags,
            resource_id: None,
        }
    }
}

/// Validate that a driver's required capabilities are reasonable
pub fn validate_driver_capabilities(
    required: &[DriverCapabilityType],
    driver_type: crate::DriverType,
) -> Result<(), crate::DriverError> {
    use crate::{DriverType, DriverError};

    for capability in required {
        match (driver_type, capability) {
            // Storage drivers should only need storage-related capabilities
            (DriverType::Storage, DriverCapabilityType::Network(_)) => {
                return Err(DriverError::PermissionDenied);
            }
            // Network drivers shouldn't need direct hardware access to storage
            (DriverType::Network, DriverCapabilityType::Hardware(HardwareCapability::PciDevice { vendor_id, .. })) => {
                // Allow network cards (this is a simplified check)
                if *vendor_id != 0x8086 && *vendor_id != 0x10EC {
                    return Err(DriverError::PermissionDenied);
                }
            }
            // Graphics drivers need specific hardware access
            (DriverType::Graphics, DriverCapabilityType::Hardware(_)) => {
                // Graphics drivers typically need hardware access
            }
            _ => {
                // Most other combinations are allowed
            }
        }
    }

    Ok(())
}