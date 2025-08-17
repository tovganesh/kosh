//! x86-64 platform implementation
//! 
//! This module provides the x86-64 specific implementation of the platform abstraction layer.

use super::traits::*;
use super::{
    CpuInfo, CpuArchitecture, CpuFeatures, MemoryMap, MemoryRegion, MemoryRegionType,
    VirtualAddress, PhysicalAddress, PageFlags, PlatformResult, PlatformError
};
use core::sync::atomic::{AtomicBool, Ordering};

pub mod registers;
pub mod memory;
pub mod interrupts;
pub mod cache;
pub mod context;
pub mod timer;
pub mod power;
pub mod io;

pub use registers::X86_64Registers;

/// x86-64 platform implementation
pub struct X86_64Platform {
    initialized: AtomicBool,
    memory_mgmt: memory::X86_64MemoryManagement,
    interrupt_handler: interrupts::X86_64InterruptHandler,
    cache_ops: cache::X86_64CacheOperations,
    context_switcher: context::X86_64ContextSwitching,
    timer_ops: timer::X86_64TimerOperations,
    power_mgmt: power::X86_64PowerManagement,
    io_ops: io::X86_64IoOperations,
}

static mut PLATFORM_INSTANCE: Option<X86_64Platform> = None;
static PLATFORM_INIT: AtomicBool = AtomicBool::new(false);

impl X86_64Platform {
    fn new() -> Self {
        Self {
            initialized: AtomicBool::new(false),
            memory_mgmt: memory::X86_64MemoryManagement::new(),
            interrupt_handler: interrupts::X86_64InterruptHandler::new(),
            cache_ops: cache::X86_64CacheOperations::new(),
            context_switcher: context::X86_64ContextSwitching::new(),
            timer_ops: timer::X86_64TimerOperations::new(),
            power_mgmt: power::X86_64PowerManagement::new(),
            io_ops: io::X86_64IoOperations::new(),
        }
    }
}

impl PlatformInterface for X86_64Platform {
    fn get_cpu_info(&self) -> CpuInfo {
        // Use CPUID instruction to get CPU information
        let cpuid = raw_cpuid::CpuId::new();
        
        let vendor = if let Some(vendor_info) = cpuid.get_vendor_info() {
            match vendor_info.as_str() {
                "GenuineIntel" => "Intel",
                "AuthenticAMD" => "AMD",
                _ => "Unknown",
            }
        } else {
            "Unknown"
        };
        
        let model_name = "x86-64 CPU"; // Simplified for now
        
        let core_count = if let Some(feature_info) = cpuid.get_feature_info() {
            // This is a simplified approach - real implementation would be more complex
            1 // Default to 1 core for now
        } else {
            1
        };
        
        let features = CpuFeatures {
            has_mmu: true, // x86-64 always has MMU
            has_cache: true,
            has_fpu: true,
            has_simd: true, // SSE is standard on x86-64
            has_virtualization: false, // Would need to check CPUID
            has_security_extensions: false, // Would need to check for SMEP/SMAP etc.
        };
        
        CpuInfo {
            architecture: CpuArchitecture::X86_64,
            vendor,
            model_name,
            core_count,
            cache_line_size: 64, // Standard for x86-64
            features,
        }
    }
    
    fn get_memory_map(&self) -> MemoryMap {
        // This would normally parse the multiboot2 memory map
        // For now, return a minimal memory map
        static REGIONS: [MemoryRegion; 1] = [
            MemoryRegion {
                start_addr: 0x100000, // 1MB
                size: 0x7F00000,      // 127MB (128MB total - 1MB)
                region_type: MemoryRegionType::Available,
            }
        ];
        
        MemoryMap {
            regions: &REGIONS,
            total_memory: 128 * 1024 * 1024, // 128MB
            available_memory: 127 * 1024 * 1024, // 127MB
        }
    }
    
    fn setup_interrupts(&mut self) -> PlatformResult<()> {
        self.interrupt_handler.setup_interrupts()
    }
    
    fn enable_mmu(&mut self, page_table_root: PhysicalAddress) -> PlatformResult<()> {
        self.memory_mgmt.enable_mmu(page_table_root)
    }
    
    fn disable_mmu(&mut self) -> PlatformResult<()> {
        self.memory_mgmt.disable_mmu()
    }
    
    fn flush_tlb(&self) -> PlatformResult<()> {
        self.memory_mgmt.flush_tlb()
    }
    
    fn flush_tlb_address(&self, addr: VirtualAddress) -> PlatformResult<()> {
        self.memory_mgmt.flush_tlb_address(addr)
    }
    
    fn get_page_table_root(&self) -> PhysicalAddress {
        self.memory_mgmt.get_page_table_root()
    }
    
    fn set_page_table_root(&mut self, root: PhysicalAddress) -> PlatformResult<()> {
        self.memory_mgmt.set_page_table_root(root)
    }
    
    fn cache_operations(&self) -> &dyn CacheOperations {
        &self.cache_ops
    }
    
    fn get_constants(&self) -> PlatformConstants {
        PlatformConstants {
            page_size: 4096,
            page_shift: 12,
            virtual_address_bits: 48,
            physical_address_bits: 52,
            cache_line_size: 64,
            max_interrupt_number: 255,
        }
    }
}

/// Initialize the x86-64 platform
pub fn init() -> PlatformResult<()> {
    if PLATFORM_INIT.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
        unsafe {
            PLATFORM_INSTANCE = Some(X86_64Platform::new());
            if let Some(ref mut platform) = PLATFORM_INSTANCE {
                platform.initialized.store(true, Ordering::SeqCst);
                return Ok(());
            }
        }
    }
    Err(PlatformError::HardwareError)
}

/// Get the current platform instance
pub fn get_platform() -> &'static dyn PlatformInterface {
    unsafe {
        PLATFORM_INSTANCE.as_ref()
            .expect("Platform not initialized")
    }
}