//! ARM64 I/O operations implementation (stub)

use super::super::traits::IoOperations;
use super::super::PhysicalAddress;

/// ARM64 I/O operations implementation (stub)
pub struct AArch64IoOperations;

impl AArch64IoOperations {
    pub fn new() -> Self {
        Self
    }
}

impl IoOperations for AArch64IoOperations {
    // ARM64 doesn't have port I/O like x86, so these are no-ops
    fn port_read_u8(&self, port: u16) -> u8 {
        0
    }
    
    fn port_read_u16(&self, port: u16) -> u16 {
        0
    }
    
    fn port_read_u32(&self, port: u16) -> u32 {
        0
    }
    
    fn port_write_u8(&self, port: u16, value: u8) {
        // No-op on ARM64
    }
    
    fn port_write_u16(&self, port: u16, value: u16) {
        // No-op on ARM64
    }
    
    fn port_write_u32(&self, port: u16, value: u32) {
        // No-op on ARM64
    }
    
    // Memory-mapped I/O is the primary I/O method on ARM64
    fn mmio_read_u8(&self, addr: PhysicalAddress) -> u8 {
        unsafe {
            core::ptr::read_volatile(addr.as_u64() as *const u8)
        }
    }
    
    fn mmio_read_u16(&self, addr: PhysicalAddress) -> u16 {
        unsafe {
            core::ptr::read_volatile(addr.as_u64() as *const u16)
        }
    }
    
    fn mmio_read_u32(&self, addr: PhysicalAddress) -> u32 {
        unsafe {
            core::ptr::read_volatile(addr.as_u64() as *const u32)
        }
    }
    
    fn mmio_read_u64(&self, addr: PhysicalAddress) -> u64 {
        unsafe {
            core::ptr::read_volatile(addr.as_u64() as *const u64)
        }
    }
    
    fn mmio_write_u8(&self, addr: PhysicalAddress, value: u8) {
        unsafe {
            core::ptr::write_volatile(addr.as_u64() as *mut u8, value);
        }
    }
    
    fn mmio_write_u16(&self, addr: PhysicalAddress, value: u16) {
        unsafe {
            core::ptr::write_volatile(addr.as_u64() as *mut u16, value);
        }
    }
    
    fn mmio_write_u32(&self, addr: PhysicalAddress, value: u32) {
        unsafe {
            core::ptr::write_volatile(addr.as_u64() as *mut u32, value);
        }
    }
    
    fn mmio_write_u64(&self, addr: PhysicalAddress, value: u64) {
        unsafe {
            core::ptr::write_volatile(addr.as_u64() as *mut u64, value);
        }
    }
}