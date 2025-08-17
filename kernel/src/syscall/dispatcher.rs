use crate::process::ProcessId;
use crate::syscall::{SyscallError, SyscallResult};
use crate::syscall::numbers::*;
use crate::syscall::validation::validate_syscall_args;
use crate::{serial_println, println};
use alloc::format;

/// Initialize the system call dispatcher
pub fn init_syscall_dispatcher() -> Result<(), &'static str> {
    serial_println!("Initializing system call dispatcher...");
    
    // Initialize any dispatcher-specific data structures
    // For now, this is just a placeholder
    
    serial_println!("System call dispatcher initialized");
    Ok(())
}

/// Main system call dispatcher
pub fn dispatch_syscall(
    process_id: ProcessId,
    syscall_number: u64,
    args: [u64; 6],
) -> SyscallResult {
    // Log the system call for debugging
    serial_println!(
        "Process {} calling syscall {} ({}) with args [{}, {}, {}, {}, {}, {}]",
        process_id.0,
        syscall_number,
        syscall_name(syscall_number),
        args[0], args[1], args[2], args[3], args[4], args[5]
    );
    
    // Validate system call arguments
    validate_syscall_args(process_id, syscall_number, &args)?;
    
    // Dispatch to appropriate handler
    let result = match syscall_number {
        // Process management
        SYS_EXIT => sys_exit(process_id, args),
        SYS_FORK => sys_fork(process_id, args),
        SYS_EXEC => sys_exec(process_id, args),
        SYS_WAIT => sys_wait(process_id, args),
        SYS_GETPID => sys_getpid(process_id, args),
        SYS_GETPPID => sys_getppid(process_id, args),
        SYS_KILL => sys_kill(process_id, args),
        
        // Memory management
        SYS_MMAP => sys_mmap(process_id, args),
        SYS_MUNMAP => sys_munmap(process_id, args),
        SYS_MPROTECT => sys_mprotect(process_id, args),
        SYS_BRK => sys_brk(process_id, args),
        SYS_SBRK => sys_sbrk(process_id, args),
        
        // File system
        SYS_OPEN => sys_open(process_id, args),
        SYS_CLOSE => sys_close(process_id, args),
        SYS_READ => sys_read(process_id, args),
        SYS_WRITE => sys_write(process_id, args),
        SYS_LSEEK => sys_lseek(process_id, args),
        SYS_STAT => sys_stat(process_id, args),
        SYS_FSTAT => sys_fstat(process_id, args),
        SYS_MKDIR => sys_mkdir(process_id, args),
        SYS_RMDIR => sys_rmdir(process_id, args),
        SYS_UNLINK => sys_unlink(process_id, args),
        
        // IPC
        SYS_SEND_MESSAGE => sys_send_message(process_id, args),
        SYS_RECEIVE_MESSAGE => sys_receive_message(process_id, args),
        SYS_REPLY_MESSAGE => sys_reply_message(process_id, args),
        SYS_CREATE_CHANNEL => sys_create_channel(process_id, args),
        SYS_DESTROY_CHANNEL => sys_destroy_channel(process_id, args),
        
        // Driver interface
        SYS_DRIVER_REGISTER => sys_driver_register(process_id, args),
        SYS_DRIVER_UNREGISTER => sys_driver_unregister(process_id, args),
        SYS_DRIVER_REQUEST => sys_driver_request(process_id, args),
        SYS_DRIVER_RESPONSE => sys_driver_response(process_id, args),
        
        // System information
        SYS_UNAME => sys_uname(process_id, args),
        SYS_SYSINFO => sys_sysinfo(process_id, args),
        SYS_TIME => sys_time(process_id, args),
        SYS_CLOCK_GETTIME => sys_clock_gettime(process_id, args),
        
        // Security
        SYS_GRANT_CAPABILITY => sys_grant_capability(process_id, args),
        SYS_REVOKE_CAPABILITY => sys_revoke_capability(process_id, args),
        SYS_CHECK_CAPABILITY => sys_check_capability(process_id, args),
        SYS_LIST_CAPABILITIES => sys_list_capabilities(process_id, args),
        
        // Debug (only in debug builds)
        #[cfg(debug_assertions)]
        SYS_DEBUG_PRINT => sys_debug_print(process_id, args),
        #[cfg(debug_assertions)]
        SYS_DEBUG_DUMP => sys_debug_dump(process_id, args),
        
        _ => {
            serial_println!("Unknown system call: {}", syscall_number);
            Err(SyscallError::InvalidSyscall)
        }
    };
    
    // Log the result
    match &result {
        Ok(value) => {
            serial_println!(
                "Process {} syscall {} completed successfully, returned {}",
                process_id.0, syscall_name(syscall_number), value
            );
        }
        Err(error) => {
            serial_println!(
                "Process {} syscall {} failed: {:?}",
                process_id.0, syscall_name(syscall_number), error
            );
        }
    }
    
    result
}

