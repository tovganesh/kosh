use multiboot2::BootInformation;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::instructions::segmentation::Segment;
use x86_64::VirtAddr;
use lazy_static::lazy_static;
use crate::{println, serial_println};
use crate::memory;

const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            let stack_start = VirtAddr::from_ptr(&raw const STACK);
            let stack_end = stack_start + STACK_SIZE;
            stack_end
        };
        tss
    };
}

lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        let data_selector = gdt.add_entry(Descriptor::kernel_data_segment());
        let tss_selector = gdt.add_entry(Descriptor::tss_segment(&TSS));
        (
            gdt,
            Selectors {
                code_selector,
                data_selector,
                tss_selector,
            },
        )
    };
}

struct Selectors {
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

/// Initialize the kernel with multiboot2 information
pub fn init_kernel(boot_info: BootInformation) {
    serial_println!("Initializing kernel...");
    
    // Set up basic CPU state first
    init_cpu_state();
    
    // Set up GDT and TSS
    init_gdt();
    
    // Parse and display memory information
    parse_memory_map(&boot_info);
    
    // Initialize physical memory manager
    init_physical_memory(&boot_info);
    
    // Initialize virtual memory management
    init_virtual_memory();
    
    // Initialize early console output (already done in main, but ensure it's working)
    test_console_output();
    
    serial_println!("Kernel initialization complete");
}

/// Initialize Global Descriptor Table and Task State Segment
fn init_gdt() {
    serial_println!("Setting up GDT and TSS...");
    
    GDT.0.load();
    
    unsafe {
        // Load code segment
        x86_64::instructions::segmentation::CS::set_reg(GDT.1.code_selector);
        
        // Load data segments
        x86_64::instructions::segmentation::DS::set_reg(GDT.1.data_selector);
        x86_64::instructions::segmentation::ES::set_reg(GDT.1.data_selector);
        x86_64::instructions::segmentation::FS::set_reg(GDT.1.data_selector);
        x86_64::instructions::segmentation::GS::set_reg(GDT.1.data_selector);
        x86_64::instructions::segmentation::SS::set_reg(GDT.1.data_selector);
        
        // Load TSS
        x86_64::instructions::tables::load_tss(GDT.1.tss_selector);
    }
    
    serial_println!("GDT and TSS initialized");
}

/// Parse and display memory map information from multiboot2
fn parse_memory_map(boot_info: &BootInformation) {
    serial_println!("Parsing memory map...");
    
    if let Some(memory_map_tag) = boot_info.memory_map_tag() {
        serial_println!("Memory areas:");
        
        let mut total_memory = 0u64;
        let mut usable_memory = 0u64;
        
        for area in memory_map_tag.memory_areas() {
            // For now, just assume all memory is available for simplicity
            // In a real implementation, we would properly parse the memory type
            usable_memory += area.size();
            let area_type = "Memory";
            
            total_memory += area.size();
            
            serial_println!(
                "  0x{:016x} - 0x{:016x} ({} KB) - {}",
                area.start_address(),
                area.end_address(),
                area.size() / 1024,
                area_type
            );
        }
        
        serial_println!("Total memory: {} MB", total_memory / (1024 * 1024));
        serial_println!("Usable memory: {} MB", usable_memory / (1024 * 1024));
        
        // Display memory info on VGA console as well
        println!("Memory detected: {} MB usable, {} MB total", 
                usable_memory / (1024 * 1024), 
                total_memory / (1024 * 1024));
    } else {
        serial_println!("No memory map found in multiboot2 info");
        println!("Warning: No memory map available");
    }
    
    // Display other boot information
    if let Some(boot_loader_name_tag) = boot_info.boot_loader_name_tag() {
        if let Ok(name) = boot_loader_name_tag.name() {
            serial_println!("Boot loader: {}", name);
            println!("Booted by: {}", name);
        }
    }
    
    if let Some(command_line_tag) = boot_info.command_line_tag() {
        if let Ok(cmdline) = command_line_tag.cmdline() {
            serial_println!("Command line: {}", cmdline);
        }
    }
}

/// Test early console output functionality
fn test_console_output() {
    serial_println!("Testing console output...");
    
    // Test VGA buffer output
    println!("VGA console test: Colors and formatting");
    
    // Test serial output
    serial_println!("Serial console test: Debug output working");
    
    // Test that both outputs are synchronized
    for i in 0..3 {
        println!("Console test line {}", i + 1);
        serial_println!("Serial test line {}", i + 1);
    }
    
    serial_println!("Console output test complete");
}

/// Initialize basic CPU features and state
pub fn init_cpu_state() {
    serial_println!("Initializing CPU state...");
    
    // Disable interrupts during initialization
    x86_64::instructions::interrupts::disable();
    
    // Clear direction flag
    unsafe {
        core::arch::asm!("cld");
    }
    
    // Initialize FPU if present
    unsafe {
        // Check if FPU is present
        let mut cr0: u64;
        core::arch::asm!("mov {}, cr0", out(reg) cr0);
        
        // Clear EM (emulation) bit and set MP (monitor coprocessor) bit
        cr0 &= !(1 << 2); // Clear EM
        cr0 |= 1 << 1;    // Set MP
        
        core::arch::asm!("mov cr0, {}", in(reg) cr0);
        
        // Initialize FPU
        core::arch::asm!("fninit");
    }
    
    serial_println!("CPU state initialized");
}

/// Initialize physical memory manager
fn init_physical_memory(boot_info: &BootInformation) {
    serial_println!("Initializing physical memory manager...");
    
    match memory::physical::init_physical_memory(boot_info) {
        Ok(()) => {
            serial_println!("Physical memory manager initialized successfully");
            
            // Test the allocator by allocating and deallocating a few frames
            test_physical_allocator();
        }
        Err(e) => {
            serial_println!("Failed to initialize physical memory manager: {}", e);
            panic!("Physical memory initialization failed");
        }
    }
}

/// Test the physical memory allocator
fn test_physical_allocator() {
    serial_println!("Testing physical memory allocator...");
    
    // Test single frame allocation
    if let Some(frame1) = memory::physical::allocate_frame() {
        serial_println!("Allocated frame: 0x{:x}", frame1.address());
        
        // Test multiple frame allocation
        if let Some(frame2) = memory::physical::allocate_frames(3) {
            serial_println!("Allocated 3 contiguous frames starting at: 0x{:x}", frame2.address());
            
            // Deallocate the frames
            memory::physical::deallocate_frames(frame2, 3);
            serial_println!("Deallocated 3 frames");
        }
        
        memory::physical::deallocate_frame(frame1);
        serial_println!("Deallocated single frame");
    }
    
    // Print memory statistics after test
    memory::physical::print_memory_stats();
    
    serial_println!("Physical memory allocator test complete");
}

/// Initialize virtual memory management
fn init_virtual_memory() {
    serial_println!("Initializing virtual memory management...");
    
    match unsafe { memory::vmm::init_virtual_memory() } {
        Ok(()) => {
            serial_println!("Virtual memory management initialized successfully");
            
            // Test virtual memory functionality
            test_virtual_memory();
        }
        Err(e) => {
            serial_println!("Failed to initialize virtual memory management: {}", e);
            panic!("Virtual memory initialization failed");
        }
    }
}

/// Test virtual memory functionality
fn test_virtual_memory() {
    serial_println!("Testing virtual memory management...");
    
    // Print virtual memory layout
    memory::vmm::print_virtual_memory_stats();
    
    // Test virtual address translation
    let test_virt_addr = memory::vmm::VirtualAddress::new(0xFFFFFFFF80000000);
    if let Some(phys_addr) = memory::vmm::translate_virtual_address(test_virt_addr) {
        serial_println!("Virtual address 0x{:x} maps to physical address 0x{:x}", 
                       test_virt_addr.as_usize(), phys_addr);
    } else {
        serial_println!("Virtual address 0x{:x} is not mapped", test_virt_addr.as_usize());
    }
    
    serial_println!("Virtual memory management test complete");
}