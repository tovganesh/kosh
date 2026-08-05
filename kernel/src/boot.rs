#[cfg(target_arch = "x86_64")]
use multiboot2::BootInformation;
use crate::{println, serial_println};
use crate::memory;

// GDT, TSS and the double-fault IST now live in `crate::gdt`, which owns the
// ring-3 descriptors and RSP0 as well. See that module for why the descriptor
// order is load-bearing once SYSCALL/SYSRET exist.

#[cfg(target_arch = "x86_64")]
/// Initialize the kernel with multiboot2 information
pub fn init_kernel(boot_info: BootInformation) {
    serial_println!("Initializing kernel...");
    
    // Initialize platform abstraction layer first
    init_platform_abstraction();
    
    // Set up basic CPU state first
    init_cpu_state();
    
    // Set up GDT and TSS (now including ring-3 descriptors and RSP0)
    crate::gdt::init();

    // GS.base has to point at the per-CPU block before the syscall stub can run,
    // and `gdt::init` reloads GS — so this comes after it, and long before
    // `syscall::init_syscall_interface`.
    crate::percpu::init();

    // Install the IDT before anything else can fault. From here on a bad
    // pointer produces a legible dump instead of a silent triple fault.
    crate::interrupts::init();
    crate::interrupts::test_breakpoint_exception();
    
    // Parse and display memory information
    parse_memory_map(&boot_info);
    
    // Record where GRUB put the boot modules. `boot_info` borrows the
    // multiboot2 structure and does not outlive this function, so the addresses
    // are copied out now and read through the physmap later.
    crate::usermode::record_boot_modules(&boot_info);
    
    // Initialize physical memory manager
    init_physical_memory(&boot_info);
    
    // Build and install the kernel's own page tables, replacing the crude
    // identity map the boot trampoline set up. Needs only the frame allocator,
    // so it can run before the heap exists.
    init_paging();
    
    // Initialize kernel heap allocator.
    //
    // ORDER MATTERS: after paging (the heap is now mapped into its own virtual
    // window, so page tables must exist) and before init_virtual_memory(),
    // which pushes region descriptors into a Vec and therefore allocates.
    // Running the VMM first meant allocating against a null heap every boot.
    init_heap_allocator();
    
    // Initialize virtual memory management bookkeeping
    init_virtual_memory();
    
    // Storage. Needs the heap (the device is boxed), nothing else.
    crate::block::init();
    init_storage_selftest();
    crate::fs::init();
    
    // DISABLED (Phase 1): init_swap_management() allocates an 8 MiB Vec as a
    // "swap file" out of a 1 MiB kernel heap whose MAX_ALLOC_SIZE is 1 MiB.
    // It cannot succeed. Re-enable once there is a real block device.
    // init_swap_management();
    
    // Initialize process management
    init_process_management();
    
    // Initialize IPC system
    init_ipc_system();
    
    // Initialize system call interface
    init_syscall_interface();
    
    // DISABLED (Phase 1): the power management subsystem is entirely simulated
    // (no ACPI, no MSRs, constant battery level) and test_power_management()
    // is 236 lines of printing those simulated results during boot.
    // init_power_management();
    
    // Initialize early console output (already done in main, but ensure it's working)
    test_console_output();
    
    // Everything is up: remap the PIC, start the PIT and enable interrupts.
    // Deliberately last, so a timer tick cannot arrive mid-initialisation.
    crate::interrupts::enable_hardware_interrupts();
    
    // Kernel threads. Needs a live timer, so it has to come after the line
    // above.
    init_scheduler();
    
    // Ring 3.
    init_usermode();
    
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
    
    // KERNEL_VMA itself is the first page of the kernel window, and it maps
    // physical page 0 — which `paging::init` deliberately leaves out, so the
    // null guard survives the move to the higher half. Unmapped is the pass.
    let guard_page =
        memory::vmm::VirtualAddress::new(memory::paging::KERNEL_VMA as usize);
    match memory::vmm::translate_virtual_address(guard_page) {
        Some(phys) => serial_println!(
            "  WARNING: the kernel window's guard page 0x{:x} maps to 0x{:x}",
            guard_page.as_usize(),
            phys
        ),
        None => serial_println!(
            "  kernel window guard page 0x{:x}: unmapped, as it should be",
            guard_page.as_usize()
        ),
    }

    // And the kernel image itself, one page in, must resolve.
    let image = memory::vmm::VirtualAddress::new(
        (memory::paging::KERNEL_VMA + 0x10_0000) as usize,
    );
    match memory::vmm::translate_virtual_address(image) {
        Some(phys) => serial_println!(
            "  kernel image 0x{:x} -> physical 0x{:x}",
            image.as_usize(),
            phys
        ),
        None => serial_println!("  WARNING: the kernel image is not mapped"),
    }
    
    serial_println!("Virtual memory management test complete");
}

