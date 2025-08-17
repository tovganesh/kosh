//! Platform abstraction traits
//! 
//! This module defines the core traits that each platform implementation must provide.
//! These traits abstract away architecture-specific details and provide a unified
//! interface for the kernel.

use super::{
    CpuInfo, MemoryMap, VirtualAddress, PhysicalAddress, PageFlags, PlatformResult
};

/// Main platform interface trait
/// 
/// This trait defines the core operations that each platform must implement.
/// It provides hardware abstraction for CPU, memory management, and interrupt handling.
pub trait PlatformInterface: Send + Sync {
    /// Get CPU information for the current platform
    fn get_cpu_info(&self) -> CpuInfo;
    
    /// Get the memory map from the bootloader or firmware
    fn get_memory_map(&self) -> MemoryMap;
    
    /// Setup interrupt handling for the platform
    fn setup_interrupts(&mut self) -> PlatformResult<()>;
    
    /// Enable the Memory Management Unit (MMU)
    fn enable_mmu(&mut self, page_table_root: PhysicalAddress) -> PlatformResult<()>;
    
    /// Disable the MMU (used for shutdown or special operations)
    fn disable_mmu(&mut self) -> PlatformResult<()>;
    
    /// Flush Translation Lookaside Buffer (TLB)
    fn flush_tlb(&self) -> PlatformResult<()>;
    
    /// Flush TLB for a specific virtual address
    fn flush_tlb_address(&self, addr: VirtualAddress) -> PlatformResult<()>;
    
    /// Get the current page table root address
    fn get_page_table_root(&self) -> PhysicalAddress;
    
    /// Set the page table root address
    fn set_page_table_root(&mut self, root: PhysicalAddress) -> PlatformResult<()>;
    
    /// Perform cache operations
    fn cache_operations(&self) -> &dyn CacheOperations;
    
    /// Get platform-specific constants
    fn get_constants(&self) -> PlatformConstants;
}

/// Cache operations trait
pub trait CacheOperations: Send + Sync {
    /// Flush all caches
    fn flush_all(&self) -> PlatformResult<()>;
    
    /// Flush data cache
    fn flush_dcache(&self) -> PlatformResult<()>;
    
    /// Flush instruction cache
    fn flush_icache(&self) -> PlatformResult<()>;
    
    /// Invalidate data cache
    fn invalidate_dcache(&self) -> PlatformResult<()>;
    
    /// Invalidate instruction cache
    fn invalidate_icache(&self) -> PlatformResult<()>;
    
    /// Clean and invalidate data cache for a specific range
    fn clean_invalidate_dcache_range(&self, start: VirtualAddress, size: usize) -> PlatformResult<()>;
    
    /// Invalidate data cache for a specific range
    fn invalidate_dcache_range(&self, start: VirtualAddress, size: usize) -> PlatformResult<()>;
}

/// Memory management operations trait
pub trait MemoryManagement: Send + Sync {
    /// Create a new page table
    fn create_page_table(&self) -> PlatformResult<PhysicalAddress>;
    
    /// Map a virtual address to a physical address
    fn map_page(&mut self, 
                virtual_addr: VirtualAddress, 
                physical_addr: PhysicalAddress, 
                flags: PageFlags) -> PlatformResult<()>;
    
    /// Unmap a virtual address
    fn unmap_page(&mut self, virtual_addr: VirtualAddress) -> PlatformResult<()>;
    
    /// Get the physical address mapped to a virtual address
    fn translate_address(&self, virtual_addr: VirtualAddress) -> PlatformResult<PhysicalAddress>;
    
    /// Update page flags for a virtual address
    fn update_page_flags(&mut self, virtual_addr: VirtualAddress, flags: PageFlags) -> PlatformResult<()>;
    
    /// Check if a virtual address is mapped
    fn is_mapped(&self, virtual_addr: VirtualAddress) -> bool;
}

/// Context switching operations trait
pub trait ContextSwitching: Send + Sync {
    /// Save the current CPU context
    fn save_context(&self, context: &mut CpuContext) -> PlatformResult<()>;
    
    /// Restore a CPU context
    fn restore_context(&self, context: &CpuContext) -> PlatformResult<()>;
    
    /// Switch to a new context (combines save and restore)
    fn switch_context(&self, old_context: &mut CpuContext, new_context: &CpuContext) -> PlatformResult<()>;
    
    /// Create a new context for a process
    fn create_context(&self, entry_point: VirtualAddress, stack_pointer: VirtualAddress) -> CpuContext;
}

