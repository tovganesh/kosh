#![no_std]
#![no_main]

extern crate alloc;

use kosh_keyboard_driver::{keyboard_interrupt_handler, register_keyboard_driver};

/// Entry point for the keyboard driver process
#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Initialize the keyboard driver
    if let Err(e) = register_keyboard_driver() {
        // In a real implementation, this would log the error
        panic!("Failed to initialize keyboard driver: {:?}", e);
    }

    // Main driver loop
    loop {
        // In a real implementation, this would:
        // 1. Wait for IPC messages from the driver manager
        // 2. Handle driver requests
        // 3. Process hardware interrupts
        // 4. Send responses back to requesters

        // For now, just halt
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::asm!("hlt");
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            // For other architectures, just loop
            continue;
        }
    }
}

/// Panic handler for the driver (only in non-test builds)
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // In a real implementation, this would log the panic and notify the driver manager
    loop {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

/// Keyboard interrupt handler entry point
#[no_mangle]
pub extern "C" fn keyboard_irq_handler() {
    keyboard_interrupt_handler();
}
