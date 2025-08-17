use core::fmt;
use alloc::format;

/// System call error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallError {
    /// Invalid system call number
    InvalidSyscall,
    /// Invalid argument provided to system call
    InvalidArgument,
    /// Permission denied
    PermissionDenied,
    /// Resource not found
    NotFound,
    /// Process not found
    ProcessNotFound,
    /// Resource already exists
    AlreadyExists,
    /// Operation not supported
    NotSupported,
    /// No memory available
    OutOfMemory,
    /// Resource temporarily unavailable
    WouldBlock,
    /// Operation interrupted
    Interrupted,
    /// Invalid file descriptor
    BadFileDescriptor,
    /// Broken pipe
    BrokenPipe,
    /// Address already in use
    AddressInUse,
    /// Connection refused
    ConnectionRefused,
    /// Operation timed out
    TimedOut,
    /// System resource exhausted
    ResourceExhausted,
    /// Internal kernel error
    InternalError,
}

impl SyscallError {
    /// Convert system call error to errno value
    pub fn to_errno(self) -> i32 {
        match self {
            SyscallError::InvalidSyscall => -1,      // EPERM equivalent
            SyscallError::InvalidArgument => -22,    // EINVAL
            SyscallError::PermissionDenied => -13,   // EACCES
            SyscallError::NotFound => -2,            // ENOENT
            SyscallError::ProcessNotFound => -3,     // ESRCH
            SyscallError::AlreadyExists => -17,      // EEXIST
            SyscallError::NotSupported => -95,       // EOPNOTSUPP
            SyscallError::OutOfMemory => -12,        // ENOMEM
            SyscallError::WouldBlock => -11,         // EAGAIN/EWOULDBLOCK
            SyscallError::Interrupted => -4,         // EINTR
            SyscallError::BadFileDescriptor => -9,   // EBADF
            SyscallError::BrokenPipe => -32,         // EPIPE
            SyscallError::AddressInUse => -98,       // EADDRINUSE
            SyscallError::ConnectionRefused => -111, // ECONNREFUSED
            SyscallError::TimedOut => -110,          // ETIMEDOUT
            SyscallError::ResourceExhausted => -105, // ENOBUFS
            SyscallError::InternalError => -5,       // EIO
        }
    }
    
    /// Get a human-readable description of the error
    pub fn description(self) -> &'static str {
        match self {
            SyscallError::InvalidSyscall => "Invalid system call number",
            SyscallError::InvalidArgument => "Invalid argument",
            SyscallError::PermissionDenied => "Permission denied",
            SyscallError::NotFound => "Resource not found",
            SyscallError::ProcessNotFound => "Process not found",
            SyscallError::AlreadyExists => "Resource already exists",
            SyscallError::NotSupported => "Operation not supported",
            SyscallError::OutOfMemory => "Out of memory",
            SyscallError::WouldBlock => "Resource temporarily unavailable",
            SyscallError::Interrupted => "Operation interrupted",
            SyscallError::BadFileDescriptor => "Bad file descriptor",
            SyscallError::BrokenPipe => "Broken pipe",
            SyscallError::AddressInUse => "Address already in use",
            SyscallError::ConnectionRefused => "Connection refused",
            SyscallError::TimedOut => "Operation timed out",
            SyscallError::ResourceExhausted => "System resource exhausted",
            SyscallError::InternalError => "Internal kernel error",
        }
    }
}

impl fmt::Display for SyscallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

/// Convert various error types to SyscallError
impl From<crate::ipc::MessageError> for SyscallError {
    fn from(error: crate::ipc::MessageError) -> Self {
        match error {
            crate::ipc::MessageError::InvalidMessage => SyscallError::InvalidArgument,
            crate::ipc::MessageError::SenderNotFound => SyscallError::NotFound,
            crate::ipc::MessageError::ReceiverNotFound => SyscallError::NotFound,
            crate::ipc::MessageError::QueueFull => SyscallError::ResourceExhausted,
            crate::ipc::MessageError::NoMessage => SyscallError::WouldBlock,
            crate::ipc::MessageError::PermissionDenied => SyscallError::PermissionDenied,
            crate::ipc::MessageError::MessageTooLarge => SyscallError::InvalidArgument,
            crate::ipc::MessageError::Timeout => SyscallError::TimedOut,
            crate::ipc::MessageError::ResourceExhausted => SyscallError::ResourceExhausted,
        }
    }
}

impl From<crate::process::ProcessError> for SyscallError {
    fn from(error: crate::process::ProcessError) -> Self {
        match error {
            crate::process::ProcessError::ProcessNotFound => SyscallError::NotFound,
            crate::process::ProcessError::ProcessTableFull => SyscallError::ResourceExhausted,
            crate::process::ProcessError::InvalidStateTransition => SyscallError::InvalidArgument,
            crate::process::ProcessError::ProcessTerminated => SyscallError::InvalidArgument,
            crate::process::ProcessError::OutOfMemory => SyscallError::OutOfMemory,
            crate::process::ProcessError::InvalidPid => SyscallError::InvalidArgument,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test_case]
    fn test_error_to_errno() {
        assert_eq!(SyscallError::InvalidArgument.to_errno(), -22);
        assert_eq!(SyscallError::PermissionDenied.to_errno(), -13);
        assert_eq!(SyscallError::NotFound.to_errno(), -2);
        assert_eq!(SyscallError::OutOfMemory.to_errno(), -12);
    }
    
    #[test_case]
    fn test_error_descriptions() {
        assert_eq!(SyscallError::InvalidArgument.description(), "Invalid argument");
        assert_eq!(SyscallError::PermissionDenied.description(), "Permission denied");
        assert_eq!(SyscallError::NotFound.description(), "Resource not found");
    }
    
    #[test_case]
    fn test_error_display() {
        let error = SyscallError::InvalidArgument;
        assert_eq!(format!("{}", error), "Invalid argument");
    }
}