//! Service Client Infrastructure for Shell IPC Communication
//!
//! This module provides a comprehensive service client for communicating with
//! Kosh system services (file system, process manager, driver manager) via IPC.
//!
//! # Features
//! - Service discovery and registration
//! - Async message sending with timeout handling
//! - Health monitoring and automatic reconnection
//! - Typed request/response handling for each service type
//!
//! # Requirements
//! Implements: 1.1, 2.1, 6.1

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use alloc::format;
use kosh_types::ProcessId;
use kosh_service::{ServiceType, ServiceData, ServiceError as KoshServiceError};
use crate::error::{ShellError, ShellResult};

// ══════════════════════════════════════════════════════════════════════════════
// Constants
// ══════════════════════════════════════════════════════════════════════════════

/// Maximum number of retry attempts for service reconnection
const MAX_RETRY_ATTEMPTS: u32 = 3;

/// Default timeout for service requests (in simulated ticks)
const DEFAULT_TIMEOUT_TICKS: u64 = 1000;

/// Health check interval (in simulated ticks)
const HEALTH_CHECK_INTERVAL: u64 = 500;

/// Maximum consecutive failures before marking service as unreachable
const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// Well-known service PIDs (simulated until real service registry is available)
const FS_SERVICE_PID: ProcessId = 100;
const PROCESS_SERVICE_PID: ProcessId = 101;
const DRIVER_SERVICE_PID: ProcessId = 102;

// ══════════════════════════════════════════════════════════════════════════════
// Service Type Enum
// ══════════════════════════════════════════════════════════════════════════════

/// Types of system services the shell can communicate with
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellServiceType {
    /// File system service for file/directory operations
    FileSystem,
    /// Process manager service for process control
    Process,
    /// Driver manager service for driver information
    Driver,
}

impl ShellServiceType {
    /// Convert to kosh_service::ServiceType
    pub fn to_kosh_service_type(self) -> ServiceType {
        match self {
            ShellServiceType::FileSystem => ServiceType::FileSystem,
            ShellServiceType::Process => ServiceType::ProcessManager,
            ShellServiceType::Driver => ServiceType::DriverManager,
        }
    }

    /// Get the service name for display/logging
    pub fn name(&self) -> &'static str {
        match self {
            ShellServiceType::FileSystem => "fs_service",
            ShellServiceType::Process => "process_service",
            ShellServiceType::Driver => "driver_service",
        }
    }

    /// Get the well-known PID for this service (simulated)
    pub fn default_pid(&self) -> ProcessId {
        match self {
            ShellServiceType::FileSystem => FS_SERVICE_PID,
            ShellServiceType::Process => PROCESS_SERVICE_PID,
            ShellServiceType::Driver => DRIVER_SERVICE_PID,
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Service Connection Status
// ══════════════════════════════════════════════════════════════════════════════

/// Status of a service connection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// Service has been discovered and is connected
    Connected,
    /// Service connection has not been established yet
    Disconnected,
    /// Service was connected but is now unreachable
    Unreachable,
    /// Currently attempting to reconnect
    Reconnecting,
}

// ══════════════════════════════════════════════════════════════════════════════
// Service Communication Errors
// ══════════════════════════════════════════════════════════════════════════════

/// Errors that can occur during service communication
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceCommError {
    /// Service is not available or not discovered
    ServiceUnavailable(String),
    /// Request timed out waiting for response
    Timeout(String),
    /// Service returned an error
    ServiceError(String),
    /// Invalid request format or parameters
    InvalidRequest(String),
    /// Permission denied for the requested operation
    PermissionDenied(String),
    /// Resource not found (file, process, etc.)
    NotFound(String),
    /// Maximum retry attempts exceeded
    MaxRetriesExceeded(String),
    /// IPC communication failure
    IpcError(String),
}

impl From<ServiceCommError> for ShellError {
    fn from(err: ServiceCommError) -> Self {
        match err {
            ServiceCommError::ServiceUnavailable(s) => ShellError::ServiceUnavailable(s),
            ServiceCommError::Timeout(s) => ShellError::ServiceTimeout(s),
            ServiceCommError::ServiceError(s) => ShellError::InternalError(s),
            ServiceCommError::InvalidRequest(s) => ShellError::InvalidArguments(s),
            ServiceCommError::PermissionDenied(s) => ShellError::PermissionDenied(s),
            ServiceCommError::NotFound(s) => ShellError::FileNotFound(s),
            ServiceCommError::MaxRetriesExceeded(s) => ShellError::ServiceUnavailable(s),
            ServiceCommError::IpcError(s) => ShellError::InternalError(s),
        }
    }
}

impl From<KoshServiceError> for ServiceCommError {
    fn from(err: KoshServiceError) -> Self {
        match err {
            KoshServiceError::NotFound => ServiceCommError::NotFound("Resource not found".to_string()),
            KoshServiceError::PermissionDenied => ServiceCommError::PermissionDenied("Permission denied".to_string()),
            KoshServiceError::InvalidRequest => ServiceCommError::InvalidRequest("Invalid request".to_string()),
            KoshServiceError::CommunicationError => ServiceCommError::IpcError("Communication error".to_string()),
            KoshServiceError::Timeout => ServiceCommError::Timeout("Request timed out".to_string()),
            KoshServiceError::NotImplemented => ServiceCommError::ServiceError("Not implemented".to_string()),
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// File System Request Types
// ══════════════════════════════════════════════════════════════════════════════

/// File system service request types
#[derive(Debug, Clone)]
pub enum FileSystemRequest {
    /// List directory contents
    ListDir { path: String },
    /// Read file contents
    ReadFile { path: String },
    /// Write data to a file
    WriteFile { path: String, data: Vec<u8> },
    /// Create a new directory
    CreateDir { path: String, recursive: bool },
    /// Delete a file
    DeleteFile { path: String },
    /// Delete a directory
    DeleteDir { path: String, recursive: bool },
    /// Get file/directory metadata
    GetMetadata { path: String },
    /// Check if path exists
    Exists { path: String },
    /// Rename/move a file or directory
    Rename { from: String, to: String },
    /// Create an empty file or update timestamps
    Touch { path: String },
}

impl FileSystemRequest {
    /// Convert to kosh_service::ServiceData
    pub fn to_service_data(&self) -> ServiceData {
        match self {
            FileSystemRequest::ListDir { path } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::List {
                    path: path.clone(),
                })
            }
            FileSystemRequest::ReadFile { path } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::Open {
                    path: path.clone(),
                    flags: 0, // read-only
                })
            }
            FileSystemRequest::WriteFile { path: _, data } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::Write {
                    fd: 0,
                    data: data.clone(),
                })
            }
            FileSystemRequest::CreateDir { path, recursive: _ } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::Create {
                    path: path.clone(),
                    is_directory: true,
                })
            }
            FileSystemRequest::DeleteFile { path } | FileSystemRequest::DeleteDir { path, .. } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::Delete {
                    path: path.clone(),
                })
            }
            FileSystemRequest::GetMetadata { path } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::List {
                    path: path.clone(),
                })
            }
            FileSystemRequest::Exists { path } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::List {
                    path: path.clone(),
                })
            }
            FileSystemRequest::Rename { from, to: _ } => {
                // Rename not directly supported, use delete + create pattern
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::List {
                    path: from.clone(),
                })
            }
            FileSystemRequest::Touch { path } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::Create {
                    path: path.clone(),
                    is_directory: false,
                })
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Process Request Types
// ══════════════════════════════════════════════════════════════════════════════