/// Read the first sector and check it looks like something.
///
/// The point is to prove the ATA driver moves real bytes before any filesystem
/// code is written on top of it. A boot sector is a convenient oracle: it has a
/// fixed signature at a fixed offset, so a driver that is silently returning
/// zeroes or reading the wrong sector cannot pass.
#[cfg(target_arch = "x86_64")]
fn init_storage_selftest() {
    use crate::block::BLOCK_SIZE;

    if !crate::block::is_present() {
        serial_println!("Storage: no device, skipping self-test");
        return;
    }

    let mut sector = [0u8; BLOCK_SIZE];
    match crate::block::read_block(0, &mut sector) {
        Ok(()) => {
            let signature = u16::from_le_bytes([sector[510], sector[511]]);
            let oem = core::str::from_utf8(&sector[3..11]).unwrap_or("????????");

            serial_println!("Storage self-test:");
            serial_println!("  LBA 0 signature : 0x{:04x}", signature);
            serial_println!("  OEM name        : '{}'", oem.trim());

            if signature == 0xAA55 {
                serial_println!("Storage: PASS — read a valid boot sector from LBA 0");
            } else {
                serial_println!(
                    "Storage: FAIL — LBA 0 signature is 0x{:04x}, expected 0xaa55",
                    signature
                );
            }
        }
        Err(e) => serial_println!("Storage: FAIL — read error {:?}", e),
    }
}

