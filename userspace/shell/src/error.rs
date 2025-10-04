#![allow(dead_code)]

use alloc::string::String;
use alloc::format;
use kosh_service::ServiceError;

/// Comprehensive error handling for the enhanced shell
#[derive(Debug, Clone)]
pub enum ShellError {
    // Command parsing errors
    ParseError(String),
    InvalidCommand(String),
    InvalidArguments(String),
    
    // File system errors
    FileNotFound(String),
    PermissionDenied(String),
    DirectoryNotFound(String),
    FileAlreadyExists(String),
    NotADirectory(String),
    IsADirectory(String),
    
    // Process management errors
    ProcessNotFound(u32),
    ProcessAccessDenied(u32),
    InvalidSignal(String),
    
    // Service communication errors
    ServiceUnavailable(String),
    ServiceTimeout(String),
    ServiceError(ServiceError),
    
    // I/O errors
    InputError(String),
    OutputError(String),
    
    // System errors
    SystemCallFailed(u64, i32),
    InsufficientMemory,
    ResourceExhausted(String),
    
    // Internal errors
    InternalError(String),
}

impl ShellError {
    /// Get a user-friendly error message
    pub fn user_message(&self) -> String {
        match self {
            ShellError::ParseError(msg) => format!("Parse error: {}", msg),
            ShellError::InvalidCommand(cmd) => format!("Command not found: {}", cmd),
            ShellError::InvalidArguments(msg) => format!("Invalid arguments: {}", msg),
            
            ShellError::FileNotFound(path) => format!("File not found: {}", path),
            ShellError::PermissionDenied(path) => format!("Permission denied: {}", path),
            ShellError::DirectoryNotFound(path) => format!("Directory not found: {}", path),
            ShellError::FileAlreadyExists(path) => format!("File already exists: {}", path),
            ShellError::NotADirectory(path) => format!("Not a directory: {}", path),
            ShellError::IsADirectory(path) => format!("Is a directory: {}", path),
            
            ShellError::ProcessNotFound(pid) => format!("Process not found: {}", pid),
            ShellError::ProcessAccessDenied(pid) => format!("Access denied to process: {}", pid),
            ShellError::InvalidSignal(signal) => format!("Invalid signal: {}", signal),
            
            ShellError::ServiceUnavailable(service) => format!("Service unavailable: {}", service),
            ShellError::ServiceTimeout(service) => format!("Service timeout: {}", service),
            ShellError::ServiceError(err) => format!("Service error: {:?}", err),
            
            ShellError::InputError(msg) => format!("Input error: {}", msg),
            ShellError::OutputError(msg) => format!("Output error: {}", msg),
            
            ShellError::SystemCallFailed(call, code) => format!("System call {} failed with code {}", call, code),
            ShellError::InsufficientMemory => String::from("Insufficient memory"),
            ShellError::ResourceExhausted(resource) => format!("Resource exhausted: {}", resource),
            
            ShellError::InternalError(msg) => format!("Internal error: {}", msg),
        }
    }
    
    /// Suggest a fix for the error if possible
    pub fn suggest_fix(&self) -> Option<String> {
        match self {
            ShellError::InvalidCommand(cmd) => {
                // In a real implementation, this could suggest similar commands
                Some(format!("Try 'help' to see available commands. Did you mean a similar command to '{}'?", cmd))
            }
            ShellError::FileNotFound(path) => {
                Some(format!("Check if the file '{}' exists using 'ls' command", path))
            }
            ShellError::PermissionDenied(_) => {
                Some(String::from("Check file permissions or run with appropriate privileges"))
            }
            ShellError::ProcessNotFound(_) => {
                Some(String::from("Use 'ps' command to list running processes"))
            }
            ShellError::ServiceUnavailable(_) => {
                Some(String::from("Wait for the service to become available or restart the system"))
            }
            _ => None,
        }
    }
    
    /// Get the error category for logging/debugging
    pub fn category(&self) -> ErrorCategory {
        match self {
            ShellError::ParseError(_) | ShellError::InvalidCommand(_) | ShellError::InvalidArguments(_) => {
                ErrorCategory::Parse
            }
            ShellError::FileNotFound(_) | ShellError::PermissionDenied(_) | ShellError::DirectoryNotFound(_) |
            ShellError::FileAlreadyExists(_) | ShellError::NotADirectory(_) | ShellError::IsADirectory(_) => {
                ErrorCategory::FileSystem
            }
            ShellError::ProcessNotFound(_) | ShellError::ProcessAccessDenied(_) | ShellError::InvalidSignal(_) => {
                ErrorCategory::Process
            }
            ShellError::ServiceUnavailable(_) | ShellError::ServiceTimeout(_) | ShellError::ServiceError(_) => {
                ErrorCategory::Service
            }
            ShellError::InputError(_) | ShellError::OutputError(_) => {
                ErrorCategory::IO
            }
            ShellError::SystemCallFailed(_, _) | ShellError::InsufficientMemory | ShellError::ResourceExhausted(_) => {
                ErrorCategory::System
            }
            ShellError::InternalError(_) => {
                ErrorCategory::Internal
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Parse,
    FileSystem,
    Process,
    Service,
    IO,
    System,
    Internal,
}

impl From<ServiceError> for ShellError {
    fn from(error: ServiceError) -> Self {
        ShellError::ServiceError(error)
    }
}

/// Result type for shell operations
pub type ShellResult<T> = Result<T, ShellError>;

/// Error recovery strategies
#[derive(Debug, Clone, Copy)]
pub enum RecoveryStrategy {
    Retry,
    Fallback,
    Abort,
    Continue,
}

impl ShellError {
    /// Get the recommended recovery strategy for this error
    pub fn recovery_strategy(&self) -> RecoveryStrategy {
        match self {
            ShellError::ServiceTimeout(_) | ShellError::ServiceUnavailable(_) => RecoveryStrategy::Retry,
            ShellError::FileNotFound(_) | ShellError::ProcessNotFound(_) => RecoveryStrategy::Abort,
            ShellError::PermissionDenied(_) => RecoveryStrategy::Abort,
            ShellError::InvalidCommand(_) => RecoveryStrategy::Continue,
            ShellError::InsufficientMemory | ShellError::ResourceExhausted(_) => RecoveryStrategy::Fallback,
            _ => RecoveryStrategy::Continue,
        }
    }
}