//! Platform abstraction layer tests

use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_platform_initialization() {
        // Test that platform can be initialized
        let result = init();
        assert!(result.is_ok(), "Platform initialization should succeed");
    }
    
    #[test]
    fn test_platform_interface() {
        // Initialize platform
        let _ = init();
        let platform = current_platform();
        
        // Test CPU info
        let cpu_info = platform.get_cpu_info();
        assert!(matches!(cpu_info.architecture, CpuArchitecture::X86_64 | CpuArchitecture::AArch64));
        assert!(cpu_info.cache_line_size > 0);
        assert!(cpu_info.core_count > 0);
        
        // Test memory map
        let memory_map = platform.get_memory_map();
        assert!(memory_map.total_memory > 0);
        assert!(memory_map.available_memory > 0);
        assert!(!memory_map.regions.is_empty());
        
        // Test platform constants
        let constants = platform.get_constants();
        assert!(constants.page_size > 0);
        assert!(constants.page_shift > 0);
        assert!(constants.virtual_address_bits > 0);
        assert!(constants.physical_address_bits > 0);
        assert!(constants.cache_line_size > 0);
    }
    
    #[test]
    fn test_virtual_address() {
        let addr = VirtualAddress::new(0x1000);
        assert_eq!(addr.as_u64(), 0x1000);
        assert_eq!(addr.as_ptr::<u8>() as u64, 0x1000);
        assert_eq!(addr.as_mut_ptr::<u8>() as u64, 0x1000);
    }
    
    #[test]
    fn test_physical_address() {
        let addr = PhysicalAddress::new(0x2000);
        assert_eq!(addr.as_u64(), 0x2000);
    }
    
    #[test]
    fn test_page_flags() {
        let flags = PageFlags::default();
        assert!(flags.present);
        assert!(!flags.writable);
        assert!(!flags.user_accessible);
        
        let mut custom_flags = PageFlags::default();
        custom_flags.writable = true;
        custom_flags.user_accessible = true;
        assert!(custom_flags.present);
        assert!(custom_flags.writable);
        assert!(custom_flags.user_accessible);
    }
    
    #[test]
    fn test_memory_region_types() {
        use MemoryRegionType::*;
        
        let region = MemoryRegion {
            start_addr: 0x1000,
            size: 0x1000,
            region_type: Available,
        };
        
        assert_eq!(region.start_addr, 0x1000);
        assert_eq!(region.size, 0x1000);
        assert_eq!(region.region_type, Available);
    }
    
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_x86_64_specific() {
        use crate::platform::x86_64::registers::X86_64Registers;
        
        let regs = X86_64Registers::new_user_mode(0x1000, 0x2000);
        assert_eq!(regs.rip, 0x1000);
        assert_eq!(regs.rsp, 0x2000);
        assert_eq!(regs.cs, 0x1B); // User code segment
        
        let mut regs = X86_64Registers::default();
        regs.set_syscall_args(&[1, 2, 3, 4, 5, 6]);
        let args = regs.get_syscall_args();
        assert_eq!(args, [1, 2, 3, 4, 5, 6]);
        
        regs.set_syscall_return(42);
        assert_eq!(regs.rax, 42);
    }
    
    #[cfg(target_arch = "aarch64")]
    #[test]
    fn test_aarch64_specific() {
        use crate::platform::aarch64::registers::AArch64Registers;
        
        let regs = AArch64Registers::new_user_mode(0x1000, 0x2000);
        assert_eq!(regs.pc, 0x1000);
        assert_eq!(regs.sp, 0x2000);
        assert_eq!(regs.pstate, 0x0); // User mode
        
        let mut regs = AArch64Registers::default();
        regs.set_syscall_args(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let args = regs.get_syscall_args();
        assert_eq!(args, [1, 2, 3, 4, 5, 6, 7, 8]);
        
        regs.set_syscall_return(42);
        assert_eq!(regs.x[0], 42);
    }
}