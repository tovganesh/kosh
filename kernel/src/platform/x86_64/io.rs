//! x86-64 I/O operations implementation

use core::arch::asm;
use super::super::traits::IoOperations;
use super::super::PhysicalAddress;

/// x86-64 I/O operations implementation
pub struct X86_64IoOperations;

impl X86_64IoOperations {
    pub fn new() -> Self {
        Self
    }
}

impl IoOperations for X86_64IoOperations {
    fn port_read_u8(&self, port: u16) -> u8 {
        unsafe {
            let value: u8;
            asm!("in al, dx", out("al") value, in("dx") port);
            value
        }
    }
    
    fn port_read_u16(&self, port: u16) -> u16 {
        unsafe {
            let value: u16;
            asm!("in ax, dx", out("ax") value, in("dx") port);
            value
        }
    }
    
    fn port_read_u32(&self, port: u16) -> u32 {
        unsafe {
            let value: u32;
            asm!("in eax, dx", out("eax") value, in("dx") port);
            value
        }
    }
    
    fn port_write_u8(&self, port: u16, value: u8) {
        unsafe {
            asm!("out dx, al", in("dx") port, in("al") value);
        }
    }
    
    fn port_write_u16(&self, port: u16, value: u16) {
        unsafe {
            asm!("out dx, ax", in("dx") port, in("ax") value);
        }
    }
    
    fn port_write_u32(&self, port: u16, value: u32) {
        unsafe {
            asm!("out dx, eax", in("dx") port, in("eax") value);
        }
    }
    
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