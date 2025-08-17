#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::format;
use linked_list_allocator::LockedHeap;
use core::panic::PanicInfo;
use kosh_types::{DriverId, DriverError, Capability};
use kosh_ipc::DriverRequestData;
use kosh_service::{ServiceHandler, ServiceMessage, ServiceResponse, ServiceType, ServiceData, ServiceStatus, ServiceRunner, DriverRequest};

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

mod driver_registry;
mod driver_loader;
mod dependency_resolver;
mod isolation;

use driver_registry::DriverRegistry;
use driver_loader::DriverLoader;
use dependency_resolver::DependencyResolver;
use isolation::DriverIsolation;

pub struct DriverManager {
    registry: DriverRegistry,
    loader: DriverLoader,
    dependency_resolver: DependencyResolver,
    isolation: DriverIsolation,
    next_driver_id: DriverId,
}

impl DriverManager {
    pub fn new() -> Self {
        Self {
            registry: DriverRegistry::new(),
            loader: DriverLoader::new(),
            dependency_resolver: DependencyResolver::new(),
            isolation: DriverIsolation::new(),
            next_driver_id: 1,
        }
    }

    pub fn load_driver(&mut self, driver_path: &str, capabilities: Vec<Capability>) -> Result<DriverId, DriverError> {
        // Load the driver binary
        let driver_binary = self.loader.load_driver_binary(driver_path)?;
        
        // Resolve dependencies
        let dependencies = self.dependency_resolver.resolve_dependencies(&driver_binary)?;
        
        // Create isolated environment
        let driver_id = self.next_driver_id;
        self.next_driver_id += 1;
        
        let process_id = self.isolation.create_driver_process(driver_id, capabilities)?;
        
        // Register the driver
        self.registry.register_driver(driver_id, driver_path, process_id, dependencies)?;
        
        // Start the driver process
        self.isolation.start_driver_process(process_id, driver_binary)?;
        
        Ok(driver_id)
    }

    pub fn unload_driver(&mut self, driver_id: DriverId) -> Result<(), DriverError> {
        // Check if other drivers depend on this one
        if self.dependency_resolver.has_dependents(driver_id) {
            return Err(DriverError::ResourceBusy);
        }

        // Get driver info
        let driver_info = self.registry.get_driver_info(driver_id)
            .ok_or(DriverError::InvalidRequest)?;

        // Stop the driver process
        self.isolation.stop_driver_process(driver_info.process_id)?;

        // Unregister the driver
        self.registry.unregister_driver(driver_id)?;

        Ok(())
    }

    pub fn handle_driver_request(&mut self, request: DriverRequestData) -> Result<Vec<u8>, DriverError> {
        let driver_info = self.registry.get_driver_info(request.driver_id)
            .ok_or(DriverError::InvalidRequest)?;

        // Forward request to the driver process
        self.isolation.send_request_to_driver(driver_info.process_id, request)
    }

    pub fn list_drivers(&self) -> Vec<DriverId> {
        self.registry.list_drivers()
    }

