use alloc::string::String;
use kosh_types::ProcessId;
use crate::syscalls::{sys_fork, sys_exec, sys_debug_print};

pub struct ProcessSpawner {
    // Could store configuration or state here in the future
}

impl ProcessSpawner {
    pub fn new() -> Self {
        Self {}
    }
    
    /// Spawn a new service process
    pub fn spawn_service(&mut self, service_name: &str, args: &[&str]) -> Result<ProcessId, SpawnError> {
        // Create the full path to the service binary
        let service_path = self.get_service_path(service_name);
        
        #[cfg(debug_assertions)]
        {
            let message = b"Attempting to spawn service\n";
            sys_debug_print(message);
        }
        
        // Fork a new process
        match sys_fork() {
            Ok(pid) => {
                if pid == 0 {
                    // We are in the child process
                    // Execute the service binary
                    match sys_exec(&service_path, args) {
                        Ok(_) => {
                            // This should never return if exec succeeds
                            unreachable!("exec returned successfully");
                        }
                        Err(_) => {
                            // Exec failed, exit child process
                            #[cfg(debug_assertions)]
                            {
                                let message = b"Failed to exec service\n";
                                sys_debug_print(message);
                            }
                            crate::syscalls::sys_exit(1);
                        }
                    }
                } else {
                    // We are in the parent process (init)
                    // Return the child PID
                    #[cfg(debug_assertions)]
                    {
                        let message = b"Service spawned successfully\n";
                        sys_debug_print(message);
                    }
                    Ok(pid)
                }
            }
            Err(_) => {
                #[cfg(debug_assertions)]
                {
                    let message = b"Failed to fork process\n";
                    sys_debug_print(message);
                }
                Err(SpawnError::ForkFailed)
            }
        }
    }
    
    /// Spawn a regular user process (not a service)
    pub fn spawn_process(&mut self, program_path: &str, args: &[&str]) -> Result<ProcessId, SpawnError> {
        #[cfg(debug_assertions)]
        {
            let message = b"Attempting to spawn process\n";
            sys_debug_print(message);
        }
        
        match sys_fork() {
            Ok(pid) => {
                if pid == 0 {
                    // Child process - execute the program
                    match sys_exec(program_path, args) {
                        Ok(_) => {
                            unreachable!("exec returned successfully");
                        }
                        Err(_) => {
                            #[cfg(debug_assertions)]
                            {
                                let message = b"Failed to exec process\n";
                                sys_debug_print(message);
                            }
                            crate::syscalls::sys_exit(1);
                        }
                    }
                } else {
                    // Parent process - return child PID
                    Ok(pid)
                }
            }
            Err(_) => {
                Err(SpawnError::ForkFailed)
            }
        }
    }
    
    /// Get the full path to a service binary
    fn get_service_path(&self, service_name: &str) -> String {
        // In a real system, this would look in standard service directories
        // For now, assume services are in /system/services/
        let mut path = String::from("/system/services/");
        path.push_str(service_name);
        path
    }
    
    /// Spawn a shell process for testing
    pub fn spawn_shell(&mut self) -> Result<ProcessId, SpawnError> {
        self.spawn_process("/system/bin/shell", &[])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    ForkFailed,
    ExecFailed,
    InvalidPath,
    PermissionDenied,
}

impl core::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SpawnError::ForkFailed => write!(f, "Failed to fork process"),
            SpawnError::ExecFailed => write!(f, "Failed to execute program"),
            SpawnError::InvalidPath => write!(f, "Invalid program path"),
            SpawnError::PermissionDenied => write!(f, "Permission denied"),
        }
    }
}