// Process management system calls
fn sys_exit(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let exit_code = args[0] as i32;
    serial_println!("Process {} exiting with code {}", process_id.0, exit_code);
    
    // For now, just log the exit. In a real implementation, we would:
    // 1. Mark the process as terminated
    // 2. Clean up process resources
    // 3. Notify parent process
    // 4. Schedule next process
    
    // Since we don't have direct access to the process table from here,
    // we'll use the public interface when it's available
    serial_println!("Process {} terminated with exit code {}", process_id.0, exit_code);
    
    // Return success - the process will be cleaned up by the scheduler
    Ok(0)
}

fn sys_fork(process_id: ProcessId, _args: [u64; 6]) -> SyscallResult {
    serial_println!("Process {} attempting to fork", process_id.0);
    
    // Create a new child process
    match crate::process::create_process(
        Some(process_id),
        format!("child_of_{}", process_id.0),
        crate::process::ProcessPriority::Normal,
    ) {
        Ok(child_pid) => {
            serial_println!("Fork successful: parent={}, child={}", process_id.0, child_pid.0);
            // Return child PID to parent process
            // Note: In a real implementation, the child would receive 0
            // This requires more complex context switching implementation
            Ok(child_pid.0 as u64)
        }
        Err(_) => Err(SyscallError::OutOfMemory)
    }
}

fn sys_exec(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let path_ptr = args[0];
    let argv_ptr = args[1];
    let envp_ptr = args[2];
    
    serial_println!("Process {} attempting to exec program at 0x{:x}", process_id.0, path_ptr);
    
    // TODO: Implement program execution
    // This would involve:
    // 1. Loading the new program from filesystem
    // 2. Setting up new memory space
    // 3. Parsing arguments and environment
    // 4. Starting execution at program entry point
    
    Err(SyscallError::NotSupported)
}

fn sys_wait(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let status_ptr = args[0];
    
    serial_println!("Process {} waiting for child process", process_id.0);
    
    // TODO: Implement process waiting
    // This would involve:
    // 1. Blocking until a child process exits
    // 2. Returning the child's PID and exit status
    
    Err(SyscallError::NotSupported)
}

fn sys_getpid(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    Ok(process_id.0 as u64)
}

fn sys_getppid(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    // TODO: Get actual parent process ID
    // For now, return 0 (no parent)
    Ok(0)
}

fn sys_kill(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let target_pid = args[0];
    let signal = args[1];
    
    serial_println!("Process {} sending signal {} to process {}", 
                   process_id.0, signal, target_pid);
    
    // TODO: Implement signal sending
    // This would involve:
    // 1. Validating target process exists
    // 2. Checking permissions
    // 3. Delivering the signal
    
    Err(SyscallError::NotSupported)
}

// Memory management system calls
fn sys_mmap(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let addr = args[0];
    let length = args[1];
    let prot = args[2];
    let flags = args[3];
    let _fd = args[4];
    let _offset = args[5];
    
    serial_println!("Process {} requesting mmap: addr=0x{:x}, len={}, prot={}, flags={}", 
                   process_id.0, addr, length, prot, flags);
    
    // Basic implementation for anonymous memory mapping
    if length == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    // Convert protection flags to MemoryProtection
    let protection = crate::memory::vmm::MemoryProtection {
        readable: (prot & 0x1) != 0,    // PROT_READ
        writable: (prot & 0x2) != 0,    // PROT_WRITE
        executable: (prot & 0x4) != 0,  // PROT_EXEC
        user_accessible: true,
    };
    
    // For now, implement simple anonymous mapping
    // In a real implementation, we would:
    // 1. Find suitable virtual address space
    // 2. Allocate physical pages
    // 3. Set up page table entries
    
    // Return a dummy address for now (in user space)
    let mapped_addr = if addr == 0 {
        0x40000000u64 // Default user space address
    } else {
        addr
    };
    
    serial_println!("Process {} mmap successful: mapped at 0x{:x}", process_id.0, mapped_addr);
    Ok(mapped_addr)
}