/// Process manager service request types
#[derive(Debug, Clone)]
pub enum ProcessRequest {
    /// List all running processes
    ListProcesses,
    /// Get information about a specific process
    GetProcessInfo { pid: ProcessId },
    /// Send a signal to terminate a process
    KillProcess { pid: ProcessId, signal: ProcessSignal },
    /// Spawn a new process
    SpawnProcess { program: String, args: Vec<String> },
}

/// Process signals for kill operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessSignal {
    /// Terminate gracefully (SIGTERM)
    Term,
    /// Force kill (SIGKILL)
    Kill,
    /// Interrupt (SIGINT)
    Int,
    /// Hangup (SIGHUP)
    Hup,
    /// Stop (SIGSTOP)
    Stop,
    /// Continue (SIGCONT)
    Cont,
}

impl ProcessSignal {
    /// Get the signal number
    pub fn number(&self) -> u32 {
        match self {
            ProcessSignal::Term => 15,
            ProcessSignal::Kill => 9,
            ProcessSignal::Int => 2,
            ProcessSignal::Hup => 1,
            ProcessSignal::Stop => 19,
            ProcessSignal::Cont => 18,
        }
    }

    /// Parse signal from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "TERM" | "SIGTERM" | "15" => Some(ProcessSignal::Term),
            "KILL" | "SIGKILL" | "9" => Some(ProcessSignal::Kill),
            "INT" | "SIGINT" | "2" => Some(ProcessSignal::Int),
            "HUP" | "SIGHUP" | "1" => Some(ProcessSignal::Hup),
            "STOP" | "SIGSTOP" | "19" => Some(ProcessSignal::Stop),
            "CONT" | "SIGCONT" | "18" => Some(ProcessSignal::Cont),
            _ => None,
        }
    }
}

