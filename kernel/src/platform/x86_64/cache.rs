//! x86-64 cache operations implementation

use core::arch::asm;
use super::super::traits::CacheOperations;
use super::super::{VirtualAddress, PlatformResult};

/// x86-64 cache operations implementation
pub struct X86_64CacheOperations;

impl X86_64CacheOperations {
    pub fn new() -> Self {
        Self
    }
}

impl CacheOperations for X86_64CacheOperations {
    fn flush_all(&self) -> PlatformResult<()> {
        unsafe {
            // WBINVD instruction flushes and invalidates all caches
            asm!("wbinvd");
        }
        Ok(())
    }
    
    fn flush_dcache(&self) -> PlatformResult<()> {
        // x86-64 has unified caches, so flush all
        self.flush_all()
    }
    
    fn flush_icache(&self) -> PlatformResult<()> {
        // x86-64 has unified caches, so flush all
        self.flush_all()
    }
    
    fn invalidate_dcache(&self) -> PlatformResult<()> {
        unsafe {
            // INVD instruction invalidates caches without writing back
            asm!("invd");
        }
        Ok(())
    }
    
    fn invalidate_icache(&self) -> PlatformResult<()> {
        // x86-64 has unified caches
        self.invalidate_dcache()
    }
    
    fn clean_invalidate_dcache_range(&self, start: VirtualAddress, size: usize) -> PlatformResult<()> {
        // x86-64 doesn't have cache line operations like ARM
        // Use CLFLUSH for individual cache lines
        unsafe {
            let cache_line_size = 64; // Standard x86-64 cache line size
            let start_addr = start.as_u64() & !(cache_line_size - 1); // Align to cache line
            let end_addr = (start.as_u64() + size as u64 + cache_line_size - 1) & !(cache_line_size - 1);
            
            let mut addr = start_addr;
            while addr < end_addr {
                asm!("clflush [{}]", in(reg) addr);
                addr += cache_line_size;
            }
            
            // Memory fence to ensure completion
            asm!("mfence");
        }
        Ok(())
    }
    
    fn invalidate_dcache_range(&self, start: VirtualAddress, size: usize) -> PlatformResult<()> {
        // x86-64 CLFLUSH both flushes and invalidates
        self.clean_invalidate_dcache_range(start, size)
    }
}