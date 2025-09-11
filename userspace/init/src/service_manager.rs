use alloc::vec::Vec;
use alloc::string::String;
use kosh_types::ProcessId;
use crate::syscalls::sys_kill;
#[cfg(debug_assertions)]
use crate::syscalls::sys_debug_print;

#[derive(Debug, Clone)]
pub struct Service {
    pub name: String,
    pub pid: ProcessId,
    pub state: ServiceState,
    pub restart_count: u32,
    pub max_restarts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

pub struct ServiceManager {
    services: Vec<Service>,
}

impl ServiceManager {
    pub fn new() -> Self {
        Self {
            services: Vec::new(),
        }
    }
    
    /// Register a new service with the service manager
    pub fn register_service(&mut self, name: &str, pid: ProcessId) {
        let service = Service {
            name: String::from(name),
            pid,
            state: ServiceState::Starting,
            restart_count: 0,
            max_restarts: 3, // Allow up to 3 restarts
        };
        
        self.services.push(service);
        
        #[cfg(debug_assertions)]
        {
            let message = b"Service registered\n";
            sys_debug_print(message);
        }
    }
    
    /// Handle when a process exits
    pub fn handle_process_exit(&mut self, pid: ProcessId) {
        for service in &mut self.services {
            if service.pid == pid {
                match service.state {
                    ServiceState::Stopping => {
                        service.state = ServiceState::Stopped;
                        #[cfg(debug_assertions)]
                        {
                            let message = b"Service stopped gracefully\n";
                            sys_debug_print(message);
                        }
                    }
                    ServiceState::Running | ServiceState::Starting => {
                        // Unexpected exit - mark as failed
                        service.state = ServiceState::Failed;
                        #[cfg(debug_assertions)]
                        {
                            let message = b"Service failed unexpectedly\n";
                            sys_debug_print(message);
                        }
                    }
                    _ => {
                        // Already in expected state
                    }
                }
                break;
            }
        }
    }
    
    /// Check all services and restart failed ones if needed
    pub fn check_services(&mut self) {
        for service in &mut self.services {
            match service.state {
                ServiceState::Starting => {
                    // Service should have started by now, mark as running
                    service.state = ServiceState::Running;
                }
                ServiceState::Failed => {
                    if service.restart_count < service.max_restarts {
                        #[cfg(debug_assertions)]
                        {
                            let message = b"Attempting to restart failed service\n";
                            sys_debug_print(message);
                        }
                        
                        // Try to restart the service
                        // This would involve spawning the service again
                        // For now, just increment the restart count
                        service.restart_count += 1;
                        service.state = ServiceState::Starting;
                    } else {
                        #[cfg(debug_assertions)]
                        {
                            let message = b"Service exceeded max restarts\n";
                            sys_debug_print(message);
                        }
                    }
                }
                _ => {
                    // Service is in a stable state
                }
            }
        }
    }
    
    /// Gracefully shutdown all services
    pub fn shutdown_all_services(&mut self) {
        for service in &mut self.services {
            if service.state == ServiceState::Running {
                // Send SIGTERM to gracefully shutdown
                match sys_kill(service.pid, 15) {
                    Ok(_) => {
                        service.state = ServiceState::Stopping;
                        #[cfg(debug_assertions)]
                        {
                            let message = b"Sent shutdown signal to service\n";
                            sys_debug_print(message);
                        }
                    }
                    Err(_) => {
                        // Process might already be dead
                        service.state = ServiceState::Stopped;
                    }
                }
            }
        }
    }
    
    /// Force kill all remaining services
    pub fn force_kill_all(&mut self) {
        for service in &mut self.services {
            if service.state != ServiceState::Stopped {
                // Send SIGKILL to force termination
                let _ = sys_kill(service.pid, 9);
                service.state = ServiceState::Stopped;
                
                #[cfg(debug_assertions)]
                {
                    let message = b"Force killed service\n";
                    sys_debug_print(message);
                }
            }
        }
    }
    
    /// Check if all services have stopped
    pub fn all_services_stopped(&self) -> bool {
        self.services.iter().all(|service| service.state == ServiceState::Stopped)
    }
    
    /// Get the PID of a service by name
    pub fn get_service_pid(&self, name: &str) -> Option<ProcessId> {
        self.services.iter()
            .find(|service| service.name == name && service.state == ServiceState::Running)
            .map(|service| service.pid)
    }
    
    /// Get the state of a service by name
    pub fn get_service_state(&self, name: &str) -> Option<ServiceState> {
        self.services.iter()
            .find(|service| service.name == name)
            .map(|service| service.state)
    }
    
    /// List all services
    pub fn list_services(&self) -> &[Service] {
        &self.services
    }
}