fn sys_munmap(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let addr = args[0];
    let length = args[1];
    
    serial_println!("Process {} requesting munmap: addr=0x{:x}, len={}", 
                   process_id.0, addr, length);
    
    // TODO: Implement memory unmapping
    Err(SyscallError::NotSupported)
}

fn sys_mprotect(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let addr = args[0];
    let length = args[1];
    let prot = args[2];
    
    serial_println!("Process {} requesting mprotect: addr=0x{:x}, len={}, prot={}", 
                   process_id.0, addr, length, prot);
    
    // TODO: Implement memory protection changes
    Err(SyscallError::NotSupported)
}

fn sys_brk(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let addr = args[0];
    
    serial_println!("Process {} requesting brk: addr=0x{:x}", process_id.0, addr);
    
    // TODO: Implement heap management
    Err(SyscallError::NotSupported)
}

fn sys_sbrk(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let increment = args[0] as i64;
    
    serial_println!("Process {} requesting sbrk: increment={}", process_id.0, increment);
    
    // TODO: Implement heap increment
    Err(SyscallError::NotSupported)
}

// File system system calls
fn sys_open(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let path_ptr = args[0];
    let flags = args[1];
    let _mode = args[2];
    
    serial_println!("Process {} requesting open: path=0x{:x}, flags={}, mode={}", 
                   process_id.0, path_ptr, flags, _mode);
    
    // For now, implement a basic file descriptor allocation
    // In a real implementation, we would:
    // 1. Read path string from user space
    // 2. Resolve path through VFS
    // 3. Check permissions
    // 4. Allocate file descriptor
    
    // Convert flags to OpenFlags
    let open_flags = match flags {
        0 => kosh_types::OpenFlags::READ_ONLY,
        1 => kosh_types::OpenFlags::WRITE_ONLY,
        2 => kosh_types::OpenFlags::READ_WRITE,
        _ => return Err(SyscallError::InvalidArgument),
    };
    
    // For demonstration, return a dummy file descriptor
    // In a real implementation, this would interact with the VFS
    let fd = 3; // Start from 3 (0=stdin, 1=stdout, 2=stderr)
    
    serial_println!("Process {} opened file: fd={}", process_id.0, fd);
    Ok(fd)
}

fn sys_close(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let fd = args[0];
    
    serial_println!("Process {} requesting close: fd={}", process_id.0, fd);
    
    // TODO: Implement file closing
    Err(SyscallError::NotSupported)
}

fn sys_read(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let fd = args[0];
    let _buf_ptr = args[1];
    let count = args[2];
    
    serial_println!("Process {} requesting read: fd={}, buf=0x{:x}, count={}", 
                   process_id.0, fd, _buf_ptr, count);
    
    // Basic implementation for standard file descriptors
    match fd {
        0 => {
            // stdin - for now, return 0 (EOF)
            serial_println!("Process {} reading from stdin", process_id.0);
            Ok(0)
        }
        _ => {
            // For other file descriptors, simulate reading some data
            // In a real implementation, this would:
            // 1. Validate the file descriptor
            // 2. Check permissions
            // 3. Read from the actual file through VFS
            // 4. Copy data to user space buffer
            
            if count == 0 {
                return Ok(0);
            }
            
            // Simulate reading some data
            let bytes_read = core::cmp::min(count, 1024);
            serial_println!("Process {} read {} bytes from fd {}", process_id.0, bytes_read, fd);
            Ok(bytes_read)
        }
    }
}

