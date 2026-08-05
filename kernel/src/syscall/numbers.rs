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
/// Give up the rest of this thread's time slice.
///
/// Cheap to implement and unusually useful as a test: it is the only way for
/// userspace to force a context switch *from inside a system call*, which is
/// exactly the situation per-thread kernel stacks exist for.
pub const SYS_YIELD: u64 = 8;
/// Load a named program and run it. Not `fork`+`exec`: there is one address
/// space, so there is nothing to duplicate. `spawn(path, len) -> task id`.
pub const SYS_SPAWN: u64 = 9;

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
/// `request_device(name_ptr, name_len)` -> 0
///
/// Ask for direct `in`/`out` access to a named device's ports. A *name*, not a
/// port range: the kernel decides what "ata0" consists of (see
/// `platform::devports`), so a driver cannot ask for the interrupt controller.
pub const SYS_REQUEST_DEVICE: u64 = 44;
/// `release_device(name_ptr, name_len)` -> 0
///
/// Give the ports back. Implicit on exit, so this is only for a driver that
/// wants to hand a disk over while it keeps running.
pub const SYS_RELEASE_DEVICE: u64 = 45;
/// `register_service(name_ptr, name_len)` -> 0
///
/// Claim a name. A service saying "anyone may talk to me" — which is the only
/// way two processes that are not parent and child can reach each other, since
/// every other capability here is scoped to a specific process.
pub const SYS_REGISTER_SERVICE: u64 = 46;
/// `lookup_service(name_ptr, name_len)` -> pid
///
/// Find a service *and* get a capability for it. The grant is the point: a pid
/// without one is a phone number with no line attached.
pub const SYS_LOOKUP_SERVICE: u64 = 47;

/// System information system calls
pub const SYS_UNAME: u64 = 50;
pub const SYS_SYSINFO: u64 = 51;
pub const SYS_TIME: u64 = 52;
pub const SYS_CLOCK_GETTIME: u64 = 53;

/// Directory listing. Fixed-size records, so userspace can walk the buffer
/// without a parser. Not in the 20..29 file-system block because that range was
/// already allocated when this was added.
pub const SYS_GETDENTS: u64 = 70;

/// Security and capability system calls
pub const SYS_GRANT_CAPABILITY: u64 = 60;
pub const SYS_REVOKE_CAPABILITY: u64 = 61;
pub const SYS_CHECK_CAPABILITY: u64 = 62;
pub const SYS_LIST_CAPABILITIES: u64 = 63;

/// Debug and testing system calls.
//
// Deliberately NOT cfg(debug_assertions): a syscall number that exists in debug
// builds and vanishes in release is exactly the kind of silent difference that
// hides bugs until the release build is the one that matters.
//
// (An earlier version of this comment cited `userspace/shell/src/output.rs` as a
// caller. That file was deleted two phases ago along with the rest of the shell's
// fake service layer. The reasoning still holds on its own — SYS_GETDENTS, which
// ksh does call, is above the old release-only limit of 63 — but the citation was
// stale, which is its own small lesson about comments that name files.)
pub const SYS_DEBUG_PRINT: u64 = 100;
pub const SYS_DEBUG_DUMP: u64 = 101;

/// Maximum system call number (for validation).
///
/// Deliberately one value, not one per build profile. It used to be 101 in debug
/// and 63 in release, which meant every syscall above 63 — including
/// SYS_DEBUG_PRINT, which userspace calls unconditionally, and SYS_GETDENTS —
/// was rejected as invalid in exactly the build that ships. A syscall surface
/// that differs between debug and release is a bug that only appears in
/// release.
pub const MAX_SYSCALL_NUMBER: u64 = 101;

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
        SYS_YIELD => "yield",
        SYS_SPAWN => "spawn",

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
        SYS_REQUEST_DEVICE => "request_device",
        SYS_RELEASE_DEVICE => "release_device",
        SYS_REGISTER_SERVICE => "register_service",
        SYS_LOOKUP_SERVICE => "lookup_service",
        
        SYS_GETDENTS => "getdents",
        SYS_UNAME => "uname",
        SYS_SYSINFO => "sysinfo",
        SYS_TIME => "time",
        SYS_CLOCK_GETTIME => "clock_gettime",
        
        SYS_GRANT_CAPABILITY => "grant_capability",
        SYS_REVOKE_CAPABILITY => "revoke_capability",
        SYS_CHECK_CAPABILITY => "check_capability",
        SYS_LIST_CAPABILITIES => "list_capabilities",
        
        SYS_DEBUG_PRINT => "debug_print",
        
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