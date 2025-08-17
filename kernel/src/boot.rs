#[cfg(target_arch = "x86_64")]
use multiboot2::BootInformation;
#[cfg(target_arch = "x86_64")]
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
#[cfg(target_arch = "x86_64")]
use x86_64::structures::tss::TaskStateSegment;
#[cfg(target_arch = "x86_64")]
use x86_64::instructions::segmentation::Segment;
#[cfg(target_arch = "x86_64")]
use x86_64::VirtAddr;
use lazy_static::lazy_static;
use crate::{println, serial_println};
use crate::memory;

#[cfg(target_arch = "x86_64")]
const DOUBLE_FAULT_IST_INDEX: u16 = 0;

#[cfg(target_arch = "x86_64")]
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

#[cfg(target_arch = "x86_64")]
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

#[cfg(target_arch = "x86_64")]
struct Selectors {
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

#[cfg(target_arch = "x86_64")]
/// Initialize the kernel with multiboot2 information
pub fn init_kernel(boot_info: BootInformation) {
    serial_println!("Initializing kernel...");
    
    // Initialize platform abstraction layer first
    init_platform_abstraction();
    
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
    
    // Initialize swap space management
    init_swap_management();
    
    // Initialize process management
    init_process_management();
    
    // Initialize IPC system
    init_ipc_system();
    
    // Initialize system call interface
    init_syscall_interface();
    
    // Initialize power management framework
    init_power_management();
    
    // Initialize early console output (already done in main, but ensure it's working)
    test_console_output();
    
    serial_println!("Kernel initialization complete");
}

#[cfg(target_arch = "aarch64")]
/// Initialize the kernel for ARM64 (without multiboot2)
pub fn init_kernel_arm64() {
    serial_println!("Initializing ARM64 kernel...");
    
    // Set up basic CPU state first
    init_cpu_state_arm64();
    
    // Initialize physical memory manager (with default memory layout)
    init_physical_memory_arm64();
    
    // Initialize virtual memory management
    init_virtual_memory();
    
    // Initialize kernel heap allocator
    init_heap_allocator();
    
    // Initialize process management
    init_process_management();
    
    // Initialize IPC system
    init_ipc_system();
    
    // Initialize power management framework
    init_power_management();
    
    // Test console output
    test_console_output();
    
    serial_println!("ARM64 kernel initialization complete");
}

/// Initialize power management framework
fn init_power_management() {
    serial_println!("Initializing power management framework...");
    
    // Initialize CPU frequency scaling
    match crate::power::cpu_scaling::init() {
        Ok(()) => {
            serial_println!("CPU frequency scaling initialized successfully");
        }
        Err(e) => {
            serial_println!("Failed to initialize CPU frequency scaling: {}", e);
            // Don't panic - power management is optional for basic functionality
            println!("Warning: CPU frequency scaling not available");
        }
    }
    
    // Initialize idle state management
    match crate::power::idle_management::init() {
        Ok(()) => {
            serial_println!("Idle state management initialized successfully");
        }
        Err(e) => {
            serial_println!("Failed to initialize idle state management: {}", e);
            println!("Warning: Idle state management not available");
        }
    }
    
    // Initialize battery monitoring
    match crate::power::battery_monitor::init() {
        Ok(()) => {
            serial_println!("Battery monitoring initialized successfully");
        }
        Err(e) => {
            serial_println!("Failed to initialize battery monitoring: {}", e);
            println!("Warning: Battery monitoring not available");
        }
    }
    
    // Initialize power policy management
    match crate::power::power_policy::init() {
        Ok(()) => {
            serial_println!("Power policy management initialized successfully");
        }
        Err(e) => {
            serial_println!("Failed to initialize power policy management: {}", e);
            println!("Warning: Power policy management not available");
        }
    }
    
    // Initialize responsiveness optimizations
    match crate::power::responsiveness::init() {
        Ok(()) => {
            serial_println!("Responsiveness optimizations initialized successfully");
            
            // Test power management functionality
            test_power_management();
        }
        Err(e) => {
            serial_println!("Failed to initialize responsiveness optimizations: {}", e);
            println!("Warning: Responsiveness optimizations not available");
        }
    }
    
    serial_println!("Power management framework initialization complete");
}

/// Test power management functionality
fn test_power_management() {
    serial_println!("Testing power management framework...");
    
    use crate::power::{
        PowerState, CpuGovernor, ProcessActivity,
        cpu_scaling, idle_management, battery_monitor, power_policy,
    };
    use crate::process::ProcessId;
    
    // Test CPU frequency scaling
    serial_println!("Testing CPU frequency scaling...");
    
    // Test different governors
    let governors = [
        CpuGovernor::Performance,
        CpuGovernor::OnDemand,
        CpuGovernor::PowerSave,
        CpuGovernor::Interactive,
    ];
    
    for governor in &governors {
        match cpu_scaling::set_governor(*governor) {
            Ok(()) => {
                serial_println!("Successfully set CPU governor to {:?}", governor);
                
                if let Ok(freq_info) = cpu_scaling::get_frequency_info() {
                    serial_println!("  Current frequency: {} MHz", freq_info.current_mhz);
                    serial_println!("  Frequency range: {} - {} MHz", 
                                   freq_info.min_mhz, freq_info.max_mhz);
                }
            }
            Err(e) => {
                serial_println!("Failed to set CPU governor {:?}: {}", governor, e);
            }
        }
    }
    
    // Test CPU load updates
    let test_loads = [10, 30, 50, 70, 90];
    for load in &test_loads {
        match cpu_scaling::update_load(*load) {
            Ok(()) => {
                serial_println!("Updated CPU load to {}%", load);
            }
            Err(e) => {
                serial_println!("Failed to update CPU load: {}", e);
            }
        }
    }
    
    // Test process activity notifications
    let test_pid = ProcessId::new(100);
    cpu_scaling::notify_process_activity(test_pid, ProcessActivity::Interactive);
    serial_println!("Notified CPU scaling of interactive process activity");
    
    // Test idle state management
    serial_println!("Testing idle state management...");
    
    let current_time = 1000; // Simulated timestamp
    
    // Test entering idle
    match idle_management::enter_idle(current_time) {
        Ok(idle_state) => {
            serial_println!("Entered idle state: {:?}", idle_state);
        }
        Err(e) => {
            serial_println!("Failed to enter idle state: {}", e);
        }
    }
    
    // Test process activity notification for idle management
    idle_management::notify_process_activity(test_pid, ProcessActivity::Interactive, current_time + 100);
    serial_println!("Notified idle management of process activity");
    
    // Test exiting idle
    match idle_management::exit_idle(current_time + 200) {
        Ok(()) => {
            serial_println!("Exited idle state successfully");
        }
        Err(e) => {
            serial_println!("Failed to exit idle state: {}", e);
        }
    }
    
    // Test idle statistics
    match idle_management::get_stats() {
        Ok(stats) => {
            serial_println!("Idle statistics:");
            serial_println!("  Total idle time: {} ms", stats.total_idle_time);
            serial_println!("  Total idle entries: {}", stats.total_idle_entries);
        }
        Err(e) => {
            serial_println!("Failed to get idle statistics: {}", e);
        }
    }
    
    // Test battery monitoring
    serial_println!("Testing battery monitoring...");
    
    // Test battery info retrieval
    match battery_monitor::get_battery_info() {
        Ok(battery_info) => {
            serial_println!("Battery information:");
            serial_println!("  Level: {}%", battery_info.level_percent);
            serial_println!("  Charging: {}", battery_info.is_charging);
            if let Some(time_remaining) = battery_info.estimated_time_remaining {
                serial_println!("  Time remaining: {} minutes", time_remaining);
            }
        }
        Err(e) => {
            serial_println!("Failed to get battery info: {}", e);
        }
    }
    
    // Test power state recommendations
    let recommended_state = battery_monitor::get_recommended_power_state();
    serial_println!("Recommended power state: {:?}", recommended_state);
    
    // Test battery status checks
    if battery_monitor::is_critical() {
        serial_println!("Battery is in critical state");
    } else if battery_monitor::is_low() {
        serial_println!("Battery is in low state");
    } else {
        serial_println!("Battery level is normal");
    }
    
    // Test power policy management
    serial_println!("Testing power policy management...");
    
    // Test power state changes
    let test_states = [
        PowerState::Performance,
        PowerState::Balanced,
        PowerState::PowerSaver,
    ];
    
    for state in &test_states {
        match power_policy::set_power_state(*state) {
            Ok(()) => {
                serial_println!("Successfully set power state to {:?}", state);
                let current_state = power_policy::get_power_state();
                serial_println!("  Current power state: {:?}", current_state);
            }
            Err(e) => {
                serial_println!("Failed to set power state {:?}: {}", state, e);
            }
        }
    }
    
    // Test process classification
    use crate::power::power_policy::ProcessPowerClass;
    power_policy::classify_process(test_pid, ProcessPowerClass::Interactive);
    serial_println!("Classified process {} as Interactive", test_pid.0);
    
    // Test process activity notification
    power_policy::notify_process_activity(test_pid, ProcessActivity::Interactive, current_time + 300);
    serial_println!("Notified power policy of process activity");
    
    // Test power-aware priority calculation
    use crate::process::ProcessPriority;
    let base_priority = ProcessPriority::Normal;
    let power_aware_priority = power_policy::get_power_aware_priority(test_pid, base_priority);
    serial_println!("Power-aware priority: {:?} -> {:?}", base_priority, power_aware_priority);
    
    // Test time slice multiplier
    let time_slice_multiplier = power_policy::get_time_slice_multiplier(test_pid);
    serial_println!("Time slice multiplier for process {}: {:.2}", test_pid.0, time_slice_multiplier);
    
    // Test background throttling check
    if power_policy::should_throttle_background(test_pid) {
        serial_println!("Process {} should be throttled", test_pid.0);
    } else {
        serial_println!("Process {} should not be throttled", test_pid.0);
    }
    
    // Test responsiveness optimizations
    serial_println!("Testing responsiveness optimizations...");
    
    use crate::power::responsiveness::{
        TouchEvent, GestureType, SwipeDirection, 
        handle_touch_event, get_adaptive_time_slice, should_throttle_process,
        update_system_metrics, get_statistics
    };
    
    // Test touch input handling
    let touch_events = [
        TouchEvent::TouchDown { x: 100, y: 200 },
        TouchEvent::TouchMove { x: 105, y: 205 },
        TouchEvent::TouchMove { x: 110, y: 210 },
        TouchEvent::TouchUp { x: 115, y: 215 },
        TouchEvent::Gesture { gesture_type: GestureType::Swipe { direction: SwipeDirection::Right } },
    ];
    
    for (i, event) in touch_events.iter().enumerate() {
        let timestamp = current_time + 400 + (i as u64 * 10);
        match handle_touch_event(*event, timestamp) {
            Ok(()) => {
                serial_println!("Handled touch event: {:?}", event);
            }
            Err(e) => {
                serial_println!("Failed to handle touch event: {}", e);
            }
        }
    }
    
    // Test adaptive time slice calculation
    let base_time_slice = 10; // 10ms
    let adaptive_time_slice = get_adaptive_time_slice(test_pid, base_time_slice, current_time + 500);
    serial_println!("Adaptive time slice for process {}: {} ms (base: {} ms)", 
                   test_pid.0, adaptive_time_slice, base_time_slice);
    
    // Test responsiveness throttling
    if should_throttle_process(test_pid) {
        serial_println!("Process {} should be throttled for responsiveness", test_pid.0);
    } else {
        serial_println!("Process {} should not be throttled for responsiveness", test_pid.0);
    }
    
    // Test system metrics update
    update_system_metrics(75, 60, current_time + 600); // 75% CPU, 60% memory
    serial_println!("Updated system metrics: 75% CPU, 60% memory");
    
    // Test responsiveness statistics
    if let Some(stats) = get_statistics() {
        serial_println!("Responsiveness statistics:");
        serial_println!("  Interactive processes: {}", stats.interactive_processes_count);
        serial_println!("  Tracked processes: {}", stats.tracked_processes_count);
        serial_println!("  Average response time: {} μs", stats.average_response_time_us);
        serial_println!("  System load: {}%", stats.system_load_percent);
        serial_println!("  Memory usage: {}%", stats.memory_usage_percent);
        serial_println!("  Touch events queued: {}", stats.touch_events_queued);
        serial_println!("  Throttled processes: {}", stats.throttled_processes_count);
    }
    
    serial_println!("Power management framework test complete");
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

/// Initialize platform abstraction layer
fn init_platform_abstraction() {
    serial_println!("Initializing platform abstraction layer...");
    
    match crate::platform::init() {
        Ok(()) => {
            serial_println!("Platform abstraction layer initialized successfully");
            
            // Get platform information
            let platform = crate::platform::current_platform();
            let cpu_info = platform.get_cpu_info();
            let memory_map = platform.get_memory_map();
            let constants = platform.get_constants();
            
            serial_println!("Platform Information:");
            serial_println!("  Architecture: {:?}", cpu_info.architecture);
            serial_println!("  Vendor: {}", cpu_info.vendor);
            serial_println!("  Model: {}", cpu_info.model_name);
            serial_println!("  Cores: {}", cpu_info.core_count);
            serial_println!("  Cache line size: {} bytes", cpu_info.cache_line_size);
            serial_println!("  Features: MMU={}, Cache={}, FPU={}, SIMD={}", 
                           cpu_info.features.has_mmu,
                           cpu_info.features.has_cache,
                           cpu_info.features.has_fpu,
                           cpu_info.features.has_simd);
            
            serial_println!("Memory Map:");
            serial_println!("  Total memory: {} MB", memory_map.total_memory / (1024 * 1024));
            serial_println!("  Available memory: {} MB", memory_map.available_memory / (1024 * 1024));
            serial_println!("  Memory regions: {}", memory_map.regions.len());
            
            serial_println!("Platform Constants:");
            serial_println!("  Page size: {} bytes", constants.page_size);
            serial_println!("  Virtual address bits: {}", constants.virtual_address_bits);
            serial_println!("  Physical address bits: {}", constants.physical_address_bits);
            
            // Display on VGA console as well
            println!("Platform: {} {} ({} cores)", 
                    cpu_info.vendor, cpu_info.model_name, cpu_info.core_count);
            println!("Memory: {} MB available", memory_map.available_memory / (1024 * 1024));
        }
        Err(e) => {
            serial_println!("Failed to initialize platform abstraction layer: {}", e);
            panic!("Platform initialization failed");
        }
    }
}

#[cfg(target_arch = "x86_64")]
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

#[cfg(target_arch = "aarch64")]
/// Initialize basic CPU features and state for ARM64
pub fn init_cpu_state_arm64() {
    serial_println!("Initializing ARM64 CPU state...");
    
    // ARM64 CPU initialization would go here
    // For now, this is a stub
    
    serial_println!("ARM64 CPU state initialized");
}

#[cfg(target_arch = "x86_64")]
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

#[cfg(target_arch = "aarch64")]
/// Initialize physical memory manager for ARM64
fn init_physical_memory_arm64() {
    serial_println!("Initializing ARM64 physical memory manager...");
    
    // ARM64 physical memory initialization would go here
    // For now, this is a stub that assumes a default memory layout
    
    serial_println!("ARM64 physical memory manager initialized (stub)");
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

/// Initialize swap space management
fn init_swap_management() {
    serial_println!("Initializing swap space management...");
    
    // Initialize the swap manager
    match memory::swap::init_swap_manager() {
        Ok(()) => {
            serial_println!("Swap manager initialized successfully");
            
            // Initialize page swapper with LRU algorithm and 1024 page limit
            match memory::swap::swap_algorithm::init_page_swapper(
                memory::swap::swap_algorithm::PageReplacementAlgorithm::LRU, 
                1024
            ) {
                Ok(()) => {
                    serial_println!("Page swapper initialized successfully");
                    
                    // Test swap space functionality
                    test_swap_management();
                }
                Err(e) => {
                    serial_println!("Failed to initialize page swapper: {:?}", e);
                    println!("Warning: Page swapping not available");
                }
            }
        }
        Err(e) => {
            serial_println!("Failed to initialize swap manager: {:?}", e);
            // Don't panic - swap is optional for basic functionality
            println!("Warning: Swap space not available");
        }
    }
}

/// Test swap space management functionality
fn test_swap_management() {
    serial_println!("Testing swap space management...");
    
    // Create and configure swap devices
    {
        extern crate alloc;
        use alloc::boxed::Box;
        use alloc::string::{String, ToString};
        use crate::memory::swap::swap_config::{create_default_config, SwapConfigManager};
        use crate::memory::swap::swap_file::FileSwapDevice;
        use crate::memory::swap::{add_swap_device, swap_out_page, swap_in_page, print_swap_stats};
        use crate::memory::physical::PageFrame;
        use crate::memory::PAGE_SIZE;
        
        // Create a test swap configuration
        let mut config_manager = create_default_config();
        config_manager.print_config();
        
        // Create a test file-based swap device (8MB for testing)
        match FileSwapDevice::new("test_swap".to_string(), 8) {
            Ok(test_device) => {
                serial_println!("Created test swap device: 8MB file-based swap");
                
                // Add the device to the swap manager
                match add_swap_device(Box::new(test_device)) {
                    Ok(device_index) => {
                        serial_println!("Added swap device with index {}", device_index);
                        
                        // Test swap operations
                        test_swap_operations();
                        
                        // Test page swapping algorithms
                        test_page_swapping_algorithms();
                        
                        // Print swap statistics
                        print_swap_stats();
                    }
                    Err(e) => {
                        serial_println!("Failed to add swap device: {:?}", e);
                    }
                }
            }
            Err(e) => {
                serial_println!("Failed to create test swap device: {:?}", e);
            }
        }
        
        // Initialize swap devices from configuration (would be empty in this test)
        match config_manager.initialize_all() {
            Ok(count) => {
                serial_println!("Initialized {} swap devices from configuration", count);
            }
            Err(e) => {
                serial_println!("Failed to initialize swap devices from config: {:?}", e);
            }
        }
    }
    
    serial_println!("Swap space management test complete");
}

/// Test basic swap operations
fn test_swap_operations() {
    serial_println!("Testing basic swap operations...");
    
    use crate::memory::swap::{swap_out_page, swap_in_page, is_page_swapped};
    use crate::memory::physical::PageFrame;
    use crate::memory::PAGE_SIZE;
    
    // Create test data
    let test_page_frame = PageFrame(1000);
    let mut test_data = [0u8; PAGE_SIZE];
    
    // Fill with test pattern
    for (i, byte) in test_data.iter_mut().enumerate() {
        *byte = (i % 256) as u8;
    }
    
    // Test swap out
    match swap_out_page(test_page_frame, &test_data) {
        Ok(swap_slot) => {
            serial_println!("Successfully swapped out page {} to slot {}", 
                           test_page_frame.0, swap_slot.slot());
            
            // Verify page is marked as swapped
            if is_page_swapped(test_page_frame) {
                serial_println!("Page correctly marked as swapped");
                
                // Test swap in
                let mut read_data = [0u8; PAGE_SIZE];
                match swap_in_page(test_page_frame, &mut read_data) {
                    Ok(()) => {
                        serial_println!("Successfully swapped in page {}", test_page_frame.0);
                        
                        // Verify data integrity
                        if read_data == test_data {
                            serial_println!("Swap data integrity verified - data matches");
                        } else {
                            serial_println!("Warning: Swap data integrity check failed");
                        }
                        
                        // Verify page is no longer marked as swapped
                        if !is_page_swapped(test_page_frame) {
                            serial_println!("Page correctly unmarked as swapped");
                        } else {
                            serial_println!("Warning: Page still marked as swapped after swap-in");
                        }
                    }
                    Err(e) => {
                        serial_println!("Failed to swap in page: {:?}", e);
                    }
                }
            } else {
                serial_println!("Warning: Page not marked as swapped after swap-out");
            }
        }
        Err(e) => {
            serial_println!("Failed to swap out page: {:?}", e);
        }
    }
    
    serial_println!("Basic swap operations test complete");
}

/// Test page swapping algorithms
fn test_page_swapping_algorithms() {
    serial_println!("Testing page swapping algorithms...");
    
    use crate::memory::swap::swap_algorithm::{
        record_page_access, handle_page_fault, check_memory_pressure, 
        set_page_replacement_algorithm, set_memory_pressure_threshold,
        swap_out_pages, print_swapper_stats, PageReplacementAlgorithm
    };
    use crate::memory::vmm::VirtualAddress;
    use crate::memory::physical::PageFrame;
    
    // Test different page replacement algorithms
    let test_algorithms = [
        PageReplacementAlgorithm::LRU,
        PageReplacementAlgorithm::FIFO,
        PageReplacementAlgorithm::Clock,
        PageReplacementAlgorithm::LFU,
    ];
    
    for algorithm in &test_algorithms {
        serial_println!("Testing {:?} algorithm...", algorithm);
        
        // Set the algorithm
        if let Err(e) = set_page_replacement_algorithm(*algorithm) {
            serial_println!("Failed to set algorithm {:?}: {:?}", algorithm, e);
            continue;
        }
        
        // Simulate page accesses
        for i in 0..10 {
            let virt_addr = VirtualAddress::new(0x10000000 + i * 0x1000);
            let page_frame = PageFrame(2000 + i);
            let is_write = i % 3 == 0; // Every third access is a write
            
            record_page_access(virt_addr, page_frame, is_write);
        }
        
        // Test memory pressure handling
        if let Err(e) = set_memory_pressure_threshold(5) {
            serial_println!("Failed to set memory pressure threshold: {:?}", e);
        } else {
            match check_memory_pressure() {
                Ok(swapped_count) => {
                    if swapped_count > 0 {
                        serial_println!("Memory pressure handling: swapped out {} pages", swapped_count);
                    } else {
                        serial_println!("No memory pressure detected");
                    }
                }
                Err(e) => {
                    serial_println!("Memory pressure check failed: {:?}", e);
                }
            }
        }
        
        // Test manual page swapping
        match swap_out_pages(2) {
            Ok(swapped_count) => {
                serial_println!("Manual swap: swapped out {} pages", swapped_count);
            }
            Err(e) => {
                serial_println!("Manual swap failed: {:?}", e);
            }
        }
        
        // Test page fault handling
        let fault_virt_addr = VirtualAddress::new(0x20000000);
        let fault_page_frame = PageFrame(3000);
        
        match handle_page_fault(fault_virt_addr, fault_page_frame) {
            Ok(()) => {
                serial_println!("Page fault handled successfully");
            }
            Err(e) => {
                serial_println!("Page fault handling failed: {:?}", e);
            }
        }
        
        serial_println!("{:?} algorithm test complete", algorithm);
    }
    
    // Print final statistics
    print_swapper_stats();
    
    serial_println!("Page swapping algorithms test complete");
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

/// Initialize system call interface
fn init_syscall_interface() {
    serial_println!("Initializing system call interface...");
    
    match crate::syscall::init_syscall_interface() {
        Ok(()) => {
            serial_println!("System call interface initialized successfully");
            
            // Test system call functionality
            test_syscall_interface();
            
            // Run comprehensive system call tests
            crate::syscall::test::run_all_syscall_tests();
        }
        Err(e) => {
            serial_println!("Failed to initialize system call interface: {}", e);
            panic!("System call interface initialization failed");
        }
    }
}

/// Test system call interface functionality
fn test_syscall_interface() {
    serial_println!("Testing system call interface...");
    
    use crate::process::ProcessId;
    use crate::syscall::{dispatch_syscall, SYS_GETPID, SYS_TIME};
    
    let test_pid = ProcessId::new(1);
    let args = [0; 6];
    
    // Test getpid system call
    match dispatch_syscall(test_pid, SYS_GETPID, args) {
        Ok(result) => {
            serial_println!("getpid syscall test passed: returned {}", result);
        }
        Err(e) => {
            serial_println!("getpid syscall test failed: {:?}", e);
        }
    }
    
    // Test time system call
    match dispatch_syscall(test_pid, SYS_TIME, args) {
        Ok(result) => {
            serial_println!("time syscall test passed: returned {}", result);
        }
        Err(e) => {
            serial_println!("time syscall test failed: {:?}", e);
        }
    }
    
    // Test invalid system call
    match dispatch_syscall(test_pid, 999, args) {
        Ok(_) => {
            serial_println!("Invalid syscall test failed: should have returned error");
        }
        Err(e) => {
            serial_println!("Invalid syscall test passed: returned error {:?}", e);
        }
    }
    
    #[cfg(debug_assertions)]
    {
        // Test debug system calls in debug builds
        use crate::syscall::SYS_DEBUG_PRINT;
        
        match dispatch_syscall(test_pid, SYS_DEBUG_PRINT, [0x1000, 10, 0, 0, 0, 0]) {
            Ok(_) => {
                serial_println!("debug_print syscall test passed");
            }
            Err(e) => {
                serial_println!("debug_print syscall test failed: {:?}", e);
            }
        }
    }
    
    serial_println!("System call interface test complete");
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