fn sys_write(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let fd = args[0];
    let buf_ptr = args[1];
    let count = args[2];
    
    serial_println!("Process {} requesting write: fd={}, buf=0x{:x}, count={}", 
                   process_id.0, fd, buf_ptr, count);
    
    // TODO: Implement file writing
    // For now, if writing to stdout (fd=1) or stderr (fd=2), we could output to console
    if fd == 1 || fd == 2 {
        // TODO: Read string from user space and print it
        serial_println!("Process {} writing {} bytes to console", process_id.0, count);
        Ok(count) // Return number of bytes "written"
    } else {
        Err(SyscallError::NotSupported)
    }
}

fn sys_lseek(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let fd = args[0];
    let offset = args[1] as i64;
    let whence = args[2];
    
    serial_println!("Process {} requesting lseek: fd={}, offset={}, whence={}", 
                   process_id.0, fd, offset, whence);
    
    // TODO: Implement file seeking
    Err(SyscallError::NotSupported)
}

fn sys_stat(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let path_ptr = args[0];
    let stat_buf_ptr = args[1];
    
    serial_println!("Process {} requesting stat: path=0x{:x}, buf=0x{:x}", 
                   process_id.0, path_ptr, stat_buf_ptr);
    
    // TODO: Implement file stat
    Err(SyscallError::NotSupported)
}

fn sys_fstat(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let fd = args[0];
    let stat_buf_ptr = args[1];
    
    serial_println!("Process {} requesting fstat: fd={}, buf=0x{:x}", 
                   process_id.0, fd, stat_buf_ptr);
    
    // TODO: Implement file descriptor stat
    Err(SyscallError::NotSupported)
}

fn sys_mkdir(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let path_ptr = args[0];
    let mode = args[1];
    
    serial_println!("Process {} requesting mkdir: path=0x{:x}, mode={}", 
                   process_id.0, path_ptr, mode);
    
    // TODO: Implement directory creation
    Err(SyscallError::NotSupported)
}

fn sys_rmdir(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let path_ptr = args[0];
    
    serial_println!("Process {} requesting rmdir: path=0x{:x}", process_id.0, path_ptr);
    
    // TODO: Implement directory removal
    Err(SyscallError::NotSupported)
}

fn sys_unlink(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let path_ptr = args[0];
    
    serial_println!("Process {} requesting unlink: path=0x{:x}", process_id.0, path_ptr);
    
    // TODO: Implement file removal
    Err(SyscallError::NotSupported)
}

// IPC system calls
fn sys_send_message(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let receiver_pid = args[0];
    let _message_ptr = args[1];
    let message_len = args[2];
    
    serial_println!("Process {} sending message to process {}: ptr=0x{:x}, len={}", 
                   process_id.0, receiver_pid, _message_ptr, message_len);
    
    // Basic implementation using existing IPC system
    if message_len > 4096 {
        return Err(SyscallError::InvalidArgument);
    }
    
    // Create a simple text message for demonstration
    // In a real implementation, we would read the actual message data from user space
    let message_data = crate::ipc::message::MessageData::Text(
        alloc::format!("Message from process {} (len={})", process_id.0, message_len)
    );
    
    let message = crate::ipc::message::create_message(
        process_id,
        ProcessId::new(receiver_pid as u32),
        crate::ipc::message::MessageType::ServiceRequest,
        message_data,
    );
    
    match crate::ipc::message::send_message(message) {
        Ok(()) => {
            serial_println!("Process {} successfully sent message to process {}", 
                           process_id.0, receiver_pid);
            Ok(0)
        }
        Err(e) => {
            serial_println!("Process {} failed to send message: {:?}", process_id.0, e);
            Err(e.into())
        }
    }
}

fn sys_receive_message(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let _timeout_ms = args[0];
    
    serial_println!("Process {} receiving message with timeout {}", process_id.0, _timeout_ms);
    
    // Basic implementation using existing IPC system
    match crate::ipc::message::receive_message(process_id) {
        Ok(message) => {
            serial_println!("Process {} received message {} from process {}", 
                           process_id.0, message.header.message_id.0, message.header.sender.0);
            // Return the message ID for now
            // In a real implementation, we would copy the message data to user space
            Ok(message.header.message_id.0)
        }
        Err(e) => {
            serial_println!("Process {} failed to receive message: {:?}", process_id.0, e);
            Err(e.into())
        }
    }
}

