use crate::process::ProcessId;
use crate::syscall::{SyscallError, SyscallResult};
use crate::syscall::numbers::*;
use crate::{serial_println};

/// Validate system call arguments before processing
pub fn validate_syscall_args(
    process_id: ProcessId,
    syscall_number: u64,
    args: &[u64; 6],
) -> Result<(), SyscallError> {
    // Check if the system call number is valid
    if !is_valid_syscall_number(syscall_number) {
        serial_println!("Invalid system call number: {}", syscall_number);
        return Err(SyscallError::InvalidSyscall);
    }
    
    // Perform syscall-specific argument validation
    match syscall_number {
        SYS_EXIT => validate_exit_args(args),
        SYS_FORK => validate_fork_args(args),
        SYS_EXEC => validate_exec_args(process_id, args),
        SYS_WAIT => validate_wait_args(args),
        SYS_GETPID | SYS_GETPPID => validate_no_args(args),
        SYS_KILL => validate_kill_args(args),
        
        SYS_MMAP => validate_mmap_args(args),
        SYS_MUNMAP => validate_munmap_args(args),
        SYS_MPROTECT => validate_mprotect_args(args),
        SYS_BRK | SYS_SBRK => validate_brk_args(args),
        
        SYS_OPEN => validate_open_args(process_id, args),
        SYS_CLOSE => validate_close_args(args),
        SYS_READ => validate_read_args(process_id, args),
        SYS_WRITE => validate_write_args(process_id, args),
        SYS_LSEEK => validate_lseek_args(args),
        SYS_STAT | SYS_FSTAT => validate_stat_args(process_id, args),
        SYS_MKDIR => validate_mkdir_args(process_id, args),
        SYS_RMDIR | SYS_UNLINK => validate_unlink_args(process_id, args),
        
        SYS_SEND_MESSAGE => validate_send_message_args(process_id, args),
        SYS_RECEIVE_MESSAGE => validate_receive_message_args(args),
        SYS_REPLY_MESSAGE => validate_reply_message_args(process_id, args),
        SYS_CREATE_CHANNEL => validate_create_channel_args(args),
        SYS_DESTROY_CHANNEL => validate_destroy_channel_args(args),
        
        SYS_DRIVER_REGISTER => validate_driver_register_args(process_id, args),
        SYS_DRIVER_UNREGISTER => validate_driver_unregister_args(process_id, args),
        SYS_DRIVER_REQUEST => validate_driver_request_args(process_id, args),
        SYS_DRIVER_RESPONSE => validate_driver_response_args(process_id, args),
        
        SYS_UNAME | SYS_SYSINFO | SYS_TIME => validate_info_args(args),
        SYS_CLOCK_GETTIME => validate_clock_gettime_args(args),
        
        SYS_GRANT_CAPABILITY => validate_grant_capability_args(process_id, args),
        SYS_REVOKE_CAPABILITY => validate_revoke_capability_args(process_id, args),
        SYS_CHECK_CAPABILITY => validate_check_capability_args(process_id, args),
        SYS_LIST_CAPABILITIES => validate_list_capabilities_args(args),
        
        #[cfg(debug_assertions)]
        SYS_DEBUG_PRINT => validate_debug_print_args(args),
        #[cfg(debug_assertions)]
        SYS_DEBUG_DUMP => validate_debug_dump_args(args),
        
        _ => {
            serial_println!("Unknown system call number: {}", syscall_number);
            Err(SyscallError::InvalidSyscall)
        }
    }
}

/// Validate that a pointer argument is valid for the given process
fn validate_user_pointer(process_id: ProcessId, ptr: u64, size: usize) -> Result<(), SyscallError> {
    if ptr == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    // TODO: Check that the pointer is within the process's valid address space
    // TODO: Check that the memory region is accessible (readable/writable as needed)
    // For now, we'll just check for null pointers
    
    Ok(())
}

/// Validate that a string pointer is valid and null-terminated
fn validate_user_string(process_id: ProcessId, ptr: u64, max_len: usize) -> Result<(), SyscallError> {
    if ptr == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    // TODO: Validate that the string is null-terminated within max_len
    // TODO: Check that the string is in valid user memory
    
    Ok(())
}

