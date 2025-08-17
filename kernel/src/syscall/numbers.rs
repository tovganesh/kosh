/// System call numbers for the Kosh operating system
/// These numbers define the interface between user space and kernel space

/// Process management system calls
pub const SYS_EXIT: u64 = 1;
pub const SYS_FORK: u64 = 2;
pub const SYS_EXEC: u64 = 3;
pub const SYS_WAIT: u64 = 4;
pub const SYS_GETPID: u64 = 5;
pub const SYS_GETPPID: u64 = 6;
pub const SYS_KILL: u64 = 7;

/// Memory management system calls
pub const SYS_MMAP: u64 = 10;
pub const SYS_MUNMAP: u64 = 11;
pub const SYS_MPROTECT: u64 = 12;
pub const SYS_BRK: u64 = 13;
pub const SYS_SBRK: u64 = 14;

/// File system system calls
pub const SYS_OPEN: u64 = 20;
pub const SYS_CLOSE: u64 = 21;
pub const SYS_READ: u64 = 22;
pub const SYS_WRITE: u64 = 23;
pub const SYS_LSEEK: u64 = 24;
pub const SYS_STAT: u64 = 25;
pub const SYS_FSTAT: u64 = 26;
pub const SYS_MKDIR: u64 = 27;
pub const SYS_RMDIR: u64 = 28;
pub const SYS_UNLINK: u64 = 29;

/// IPC system calls
pub const SYS_SEND_MESSAGE: u64 = 30;
pub const SYS_RECEIVE_MESSAGE: u64 = 31;
pub const SYS_REPLY_MESSAGE: u64 = 32;
pub const SYS_CREATE_CHANNEL: u64 = 33;
pub const SYS_DESTROY_CHANNEL: u64 = 34;

/// Driver interface system calls
pub const SYS_DRIVER_REGISTER: u64 = 40;
pub const SYS_DRIVER_UNREGISTER: u64 = 41;
pub const SYS_DRIVER_REQUEST: u64 = 42;
pub const SYS_DRIVER_RESPONSE: u64 = 43;

/// System information system calls
pub const SYS_UNAME: u64 = 50;
pub const SYS_SYSINFO: u64 = 51;
pub const SYS_TIME: u64 = 52;
pub const SYS_CLOCK_GETTIME: u64 = 53;

/// Security and capability system calls
pub const SYS_GRANT_CAPABILITY: u64 = 60;
pub const SYS_REVOKE_CAPABILITY: u64 = 61;
pub const SYS_CHECK_CAPABILITY: u64 = 62;
pub const SYS_LIST_CAPABILITIES: u64 = 63;

/// Debug and testing system calls (only available in debug builds)
#[cfg(debug_assertions)]
pub const SYS_DEBUG_PRINT: u64 = 100;
#[cfg(debug_assertions)]
pub const SYS_DEBUG_DUMP: u64 = 101;

/// Maximum system call number (for validation)
#[cfg(debug_assertions)]
pub const MAX_SYSCALL_NUMBER: u64 = 101;
#[cfg(not(debug_assertions))]
pub const MAX_SYSCALL_NUMBER: u64 = 63;

/// Check if a system call number is valid
pub fn is_valid_syscall_number(syscall_number: u64) -> bool {
    syscall_number > 0 && syscall_number <= MAX_SYSCALL_NUMBER
}

/// Get the name of a system call for debugging purposes
pub fn syscall_name(syscall_number: u64) -> &'static str {
    match syscall_number {
        SYS_EXIT => "exit",
        SYS_FORK => "fork",
        SYS_EXEC => "exec",
        SYS_WAIT => "wait",
        SYS_GETPID => "getpid",
        SYS_GETPPID => "getppid",
        SYS_KILL => "kill",
        
        SYS_MMAP => "mmap",
        SYS_MUNMAP => "munmap",
        SYS_MPROTECT => "mprotect",
        SYS_BRK => "brk",
        SYS_SBRK => "sbrk",
        
        SYS_OPEN => "open",
        SYS_CLOSE => "close",
        SYS_READ => "read",
        SYS_WRITE => "write",
        SYS_LSEEK => "lseek",
        SYS_STAT => "stat",
        SYS_FSTAT => "fstat",
        SYS_MKDIR => "mkdir",
        SYS_RMDIR => "rmdir",
        SYS_UNLINK => "unlink",
        
        SYS_SEND_MESSAGE => "send_message",
        SYS_RECEIVE_MESSAGE => "receive_message",
        SYS_REPLY_MESSAGE => "reply_message",
        SYS_CREATE_CHANNEL => "create_channel",
        SYS_DESTROY_CHANNEL => "destroy_channel",
        
        SYS_DRIVER_REGISTER => "driver_register",
        SYS_DRIVER_UNREGISTER => "driver_unregister",
        SYS_DRIVER_REQUEST => "driver_request",
        SYS_DRIVER_RESPONSE => "driver_response",
        
        SYS_UNAME => "uname",
        SYS_SYSINFO => "sysinfo",
        SYS_TIME => "time",
        SYS_CLOCK_GETTIME => "clock_gettime",
        
        SYS_GRANT_CAPABILITY => "grant_capability",
        SYS_REVOKE_CAPABILITY => "revoke_capability",
        SYS_CHECK_CAPABILITY => "check_capability",
        SYS_LIST_CAPABILITIES => "list_capabilities",
        
        #[cfg(debug_assertions)]
        SYS_DEBUG_PRINT => "debug_print",
        #[cfg(debug_assertions)]
        SYS_DEBUG_DUMP => "debug_dump",
        
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test_case]
    fn test_valid_syscall_numbers() {
        assert!(is_valid_syscall_number(SYS_EXIT));
        assert!(is_valid_syscall_number(SYS_READ));
        assert!(is_valid_syscall_number(SYS_SEND_MESSAGE));
        assert!(!is_valid_syscall_number(0));
        assert!(!is_valid_syscall_number(MAX_SYSCALL_NUMBER + 1));
    }
    
    #[test_case]
    fn test_syscall_names() {
        assert_eq!(syscall_name(SYS_EXIT), "exit");
        assert_eq!(syscall_name(SYS_READ), "read");
        assert_eq!(syscall_name(SYS_SEND_MESSAGE), "send_message");
        assert_eq!(syscall_name(999), "unknown");
    }
}