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
        SYS_SPAWN => validate_spawn_args(process_id, args),
        SYS_GETPID | SYS_GETPPID | SYS_YIELD => validate_no_args(args),
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
        SYS_RECEIVE_MESSAGE => validate_receive_message_args(process_id, args),
        SYS_REPLY_MESSAGE => validate_reply_message_args(process_id, args),
        SYS_CREATE_CHANNEL => validate_create_channel_args(args),
        SYS_DESTROY_CHANNEL => validate_destroy_channel_args(args),
        
        SYS_DRIVER_REGISTER => validate_driver_register_args(process_id, args),
        SYS_DRIVER_UNREGISTER => validate_driver_unregister_args(process_id, args),
        SYS_DRIVER_REQUEST => validate_driver_request_args(process_id, args),
        SYS_DRIVER_RESPONSE => validate_driver_response_args(process_id, args),
        SYS_REQUEST_DEVICE | SYS_RELEASE_DEVICE => validate_device_args(process_id, args),
        SYS_REGISTER_SERVICE | SYS_LOOKUP_SERVICE => validate_service_args(process_id, args),
        
        SYS_UNAME | SYS_SYSINFO | SYS_TIME => validate_info_args(args),
        SYS_CLOCK_GETTIME => validate_clock_gettime_args(args),
        
        SYS_GRANT_CAPABILITY => validate_grant_capability_args(process_id, args),
        SYS_REVOKE_CAPABILITY => validate_revoke_capability_args(process_id, args),
        SYS_CHECK_CAPABILITY => validate_check_capability_args(process_id, args),
        SYS_LIST_CAPABILITIES => validate_list_capabilities_args(args),
        
        SYS_GETDENTS => validate_getdents_args(process_id, args),

        SYS_DEBUG_PRINT => validate_debug_print_args(args),
        SYS_DEBUG_DUMP => validate_debug_dump_args(args),
        
        _ => {
            serial_println!("Unknown system call number: {}", syscall_number);
            Err(SyscallError::InvalidSyscall)
        }
    }
}

/// Validate that a pointer argument is valid for the given process
/// Check that a user pointer really is one.
///
/// This used to be a null check and two TODOs. The checks now exist, in
/// `syscall::uaccess`, so this delegates to them: the span must lie in the lower
/// canonical half and every page must be present and USER_ACCESSIBLE. The
/// copy helpers verify the same thing when they run, but failing here means a
/// bad pointer is rejected before any handler starts acting on the call.
fn validate_user_pointer(
    _process_id: ProcessId,
    ptr: u64,
    size: usize,
    writable: bool,
) -> Result<(), SyscallError> {
    if ptr == 0 {
        return Err(SyscallError::InvalidArgument);
    }

    crate::syscall::uaccess::validate_user_range(ptr, size, writable)
        .map_err(|_| SyscallError::InvalidArgument)
}

/// Validate that a string pointer is valid and null-terminated
fn validate_user_string(process_id: ProcessId, ptr: u64, max_len: usize) -> Result<(), SyscallError> {
    if ptr == 0 {
        return Err(SyscallError::InvalidArgument);
    }

    // Only the first byte can be checked cheaply — the length is not known until
    // the terminator is found, and walking user memory to find it is the copy
    // helper's job. This at least rejects a pointer into the kernel half.
    validate_user_pointer(process_id, ptr, 1, false)?;
    let _ = max_len;
    
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

/// `exec(path_ptr, path_len)`.
///
/// The same shape as `spawn`: an explicit length rather than a NUL terminator,
/// so the span can be bound-checked before the kernel walks user memory. The
/// previous version validated `args[1]` and `args[2]` as `argv`/`envp` pointer
/// arrays, which this ABI does not have — and `argv` support needs somewhere to
/// put the strings in the new address space, which is its own job.
fn validate_exec_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let path_ptr = args[0];
    let path_len = args[1];

    if path_len == 0 || path_len > 255 {
        return Err(SyscallError::InvalidArgument);
    }
    validate_user_pointer(process_id, path_ptr, path_len as usize, false)
}

/// `wait(task_id, status_ptr)`.
///
/// The status pointer is optional and checked in the handler rather than here,
/// because the handler has to check it *before* blocking — validating a
/// destination after the child has been reaped leaves nowhere to put the answer.
fn validate_wait_args(_args: &[u64; 6]) -> Result<(), SyscallError> {
    Ok(())
}