/// Validate file descriptor
fn validate_file_descriptor(fd: u64) -> Result<(), SyscallError> {
    // File descriptors should be reasonable values
    if fd > 1024 {
        return Err(SyscallError::BadFileDescriptor);
    }
    Ok(())
}

// Process management syscall validations
fn validate_exit_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    // Exit code can be any value
    Ok(())
}

fn validate_fork_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    // Fork takes no arguments
    Ok(())
}

fn validate_exec_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let path_ptr = args[0];
    let argv_ptr = args[1];
    let envp_ptr = args[2];
    
    validate_user_string(process_id, path_ptr, 4096)?;
    
    if argv_ptr != 0 {
        validate_user_pointer(process_id, argv_ptr, 8)?; // At least one pointer
    }
    
    if envp_ptr != 0 {
        validate_user_pointer(process_id, envp_ptr, 8)?; // At least one pointer
    }
    
    Ok(())
}

fn validate_wait_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    // Wait can take a status pointer (optional)
    Ok(())
}

fn validate_no_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    // These syscalls take no arguments
    Ok(())
}

fn validate_kill_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    let pid = args[0];
    let signal = args[1];
    
    if pid == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    // Validate signal number (basic range check)
    if signal > 64 {
        return Err(SyscallError::InvalidArgument);
    }
    
    Ok(())
}

// Memory management syscall validations
fn validate_mmap_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    let addr = args[0];
    let length = args[1];
    let prot = args[2];
    let flags = args[3];
    let fd = args[4];
    let offset = args[5];
    
    if length == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    // Validate protection flags
    if prot > 7 {  // PROT_READ | PROT_WRITE | PROT_EXEC
        return Err(SyscallError::InvalidArgument);
    }
    
    // If mapping a file, validate file descriptor
    if (flags & 0x01) == 0 && fd != u64::MAX {  // Not MAP_ANONYMOUS
        validate_file_descriptor(fd)?;
    }
    
    Ok(())
}

fn validate_munmap_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    let addr = args[0];
    let length = args[1];
    
    if addr == 0 || length == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    Ok(())
}

fn validate_mprotect_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    let addr = args[0];
    let length = args[1];
    let prot = args[2];
    
    if addr == 0 || length == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    if prot > 7 {  // PROT_READ | PROT_WRITE | PROT_EXEC
        return Err(SyscallError::InvalidArgument);
    }
    
    Ok(())
}

fn validate_brk_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    // brk/sbrk can take any address value
    Ok(())
}

// File system syscall validations
fn validate_open_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let path_ptr = args[0];
    let flags = args[1];
    let mode = args[2];
    
    validate_user_string(process_id, path_ptr, 4096)?;
    
    // Basic flag validation
    if flags > 0xFFFF {
        return Err(SyscallError::InvalidArgument);
    }
    
    Ok(())
}

fn validate_close_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    let fd = args[0];
    validate_file_descriptor(fd)
}

fn validate_read_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let fd = args[0];
    let buf_ptr = args[1];
    let count = args[2];
    
    validate_file_descriptor(fd)?;
    
    if count > 0 {
        validate_user_pointer(process_id, buf_ptr, count as usize)?;
    }
    
    Ok(())
}

fn validate_write_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let fd = args[0];
    let buf_ptr = args[1];
    let count = args[2];
    
    validate_file_descriptor(fd)?;
    
    if count > 0 {
        validate_user_pointer(process_id, buf_ptr, count as usize)?;
    }
    
    Ok(())
}

fn validate_lseek_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    let fd = args[0];
    let offset = args[1];
    let whence = args[2];
    
    validate_file_descriptor(fd)?;
    
    // Validate whence parameter (SEEK_SET, SEEK_CUR, SEEK_END)
    if whence > 2 {
        return Err(SyscallError::InvalidArgument);
    }
    
    Ok(())
}

fn validate_stat_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let path_or_fd = args[0];
    let stat_buf_ptr = args[1];
    
    validate_user_pointer(process_id, stat_buf_ptr, 144)?; // sizeof(struct stat)
    
    Ok(())
}

fn validate_mkdir_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let path_ptr = args[0];
    let mode = args[1];
    
    validate_user_string(process_id, path_ptr, 4096)?;
    
    Ok(())
}

