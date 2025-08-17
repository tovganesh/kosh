//! Platform Abstraction Layer (PAL)
//! 
//! This module provides platform-independent interfaces for hardware-specific operations.
//! It abstracts away architecture-specific details and provides a unified interface
//! for the kernel to interact with different hardware platforms.

use core::fmt;

pub mod traits;
pub mod x86_64;
pub mod aarch64;

#[cfg(test)]
pub mod tests;

// Re-export platform-specific implementations
#[cfg(target_arch = "x86_64")]
pub use self::x86_64::*;

#[cfg(target_arch = "aarch64")]
pub use self::aarch64::*;

/// CPU architecture information
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuArchitecture {
    X86_64,
    AArch64,
}

/// CPU feature flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuFeatures {
    pub has_mmu: bool,
    pub has_cache: bool,
    pub has_fpu: bool,
    pub has_simd: bool,
    pub has_virtualization: bool,
    pub has_security_extensions: bool,
}

/// CPU information structure
#[derive(Debug, Clone)]
pub struct CpuInfo {
    pub architecture: CpuArchitecture,
    pub vendor: &'static str,
    pub model_name: &'static str,
    pub core_count: u32,
    pub cache_line_size: u32,
    pub features: CpuFeatures,
}

/// Memory region type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRegionType {
    Available,
    Reserved,
    AcpiReclaimable,
    AcpiNvs,
    BadMemory,
    Bootloader,
    Kernel,
    Module,
}

/// Memory region descriptor
#[derive(Debug, Clone, Copy)]
pub struct MemoryRegion {
    pub start_addr: u64,
    pub size: u64,
    pub region_type: MemoryRegionType,
}

/// Memory map containing all memory regions
#[derive(Debug)]
pub struct MemoryMap {
    pub regions: &'static [MemoryRegion],
    pub total_memory: u64,
    pub available_memory: u64,
}

/// Page table entry flags (generic across architectures)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageFlags {
    pub present: bool,
    pub writable: bool,
    pub user_accessible: bool,
    pub write_through: bool,
    pub cache_disabled: bool,
    pub accessed: bool,
    pub dirty: bool,
    pub executable: bool,
}

impl Default for PageFlags {
    fn default() -> Self {
        Self {
            present: true,
            writable: false,
            user_accessible: false,
            write_through: false,
            cache_disabled: false,
            accessed: false,
            dirty: false,
            executable: false,
        }
    }
}

/// Virtual address type
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtualAddress(pub u64);

impl VirtualAddress {
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }
    
    pub const fn as_u64(self) -> u64 {
        self.0
    }
    
    pub const fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }
    
    pub const fn as_mut_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }
}

impl fmt::Display for VirtualAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "0x{:016x}", self.0)
    }
}

/// Physical address type
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysicalAddress(pub u64);

impl PhysicalAddress {
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }
    
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PhysicalAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "0x{:016x}", self.0)
    }
}

/// Platform-specific error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformError {
    InvalidAddress,
    InvalidPageFlags,
    MmuNotSupported,
    InterruptSetupFailed,
    CacheOperationFailed,
    UnsupportedOperation,
    HardwareError,
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PlatformError::InvalidAddress => write!(f, "Invalid address"),
            PlatformError::InvalidPageFlags => write!(f, "Invalid page flags"),
            PlatformError::MmuNotSupported => write!(f, "MMU not supported"),
            PlatformError::InterruptSetupFailed => write!(f, "Interrupt setup failed"),
            PlatformError::CacheOperationFailed => write!(f, "Cache operation failed"),
            PlatformError::UnsupportedOperation => write!(f, "Unsupported operation"),
            PlatformError::HardwareError => write!(f, "Hardware error"),
        }
    }
}

pub type PlatformResult<T> = Result<T, PlatformError>;

/// Initialize the platform abstraction layer
pub fn init() -> PlatformResult<()> {
    #[cfg(target_arch = "x86_64")]
    return x86_64::init();
    
    #[cfg(target_arch = "aarch64")]
    return aarch64::init();
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    Err(PlatformError::UnsupportedOperation)
}

/// Get the current platform implementation
pub fn current_platform() -> &'static dyn traits::PlatformInterface {
    #[cfg(target_arch = "x86_64")]
    return x86_64::get_platform();
    
    #[cfg(target_arch = "aarch64")]
    return aarch64::get_platform();
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    panic!("Unsupported platform");
}