//! Power Management Framework
//! 
//! This module provides power management capabilities for mobile optimization,
//! including CPU frequency scaling, idle state management, and battery monitoring.

pub mod cpu_scaling;
pub mod idle_management;
pub mod battery_monitor;
pub mod power_policy;
pub mod responsiveness;

use crate::process::ProcessId;

/// Power management states
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PowerState {
    /// Full performance mode
    Performance,
    /// Balanced power and performance
    Balanced,
    /// Power saving mode
    PowerSaver,
    /// Critical battery mode
    Critical,
}

/// Battery level information
#[derive(Debug, Clone, Copy)]
pub struct BatteryInfo {
    pub level_percent: u8,
    pub is_charging: bool,
    pub estimated_time_remaining: Option<u32>, // minutes
}

/// CPU frequency scaling information
#[derive(Debug, Clone, Copy)]
pub struct CpuFrequency {
    pub current_mhz: u32,
    pub min_mhz: u32,
    pub max_mhz: u32,
}

/// Power management interface
pub trait PowerManager {
    /// Get current power state
    fn get_power_state(&self) -> PowerState;
    
    /// Set power state
    fn set_power_state(&mut self, state: PowerState) -> Result<(), PowerError>;
    
    /// Get battery information
    fn get_battery_info(&self) -> Result<BatteryInfo, PowerError>;
    
    /// Get CPU frequency information
    fn get_cpu_frequency(&self) -> Result<CpuFrequency, PowerError>;
    
    /// Set CPU frequency scaling governor
    fn set_cpu_governor(&mut self, governor: CpuGovernor) -> Result<(), PowerError>;
    
    /// Notify of process activity for power-aware scheduling
    fn notify_process_activity(&mut self, pid: ProcessId, activity: ProcessActivity);
    
    /// Enter idle state
    fn enter_idle(&mut self) -> Result<(), PowerError>;
    
    /// Exit idle state
    fn exit_idle(&mut self) -> Result<(), PowerError>;
}

/// CPU frequency scaling governors
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CpuGovernor {
    /// Always run at maximum frequency
    Performance,
    /// Scale frequency based on load
    OnDemand,
    /// Always run at minimum frequency
    PowerSave,
    /// Conservative scaling with gradual changes
    Conservative,
    /// Interactive governor for mobile devices
    Interactive,
}

/// Process activity types for power management
#[derive(Debug, Clone, Copy)]
pub enum ProcessActivity {
    /// Interactive user activity (UI, input)
    Interactive,
    /// Background computation
    Background,
    /// I/O intensive operation
    IoIntensive,
    /// Network activity
    Network,
    /// Idle/sleeping
    Idle,
}

/// Power management errors
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PowerError {
    /// Hardware not supported
    NotSupported,
    /// Invalid power state transition
    InvalidTransition,
    /// Battery information unavailable
    BatteryUnavailable,
    /// CPU frequency scaling not available
    FrequencyScalingUnavailable,
    /// Permission denied
    PermissionDenied,
    /// Hardware error
    HardwareError,
}

impl core::fmt::Display for PowerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PowerError::NotSupported => write!(f, "Power management feature not supported"),
            PowerError::InvalidTransition => write!(f, "Invalid power state transition"),
            PowerError::BatteryUnavailable => write!(f, "Battery information unavailable"),
            PowerError::FrequencyScalingUnavailable => write!(f, "CPU frequency scaling unavailable"),
            PowerError::PermissionDenied => write!(f, "Permission denied for power management operation"),
            PowerError::HardwareError => write!(f, "Hardware error in power management"),
        }
    }
}