fn validate_unlink_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let path_ptr = args[0];
    validate_user_string(process_id, path_ptr, 4096)
}

// IPC syscall validations
fn validate_send_message_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let receiver_pid = args[0];
    let message_ptr = args[1];
    let message_len = args[2];
    
    if receiver_pid == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    if message_len > 0 {
        validate_user_pointer(process_id, message_ptr, message_len as usize)?;
    }
    
    Ok(())
}

fn validate_receive_message_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    // Receive message takes optional timeout
    Ok(())
}

fn validate_reply_message_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let message_id = args[0];
    let reply_ptr = args[1];
    let reply_len = args[2];
    
    if message_id == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    if reply_len > 0 {
        validate_user_pointer(process_id, reply_ptr, reply_len as usize)?;
    }
    
    Ok(())
}

fn validate_create_channel_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    let other_pid = args[0];
    
    if other_pid == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    Ok(())
}

fn validate_destroy_channel_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    let channel_id = args[0];
    
    if channel_id == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    Ok(())
}

// Driver interface syscall validations
fn validate_driver_register_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let driver_info_ptr = args[0];
    validate_user_pointer(process_id, driver_info_ptr, 64) // Basic driver info struct size
}

fn validate_driver_unregister_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let driver_id = args[0];
    
    if driver_id == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    Ok(())
}

fn validate_driver_request_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let driver_id = args[0];
    let request_ptr = args[1];
    let request_len = args[2];
    
    if driver_id == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    if request_len > 0 {
        validate_user_pointer(process_id, request_ptr, request_len as usize)?;
    }
    
    Ok(())
}

fn validate_driver_response_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let request_id = args[0];
    let response_ptr = args[1];
    let response_len = args[2];
    
    if request_id == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    if response_len > 0 {
        validate_user_pointer(process_id, response_ptr, response_len as usize)?;
    }
    
    Ok(())
}

// System information syscall validations
fn validate_info_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    // These syscalls typically take a buffer pointer
    Ok(())
}

fn validate_clock_gettime_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    let clock_id = args[0];
    
    // Validate clock ID (basic range check)
    if clock_id > 10 {
        return Err(SyscallError::InvalidArgument);
    }
    
    Ok(())
}

// Security syscall validations
fn validate_grant_capability_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let target_pid = args[0];
    let capability_type = args[1];
    let resource_ptr = args[2];
    
    if target_pid == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    if resource_ptr != 0 {
        validate_user_pointer(process_id, resource_ptr, 64)?; // Basic resource descriptor size
    }
    
    Ok(())
}

fn validate_revoke_capability_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let target_pid = args[0];
    let capability_id = args[1];
    
    if target_pid == 0 || capability_id == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    Ok(())
}

fn validate_check_capability_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let capability_type = args[0];
    let resource_ptr = args[1];
    
    if resource_ptr != 0 {
        validate_user_pointer(process_id, resource_ptr, 64)?; // Basic resource descriptor size
    }
    
    Ok(())
}

fn validate_list_capabilities_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    // List capabilities takes optional buffer parameters
    Ok(())
}

// Debug syscall validations (only in debug builds)
#[cfg(debug_assertions)]
fn validate_debug_print_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    // Debug print can take any arguments
    Ok(())
}

#[cfg(debug_assertions)]
fn validate_debug_dump_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    // Debug dump can take any arguments
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessId;
    
    #[test_case]
    fn test_validate_syscall_args() {
        let pid = ProcessId::new(1);
        let args = [0; 6];
        
        // Test valid syscall
        assert!(validate_syscall_args(pid, SYS_GETPID, &args).is_ok());
        
        // Test invalid syscall number
        assert_eq!(
            validate_syscall_args(pid, 999, &args),
            Err(SyscallError::InvalidSyscall)
        );
    }
    
    #[test_case]
    fn test_validate_file_descriptor() {
        assert!(validate_file_descriptor(0).is_ok());
        assert!(validate_file_descriptor(10).is_ok());
        assert_eq!(
            validate_file_descriptor(2000),
            Err(SyscallError::BadFileDescriptor)
        );
    }
}