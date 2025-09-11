#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use core::panic::PanicInfo;
#[cfg(target_arch = "x86_64")]
use multiboot2::BootInformation;

mod serial;
mod vga_buffer;
mod boot;
mod memory;
mod process;
mod ipc;
mod syscall;
mod power;
mod platform;

#[cfg(test)]
mod test_harness;
#[cfg(test)]
mod driver_tests;

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

// Multiboot2 header - simple and reliable approach
#[repr(C, align(8))]
struct Multiboot2Header {
    magic: u32,
    architecture: u32,
    header_length: u32,
    checksum: u32,
    // End tag
    end_type: u16,
    end_flags: u16,
    end_size: u32,
}

#[link_section = ".multiboot2"]
#[no_mangle]
#[used]
static MULTIBOOT2_HEADER: Multiboot2Header = {
    const MAGIC: u32 = 0xE85250D6;
    const ARCH: u32 = 0;
    const HEADER_LEN: u32 = 16; // 4 u32 fields = 16 bytes for main header
    
    Multiboot2Header {
        magic: MAGIC,
        architecture: ARCH,
        header_length: HEADER_LEN,
        checksum: 0u32.wrapping_sub(MAGIC).wrapping_sub(ARCH).wrapping_sub(HEADER_LEN),
        // End tag
        end_type: 0,
        end_flags: 0,
        end_size: 8,
    }
};

/// Parse boot parameters from multiboot2 command line
fn parse_boot_parameters(boot_info: &BootInformation) {
    serial_println!("Parsing boot parameters...");
    
    if let Some(command_line_tag) = boot_info.command_line_tag() {
        if let Ok(cmdline) = command_line_tag.cmdline() {
            serial_println!("Kernel command line: {}", cmdline);
            println!("Boot parameters: {}", cmdline);
            
            // Parse individual parameters
            for param in cmdline.split_whitespace() {
                if let Some((key, value)) = param.split_once('=') {
                    match key {
                        "debug" => {
                            if value == "1" || value == "true" {
                                serial_println!("Debug mode enabled");
                                println!("Debug mode: ON");
                            }
                        }
                        "log_level" => {
                            serial_println!("Log level set to: {}", value);
                            println!("Log level: {}", value);
                        }
                        "safe_mode" => {
                            if value == "1" || value == "true" {
                                serial_println!("Safe mode enabled");
                                println!("Safe mode: ON");
                            }
                        }
                        "driver_autoload" => {
                            if value == "false" || value == "0" {
                                serial_println!("Driver autoload disabled");
                                println!("Driver autoload: OFF");
                            }
                        }
                        "recovery" => {
                            if value == "1" || value == "true" {
                                serial_println!("Recovery mode enabled");
                                println!("Recovery mode: ON");
                            }
                        }
                        "single_user" => {
                            if value == "1" || value == "true" {
                                serial_println!("Single user mode enabled");
                                println!("Single user mode: ON");
                            }
                        }
                        _ => {
                            serial_println!("Unknown boot parameter: {}={}", key, value);
                        }
                    }
                } else {
                    // Handle boolean flags without values
                    match param {
                        "debug" => {
                            serial_println!("Debug mode enabled (flag)");
                            println!("Debug mode: ON");
                        }
                        "safe_mode" => {
                            serial_println!("Safe mode enabled (flag)");
                            println!("Safe mode: ON");
                        }
                        _ => {
                            serial_println!("Unknown boot flag: {}", param);
                        }
                    }
                }
            }
        }
    } else {
        serial_println!("No command line parameters found");
        println!("No boot parameters");
    }
    
    // Display additional boot information
    if let Some(boot_loader_name_tag) = boot_info.boot_loader_name_tag() {
        if let Ok(name) = boot_loader_name_tag.name() {
            serial_println!("Bootloader: {}", name);
            println!("Bootloader: {}", name);
        }
    }
    
    // Display ELF sections if available
    if let Some(elf_sections_tag) = boot_info.elf_sections_tag() {
        serial_println!("ELF sections available: {} sections", elf_sections_tag.sections().count());
    }
    
    // Display framebuffer info if available
    if let Some(framebuffer_tag) = boot_info.framebuffer_tag() {
        if let Ok(framebuffer) = framebuffer_tag {
            serial_println!("Framebuffer: {}x{} @ {} bpp", 
                           framebuffer.width(), 
                           framebuffer.height(),
                           framebuffer.bpp());
        }
    }
    
    serial_println!("Boot parameter parsing complete");
}

#[cfg(target_arch = "x86_64")]
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
            
            // Parse and display boot parameters
            parse_boot_parameters(&boot_info);
            
            // Initialize kernel with boot information
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
        #[cfg(target_arch = "x86_64")]
        x86_64::instructions::hlt();
        
        #[cfg(target_arch = "aarch64")]
        unsafe { core::arch::asm!("wfi") }; // Wait for interrupt on ARM64
    }
}

#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Initialize early console output for debugging
    serial_println!("Kosh Kernel Starting on ARM64...");
    println!("Kosh Kernel Starting on ARM64...");

    // Initialize platform abstraction layer first
    init_platform_abstraction();
    
    // Initialize kernel without multiboot2 info (ARM64 uses different boot protocol)
    boot::init_kernel_arm64();

    #[cfg(test)]
    test_main();

    println!("Kosh kernel initialized successfully on ARM64!");

    // Halt the CPU in an infinite loop
    loop {
        unsafe { core::arch::asm!("wfi") }; // Wait for interrupt
    }
}

/// Initialize platform abstraction layer
fn init_platform_abstraction() {
    serial_println!("Initializing platform abstraction layer...");
    
    // Initialize the appropriate platform
    #[cfg(target_arch = "x86_64")]
    {
        if let Err(e) = platform::x86_64::init() {
            serial_println!("Failed to initialize x86_64 platform: {:?}", e);
            panic!("Platform initialization failed");
        }
    }
    
    #[cfg(target_arch = "aarch64")]
    {
        if let Err(e) = platform::aarch64::init() {
            serial_println!("Failed to initialize ARM64 platform: {:?}", e);
            panic!("Platform initialization failed");
        }
    }
    
    serial_println!("Platform abstraction layer initialized successfully");
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
    serial_println!("Running {} legacy tests", tests.len());
    for test in tests {
        test();
    }
    
    // Run comprehensive kernel test suite
    run_comprehensive_tests();
    
    exit_qemu(QemuExitCode::Success);
}

#[cfg(test)]
fn run_comprehensive_tests() {
    use test_harness::KernelTestRunner;
    
    let mut runner = KernelTestRunner::new();
    
    // Register all test modules
    memory::tests::register_memory_tests(&mut runner);
    process::tests::register_process_tests(&mut runner);
    ipc::tests::register_ipc_tests(&mut runner);
    driver_tests::register_driver_tests(&mut runner);
    
    // Run all tests
    runner.run_all_tests();
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