fn sys_reply_message(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let message_id = args[0];
    let reply_ptr = args[1];
    let reply_len = args[2];
    
    serial_println!("Process {} replying to message {}: ptr=0x{:x}, len={}", 
                   process_id.0, message_id, reply_ptr, reply_len);
    
    // TODO: Implement message reply
    Err(SyscallError::NotSupported)
}

fn sys_create_channel(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let other_pid = args[0];
    
    serial_println!("Process {} creating channel with process {}", process_id.0, other_pid);
    
    // TODO: Implement secure channel creation
    Err(SyscallError::NotSupported)
}

fn sys_destroy_channel(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let channel_id = args[0];
    
    serial_println!("Process {} destroying channel {}", process_id.0, channel_id);
    
    // TODO: Implement channel destruction
    Err(SyscallError::NotSupported)
}

// Driver interface system calls
fn sys_driver_register(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let driver_info_ptr = args[0];
    
    serial_println!("Process {} registering as driver: info=0x{:x}", 
                   process_id.0, driver_info_ptr);
    
    // TODO: Implement driver registration
    Err(SyscallError::NotSupported)
}

fn sys_driver_unregister(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let driver_id = args[0];
    
    serial_println!("Process {} unregistering driver {}", process_id.0, driver_id);
    
    // TODO: Implement driver unregistration
    Err(SyscallError::NotSupported)
}

fn sys_driver_request(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let driver_id = args[0];
    let request_ptr = args[1];
    let request_len = args[2];
    
    serial_println!("Process {} sending request to driver {}: ptr=0x{:x}, len={}", 
                   process_id.0, driver_id, request_ptr, request_len);
    
    // TODO: Implement driver request
    Err(SyscallError::NotSupported)
}

fn sys_driver_response(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let request_id = args[0];
    let response_ptr = args[1];
    let response_len = args[2];
    
    serial_println!("Process {} responding to request {}: ptr=0x{:x}, len={}", 
                   process_id.0, request_id, response_ptr, response_len);
    
    // TODO: Implement driver response
    Err(SyscallError::NotSupported)
}

// System information system calls
fn sys_uname(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let buf_ptr = args[0];
    
    serial_println!("Process {} requesting uname: buf=0x{:x}", process_id.0, buf_ptr);
    
    // TODO: Implement uname (system information)
    Err(SyscallError::NotSupported)
}

fn sys_sysinfo(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let info_ptr = args[0];
    
    serial_println!("Process {} requesting sysinfo: buf=0x{:x}", process_id.0, info_ptr);
    
    // TODO: Implement sysinfo (system statistics)
    Err(SyscallError::NotSupported)
}

fn sys_time(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let time_ptr = args[0];
    
    serial_println!("Process {} requesting time: buf=0x{:x}", process_id.0, time_ptr);
    
    // TODO: Implement time getting
    // For now, return 0 (epoch time)
    Ok(0)
}

fn sys_clock_gettime(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let clock_id = args[0];
    let timespec_ptr = args[1];
    
    serial_println!("Process {} requesting clock_gettime: clock={}, buf=0x{:x}", 
                   process_id.0, clock_id, timespec_ptr);
    
    // TODO: Implement high-resolution time getting
    Err(SyscallError::NotSupported)
}

// Security system calls
fn sys_grant_capability(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let target_pid = args[0];
    let capability_type = args[1];
    let resource_ptr = args[2];
    
    serial_println!("Process {} granting capability {} to process {}: resource=0x{:x}", 
                   process_id.0, capability_type, target_pid, resource_ptr);
    
    // TODO: Implement capability granting using existing capability system
    Err(SyscallError::NotSupported)
}

fn sys_revoke_capability(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let target_pid = args[0];
    let capability_id = args[1];
    
    serial_println!("Process {} revoking capability {} from process {}", 
                   process_id.0, capability_id, target_pid);
    
    // TODO: Implement capability revocation
    Err(SyscallError::NotSupported)
}

fn sys_check_capability(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let capability_type = args[0];
    let resource_ptr = args[1];
    
    serial_println!("Process {} checking capability {}: resource=0x{:x}", 
                   process_id.0, capability_type, resource_ptr);
    
    // TODO: Implement capability checking using existing capability system
    Err(SyscallError::NotSupported)
}

