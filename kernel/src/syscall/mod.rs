use core::arch::asm;
use crate::process::ProcessId;
use crate::{serial_println, println};

pub mod dispatcher;
pub mod entry;
pub mod files;
pub mod numbers;
pub mod uaccess;
pub mod validation;
pub mod error;
pub mod test;

pub use dispatcher::*;
pub use numbers::*;
pub use validation::*;
pub use error::*;

/// System call result type
pub type SyscallResult = Result<u64, SyscallError>;

/// Initialize the system call interface
pub fn init_syscall_interface() -> Result<(), &'static str> {
    serial_println!("Initializing system call interface...");
    
    // Initialize the system call dispatcher
    dispatcher::init_syscall_dispatcher()?;
    
    // Program the SYSCALL/SYSRET MSRs. This replaces the int 0x80 stub that
    // never existed.
    entry::init();
    
    serial_println!("System call interface initialized successfully");
    Ok(())
}

/// System call entry point (called from assembly interrupt handler)
#[no_mangle]
pub extern "C" fn syscall_entry(
    syscall_number: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
) -> u64 {
    // Get current process ID (for now, use a dummy value)
    let current_pid = ProcessId::new(1); // TODO: Get actual current process
    
    // Dispatch the system call
    match dispatcher::dispatch_syscall(
        current_pid,
        syscall_number,
        [arg1, arg2, arg3, arg4, arg5, arg6],
    ) {
        Ok(result) => result,
        Err(error) => {
            serial_println!("System call {} failed: {:?}", syscall_number, error);
            error.to_errno() as u64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test_case]
    fn test_syscall_interface_init() {
        // Test that system call interface can be initialized
        // This is a basic test since we can't actually test interrupt handling in unit tests
        assert!(init_syscall_interface().is_ok());
    }
}