impl ProcessRequest {
    /// Convert to kosh_service::ServiceData
    pub fn to_service_data(&self) -> ServiceData {
        match self {
            ProcessRequest::ListProcesses => {
                ServiceData::ProcessRequest(kosh_service::ProcessRequest::List)
            }
            ProcessRequest::GetProcessInfo { pid } => {
                ServiceData::ProcessRequest(kosh_service::ProcessRequest::GetInfo { pid: *pid })
            }
            ProcessRequest::KillProcess { pid, signal: _ } => {
                ServiceData::ProcessRequest(kosh_service::ProcessRequest::Kill { pid: *pid })
            }
            ProcessRequest::SpawnProcess { program, args } => {
                ServiceData::ProcessRequest(kosh_service::ProcessRequest::Spawn {
                    program: program.clone(),
                    args: args.clone(),
                })
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Driver Request Types
// ══════════════════════════════════════════════════════════════════════════════

/// Driver manager service request types
#[derive(Debug, Clone)]
pub enum DriverRequest {
    /// List all loaded drivers
    ListDrivers,
    /// Get information about a specific driver
    GetDriverInfo { driver_id: u32 },
    /// Load a driver from path
    LoadDriver { path: String },
    /// Unload a driver
    UnloadDriver { driver_id: u32 },
}

impl DriverRequest {
    /// Convert to kosh_service::ServiceData
    pub fn to_service_data(&self) -> ServiceData {
        match self {
            DriverRequest::ListDrivers => {
                ServiceData::DriverRequest(kosh_service::DriverRequest::ListDrivers)
            }
            DriverRequest::GetDriverInfo { driver_id: _ } => {
                // GetDriverInfo maps to ListDrivers (filter client-side)
                ServiceData::DriverRequest(kosh_service::DriverRequest::ListDrivers)
            }
            DriverRequest::LoadDriver { path } => {
                ServiceData::DriverRequest(kosh_service::DriverRequest::LoadDriver {
                    path: path.clone(),
                })
            }
            DriverRequest::UnloadDriver { driver_id } => {
                ServiceData::DriverRequest(kosh_service::DriverRequest::UnloadDriver {
                    driver_id: *driver_id,
                })
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Service Request Wrapper
// ══════════════════════════════════════════════════════════════════════════════

/// Unified service request type
#[derive(Debug, Clone)]
pub enum ServiceRequest {
    FileSystem(FileSystemRequest),
    Process(ProcessRequest),
    Driver(DriverRequest),
}

impl ServiceRequest {
    /// Get the service type for this request
    pub fn service_type(&self) -> ShellServiceType {
        match self {
            ServiceRequest::FileSystem(_) => ShellServiceType::FileSystem,
            ServiceRequest::Process(_) => ShellServiceType::Process,
            ServiceRequest::Driver(_) => ShellServiceType::Driver,
        }
    }

    /// Convert to kosh_service::ServiceData
    pub fn to_service_data(&self) -> ServiceData {
        match self {
            ServiceRequest::FileSystem(req) => req.to_service_data(),
            ServiceRequest::Process(req) => req.to_service_data(),
            ServiceRequest::Driver(req) => req.to_service_data(),
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Service Response Types
// ══════════════════════════════════════════════════════════════════════════════

/// Status of a service response
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseStatus {
    Success,
    Error,
    NotFound,
    PermissionDenied,
    Timeout,
    InvalidRequest,
}

/// Response from a service request
#[derive(Debug, Clone)]
pub struct ServiceResponse {
    pub request_id: u64,
    pub status: ResponseStatus,
    pub data: String,
    pub raw_data: Option<Vec<u8>>,
}

impl ServiceResponse {
    /// Create a successful response
    pub fn success(request_id: u64, data: String) -> Self {
        Self {
            request_id,
            status: ResponseStatus::Success,
            data,
            raw_data: None,
        }
    }

    /// Create an error response
    pub fn error(request_id: u64, message: String) -> Self {
        Self {
            request_id,
            status: ResponseStatus::Error,
            data: message,
            raw_data: None,
        }
    }

    /// Check if the response indicates success
    pub fn is_success(&self) -> bool {
        self.status == ResponseStatus::Success
    }
}


// ══════════════════════════════════════════════════════════════════════════════
// Service Endpoint
// ══════════════════════════════════════════════════════════════════════════════

/// Tracks the health and connection state of a single service
#[derive(Debug, Clone)]
pub struct ServiceEndpoint {
    /// Process ID of the service (if connected)
    pub pid: Option<ProcessId>,
    /// Type of service
    pub service_type: ShellServiceType,
    /// Human-readable service name
    pub name: String,
    /// Current connection status
    pub status: ConnectionStatus,
    /// Number of reconnection attempts made
    pub retry_count: u32,
    /// Tick count of last health check
    pub last_health_check: u64,
    /// Number of consecutive request failures
    pub consecutive_failures: u32,
    /// Last successful request tick
    pub last_success_tick: u64,
}

impl ServiceEndpoint {
    /// Create a new service endpoint
    pub fn new(service_type: ShellServiceType) -> Self {
        Self {
            pid: None,
            service_type,
            name: service_type.name().to_string(),
            status: ConnectionStatus::Disconnected,
            retry_count: 0,
            last_health_check: 0,
            consecutive_failures: 0,
            last_success_tick: 0,
        }
    }

    /// Mark the service as connected with the given PID
    pub fn connect(&mut self, pid: ProcessId) {
        self.pid = Some(pid);
        self.status = ConnectionStatus::Connected;
        self.retry_count = 0;
        self.consecutive_failures = 0;
    }

    /// Mark the service as disconnected
    pub fn disconnect(&mut self) {
        self.status = ConnectionStatus::Disconnected;
        self.pid = None;
    }

    /// Record a successful request
    pub fn record_success(&mut self, tick: u64) {
        self.consecutive_failures = 0;
        self.status = ConnectionStatus::Connected;
        self.last_success_tick = tick;
    }

    /// Record a failed request and return whether reconnection should be attempted
    pub fn record_failure(&mut self) -> bool {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            self.status = ConnectionStatus::Unreachable;
            self.retry_count < MAX_RETRY_ATTEMPTS
        } else {
            false
        }
    }

    /// Check if the service is available for requests
    pub fn is_available(&self) -> bool {
        self.status == ConnectionStatus::Connected && self.pid.is_some()
    }

    /// Check if health check is due
    pub fn needs_health_check(&self, current_tick: u64) -> bool {
        current_tick - self.last_health_check >= HEALTH_CHECK_INTERVAL
    }

    /// Reset retry counter (called after successful reconnection)
    pub fn reset_retries(&mut self) {
        self.retry_count = 0;
    }

    /// Increment retry counter and check if max retries exceeded
    pub fn increment_retry(&mut self) -> bool {
        self.retry_count += 1;
        self.retry_count <= MAX_RETRY_ATTEMPTS
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Service Client
// ══════════════════════════════════════════════════════════════════════════════

/// Main service client for shell IPC communication.
///
/// Manages connections to file system, process, and driver services.
/// Provides service discovery, health monitoring, request/response handling
/// with timeout support, and automatic reconnection on failure.
///
/// # Example
/// ```ignore
/// let mut client = ServiceClient::new();
/// client.discover_services()?;
///
/// let request = FileSystemRequest::ListDir { path: "/".to_string() };
/// let response = client.send_fs_request(request)?;
/// ```
pub struct ServiceClient {
    /// File system service endpoint
    fs_endpoint: ServiceEndpoint,
    /// Process manager service endpoint
    process_endpoint: ServiceEndpoint,
    /// Driver manager service endpoint
    driver_endpoint: ServiceEndpoint,
    /// Next request ID to assign
    next_request_id: u64,
    /// Current tick counter for timing
    current_tick: u64,
    /// Timeout for requests (in ticks)
    timeout_ticks: u64,
    /// Whether services have been discovered
    services_discovered: bool,
}

impl ServiceClient {
    /// Create a new service client
    pub fn new() -> Self {
        Self {
            fs_endpoint: ServiceEndpoint::new(ShellServiceType::FileSystem),
            process_endpoint: ServiceEndpoint::new(ShellServiceType::Process),
            driver_endpoint: ServiceEndpoint::new(ShellServiceType::Driver),
            next_request_id: 1,
            current_tick: 0,
            timeout_ticks: DEFAULT_TIMEOUT_TICKS,
            services_discovered: false,
        }
    }

    /// Set the timeout for service requests (in ticks)
    pub fn set_timeout(&mut self, ticks: u64) {
        self.timeout_ticks = ticks;
    }

    /// Get the current timeout setting
    pub fn timeout(&self) -> u64 {
        self.timeout_ticks
    }

    /// Advance the internal tick counter (called by the shell main loop)
    pub fn tick(&mut self) {
        self.current_tick += 1;
    }

    /// Get the current tick count
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    // ── Service Discovery ──────────────────────────────────────────────

    /// Discover and connect to all system services.
    ///
    /// In a real implementation this would query the kernel service registry
    /// via IPC. For now it uses simulated well-known PIDs.
    pub fn discover_services(&mut self) -> Result<(), ServiceCommError> {
        self.discover_service(ShellServiceType::FileSystem)?;
        self.discover_service(ShellServiceType::Process)?;
        self.discover_service(ShellServiceType::Driver)?;
        self.services_discovered = true;
        Ok(())
    }

    /// Discover a single service by type
    pub fn discover_service(&mut self, service_type: ShellServiceType) -> Result<ProcessId, ServiceCommError> {
        // In a real implementation, this would:
        // 1. Send a service lookup request to the kernel
        // 2. Wait for the response with the service PID
        // 3. Verify the service is responding
        
        // For now, use well-known simulated PIDs
        let pid = service_type.default_pid();
        let endpoint = self.endpoint_mut(service_type);
        endpoint.connect(pid);
        Ok(pid)
    }

    /// Find a service by name and return its PID if found
    pub fn find_service(&self, name: &str) -> Option<ProcessId> {
        let endpoints = [&self.fs_endpoint, &self.process_endpoint, &self.driver_endpoint];
        endpoints
            .iter()
            .find(|ep| ep.name == name && ep.is_available())
            .and_then(|ep| ep.pid)
    }

    /// Find a service by type and return its PID if found
    pub fn find_service_by_type(&self, service_type: ShellServiceType) -> Option<ProcessId> {
        self.endpoint(service_type).pid
    }

    /// Return whether all required services are connected
    pub fn all_services_available(&self) -> bool {
        self.fs_endpoint.is_available()
            && self.process_endpoint.is_available()
            && self.driver_endpoint.is_available()
    }

    /// Check if services have been discovered
    pub fn is_initialized(&self) -> bool {
        self.services_discovered
    }

    // ── Request/Response Handling ──────────────────────────────────────

    /// Send a request to a service and wait for a response with timeout.
    pub fn send_request(&mut self, request: ServiceRequest) -> Result<ServiceResponse, ServiceCommError> {
        let service_type = request.service_type();
        
        // Ensure service is available
        if !self.endpoint(service_type).is_available() {
            // Attempt reconnection
            if !self.try_reconnect(service_type) {
                return Err(ServiceCommError::ServiceUnavailable(
                    format!("Service {} is not available", service_type.name())
                ));
            }
        }

        let pid = self.endpoint(service_type).pid.ok_or_else(|| {
            ServiceCommError::ServiceUnavailable(service_type.name().to_string())
        })?;

        let request_id = self.next_request_id;
        self.next_request_id += 1;

        // Convert request to service data
        let _service_data = request.to_service_data();

        // In a real implementation:
        // 1. Serialize the request
        // 2. Send via IPC to the service PID
        // 3. Wait for response with timeout
        // 4. Deserialize and return response

        // Simulated: return success response
        let response = self.wait_for_response(request_id, pid)?;
        
        // Record success
        let tick = self.current_tick;
        self.endpoint_mut(service_type).record_success(tick);
        
        Ok(response)
    }

    /// Wait for a response with timeout handling
    fn wait_for_response(&self, request_id: u64, _pid: ProcessId) -> Result<ServiceResponse, ServiceCommError> {
        // In a real implementation:
        // 1. Poll the IPC receive queue
        // 2. Check for timeout
        // 3. Match response to request_id
        
        // Simulated: return placeholder success response
        Ok(ServiceResponse::success(request_id, String::new()))
    }

    // ── Typed Request Helpers ─────────────────────────────────────────

    /// Send a request to the file system service
    pub fn send_fs_request(&mut self, request: FileSystemRequest) -> Result<ServiceResponse, ServiceCommError> {
        self.send_request(ServiceRequest::FileSystem(request))
    }

    /// Send a request to the process service
    pub fn send_process_request(&mut self, request: ProcessRequest) -> Result<ServiceResponse, ServiceCommError> {
        self.send_request(ServiceRequest::Process(request))
    }

    /// Send a request to the driver service
    pub fn send_driver_request(&mut self, request: DriverRequest) -> Result<ServiceResponse, ServiceCommError> {
        self.send_request(ServiceRequest::Driver(request))
    }

    // ── Convenience Methods ───────────────────────────────────────────

    /// List directory contents
    pub fn list_directory(&mut self, path: &str) -> ShellResult<ServiceResponse> {
        self.send_fs_request(FileSystemRequest::ListDir { path: path.to_string() })
            .map_err(|e| e.into())
    }

    /// Read file contents
    pub fn read_file(&mut self, path: &str) -> ShellResult<ServiceResponse> {
        self.send_fs_request(FileSystemRequest::ReadFile { path: path.to_string() })
            .map_err(|e| e.into())
    }

    /// Write to a file
    pub fn write_file(&mut self, path: &str, data: Vec<u8>) -> ShellResult<ServiceResponse> {
        self.send_fs_request(FileSystemRequest::WriteFile { 
            path: path.to_string(), 
            data 
        }).map_err(|e| e.into())
    }

    /// Create a directory
    pub fn create_directory(&mut self, path: &str, recursive: bool) -> ShellResult<ServiceResponse> {
        self.send_fs_request(FileSystemRequest::CreateDir { 
            path: path.to_string(), 
            recursive 
        }).map_err(|e| e.into())
    }

    /// Delete a file
    pub fn delete_file(&mut self, path: &str) -> ShellResult<ServiceResponse> {
        self.send_fs_request(FileSystemRequest::DeleteFile { path: path.to_string() })
            .map_err(|e| e.into())
    }

    /// List all processes
    pub fn list_processes(&mut self) -> ShellResult<ServiceResponse> {
        self.send_process_request(ProcessRequest::ListProcesses)
            .map_err(|e| e.into())
    }

    /// Kill a process
    pub fn kill_process(&mut self, pid: ProcessId, signal: ProcessSignal) -> ShellResult<ServiceResponse> {
        self.send_process_request(ProcessRequest::KillProcess { pid, signal })
            .map_err(|e| e.into())
    }

    /// List all drivers
    pub fn list_drivers(&mut self) -> ShellResult<ServiceResponse> {
        self.send_driver_request(DriverRequest::ListDrivers)
            .map_err(|e| e.into())
    }

    // ── Health Monitoring ─────────────────────────────────────────────

    /// Perform health checks on all services.
    ///
    /// Should be called periodically from the shell main loop.
    pub fn check_health(&mut self) {
        let tick = self.current_tick;
        
        self.check_endpoint_health(ShellServiceType::FileSystem, tick);
        self.check_endpoint_health(ShellServiceType::Process, tick);
        self.check_endpoint_health(ShellServiceType::Driver, tick);
    }

    fn check_endpoint_health(&mut self, service_type: ShellServiceType, tick: u64) {
        let endpoint = self.endpoint_mut(service_type);
        
        if !endpoint.needs_health_check(tick) {
            return;
        }
        
        endpoint.last_health_check = tick;

        if !endpoint.is_available() {
            return;
        }

        // In a real implementation:
        // 1. Send a ping/health-check message
        // 2. Wait for response with short timeout
        // 3. Update status based on response
        
        // For now, assume healthy if connected
    }

    /// Get the connection status of a specific service type
    pub fn get_status(&self, service_type: ShellServiceType) -> ConnectionStatus {
        self.endpoint(service_type).status
    }

    /// Get the PID of a specific service type if connected
    pub fn get_pid(&self, service_type: ShellServiceType) -> Option<ProcessId> {
        self.endpoint(service_type).pid
    }

    /// Get detailed status of all services
    pub fn get_all_status(&self) -> Vec<(ShellServiceType, ConnectionStatus, Option<ProcessId>)> {
        vec![
            (ShellServiceType::FileSystem, self.fs_endpoint.status, self.fs_endpoint.pid),
            (ShellServiceType::Process, self.process_endpoint.status, self.process_endpoint.pid),
            (ShellServiceType::Driver, self.driver_endpoint.status, self.driver_endpoint.pid),
        ]
    }

    // ── Reconnection Logic ────────────────────────────────────────────

    /// Attempt to reconnect a single service.
    /// Returns `true` if reconnection succeeded.
    pub fn try_reconnect(&mut self, service_type: ShellServiceType) -> bool {
        let endpoint = self.endpoint_mut(service_type);
        
        if endpoint.retry_count >= MAX_RETRY_ATTEMPTS {
            return false;
        }

        endpoint.status = ConnectionStatus::Reconnecting;
        endpoint.retry_count += 1;

        // In a real implementation, re-query the service registry
        // For now, simulate success by re-assigning the well-known PID
        let pid = service_type.default_pid();
        endpoint.connect(pid);
        true
    }

    /// Attempt to reconnect all unreachable services.
    /// Returns the number of services successfully reconnected.
    pub fn reconnect_all(&mut self) -> u32 {
        let mut reconnected = 0;

        for service_type in &[ShellServiceType::FileSystem, ShellServiceType::Process, ShellServiceType::Driver] {
            let status = self.endpoint(*service_type).status;
            if status == ConnectionStatus::Unreachable || status == ConnectionStatus::Disconnected {
                if self.try_reconnect(*service_type) {
                    reconnected += 1;
                }
            }
        }
        
        reconnected
    }

    /// Reset all service connections (useful for testing or recovery)
    pub fn reset(&mut self) {
        self.fs_endpoint = ServiceEndpoint::new(ShellServiceType::FileSystem);
        self.process_endpoint = ServiceEndpoint::new(ShellServiceType::Process);
        self.driver_endpoint = ServiceEndpoint::new(ShellServiceType::Driver);
        self.services_discovered = false;
    }

    // ── Endpoint Accessors ────────────────────────────────────────────

    fn endpoint(&self, service_type: ShellServiceType) -> &ServiceEndpoint {
        match service_type {
            ShellServiceType::FileSystem => &self.fs_endpoint,
            ShellServiceType::Process => &self.process_endpoint,
            ShellServiceType::Driver => &self.driver_endpoint,
        }
    }

    fn endpoint_mut(&mut self, service_type: ShellServiceType) -> &mut ServiceEndpoint {
        match service_type {
            ShellServiceType::FileSystem => &mut self.fs_endpoint,
            ShellServiceType::Process => &mut self.process_endpoint,
            ShellServiceType::Driver => &mut self.driver_endpoint,
        }
    }

    // ── Backward Compatibility Accessors ──────────────────────────────

    /// Get the file system service PID
    pub fn fs_service_pid(&self) -> Option<ProcessId> {
        self.fs_endpoint.pid
    }

    /// Get the process service PID
    pub fn process_service_pid(&self) -> Option<ProcessId> {
        self.process_endpoint.pid
    }

    /// Get the driver service PID
    pub fn driver_service_pid(&self) -> Option<ProcessId> {
        self.driver_endpoint.pid
    }
}

impl Default for ServiceClient {
    fn default() -> Self {
        Self::new()
    }
}


// ══════════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    // ── ShellServiceType Tests ────────────────────────────────────────

    #[test]
    fn test_service_type_name() {
        assert_eq!(ShellServiceType::FileSystem.name(), "fs_service");
        assert_eq!(ShellServiceType::Process.name(), "process_service");
        assert_eq!(ShellServiceType::Driver.name(), "driver_service");
    }

    #[test]
    fn test_service_type_default_pid() {
        assert_eq!(ShellServiceType::FileSystem.default_pid(), 100);
        assert_eq!(ShellServiceType::Process.default_pid(), 101);
        assert_eq!(ShellServiceType::Driver.default_pid(), 102);
    }

    #[test]
    fn test_service_type_to_kosh() {
        assert_eq!(ShellServiceType::FileSystem.to_kosh_service_type(), ServiceType::FileSystem);
        assert_eq!(ShellServiceType::Process.to_kosh_service_type(), ServiceType::ProcessManager);
        assert_eq!(ShellServiceType::Driver.to_kosh_service_type(), ServiceType::DriverManager);
    }

    // ── ServiceEndpoint Tests ─────────────────────────────────────────

    #[test]
    fn test_endpoint_new() {
        let ep = ServiceEndpoint::new(ShellServiceType::FileSystem);
        assert_eq!(ep.name, "fs_service");
        assert_eq!(ep.status, ConnectionStatus::Disconnected);
        assert!(ep.pid.is_none());
        assert!(!ep.is_available());
        assert_eq!(ep.retry_count, 0);
        assert_eq!(ep.consecutive_failures, 0);
    }

    #[test]
    fn test_endpoint_connect() {
        let mut ep = ServiceEndpoint::new(ShellServiceType::FileSystem);
        ep.connect(100);

        assert_eq!(ep.pid, Some(100));
        assert_eq!(ep.status, ConnectionStatus::Connected);
        assert!(ep.is_available());
        assert_eq!(ep.retry_count, 0);
        assert_eq!(ep.consecutive_failures, 0);
    }

    #[test]
    fn test_endpoint_disconnect() {
        let mut ep = ServiceEndpoint::new(ShellServiceType::FileSystem);
        ep.connect(100);
        ep.disconnect();

        assert!(ep.pid.is_none());
        assert_eq!(ep.status, ConnectionStatus::Disconnected);
        assert!(!ep.is_available());
    }

    #[test]
    fn test_endpoint_record_success() {
        let mut ep = ServiceEndpoint::new(ShellServiceType::FileSystem);
        ep.connect(100);
        ep.consecutive_failures = 2;

        ep.record_success(42);

        assert_eq!(ep.consecutive_failures, 0);
        assert_eq!(ep.status, ConnectionStatus::Connected);
        assert_eq!(ep.last_success_tick, 42);
    }

    #[test]
    fn test_endpoint_record_failure_below_threshold() {
        let mut ep = ServiceEndpoint::new(ShellServiceType::FileSystem);
        ep.connect(100);

        assert!(!ep.record_failure());
        assert_eq!(ep.consecutive_failures, 1);
        assert!(!ep.record_failure());
        assert_eq!(ep.consecutive_failures, 2);
    }

    #[test]
    fn test_endpoint_record_failure_triggers_unreachable() {
        let mut ep = ServiceEndpoint::new(ShellServiceType::FileSystem);
        ep.connect(100);

        ep.record_failure();
        ep.record_failure();
        let should_reconnect = ep.record_failure();

        assert!(should_reconnect);
        assert_eq!(ep.status, ConnectionStatus::Unreachable);
    }

    #[test]
    fn test_endpoint_record_failure_max_retries_exceeded() {
        let mut ep = ServiceEndpoint::new(ShellServiceType::FileSystem);
        ep.connect(100);
        ep.retry_count = MAX_RETRY_ATTEMPTS;

        ep.record_failure();
        ep.record_failure();
        let should_reconnect = ep.record_failure();

        assert!(!should_reconnect); // Max retries exceeded
    }

    #[test]
    fn test_endpoint_needs_health_check() {
        let mut ep = ServiceEndpoint::new(ShellServiceType::FileSystem);
        ep.last_health_check = 0;

        assert!(ep.needs_health_check(HEALTH_CHECK_INTERVAL));
        assert!(ep.needs_health_check(HEALTH_CHECK_INTERVAL + 1));
        assert!(!ep.needs_health_check(HEALTH_CHECK_INTERVAL - 1));
    }

    // ── ServiceClient Tests ───────────────────────────────────────────

    #[test]
    fn test_client_new() {
        let client = ServiceClient::new();
        assert!(!client.all_services_available());
        assert!(!client.is_initialized());
        assert!(client.fs_service_pid().is_none());
        assert!(client.process_service_pid().is_none());
        assert!(client.driver_service_pid().is_none());
    }

    #[test]
    fn test_client_discover_services() {
        let mut client = ServiceClient::new();
        let result = client.discover_services();
        assert!(result.is_ok());

        assert!(client.all_services_available());
        assert!(client.is_initialized());
        assert_eq!(client.fs_service_pid(), Some(100));
        assert_eq!(client.process_service_pid(), Some(101));
        assert_eq!(client.driver_service_pid(), Some(102));
    }

    #[test]
    fn test_client_discover_single_service() {
        let mut client = ServiceClient::new();
        let pid = client.discover_service(ShellServiceType::FileSystem);
        assert!(pid.is_ok());
        assert_eq!(pid.unwrap(), 100);
        assert_eq!(client.get_status(ShellServiceType::FileSystem), ConnectionStatus::Connected);
    }

    #[test]
    fn test_client_find_service_by_name() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        assert_eq!(client.find_service("fs_service"), Some(100));
        assert_eq!(client.find_service("process_service"), Some(101));
        assert_eq!(client.find_service("driver_service"), Some(102));
        assert_eq!(client.find_service("nonexistent"), None);
    }

    #[test]
    fn test_client_find_service_by_type() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        assert_eq!(client.find_service_by_type(ShellServiceType::FileSystem), Some(100));
        assert_eq!(client.find_service_by_type(ShellServiceType::Process), Some(101));
        assert_eq!(client.find_service_by_type(ShellServiceType::Driver), Some(102));
    }

    #[test]
    fn test_client_get_status() {
        let mut client = ServiceClient::new();

        assert_eq!(client.get_status(ShellServiceType::FileSystem), ConnectionStatus::Disconnected);

        client.discover_services().unwrap();

        assert_eq!(client.get_status(ShellServiceType::FileSystem), ConnectionStatus::Connected);
        assert_eq!(client.get_status(ShellServiceType::Process), ConnectionStatus::Connected);
        assert_eq!(client.get_status(ShellServiceType::Driver), ConnectionStatus::Connected);
    }

    #[test]
    fn test_client_get_all_status() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        let statuses = client.get_all_status();
        assert_eq!(statuses.len(), 3);
        for (_, status, pid) in &statuses {
            assert_eq!(*status, ConnectionStatus::Connected);
            assert!(pid.is_some());
        }
    }

    #[test]
    fn test_client_send_fs_request() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        let request = FileSystemRequest::ListDir { path: "/".to_string() };
        let result = client.send_fs_request(request);
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response.is_success());
    }

    #[test]
    fn test_client_send_process_request() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        let result = client.send_process_request(ProcessRequest::ListProcesses);
        assert!(result.is_ok());
    }

    #[test]
    fn test_client_send_driver_request() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        let result = client.send_driver_request(DriverRequest::ListDrivers);
        assert!(result.is_ok());
    }

    #[test]
    fn test_client_send_request_without_discovery() {
        let mut client = ServiceClient::new();

        // Without discovery, reconnection should be attempted (and succeed in simulation)
        let request = FileSystemRequest::ListDir { path: "/".to_string() };
        let result = client.send_fs_request(request);
        assert!(result.is_ok());
    }

    #[test]
    fn test_client_convenience_list_directory() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        let result = client.list_directory("/home");
        assert!(result.is_ok());
    }

    #[test]
    fn test_client_convenience_read_file() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        let result = client.read_file("/etc/config");
        assert!(result.is_ok());
    }

    #[test]
    fn test_client_convenience_list_processes() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        let result = client.list_processes();
        assert!(result.is_ok());
    }

    #[test]
    fn test_client_convenience_list_drivers() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        let result = client.list_drivers();
        assert!(result.is_ok());
    }