/// Drop into ring 3 and run the user payload.
#[cfg(target_arch = "x86_64")]
fn init_usermode() {
    crate::syscall::uaccess::self_test();
    crate::platform::rtc::report();
    crate::memory::address_space::self_test();
    serial_println!();

    let before = crate::syscall::entry::syscall_count();

    // Demo 1: syscalls from ring 3, including one the kernel must refuse.
    if let Err(e) = crate::task::spawn("user-syscall", crate::usermode::run_user_demo, 0) {
        serial_println!("Failed to spawn user thread: {}", e);
        return;
    }
    while crate::task::live_threads() > 0 {
        x86_64::instructions::hlt();
    }
    serial_println!("--- back in ring 0 ---");
    crate::task::reap_finished();

    // Demo 2: a ring-3 program that dereferences kernel memory. The kernel must
    // kill it and survive.
    serial_println!();
    if let Err(e) = crate::task::spawn("user-fault", crate::usermode::run_user_demo, 1) {
        serial_println!("Failed to spawn faulting user thread: {}", e);
        return;
    }
    while crate::task::live_threads() > 0 {
        x86_64::instructions::hlt();
    }
    serial_println!("--- kernel survived a ring 3 fault ---");
    crate::task::reap_finished();

    let serviced = crate::syscall::entry::syscall_count() - before;
    if serviced >= 4 {
        serial_println!("Ring 3: PASS — {} syscalls serviced from user mode", serviced);
    } else {
        serial_println!("Ring 3: FAIL — only {} syscalls serviced", serviced);
    }

    // Demo 3: two ring-3 threads at once, each yielding from inside a syscall.
    //
    // Everything above ran one ring-3 thread at a time, which is what let the
    // syscall path get away with a single static kernel stack. This is the test
    // that stack could not pass.
    serial_println!();
    serial_println!("Two ring-3 threads, each yielding from inside a syscall:");
    let before_pp = crate::syscall::entry::syscall_count();
    let switches_before = crate::task::switch_count();

    for which in 0..2 {
        if let Err(e) = crate::task::spawn("pingpong", crate::usermode::run_pingpong, which) {
            serial_println!("Concurrent ring 3: FAIL — could not spawn: {}", e);
            return;
        }
    }
    serial_println!("--- interleaved ring 3 output ---");
    while crate::task::live_threads() > 0 {
        x86_64::instructions::hlt();
    }
    serial_println!();
    crate::task::reap_finished();

    let pp_syscalls = crate::syscall::entry::syscall_count() - before_pp;
    let pp_switches = crate::task::switch_count() - switches_before;

    // 2 threads x 12 iterations x (yield + write) + 2 final writes + 2 exits.
    // Anything less means a thread died partway, which is exactly the failure a
    // shared syscall stack produces.
    let expected = 2 * 12 * 2 + 4;
    if pp_syscalls >= expected as u64 && pp_switches >= 24 {
        serial_println!(
            "Concurrent ring 3: PASS — {} syscalls across {} context switches, per-thread kernel stacks held",
            pp_syscalls,
            pp_switches
        );
    } else {
        serial_println!(
            "Concurrent ring 3: FAIL — {} syscalls (expected >= {}), {} switches (expected >= 24)",
            pp_syscalls,
            expected,
            pp_switches
        );
    }

    // Demo 4: an ELF the kernel was not compiled with, loaded from a GRUB
    // module. Reported separately, because it exercises the loader rather than
    // the ring-3 mechanics above.
    serial_println!();
    let before_elf = crate::syscall::entry::syscall_count();

    if let Err(e) = crate::task::spawn("elf-loader", crate::usermode::run_boot_module, 0) {
        serial_println!("Failed to spawn ELF loader thread: {}", e);
        return;
    }
    while crate::task::live_threads() > 0 {
        x86_64::instructions::hlt();
    }
    serial_println!("--- back in ring 0 ---");
    crate::task::reap_finished();

    let (demand_faults, reserved) = crate::memory::paging::demand_stats();
    serial_println!(
        "Demand paging: {} page(s) reserved, {} touched ({} never allocated)",
        reserved,
        demand_faults,
        reserved.saturating_sub(demand_faults)
    );

    let (cow_resolved, cow_copied) = crate::memory::paging::cow_stats();
    serial_println!(
        "Copy-on-write: {} fault(s) resolved, {} needed a copy, {} frame(s) still shared",
        cow_resolved,
        cow_copied,
        crate::memory::physical::shared_frames()
    );

    let elf_syscalls = crate::syscall::entry::syscall_count() - before_elf;
    if elf_syscalls >= 5 {
        serial_println!(
            "ELF loader: PASS — loaded program ran and made {} syscalls",
            elf_syscalls
        );
    } else {
        serial_println!(
            "ELF loader: FAIL — loaded program made only {} syscalls",
            elf_syscalls
        );
    }
}

/// Bring up preemptive kernel threading and prove it works.
#[cfg(target_arch = "x86_64")]
fn init_scheduler() {
    serial_println!("Initializing kernel threads...");
    crate::task::init();

    for (i, name) in ["worker-A", "worker-B", "worker-C"].iter().enumerate() {
        if let Err(e) = crate::task::spawn(name, scheduler_demo_thread, i) {
            serial_println!("Failed to spawn {}: {}", name, e);
            return;
        }
    }

    serial_println!("Starting preemptive scheduling...");
    serial_println!("Expect A/B/C to interleave — none of them ever yields:");
    crate::serial_print!("  ");

    crate::task::start();

    // Wait for the workers. `hlt` parks the CPU until the next interrupt, which
    // is what lets the timer preempt us into the workers in the first place.
    while crate::task::live_threads() > 0 {
        x86_64::instructions::hlt();
    }

    serial_println!();
    crate::task::print_threads();
    crate::task::reap_finished();

    let switches = crate::task::switch_count();
    if switches > 10 {
        serial_println!("Scheduler: PASS — {} real context switches", switches);
    } else {
        serial_println!("Scheduler: FAIL — only {} context switches", switches);
    }
}

