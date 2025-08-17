#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use core::panic::PanicInfo;
use kosh_types::ProcessId;
use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Initialize the heap allocator
fn init_heap() {
    const HEAP_SIZE: usize = 64 * 1024; // 64KB heap
    static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
    
    unsafe {
        ALLOCATOR.lock().init(HEAP.as_mut_ptr(), HEAP_SIZE);
    }
}

mod syscalls;
mod service_manager;
mod process_spawner;

use service_manager::ServiceManager;
use process_spawner::ProcessSpawner;
use syscalls::{sys_debug_print, sys_wait, sys_getpid};

/// Signal numbers for process management
const SIGTERM: i32 = 15;
const SIGKILL: i32 = 9;
const SIGCHLD: i32 = 17;

/// Main init process state
struct InitProcess {
    service_manager: ServiceManager,
    process_spawner: ProcessSpawner,
    shutdown_requested: bool,
    essential_services: Vec<&'static str>,
}

impl InitProcess {
    fn new() -> Self {
        Self {
            service_manager: ServiceManager::new(),
            process_spawner: ProcessSpawner::new(),
            shutdown_requested: false,
            essential_services: vec![
                "fs-service",
                "driver-manager",
            ],
        }
    }

    /// Initialize the system by starting essential services
    fn initialize_system(&mut self) {
        #[cfg(debug_assertions)]
        {
            let message = b"Init: Starting system initialization\n";
            sys_debug_print(message);
        }

        // Start essential system services
        for service_name in &self.essential_services {
            match self.process_spawner.spawn_service(service_name, &[]) {
                Ok(pid) => {
                    self.service_manager.register_service(service_name, pid);
                    #[cfg(debug_assertions)]
                    {
                        let message = b"Init: Essential service started\n";
                        sys_debug_print(message);
                    }
                }
                Err(_) => {
                    #[cfg(debug_assertions)]
                    {
                        let message = b"Init: Failed to start essential service\n";
                        sys_debug_print(message);
                    }
                    // For essential services, we might want to retry or panic
                    // For now, continue with other services
                }
            }
        }

        // Give services time to initialize
        self.wait_for_services_to_start();

        // Start a shell for testing/debugging
        match self.process_spawner.spawn_shell() {
            Ok(pid) => {
                self.service_manager.register_service("shell", pid);
                #[cfg(debug_assertions)]
                {
                    let message = b"Init: Shell started for testing\n";
                    sys_debug_print(message);
                }
            }
            Err(_) => {
                #[cfg(debug_assertions)]
                {
                    let message = b"Init: Failed to start shell\n";
                    sys_debug_print(message);
                }
            }
        }
    }

    /// Wait for services to start up
    fn wait_for_services_to_start(&mut self) {
        // Simple delay mechanism - in a real system this would be more sophisticated
        for _ in 0..1000000 {
            core::hint::spin_loop();
        }
        
        // Update service states
        self.service_manager.check_services();
    }

    /// Main event loop for the init process
    fn run_main_loop(&mut self) {
        #[cfg(debug_assertions)]
        {
            let message = b"Init: Entering main event loop\n";
            sys_debug_print(message);
        }

        loop {
            // Check for child process exits
            self.handle_child_processes();

            // Check service health and restart failed services
            self.service_manager.check_services();

            // Handle shutdown if requested
            if self.shutdown_requested {
                self.handle_shutdown();
                break;
            }

            // Small delay to prevent busy waiting
            self.yield_cpu();
        }
    }

    /// Handle child process exits
    fn handle_child_processes(&mut self) {
        // Non-blocking wait for child processes
        loop {
            match sys_wait() {
                Ok((pid, _status)) => {
                    #[cfg(debug_assertions)]
                    {
                        let message = b"Init: Child process exited\n";
                        sys_debug_print(message);
                    }
                    
                    // Notify service manager about the exit
                    self.service_manager.handle_process_exit(pid);
                    
                    // Check if this was an essential service
                    if self.is_essential_service_pid(pid) {
                        #[cfg(debug_assertions)]
                        {
                            let message = b"Init: Essential service died, attempting restart\n";
                            sys_debug_print(message);
                        }
                    }
                }
                Err(_) => {
                    // No more child processes to wait for
                    break;
                }
            }
        }
    }

    /// Check if a PID belongs to an essential service
    fn is_essential_service_pid(&self, pid: ProcessId) -> bool {
        for service_name in &self.essential_services {
            if let Some(service_pid) = self.service_manager.get_service_pid(service_name) {
                if service_pid == pid {
                    return true;
                }
            }
        }
        false
    }

    /// Handle system shutdown
    fn handle_shutdown(&mut self) {
        #[cfg(debug_assertions)]
        {
            let message = b"Init: Beginning system shutdown\n";
            sys_debug_print(message);
        }

        // Phase 1: Graceful shutdown of non-essential services
        self.service_manager.shutdown_all_services();
        
        // Wait for services to shut down gracefully
        let mut wait_cycles = 0;
        const MAX_WAIT_CYCLES: u32 = 100;
        
        while !self.service_manager.all_services_stopped() && wait_cycles < MAX_WAIT_CYCLES {
            self.handle_child_processes();
            self.yield_cpu();
            wait_cycles += 1;
        }

        // Phase 2: Force kill any remaining services
        if !self.service_manager.all_services_stopped() {
            #[cfg(debug_assertions)]
            {
                let message = b"Init: Force killing remaining services\n";
                sys_debug_print(message);
            }
            self.service_manager.force_kill_all();
        }

        #[cfg(debug_assertions)]
        {
            let message = b"Init: System shutdown complete\n";
            sys_debug_print(message);
        }
    }

    /// Yield CPU to other processes
    fn yield_cpu(&self) {
        // Simple spin loop for yielding - in a real system this would be a proper yield syscall
        for _ in 0..10000 {
            core::hint::spin_loop();
        }
    }

    /// Request system shutdown
    fn request_shutdown(&mut self) {
        self.shutdown_requested = true;
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    #[cfg(debug_assertions)]
    {
        let message = b"Init: Process starting\n";
        sys_debug_print(message);
    }

    // Verify we are PID 1
    let pid = sys_getpid();
    if pid != 1 {
        #[cfg(debug_assertions)]
        {
            let message = b"Init: Warning - not running as PID 1\n";
            sys_debug_print(message);
        }
    }

    // Initialize heap allocator
    init_heap();
    
    // Create and initialize the init process
    let mut init = InitProcess::new();
    
    // Initialize the system
    init.initialize_system();
    
    // Run the main event loop
    init.run_main_loop();
    
    // If we reach here, the system is shutting down
    #[cfg(debug_assertions)]
    {
        let message = b"Init: Exiting\n";
        sys_debug_print(message);
    }
    
    syscalls::sys_exit(0);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    #[cfg(debug_assertions)]
    {
        let message = b"Init: PANIC occurred!\n";
        sys_debug_print(message);
    }
    
    // In a real system, we might want to try to save state or restart
    // For now, just halt
    loop {
        core::hint::spin_loop();
    }
}