    #[test]
    fn test_client_set_timeout() {
        let mut client = ServiceClient::new();
        client.set_timeout(2000);
        assert_eq!(client.timeout(), 2000);
    }

    #[test]
    fn test_client_tick() {
        let mut client = ServiceClient::new();
        assert_eq!(client.current_tick(), 0);
        client.tick();
        assert_eq!(client.current_tick(), 1);
        client.tick();
        assert_eq!(client.current_tick(), 2);
    }

    #[test]
    fn test_client_reconnect_all() {
        let mut client = ServiceClient::new();
        // All services start disconnected
        let reconnected = client.reconnect_all();
        assert_eq!(reconnected, 3);
        assert!(client.all_services_available());
    }

    #[test]
    fn test_client_health_check() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        // Health check should not panic and services should remain connected
        client.check_health();
        assert!(client.all_services_available());
    }

    #[test]
    fn test_client_reset() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();
        assert!(client.all_services_available());

        client.reset();
        assert!(!client.all_services_available());
        assert!(!client.is_initialized());
    }

    #[test]
    fn test_client_default() {
        let client = ServiceClient::default();
        assert!(!client.all_services_available());
    }

    // ── ProcessSignal Tests ───────────────────────────────────────────

    #[test]
    fn test_process_signal_number() {
        assert_eq!(ProcessSignal::Term.number(), 15);
        assert_eq!(ProcessSignal::Kill.number(), 9);
        assert_eq!(ProcessSignal::Int.number(), 2);
        assert_eq!(ProcessSignal::Hup.number(), 1);
        assert_eq!(ProcessSignal::Stop.number(), 19);
        assert_eq!(ProcessSignal::Cont.number(), 18);
    }

    #[test]
    fn test_process_signal_from_str() {
        assert_eq!(ProcessSignal::from_str("TERM"), Some(ProcessSignal::Term));
        assert_eq!(ProcessSignal::from_str("SIGTERM"), Some(ProcessSignal::Term));
        assert_eq!(ProcessSignal::from_str("15"), Some(ProcessSignal::Term));
        assert_eq!(ProcessSignal::from_str("KILL"), Some(ProcessSignal::Kill));
        assert_eq!(ProcessSignal::from_str("9"), Some(ProcessSignal::Kill));
        assert_eq!(ProcessSignal::from_str("INT"), Some(ProcessSignal::Int));
        assert_eq!(ProcessSignal::from_str("INVALID"), None);
    }

    // ── ServiceRequest Tests ──────────────────────────────────────────

    #[test]
    fn test_service_request_type() {
        let fs_req = ServiceRequest::FileSystem(FileSystemRequest::ListDir { path: "/".to_string() });
        assert_eq!(fs_req.service_type(), ShellServiceType::FileSystem);

        let proc_req = ServiceRequest::Process(ProcessRequest::ListProcesses);
        assert_eq!(proc_req.service_type(), ShellServiceType::Process);

        let drv_req = ServiceRequest::Driver(DriverRequest::ListDrivers);
        assert_eq!(drv_req.service_type(), ShellServiceType::Driver);
    }

    // ── ServiceResponse Tests ─────────────────────────────────────────

    #[test]
    fn test_service_response_success() {
        let resp = ServiceResponse::success(1, "data".to_string());
        assert!(resp.is_success());
        assert_eq!(resp.request_id, 1);
        assert_eq!(resp.data, "data");
    }

    #[test]
    fn test_service_response_error() {
        let resp = ServiceResponse::error(2, "failed".to_string());
        assert!(!resp.is_success());
        assert_eq!(resp.status, ResponseStatus::Error);
    }

    // ── ServiceCommError Conversion Tests ─────────────────────────────

    #[test]
    fn test_service_comm_error_to_shell_error() {
        let err: ShellError = ServiceCommError::ServiceUnavailable("test".to_string()).into();
        assert!(matches!(err, ShellError::ServiceUnavailable(_)));

        let err: ShellError = ServiceCommError::Timeout("test".to_string()).into();
        assert!(matches!(err, ShellError::ServiceTimeout(_)));

        let err: ShellError = ServiceCommError::PermissionDenied("test".to_string()).into();
        assert!(matches!(err, ShellError::PermissionDenied(_)));

        let err: ShellError = ServiceCommError::NotFound("test".to_string()).into();
        assert!(matches!(err, ShellError::FileNotFound(_)));
    }

    #[test]
    fn test_kosh_service_error_conversion() {
        let err: ServiceCommError = KoshServiceError::NotFound.into();
        assert!(matches!(err, ServiceCommError::NotFound(_)));

        let err: ServiceCommError = KoshServiceError::Timeout.into();
        assert!(matches!(err, ServiceCommError::Timeout(_)));

        let err: ServiceCommError = KoshServiceError::PermissionDenied.into();
        assert!(matches!(err, ServiceCommError::PermissionDenied(_)));
    }

    // ── Reconnection Behavior Tests ───────────────────────────────────

    #[test]
    fn test_reconnect_after_failure() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        // Simulate failures on the FS endpoint
        let ep = client.endpoint_mut(ShellServiceType::FileSystem);
        ep.record_failure();
        ep.record_failure();
        ep.record_failure(); // Now unreachable

        assert_eq!(client.get_status(ShellServiceType::FileSystem), ConnectionStatus::Unreachable);

        // Reconnect should succeed
        assert!(client.try_reconnect(ShellServiceType::FileSystem));
        assert_eq!(client.get_status(ShellServiceType::FileSystem), ConnectionStatus::Connected);
    }

    #[test]
    fn test_reconnect_max_retries() {
        let mut client = ServiceClient::new();
        let ep = client.endpoint_mut(ShellServiceType::FileSystem);
        ep.retry_count = MAX_RETRY_ATTEMPTS;

        assert!(!client.try_reconnect(ShellServiceType::FileSystem));
    }

    #[test]
    fn test_kill_process_request() {
        let mut client = ServiceClient::new();
        client.discover_services().unwrap();

        let result = client.kill_process(42, ProcessSignal::Term);
        assert!(result.is_ok());
    }
}