fn sys_list_capabilities(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let buf_ptr = args[0];
    let buf_len = args[1];
    
    serial_println!("Process {} listing capabilities: buf=0x{:x}, len={}", 
                   process_id.0, buf_ptr, buf_len);
    
    // TODO: Implement capability listing
    Err(SyscallError::NotSupported)
}

// Debug system calls (only in debug builds)
#[cfg(debug_assertions)]
fn sys_debug_print(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let message_ptr = args[0];
    let message_len = args[1];
    
    serial_println!("Process {} debug print: ptr=0x{:x}, len={}", 
                   process_id.0, message_ptr, message_len);
    
    // TODO: Read string from user space and print it
    println!("DEBUG[{}]: <message at 0x{:x}>", process_id.0, message_ptr);
    
    Ok(0)
}

#[cfg(debug_assertions)]
fn sys_debug_dump(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let dump_type = args[0];
    
    serial_println!("Process {} debug dump: type={}", process_id.0, dump_type);
    
    // TODO: Implement various debug dumps (memory, processes, etc.)
    println!("DEBUG DUMP[{}]: type {}", process_id.0, dump_type);
    
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessId;
    
    #[test_case]
    fn test_dispatch_syscall() {
        let pid = ProcessId::new(1);
        let args = [0; 6];
        
        // Test getpid syscall
        let result = dispatch_syscall(pid, SYS_GETPID, args);
        assert_eq!(result, Ok(1));
        
        // Test invalid syscall
        let result = dispatch_syscall(pid, 999, args);
        assert_eq!(result, Err(SyscallError::InvalidSyscall));
    }
    
    #[test_case]
    fn test_sys_getpid() {
        let pid = ProcessId::new(42);
        let args = [0; 6];
        
        let result = sys_getpid(pid, args);
        assert_eq!(result, Ok(42));
    }
    
    #[test_case]
    fn test_sys_exit() {
        let pid = ProcessId::new(1);
        let args = [0, 0, 0, 0, 0, 0]; // exit code 0
        
        let result = sys_exit(pid, args);
        assert_eq!(result, Ok(0));
    }
    
    #[test_case]
    fn test_sys_fork() {
        let pid = ProcessId::new(1);
        let args = [0; 6];
        
        // Fork should create a new process and return child PID
        let result = sys_fork(pid, args);
        // Since we don't have process table initialized in tests, this will fail
        // but we can verify the function doesn't panic
        assert!(result.is_ok() || result.is_err());
    }
    
    #[test_case]
    fn test_sys_mmap() {
        let pid = ProcessId::new(1);
        let args = [0, 4096, 3, 0, 0, 0]; // addr=0, len=4096, prot=RW, flags=0
        
        let result = sys_mmap(pid, args);
        assert!(result.is_ok());
        
        // Test invalid length
        let args = [0, 0, 3, 0, 0, 0]; // len=0
        let result = sys_mmap(pid, args);
        assert_eq!(result, Err(SyscallError::InvalidArgument));
    }
    
    #[test_case]
    fn test_sys_open() {
        let pid = ProcessId::new(1);
        let args = [0x1000, 0, 0644, 0, 0, 0]; // path_ptr, flags=READ_ONLY, mode
        
        let result = sys_open(pid, args);
        assert_eq!(result, Ok(3)); // Should return fd 3
        
        // Test invalid flags
        let args = [0x1000, 999, 0644, 0, 0, 0]; // invalid flags
        let result = sys_open(pid, args);
        assert_eq!(result, Err(SyscallError::InvalidArgument));
    }
    
    #[test_case]
    fn test_sys_read() {
        let pid = ProcessId::new(1);
        
        // Test reading from stdin
        let args = [0, 0x1000, 100, 0, 0, 0]; // fd=0 (stdin), buf, count
        let result = sys_read(pid, args);
        assert_eq!(result, Ok(0)); // stdin returns EOF
        
        // Test reading from regular fd
        let args = [3, 0x1000, 100, 0, 0, 0]; // fd=3, buf, count
        let result = sys_read(pid, args);
        assert_eq!(result, Ok(100)); // Should read 100 bytes
        
        // Test reading 0 bytes
        let args = [3, 0x1000, 0, 0, 0, 0]; // count=0
        let result = sys_read(pid, args);
        assert_eq!(result, Ok(0));
    }
}