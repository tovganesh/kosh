#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use core::panic::PanicInfo;
use multiboot2::BootInformation;

mod serial;
mod vga_buffer;
mod boot;
mod memory;
mod process;
mod ipc;
mod syscall;
mod power;

#[global_allocator]
static ALLOCATOR: memory::heap::GlobalKernelAllocator = memory::heap::GlobalKernelAllocator;

// Required by the linker for some core operations
#[no_mangle]
pub extern "C" fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    unsafe {
        for i in 0..n {
            let a = *s1.add(i);
            let b = *s2.add(i);
            if a != b {
                return a as i32 - b as i32;
            }
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    unsafe {
        for i in 0..n {
            *s.add(i) = c as u8;
        }
    }
    s
}

#[no_mangle]
pub extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        for i in 0..n {
            *dest.add(i) = *src.add(i);
        }
    }
    dest
}

#[no_mangle]
pub extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        if src < dest as *const u8 {
            // Copy backwards to avoid overlap issues
            for i in (0..n).rev() {
                *dest.add(i) = *src.add(i);
            }
        } else {
            // Copy forwards
            for i in 0..n {
                *dest.add(i) = *src.add(i);
            }
        }
    }
    dest
}

// Multiboot2 header
#[repr(C, align(8))]
struct Multiboot2Header {
    magic: u32,
    architecture: u32,
    header_length: u32,
    checksum: u32,
    // End tag
    end_tag_type: u16,
    end_tag_flags: u16,
    end_tag_size: u32,
}

#[link_section = ".multiboot2"]
#[no_mangle]
static MULTIBOOT2_HEADER: Multiboot2Header = Multiboot2Header {
    magic: 0xE85250D6,
    architecture: 0, // i386 protected mode
    header_length: core::mem::size_of::<Multiboot2Header>() as u32,
    checksum: 0x100000000u64.wrapping_sub(
        0xE85250D6u64 + 0 + core::mem::size_of::<Multiboot2Header>() as u64
    ) as u32,
    end_tag_type: 0,
    end_tag_flags: 0,
    end_tag_size: 8,
};

#[no_mangle]
pub extern "C" fn _start(multiboot_info_addr: usize) -> ! {
    // Initialize early console output for debugging
    serial_println!("Kosh Kernel Starting...");
    println!("Kosh Kernel Starting...");

    // Parse multiboot2 information
    let boot_info = unsafe { BootInformation::load(multiboot_info_addr as *const _) };
    
    match boot_info {
        Ok(boot_info) => {
            serial_println!("Multiboot2 info parsed successfully");
            boot::init_kernel(boot_info);
        }
        Err(e) => {
            serial_println!("Failed to parse multiboot2 info: {:?}", e);
            panic!("Failed to parse multiboot2 information");
        }
    }

    #[cfg(test)]
    test_main();

    println!("Kosh kernel initialized successfully!");

    // Halt the CPU in an infinite loop
    loop {
        x86_64::instructions::hlt();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Disable interrupts to prevent further issues
    x86_64::instructions::interrupts::disable();
    
    // Output panic information to both serial and VGA console
    serial_println!("\n!!! KERNEL PANIC !!!");
    println!("\n!!! KERNEL PANIC !!!");
    
    if let Some(location) = info.location() {
        serial_println!("Panic occurred in file '{}' at line {}", 
                       location.file(), location.line());
        println!("Panic at {}:{}", location.file(), location.line());
    }
    
    let message = info.message();
    serial_println!("Panic message: {}", message);
    println!("Message: {}", message);
    
    serial_println!("System halted.");
    println!("System halted.");
    
    // Halt the CPU
    loop {
        x86_64::instructions::hlt();
    }
}

#[cfg(test)]
fn test_runner(tests: &[&dyn Fn()]) {
    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test();
    }
    exit_qemu(QemuExitCode::Success);
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

#[cfg(test)]
pub fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;

    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}

#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}
