//! x86-64 interrupt handling implementation

use core::arch::asm;
use super::super::traits::{InterruptHandling, InterruptHandler};
use super::super::{PlatformResult, PlatformError};

/// x86-64 interrupt handler implementation
pub struct X86_64InterruptHandler {
    handlers: [Option<InterruptHandler>; 256],
}

impl X86_64InterruptHandler {
    pub fn new() -> Self {
        Self {
            handlers: [None; 256],
        }
    }
    
    /// Setup the Interrupt Descriptor Table (IDT)
    pub fn setup_interrupts(&mut self) -> PlatformResult<()> {
        // This would setup the IDT with proper interrupt handlers
        // For now, just return success
        Ok(())
    }
}

impl InterruptHandling for X86_64InterruptHandler {
    fn enable_interrupts(&self) {
        unsafe {
            asm!("sti");
        }
    }
    
    fn disable_interrupts(&self) {
        unsafe {
            asm!("cli");
        }
    }
    
    fn interrupts_enabled(&self) -> bool {
        unsafe {
            let flags: u64;
            asm!("pushfq; pop {}", out(reg) flags);
            (flags & (1 << 9)) != 0 // Check IF flag
        }
    }
    
    fn register_interrupt_handler(&mut self, interrupt_number: u8, handler: InterruptHandler) -> PlatformResult<()> {
        if interrupt_number as usize >= self.handlers.len() {
            return Err(PlatformError::InvalidAddress);
        }
        self.handlers[interrupt_number as usize] = Some(handler);
        Ok(())
    }
    
    fn unregister_interrupt_handler(&mut self, interrupt_number: u8) -> PlatformResult<()> {
        if interrupt_number as usize >= self.handlers.len() {
            return Err(PlatformError::InvalidAddress);
        }
        self.handlers[interrupt_number as usize] = None;
        Ok(())
    }
    
    fn send_eoi(&self, interrupt_number: u8) -> PlatformResult<()> {
        // Send End of Interrupt to APIC/PIC
        // This is a simplified implementation
        if interrupt_number >= 32 && interrupt_number < 48 {
            // PIC interrupts
            unsafe {
                if interrupt_number >= 40 {
                    // Slave PIC
                    asm!("out 0xA0, al", in("al") 0x20u8);
                }
                // Master PIC
                asm!("out 0x20, al", in("al") 0x20u8);
            }
        }
        Ok(())
    }
}