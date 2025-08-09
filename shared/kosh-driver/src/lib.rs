#![no_std]

extern crate alloc;

use alloc::{vec::Vec, string::String, boxed::Box};
use kosh_types::{DriverId, ProcessId, Capability, DriverError};
use kosh_ipc::{Message, DriverRequestData};

pub mod capability;
pub mod communication;
pub mod error;

pub use capability::*;
pub use communication::*;
pub use error::*;

/// Core trait that all Kosh drivers must implement
pub trait KoshDriver {
    /// Initialize the driver with the given capabilities
    fn init(&mut self, capabilities: Vec<Capability>) -> Result<(), DriverError>;
    
    /// Handle a request from the system or another driver
    fn handle_request(&mut self, request: DriverRequest) -> Result<DriverResponse, DriverError>;
    
    /// Clean up resources when the driver is being unloaded
    fn cleanup(&mut self) -> Result<(), DriverError>;
    
    /// Get the capabilities this driver requires
    fn get_required_capabilities(&self) -> Vec<DriverCapabilityType>;
    
    /// Get the capabilities this driver provides to other drivers
    fn get_provided_capabilities(&self) -> Vec<DriverCapabilityType>;
    
    /// Get driver metadata and information
    fn get_driver_info(&self) -> DriverInfo;
    
    /// Handle power management events
    fn handle_power_event(&mut self, event: PowerEvent) -> Result<(), DriverError>;
    
    /// Get current driver status
    fn get_status(&self) -> DriverStatus;
}

/// Information about a driver
#[derive(Debug, Clone)]
pub struct DriverInfo {
    pub name: String,
    pub version: String,
    pub vendor: String,
    pub description: String,
    pub driver_type: DriverType,
    pub hardware_ids: Vec<HardwareId>,
}

/// Types of drivers in the system
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverType {
    Storage,
    Network,
    Graphics,
    Audio,
    Input,
    Power,
    System,
    Custom(u32),
}

/// Hardware identification
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareId {
    pub vendor_id: u32,
    pub device_id: u32,
    pub subsystem_vendor_id: Option<u32>,
    pub subsystem_device_id: Option<u32>,
}

/// Driver status information
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverStatus {
    Uninitialized,
    Initializing,
    Ready,
    Busy,
    Error(DriverErrorCode),
    Suspended,
    Stopping,
}

/// Power management events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    Suspend,
    Resume,
    PowerDown,
    LowPower,
    FullPower,
}

/// Driver request types
#[derive(Debug, Clone)]
pub enum DriverRequest {
    /// Initialize hardware
    Initialize,
    /// Read data from device
    Read { offset: u64, length: usize },
    /// Write data to device
    Write { offset: u64, data: Vec<u8> },
    /// Control operation
    Control { command: u32, data: Vec<u8> },
    /// Query device information
    Query { query_type: QueryType },
    /// Custom driver-specific request
    Custom { request_id: u32, data: Vec<u8> },
}

/// Driver response types
#[derive(Debug, Clone)]
pub enum DriverResponse {
    /// Operation completed successfully
    Success,
    /// Data response
    Data(Vec<u8>),
    /// Status response
    Status(DriverStatus),
    /// Information response
    Info(DriverInfo),
    /// Custom response
    Custom { response_id: u32, data: Vec<u8> },
}

/// Query types for driver information
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryType {
    Status,
    Capabilities,
    HardwareInfo,
    Statistics,
    Configuration,
}

/// Error codes specific to driver operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverErrorCode {
    HardwareFailure,
    InvalidOperation,
    ResourceExhausted,
    Timeout,
    ConfigurationError,
    PermissionDenied,
    NotSupported,
    DeviceNotFound,
    DriverBusy,
    InvalidParameter,
}

impl From<DriverErrorCode> for DriverError {
    fn from(code: DriverErrorCode) -> Self {
        match code {
            DriverErrorCode::HardwareFailure => DriverError::HardwareNotFound,
            DriverErrorCode::InvalidOperation => DriverError::InvalidRequest,
            DriverErrorCode::ResourceExhausted => DriverError::ResourceBusy,
            DriverErrorCode::PermissionDenied => DriverError::PermissionDenied,
            _ => DriverError::InitializationFailed,
        }
    }
}

/// Base trait for driver factories
pub trait DriverFactory {
    /// Create a new driver instance
    fn create_driver(&self, hardware_id: &HardwareId) -> Result<Box<dyn KoshDriver>, DriverError>;
    
    /// Check if this factory can handle the given hardware
    fn can_handle(&self, hardware_id: &HardwareId) -> bool;
    
    /// Get the driver type this factory creates
    fn get_driver_type(&self) -> DriverType;
}