/// Interrupt handling operations trait
pub trait InterruptHandling: Send + Sync {
    /// Enable interrupts globally
    fn enable_interrupts(&self);
    
    /// Disable interrupts globally
    fn disable_interrupts(&self);
    
    /// Check if interrupts are enabled
    fn interrupts_enabled(&self) -> bool;
    
    /// Register an interrupt handler
    fn register_interrupt_handler(&mut self, interrupt_number: u8, handler: InterruptHandler) -> PlatformResult<()>;
    
    /// Unregister an interrupt handler
    fn unregister_interrupt_handler(&mut self, interrupt_number: u8) -> PlatformResult<()>;
    
    /// Send an End of Interrupt (EOI) signal
    fn send_eoi(&self, interrupt_number: u8) -> PlatformResult<()>;
}

/// Timer operations trait
pub trait TimerOperations: Send + Sync {
    /// Get the current system time in nanoseconds
    fn get_system_time(&self) -> u64;
    
    /// Set up a periodic timer
    fn setup_periodic_timer(&mut self, frequency_hz: u32) -> PlatformResult<()>;
    
    /// Set up a one-shot timer
    fn setup_oneshot_timer(&mut self, nanoseconds: u64) -> PlatformResult<()>;
    
    /// Stop the timer
    fn stop_timer(&mut self) -> PlatformResult<()>;
}

/// Platform-specific constants
#[derive(Debug, Clone, Copy)]
pub struct PlatformConstants {
    pub page_size: usize,
    pub page_shift: usize,
    pub virtual_address_bits: usize,
    pub physical_address_bits: usize,
    pub cache_line_size: usize,
    pub max_interrupt_number: u8,
}

/// CPU context structure (platform-specific)
#[derive(Debug, Clone)]
pub struct CpuContext {
    pub registers: PlatformRegisters,
    pub stack_pointer: VirtualAddress,
    pub instruction_pointer: VirtualAddress,
    pub flags: u64,
}

/// Platform-specific register storage
#[derive(Debug, Clone)]
pub enum PlatformRegisters {
    X86_64(crate::platform::x86_64::X86_64Registers),
    AArch64(crate::platform::aarch64::AArch64Registers),
    Unsupported,
}

/// Interrupt handler function type
pub type InterruptHandler = fn(interrupt_number: u8, context: &mut CpuContext);

/// Power management operations trait
pub trait PowerManagement: Send + Sync {
    /// Put the CPU into an idle state
    fn cpu_idle(&self);
    
    /// Halt the CPU
    fn cpu_halt(&self) -> !;
    
    /// Reset the system
    fn system_reset(&self) -> !;
    
    /// Shutdown the system
    fn system_shutdown(&self) -> !;
    
    /// Set CPU frequency (if supported)
    fn set_cpu_frequency(&mut self, frequency_mhz: u32) -> PlatformResult<()>;
    
    /// Get current CPU frequency
    fn get_cpu_frequency(&self) -> u32;
    
    /// Enable/disable CPU cores (if supported)
    fn set_core_state(&mut self, core_id: u32, enabled: bool) -> PlatformResult<()>;
}

/// I/O operations trait
pub trait IoOperations: Send + Sync {
    /// Read from an I/O port (x86-specific, no-op on other architectures)
    fn port_read_u8(&self, port: u16) -> u8;
    fn port_read_u16(&self, port: u16) -> u16;
    fn port_read_u32(&self, port: u16) -> u32;
    
    /// Write to an I/O port (x86-specific, no-op on other architectures)
    fn port_write_u8(&self, port: u16, value: u8);
    fn port_write_u16(&self, port: u16, value: u16);
    fn port_write_u32(&self, port: u16, value: u32);
    
    /// Memory-mapped I/O operations
    fn mmio_read_u8(&self, addr: PhysicalAddress) -> u8;
    fn mmio_read_u16(&self, addr: PhysicalAddress) -> u16;
    fn mmio_read_u32(&self, addr: PhysicalAddress) -> u32;
    fn mmio_read_u64(&self, addr: PhysicalAddress) -> u64;
    
    fn mmio_write_u8(&self, addr: PhysicalAddress, value: u8);
    fn mmio_write_u16(&self, addr: PhysicalAddress, value: u16);
    fn mmio_write_u32(&self, addr: PhysicalAddress, value: u32);
    fn mmio_write_u64(&self, addr: PhysicalAddress, value: u64);
}