/// Body of each demo thread.
///
/// The busy-wait is the point. These threads never call `yield_now`, never
/// sleep, and never block on anything — so the *only* way their output can
/// interleave is if the timer interrupt takes the CPU away from them. If the
/// letters come out as `A A A ... B B B ... C C C`, scheduling is cooperative
/// and something is wrong.
#[cfg(target_arch = "x86_64")]
fn scheduler_demo_thread(index: usize) {
    const NAMES: [&str; 3] = ["A", "B", "C"];
    const ROUNDS: usize = 8;
    const BUSY_TICKS: u64 = 5; // 50 ms of work per round, slice is 20 ms

    let name = NAMES[index % NAMES.len()];

    for _ in 0..ROUNDS {
        crate::serial_print!("{} ", name);

        let target = crate::interrupts::timer::ticks() + BUSY_TICKS;
        while crate::interrupts::timer::ticks() < target {
            core::hint::spin_loop();
        }
    }
}

/// Build and install the kernel page tables.
#[cfg(target_arch = "x86_64")]
fn init_paging() {
    let phys_end = memory::physical::physical_memory_end();

    match memory::paging::init(phys_end) {
        Ok(()) => {
            memory::paging::self_test();
        }
        Err(e) => {
            serial_println!("Failed to build kernel page tables: {}", e);
            panic!("paging initialization failed");
        }
    }
}

/// Initialize kernel heap allocator
fn init_heap_allocator() {
    serial_println!("Initializing kernel heap allocator...");
    
    // 4 MiB of kernel heap. Frames no longer need to be physically contiguous,
    // so this is just 1024 mapped pages.
    const HEAP_SIZE_PAGES: usize = 1024;
    
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
    
    // Prove alignment and coalescing actually work.
    memory::heap::stress_test();
    
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
/// Bring up the process table.
///
/// This used to run `test_process_management()`, which created three processes
/// called `init`, `shell` and `background_task` — at pids 1, 2 and 3, because
/// the table's allocator is a monotonic counter starting at 1.
///
/// Those pids are *thread* ids. A ring-3 thread landing in slot 1, 2 or 3 was
/// attributed to one of them: `send_message`'s existence check passed, and
/// "init" had been granted `(SendMessage, Any)`, so the capability check passed
/// too. A thread in slot 4 or above got `SenderNotFound`. The failure was
/// id-dependent, which is another way of saying it looked intermittent.
///
/// The test also drove `process::scheduler`, which schedules nothing: it picks a
/// pid and writes it into a field. The real scheduler is `task`. Both tests are
/// gone; what replaces them is two ring-3 processes exchanging a message, which
/// exercises the same queueing and capability code with real senders.
fn init_process_management() {
    serial_println!("Initializing process management...");
    
    match crate::process::init_process_management() {
        Ok(()) => {
            serial_println!("Process management initialized successfully");
        }
        Err(e) => {
            serial_println!("Failed to initialize process management: {}", e);
            panic!("Process management initialization failed");
        }
    }
}

/// Initialize IPC system
fn init_ipc_system() {
    serial_println!("Initializing IPC system...");
    
    match crate::ipc::init_ipc_system() {
        Ok(()) => {
            serial_println!("IPC system initialized successfully");
        }
        Err(e) => {
            serial_println!("Failed to initialize IPC system: {}", e);
            panic!("IPC system initialization failed");
        }
    }
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

/// Test context switching functionality
fn test_context_switching_functionality() {
    serial_println!("Testing context switching functionality...");
    
    use crate::process::test_context_switching;
    
    // Test context switching
    test_context_switching();
    
    serial_println!("Context switching test complete");
}