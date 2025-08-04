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
    
    // Initialize kernel heap allocator
    init_heap_allocator();
    
    // Initialize process management
    init_process_management();
    
    // Initialize IPC system
    init_ipc_system();
    
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

/// Initialize kernel heap allocator
fn init_heap_allocator() {
    serial_println!("Initializing kernel heap allocator...");
    
    // Allocate 1MB (256 pages) for the kernel heap
    const HEAP_SIZE_PAGES: usize = 256;
    
    match memory::heap::init_kernel_heap(HEAP_SIZE_PAGES) {
        Ok(()) => {
            serial_println!("Kernel heap allocator initialized successfully");
            
            // Test the heap allocator
            test_heap_allocator();
        }
        Err(e) => {
            serial_println!("Failed to initialize kernel heap allocator: {}", e);
            panic!("Heap allocator initialization failed");
        }
    }
}

/// Test kernel heap allocator
fn test_heap_allocator() {
    serial_println!("Testing kernel heap allocator...");
    
    // Test the heap allocator functionality
    memory::heap::test_heap_allocator();
    
    // Test Rust's built-in allocation using Vec
    {
        extern crate alloc;
        use alloc::vec::Vec;
        use alloc::string::String;
        
        // Test Vec allocation
        let mut test_vec: Vec<u32> = Vec::new();
        for i in 0..100 {
            test_vec.push(i);
        }
        serial_println!("Successfully allocated and used Vec with {} elements", test_vec.len());
        
        // Test String allocation
        let test_string = String::from("Hello, Kosh kernel heap!");
        serial_println!("Successfully allocated String: '{}'", test_string);
        
        // Test larger allocation
        let mut large_vec: Vec<u8> = Vec::new();
        for _ in 0..4096 {
            large_vec.push(0x42);
        }
        serial_println!("Successfully allocated large Vec with {} bytes", large_vec.len());
    } // All allocations should be automatically freed here
    
    // Print final heap statistics
    memory::heap::print_heap_stats();
    
    // Validate heap integrity
    match memory::heap::validate_heap() {
        Ok(()) => serial_println!("Heap integrity validation passed"),
        Err(e) => serial_println!("Heap integrity validation failed: {}", e),
    }
    
    serial_println!("Kernel heap allocator test complete");
}

/// Initialize process management
fn init_process_management() {
    serial_println!("Initializing process management...");
    
    match crate::process::init_process_management() {
        Ok(()) => {
            serial_println!("Process management initialized successfully");
            
            // Test process management functionality
            test_process_management();
        }
        Err(e) => {
            serial_println!("Failed to initialize process management: {}", e);
            panic!("Process management initialization failed");
        }
    }
}

/// Test process management functionality
fn test_process_management() {
    serial_println!("Testing process management...");
    
    // Test process creation
    {
        extern crate alloc;
        use alloc::string::String;
        use crate::process::{create_process, ProcessPriority, ProcessId, print_process_table};
        
        // Create a few test processes
        let init_pid = create_process(
            None,
            String::from("init"),
            ProcessPriority::System,
        );
        
        if let Ok(init_pid) = init_pid {
            serial_println!("Created init process with PID {}", init_pid.0);
            
            // Create child processes
            let shell_pid = create_process(
                Some(init_pid),
                String::from("shell"),
                ProcessPriority::Interactive,
            );
            
            let background_pid = create_process(
                Some(init_pid),
                String::from("background_task"),
                ProcessPriority::Background,
            );
            
            if let (Ok(shell_pid), Ok(bg_pid)) = (shell_pid, background_pid) {
                serial_println!("Created shell process with PID {}", shell_pid.0);
                serial_println!("Created background process with PID {}", bg_pid.0);
                
                // Print process table
                print_process_table();
                
                // Test scheduler
                test_scheduler();
                
                // Test context switching
                test_context_switching_functionality();
            }
        }
    }
    
    serial_println!("Process management test complete");
}

/// Initialize IPC system
fn init_ipc_system() {
    serial_println!("Initializing IPC system...");
    
    match crate::ipc::init_ipc_system() {
        Ok(()) => {
            serial_println!("IPC system initialized successfully");
            
            // Test IPC functionality
            test_ipc_system();
        }
        Err(e) => {
            serial_println!("Failed to initialize IPC system: {}", e);
            panic!("IPC system initialization failed");
        }
    }
}