    pub fn get_driver_status(&self, driver_id: DriverId) -> Option<DriverStatus> {
        self.registry.get_driver_status(driver_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverStatus {
    Loading,
    Running,
    Stopped,
    Error,
}

/// Driver Manager Service Handler
struct DriverManagerService {
    driver_manager: DriverManager,
}

impl DriverManagerService {
    fn new() -> Self {
        Self {
            driver_manager: DriverManager::new(),
        }
    }
}

impl ServiceHandler for DriverManagerService {
    fn handle_request(&mut self, request: ServiceMessage) -> ServiceResponse {
        let response_data = match request.data {
            ServiceData::DriverRequest(driver_request) => {
                match driver_request {
                    DriverRequest::LoadDriver { path } => {
                        // Load driver with default capabilities
                        let capabilities = vec![]; // Would be configured based on driver requirements
                        match self.driver_manager.load_driver(&path, capabilities) {
                            Ok(driver_id) => ServiceData::Binary(driver_id.to_le_bytes().to_vec()),
                            Err(_) => ServiceData::Empty,
                        }
                    }
                    DriverRequest::UnloadDriver { driver_id } => {
                        match self.driver_manager.unload_driver(driver_id) {
                            Ok(_) => ServiceData::Empty,
                            Err(_) => ServiceData::Empty,
                        }
                    }
                    DriverRequest::ListDrivers => {
                        let drivers = self.driver_manager.list_drivers();
                        let mut result = String::new();
                        for driver_id in drivers {
                            result.push_str(&format!("Driver ID: {}\n", driver_id));
                        }
                        ServiceData::Text(result)
                    }
                    DriverRequest::SendToDriver { driver_id, data } => {
                        // For now, just return a mock response since we can't easily
                        // handle the lifetime issue with DriverRequestData
                        let response = format!("Response from driver {}", driver_id);
                        ServiceData::Text(response)
                    }
                }
            }
            _ => ServiceData::Empty,
        };

        ServiceResponse {
            request_id: request.request_id,
            status: ServiceStatus::Success,
            data: response_data,
        }
    }

    fn get_service_type(&self) -> ServiceType {
        ServiceType::DriverManager
    }

    fn initialize(&mut self) -> Result<(), kosh_service::ServiceError> {
        debug_print(b"Driver Manager: Initializing service\n");
        
        // Load essential drivers
        let essential_drivers = vec![
            "/drivers/graphics.ko",
            "/drivers/keyboard.ko",
            "/drivers/storage.ko",
        ];
        
        for driver_path in essential_drivers {
            match self.driver_manager.load_driver(driver_path, vec![]) {
                Ok(driver_id) => {
                    debug_print(b"Driver Manager: Essential driver loaded\n");
                }
                Err(_) => {
                    debug_print(b"Driver Manager: Failed to load essential driver\n");
                    // Continue with other drivers
                }
            }
        }
        
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), kosh_service::ServiceError> {
        debug_print(b"Driver Manager: Shutting down service\n");
        
        // Unload all drivers
        let drivers = self.driver_manager.list_drivers();
        for driver_id in drivers {
            let _ = self.driver_manager.unload_driver(driver_id);
        }
        
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Initialize the heap allocator
    init_heap();
    
    debug_print(b"Driver Manager: Starting driver manager service\n");
    
    // Create and start the driver manager service
    let driver_service = DriverManagerService::new();
    let mut service_runner = ServiceRunner::new(driver_service);
    
    // Initialize the service
    if let Err(_) = service_runner.start() {
        debug_print(b"Driver Manager: Failed to start service\n");
        sys_exit(1);
    }
    
    debug_print(b"Driver Manager: Service started, entering main loop\n");
    
    // Main service loop
    loop {
        // Process incoming requests
        if let Err(_) = service_runner.run_once() {
            debug_print(b"Driver Manager: Error processing request\n");
        }
        
        // Yield CPU to prevent busy waiting
        yield_cpu();
    }
}

fn init_heap() {
    const HEAP_SIZE: usize = 64 * 1024; // 64KB heap
    static mut HEAP_MEMORY: [u8; 64 * 1024] = [0; 64 * 1024];
    
    unsafe {
        let heap_ptr = core::ptr::addr_of_mut!(HEAP_MEMORY);
        ALLOCATOR.lock().init((*heap_ptr).as_mut_ptr(), HEAP_SIZE);
    }
}

fn yield_cpu() {
    for _ in 0..1000 {
        core::hint::spin_loop();
    }
}

fn debug_print(message: &[u8]) {
    #[cfg(debug_assertions)]
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 100u64, // SYS_DEBUG_PRINT
            in("rdi") message.as_ptr(),
            in("rsi") message.len(),
            options(nostack, preserves_flags)
        );
    }
}

fn sys_exit(status: i32) -> ! {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 1u64, // SYS_EXIT
            in("rdi") status,
            options(noreturn)
        );
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    debug_print(b"Driver Manager: PANIC occurred!\n");
    sys_exit(1);
}