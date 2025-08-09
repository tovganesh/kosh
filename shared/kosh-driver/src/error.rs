use alloc::{string::String, vec::Vec, boxed::Box};
use kosh_types::DriverError;

/// Extended error information for drivers
#[derive(Debug, Clone)]
pub struct DriverErrorInfo {
    pub error: DriverError,
    pub context: String,
    pub error_code: Option<u32>,
    pub recovery_suggestions: Vec<RecoverySuggestion>,
    pub timestamp: u64, // In a real implementation, this would be a proper timestamp
}

/// Suggestions for error recovery
#[derive(Debug, Clone)]
pub enum RecoverySuggestion {
    /// Retry the operation
    Retry { max_attempts: u32, delay_ms: u32 },
    /// Restart the driver
    RestartDriver,
    /// Reset the hardware
    ResetHardware,
    /// Fallback to a different mode
    Fallback { mode: String },
    /// Contact system administrator
    ContactAdmin { message: String },
    /// No recovery possible
    NoRecovery,
}

/// Error reporting and handling framework for drivers
pub trait DriverErrorHandler {
    /// Report an error with context
    fn report_error(&mut self, error: DriverErrorInfo);
    
    /// Get error statistics
    fn get_error_stats(&self) -> ErrorStatistics;
    
    /// Clear error history
    fn clear_errors(&mut self);
    
    /// Set error reporting level
    fn set_error_level(&mut self, level: ErrorLevel);
    
    /// Register error callback
    fn register_error_callback(&mut self, callback: Box<dyn Fn(&DriverErrorInfo)>);
}

/// Error reporting levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorLevel {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

/// Error statistics
#[derive(Debug, Clone)]
pub struct ErrorStatistics {
    pub total_errors: u64,
    pub errors_by_type: Vec<(DriverError, u64)>,
    pub recent_errors: Vec<DriverErrorInfo>,
    pub error_rate: f32, // errors per second
}

/// Default error handler implementation
pub struct DefaultDriverErrorHandler {
    errors: Vec<DriverErrorInfo>,
    error_level: ErrorLevel,
    callbacks: Vec<Box<dyn Fn(&DriverErrorInfo)>>,
    max_stored_errors: usize,
}

impl DefaultDriverErrorHandler {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            error_level: ErrorLevel::Warning,
            callbacks: Vec::new(),
            max_stored_errors: 100,
        }
    }

    pub fn with_max_errors(max_errors: usize) -> Self {
        Self {
            errors: Vec::new(),
            error_level: ErrorLevel::Warning,
            callbacks: Vec::new(),
            max_stored_errors: max_errors,
        }
    }
}

impl DriverErrorHandler for DefaultDriverErrorHandler {
    fn report_error(&mut self, error: DriverErrorInfo) {
        // Store the error
        self.errors.push(error.clone());
        
        // Limit stored errors
        if self.errors.len() > self.max_stored_errors {
            self.errors.remove(0);
        }
        
        // Call registered callbacks
        for callback in &self.callbacks {
            callback(&error);
        }
        
        // In a real implementation, this might also:
        // - Log to system log
        // - Send to monitoring system
        // - Trigger recovery actions
    }

    fn get_error_stats(&self) -> ErrorStatistics {
        let mut error_counts = Vec::new();
        let mut total_errors = 0u64;
        
        for error_info in &self.errors {
            total_errors += 1;
            
            // Count errors by type
            let mut found = false;
            for (error_type, count) in &mut error_counts {
                if core::mem::discriminant(error_type) == core::mem::discriminant(&error_info.error) {
                    *count += 1;
                    found = true;
                    break;
                }
            }
            
            if !found {
                error_counts.push((error_info.error.clone(), 1));
            }
        }
        
        ErrorStatistics {
            total_errors,
            errors_by_type: error_counts,
            recent_errors: self.errors.iter().rev().take(10).cloned().collect(),
            error_rate: 0.0, // Would calculate based on timestamps in real implementation
        }
    }

    fn clear_errors(&mut self) {
        self.errors.clear();
    }

    fn set_error_level(&mut self, level: ErrorLevel) {
        self.error_level = level;
    }

    fn register_error_callback(&mut self, callback: Box<dyn Fn(&DriverErrorInfo)>) {
        self.callbacks.push(callback);
    }
}

/// Helper functions for creating error information
impl DriverErrorInfo {
    pub fn new(error: DriverError, context: String) -> Self {
        Self {
            error,
            context,
            error_code: None,
            recovery_suggestions: Vec::new(),
            timestamp: 0, // Would use actual timestamp in real implementation
        }
    }

    pub fn with_code(mut self, code: u32) -> Self {
        self.error_code = Some(code);
        self
    }

    pub fn with_suggestion(mut self, suggestion: RecoverySuggestion) -> Self {
        self.recovery_suggestions.push(suggestion);
        self
    }

    pub fn with_suggestions(mut self, suggestions: Vec<RecoverySuggestion>) -> Self {
        self.recovery_suggestions = suggestions;
        self
    }
}

/// Macros for easier error reporting
#[macro_export]
macro_rules! driver_error {
    ($error:expr, $context:expr) => {
        DriverErrorInfo::new($error, String::from($context))
    };
    
    ($error:expr, $context:expr, $code:expr) => {
        DriverErrorInfo::new($error, String::from($context)).with_code($code)
    };
}

#[macro_export]
macro_rules! driver_error_with_recovery {
    ($error:expr, $context:expr, $suggestion:expr) => {
        DriverErrorInfo::new($error, String::from($context)).with_suggestion($suggestion)
    };
}

/// Error recovery utilities
pub struct ErrorRecovery;

impl ErrorRecovery {
    /// Attempt to recover from an error using the provided suggestions
    pub fn attempt_recovery(error_info: &DriverErrorInfo) -> Result<(), DriverError> {
        for suggestion in &error_info.recovery_suggestions {
            match suggestion {
                RecoverySuggestion::Retry { max_attempts, delay_ms } => {
                    // In a real implementation, this would retry the failed operation
                    return Ok(());
                }
                RecoverySuggestion::RestartDriver => {
                    // In a real implementation, this would request driver restart
                    return Ok(());
                }
                RecoverySuggestion::ResetHardware => {
                    // In a real implementation, this would reset the hardware
                    return Ok(());
                }
                RecoverySuggestion::Fallback { mode } => {
                    // In a real implementation, this would switch to fallback mode
                    return Ok(());
                }
                _ => continue,
            }
        }
        
        Err(error_info.error.clone())
    }

    /// Check if an error is recoverable
    pub fn is_recoverable(error_info: &DriverErrorInfo) -> bool {
        !error_info.recovery_suggestions.is_empty() &&
        !error_info.recovery_suggestions.iter().all(|s| matches!(s, RecoverySuggestion::NoRecovery))
    }
}