/// `spawn(path_ptr, path_len)`.
fn validate_spawn_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let path_ptr = args[0];
    let path_len = args[1];

    if path_len == 0 || path_len > 255 {
        return Err(SyscallError::InvalidArgument);
    }
    validate_user_pointer(process_id, path_ptr, path_len as usize, false)
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
/// `mmap(addr, length, prot, flags, fd, offset)`.
///
/// **`fd` is only read when `MAP_ANONYMOUS` is absent**, and that is not a
/// nicety. `fd` arrives in R8, and a caller using a four-argument syscall
/// wrapper — which is every caller here, because anonymous mappings need no more
/// — never sets R8. Reading it means reading a stale register.
///
/// This bug was found once already, in `sys_mmap` itself, and fixed there. The
/// validator kept it, and the symptom was worth the reminder: the *first*
/// `mmap` in a program worked and a later one failed with `EBADF`, because by
/// then R8 happened to hold something above 1024. The lesson from last time was
/// written down and the second instance still shipped, which is an argument for
/// grepping rather than remembering.
///
/// The mask is also corrected: `MAP_ANONYMOUS` is `0x20`, not `0x01`.
fn validate_mmap_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    const MAP_ANONYMOUS: u64 = 0x20;

    let length = args[1];
    let prot = args[2];
    let flags = args[3];

    if length == 0 {
        return Err(SyscallError::InvalidArgument);
    }

    // PROT_READ | PROT_WRITE | PROT_EXEC
    if prot > 7 {
        return Err(SyscallError::InvalidArgument);
    }

    if flags & MAP_ANONYMOUS == 0 {
        // A file mapping, which the handler refuses anyway — but the caller has
        // told us it set `fd`, so checking it is meaningful.
        validate_file_descriptor(args[4])?;
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

fn validate_getdents_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let path_ptr = args[0];
    let path_len = args[1];
    let buf_ptr = args[2];
    let buf_len = args[3];

    if path_len == 0 || path_len > 4096 {
        return Err(SyscallError::InvalidArgument);
    }
    validate_user_pointer(process_id, path_ptr, path_len as usize, false)?;

    if buf_len == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    validate_user_pointer(process_id, buf_ptr, buf_len as usize, true)?;

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
    
    // read() writes into the caller's buffer, so it must be writable.
    if count > 0 {
        validate_user_pointer(process_id, buf_ptr, count as usize, true)?;
    }
    
    Ok(())
}

fn validate_write_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let fd = args[0];
    let buf_ptr = args[1];
    let count = args[2];
    
    validate_file_descriptor(fd)?;
    
    // write() only reads the caller's buffer.
    if count > 0 {
        validate_user_pointer(process_id, buf_ptr, count as usize, false)?;
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
    // ABI: (path_ptr, path_len, out_ptr). This used to validate args[1] as a
    // 144-byte writable buffer — but args[1] is the path *length*, so it was
    // checking whether the address `9` (or whatever the path length happened to
    // be) was mapped. It never is: that is page 0, the null guard. Every stat
    // call failed with InvalidArgument before its handler ran, while open and
    // getdents — whose validators happen to match their ABIs — worked.
    let path_ptr = args[0];
    let path_len = args[1];
    let out_ptr = args[2];

    if path_len == 0 || path_len > 4096 {
        return Err(SyscallError::InvalidArgument);
    }
    validate_user_pointer(process_id, path_ptr, path_len as usize, false)?;
    validate_user_pointer(process_id, out_ptr, core::mem::size_of::<crate::syscall::files::UserDirEntry>(), true)?;

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
/// `send_message(receiver_pid, ptr, len)`.
fn validate_send_message_args(
    process_id: ProcessId,
    args: &[u64; 6],
) -> Result<(), SyscallError> {
    let receiver = args[0];
    let ptr = args[1];
    let len = args[2];

    // Pid 0 is the kernel's own thread, which has no queue and nothing to say.
    if receiver == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    if len == 0 || len > 4096 {
        return Err(SyscallError::InvalidArgument);
    }
    validate_user_pointer(process_id, ptr, len as usize, false)
}

/// `receive_message(buf_ptr, buf_len, blocking)`.
///
/// The destination is checked here rather than in the handler because a receive
/// that blocks and *then* discovers it has nowhere to put the answer has already
/// consumed the message.
fn validate_receive_message_args(
    process_id: ProcessId,
    args: &[u64; 6],
) -> Result<(), SyscallError> {
    let buf = args[0];
    let len = args[1];

    if len == 0 || len > 4096 {
        return Err(SyscallError::InvalidArgument);
    }
    validate_user_pointer(process_id, buf, len as usize, true)
}

fn validate_reply_message_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let message_id = args[0];
    let reply_ptr = args[1];
    let reply_len = args[2];
    
    if message_id == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    if reply_len > 0 {
        validate_user_pointer(process_id, reply_ptr, reply_len as usize, false)?;
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
    validate_user_pointer(process_id, driver_info_ptr, 64, false) // Basic driver info struct size
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
        validate_user_pointer(process_id, request_ptr, request_len as usize, false)?;
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
        validate_user_pointer(process_id, response_ptr, response_len as usize, true)?;
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
        validate_user_pointer(process_id, resource_ptr, 64, false)?; // Basic resource descriptor size
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
        validate_user_pointer(process_id, resource_ptr, 64, false)?; // Basic resource descriptor size
    }
    
    Ok(())
}

fn validate_list_capabilities_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    // List capabilities takes optional buffer parameters
    Ok(())
}

// Debug syscall validations (only in debug builds)
/// `register_service` / `lookup_service` take a service name.
fn validate_service_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let len = args[1] as usize;
    if len == 0 || len > 16 {
        return Err(SyscallError::InvalidArgument);
    }
    validate_user_pointer(process_id, args[0], len, false)
}

/// `request_device` / `release_device` take a device name.
///
/// A validator is not optional here. The default arm of `validate_syscall_args`
/// rejects anything it does not recognise, so a syscall added to the dispatcher
/// and not to this table is refused with `InvalidSyscall` before its handler is
/// ever reached — which looks exactly like the syscall not existing.
fn validate_device_args(process_id: ProcessId, args: &[u64; 6]) -> Result<(), SyscallError> {
    let len = args[1] as usize;
    if len == 0 || len > 16 {
        return Err(SyscallError::InvalidArgument);
    }
    validate_user_pointer(process_id, args[0], len, false)
}

fn validate_debug_print_args(args: &[u64; 6]) -> Result<(), SyscallError> {
    // Debug print can take any arguments
    Ok(())
}

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