/// Test IPC system functionality
fn test_ipc_system() {
    serial_println!("Testing IPC system...");
    
    // Test message passing
    {
        extern crate alloc;
        use alloc::string::String;
        use crate::process::ProcessId;
        use crate::ipc::{
            create_message, send_message, receive_message,
            MessageType, MessageData, create_capability, check_capability,
            CapabilityType, grant_system_process_capabilities, grant_user_process_capabilities,
            create_secure_ipc_channel, is_restricted_operation, validate_capability_request
        };
        use crate::ipc::capability::ResourceId;
        
        let sender_pid = ProcessId::new(1);
        let receiver_pid = ProcessId::new(2);
        
        // Test basic message creation and sending
        let message = create_message(
            sender_pid,
            receiver_pid,
            MessageType::ServiceRequest,
            MessageData::Text(String::from("Hello, IPC!")),
        );
        
        serial_println!("Created test message: {}", message);
        
        // Test message sending
        match send_message(message) {
            Ok(()) => {
                serial_println!("Message sent successfully");
                
                // Test message receiving
                match receive_message(receiver_pid) {
                    Ok(received_msg) => {
                        serial_println!("Message received: {}", received_msg);
                        
                        if let MessageData::Text(text) = &received_msg.data {
                            serial_println!("Message content: '{}'", text);
                        }
                    }
                    Err(e) => {
                        serial_println!("Failed to receive message: {}", e);
                    }
                }
            }
            Err(e) => {
                serial_println!("Failed to send message: {}", e);
            }
        }
        
        // Test capability system
        serial_println!("Testing capability system...");
        
        // Grant a capability to a process
        let file_resource = ResourceId::File(String::from("/test/file.txt"));
        match create_capability(
            sender_pid,
            CapabilityType::Read,
            file_resource.clone(),
            None, // System-granted
        ) {
            Ok(cap_id) => {
                serial_println!("Created capability {} for process {}", cap_id.0, sender_pid.0);
                
                // Test capability checking
                if check_capability(sender_pid, CapabilityType::Read, &file_resource) {
                    serial_println!("Capability check passed for read access");
                } else {
                    serial_println!("Capability check failed for read access");
                }
                
                // Test capability check for different permission
                if check_capability(sender_pid, CapabilityType::Write, &file_resource) {
                    serial_println!("Capability check passed for write access");
                } else {
                    serial_println!("Capability check failed for write access (expected)");
                }
            }
            Err(e) => {
                serial_println!("Failed to create capability: {}", e);
            }
        }
        
        // Test security policy system
        serial_println!("Testing security policy system...");
        
        // Test granting system capabilities
        match grant_system_process_capabilities(sender_pid) {
            Ok(capabilities) => {
                serial_println!("Granted {} system capabilities to process {}", 
                               capabilities.len(), sender_pid.0);
            }
            Err(e) => {
                serial_println!("Failed to grant system capabilities: {}", e);
            }
        }
        
        // Test granting user capabilities
        match grant_user_process_capabilities(receiver_pid) {
            Ok(capabilities) => {
                serial_println!("Granted {} user capabilities to process {}", 
                               capabilities.len(), receiver_pid.0);
            }
            Err(e) => {
                serial_println!("Failed to grant user capabilities: {}", e);
            }
        }
        
        // Test secure IPC channel creation
        match create_secure_ipc_channel(sender_pid, receiver_pid) {
            Ok(()) => {
                serial_println!("Created secure IPC channel between processes {} and {}", 
                               sender_pid.0, receiver_pid.0);
            }
            Err(e) => {
                serial_println!("Failed to create secure IPC channel: {}", e);
            }
        }
        
        // Test restricted operation validation
        if is_restricted_operation(CapabilityType::Admin) {
            serial_println!("Admin operation correctly identified as restricted");
        }
        
        if !is_restricted_operation(CapabilityType::Read) {
            serial_println!("Read operation correctly identified as non-restricted");
        }
        
        // Test capability request validation
        if validate_capability_request(
            sender_pid,
            CapabilityType::SendMessage,
            &ResourceId::Process(receiver_pid),
        ) {
            serial_println!("SendMessage capability request validated successfully");
        } else {
            serial_println!("SendMessage capability request validation failed");
        }
    }
    
    // Print IPC statistics
    crate::ipc::print_ipc_info();
    
    serial_println!("IPC system test complete");
}

/// Test scheduler functionality
fn test_scheduler() {
    serial_println!("Testing scheduler...");
    
    use crate::process::{schedule_next_process, print_scheduler_info, set_scheduling_algorithm, SchedulingAlgorithm};
    
    // Test round-robin scheduling
    serial_println!("Testing round-robin scheduling...");
    if let Ok(scheduled_pid) = schedule_next_process() {
        if let Some(pid) = scheduled_pid {
            serial_println!("Scheduled process: {}", pid.0);
        } else {
            serial_println!("No process scheduled");
        }
    }
    
    // Test priority scheduling
    serial_println!("Testing priority scheduling...");
    if set_scheduling_algorithm(SchedulingAlgorithm::Priority).is_ok() {
        if let Ok(scheduled_pid) = schedule_next_process() {
            if let Some(pid) = scheduled_pid {
                serial_println!("Priority scheduled process: {}", pid.0);
            }
        }
    }
    
    // Print scheduler information
    print_scheduler_info();
    
    serial_println!("Scheduler test complete");
}

/// Test context switching functionality
fn test_context_switching_functionality() {
    serial_println!("Testing context switching functionality...");
    
    use crate::process::test_context_switching;
    
    // Test context switching
    test_context_switching();
    
    serial_println!("Context switching test complete");
}