//! Kosh Kernel Library
//! 
//! This library exposes kernel modules for testing and external use.

#![no_std]
#![feature(custom_test_frameworks)]

extern crate alloc;

// Simplified modules for library use (without console dependencies)
pub mod memory {
    pub use super::memory_impl::*;
}

// Internal memory implementation
mod memory_impl {
    /// Page size constant (4KB on x86-64)
    pub const PAGE_SIZE: usize = 4096;

    /// Convert bytes to pages (rounded up)
    pub const fn bytes_to_pages(bytes: usize) -> usize {
        (bytes + PAGE_SIZE - 1) / PAGE_SIZE
    }

    /// Convert pages to bytes
    pub const fn pages_to_bytes(pages: usize) -> usize {
        pages * PAGE_SIZE
    }

    /// Align address up to page boundary
    pub const fn align_up(addr: usize) -> usize {
        (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
    }

    /// Align address down to page boundary
    pub const fn align_down(addr: usize) -> usize {
        addr & !(PAGE_SIZE - 1)
    }

    /// Check if address is page-aligned
    pub const fn is_aligned(addr: usize) -> bool {
        addr & (PAGE_SIZE - 1) == 0
    }
}

// Basic types for testing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessId(u32);

impl ProcessId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
    
    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Zombie,
}

#[cfg(test)]
pub mod test_harness;
#[cfg(test)]
pub mod driver_tests;

// Test runner for library tests
#[cfg(test)]
pub fn test_runner(tests: &[&dyn Fn()]) {
    // Run built-in tests
    for test in tests {
        test();
    }
    
    // Run comprehensive test suite if available
    #[cfg(feature = "comprehensive_tests")]
    {
        use test_harness::KernelTestRunner;
        
        let mut runner = KernelTestRunner::new();
        
        // Register test modules
        if let Some(register_fn) = test_harness::get_memory_tests() {
            register_fn(&mut runner);
        }
        
        // Run all tests
        runner.run_all_tests();
    }
}