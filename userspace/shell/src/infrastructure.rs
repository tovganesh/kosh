#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use kosh_service::{ServiceClient, ServiceType, ServiceData};
use kosh_types::ProcessId;
use crate::error::{ShellError, ShellResult};
use crate::types::*;

/// Maximum number of retry attempts for service reconnection
const MAX_RETRY_ATTEMPTS: u32 = 3;

/// Default timeout for service requests (in simulated ticks)
const DEFAULT_TIMEOUT_TICKS: u64 = 1000;

/// Health check interval (in simulated ticks)
const HEALTH_CHECK_INTERVAL: u64 = 500;

/// Status of a service connection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceConnectionStatus {
    /// Service has been discovered and is connected
    Connected,
    /// Service connection has not been established yet
    Disconnected,
    /// Service was connected but is now unreachable
    Unreachable,
    /// Currently attempting to reconnect
    Reconnecting,
}

/// Tracks the health and connection state of a single service
#[derive(Debug, Clone)]
pub struct ServiceEndpoint {
    pub pid: Option<ProcessId>,
    pub service_type: ServiceType,
    pub name: String,
    pub status: ServiceConnectionStatus,
    pub retry_count: u32,
    pub last_health_check: u64,
    pub consecutive_failures: u32,
}

impl ServiceEndpoint {
    pub fn new(service_type: ServiceType, name: &str) -> Self {
        Self {
            pid: None,
            service_type,
            name: name.to_string(),
            status: ServiceConnectionStatus::Disconnected,
            retry_count: 0,
            last_health_check: 0,
            consecutive_failures: 0,
        }
    }

    /// Mark the service as connected with the given PID
    pub fn connect(&mut self, pid: ProcessId) {
        self.pid = Some(pid);
        self.status = ServiceConnectionStatus::Connected;
        self.retry_count = 0;
        self.consecutive_failures = 0;
    }

    /// Mark the service as disconnected
    pub fn disconnect(&mut self) {
        self.status = ServiceConnectionStatus::Disconnected;
        self.pid = None;
    }

    /// Record a successful request
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.status = ServiceConnectionStatus::Connected;
    }

    /// Record a failed request and return whether reconnection should be attempted
    pub fn record_failure(&mut self) -> bool {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= 3 {
            self.status = ServiceConnectionStatus::Unreachable;
            self.retry_count < MAX_RETRY_ATTEMPTS
        } else {
            false
        }
    }

    /// Check if the service is available for requests
    pub fn is_available(&self) -> bool {
        self.status == ServiceConnectionStatus::Connected && self.pid.is_some()
    }
}

/// Request types for service communication
#[derive(Debug, Clone)]
pub enum ServiceRequest {
    FileSystem(FileSystemRequest),
    Process(ProcessRequest),
    Driver(DriverRequest),
}

/// File system request types
#[derive(Debug, Clone)]
pub enum FileSystemRequest {
    List { path: String },
    Read { path: String },
    Write { path: String, data: Vec<u8> },
    Create { path: String, is_directory: bool },
    Delete { path: String },
}

/// Process request types
#[derive(Debug, Clone)]
pub enum ProcessRequest {
    List,
    Kill { pid: ProcessId },
    GetInfo { pid: ProcessId },
}

/// Driver request types
#[derive(Debug, Clone)]
pub enum DriverRequest {
    List,
    Load { path: String },
    Unload { driver_id: u32 },
}

/// Response from a service request
#[derive(Debug, Clone)]
pub struct ServiceResponse {
    pub request_id: u64,
    pub status: ServiceResponseStatus,
    pub data: String,
}

/// Status of a service response
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceResponseStatus {
    Success,
    Error,
    NotFound,
    PermissionDenied,
    Timeout,
}

/// Service communication layer for the shell.
///
/// Manages connections to the file system, process, and driver services.
/// Provides service discovery, health monitoring, request/response handling
/// with timeout support, and automatic reconnection on failure.
pub struct ShellServiceClient {
    service_client: ServiceClient,
    fs_endpoint: ServiceEndpoint,
    process_endpoint: ServiceEndpoint,
    driver_endpoint: ServiceEndpoint,
    next_request_id: u64,
    current_tick: u64,
    timeout_ticks: u64,
}

impl ShellServiceClient {
    pub fn new() -> Self {
        Self {
            service_client: ServiceClient::new(),
            fs_endpoint: ServiceEndpoint::new(ServiceType::FileSystem, "fs_service"),
            process_endpoint: ServiceEndpoint::new(ServiceType::ProcessManager, "process_service"),
            driver_endpoint: ServiceEndpoint::new(ServiceType::DriverManager, "driver_service"),
            next_request_id: 1,
            current_tick: 0,
            timeout_ticks: DEFAULT_TIMEOUT_TICKS,
        }
    }

    /// Set the timeout for service requests (in ticks)
    pub fn set_timeout(&mut self, ticks: u64) {
        self.timeout_ticks = ticks;
    }

    /// Advance the internal tick counter (called by the shell main loop)
    pub fn tick(&mut self) {
        self.current_tick += 1;
    }

    // ── Service Discovery ──────────────────────────────────────────────

    /// Discover and connect to all system services.
    ///
    /// In a real implementation this would query the kernel service registry
    /// via IPC. For now it uses simulated well-known PIDs.
    pub fn discover_services(&mut self) -> ShellResult<()> {
        self.discover_service_by_type(ServiceType::FileSystem, 100)?;
        self.discover_service_by_type(ServiceType::ProcessManager, 101)?;
        self.discover_service_by_type(ServiceType::DriverManager, 102)?;
        Ok(())
    }

    /// Discover a single service by type.
    /// `simulated_pid` is used until real IPC service registry is available.
    fn discover_service_by_type(
        &mut self,
        service_type: ServiceType,
        simulated_pid: ProcessId,
    ) -> ShellResult<()> {
        let endpoint = self.endpoint_mut(service_type);
        endpoint.connect(simulated_pid);
        Ok(())
    }

    /// Look up a service by name and return its PID if found.
    pub fn find_service_by_name(&self, name: &str) -> Option<ProcessId> {
        let endpoints = [&self.fs_endpoint, &self.process_endpoint, &self.driver_endpoint];
        endpoints
            .iter()
            .find(|ep| ep.name == name && ep.is_available())
            .and_then(|ep| ep.pid)
    }

    /// Return whether all required services are connected.
    pub fn all_services_available(&self) -> bool {
        self.fs_endpoint.is_available()
            && self.process_endpoint.is_available()
            && self.driver_endpoint.is_available()
    }

    // ── Request/Response Handling ──────────────────────────────────────

    /// Send a request to a service and wait for a response with timeout.
    pub fn send_request(&mut self, request: ServiceRequest) -> ShellResult<ServiceResponse> {
        let (endpoint, service_data) = match &request {
            ServiceRequest::FileSystem(fs_req) => {
                let data = self.fs_request_to_service_data(fs_req);
                (&mut self.fs_endpoint as *mut ServiceEndpoint, data)
            }
            ServiceRequest::Process(proc_req) => {
                let data = self.process_request_to_service_data(proc_req);
                (&mut self.process_endpoint as *mut ServiceEndpoint, data)
            }
            ServiceRequest::Driver(drv_req) => {
                let data = self.driver_request_to_service_data(drv_req);
                (&mut self.driver_endpoint as *mut ServiceEndpoint, data)
            }
        };

        // Safety: we're just avoiding borrow checker issues with the mutable reference
        let endpoint = unsafe { &mut *endpoint };

        if !endpoint.is_available() {
            // Attempt reconnection
            if !self.try_reconnect_endpoint(endpoint) {
                return Err(ShellError::ServiceUnavailable(endpoint.name.clone()));
            }
        }

        let pid = endpoint.pid.ok_or_else(|| {
            ShellError::ServiceUnavailable(endpoint.name.clone())
        })?;

        let request_id = self.next_request_id;
        self.next_request_id += 1;

        // Send via underlying service client (simulated for now)
        let send_result = self.service_client.send_request(pid, endpoint.service_type, service_data);

        match send_result {
            Ok(_) => {
                // Simulate waiting for response with timeout
                let response = self.wait_for_response_with_timeout(request_id)?;
                endpoint.record_success();
                Ok(response)
            }
            Err(e) => {
                if endpoint.record_failure() {
                    let _ = self.try_reconnect_endpoint(endpoint);
                }
                Err(ShellError::ServiceError(e))
            }
        }
    }

    /// Wait for a response with timeout handling.
    ///
    /// In a real implementation this would poll the IPC receive queue.
    /// Currently returns a simulated "not implemented" response since
    /// the actual IPC transport is not yet wired up.
    fn wait_for_response_with_timeout(
        &self,
        request_id: u64,
    ) -> ShellResult<ServiceResponse> {
        // Simulated: in a real system we would loop up to timeout_ticks
        // polling receive_response(). For now, return a placeholder.
        Ok(ServiceResponse {
            request_id,
            status: ServiceResponseStatus::Success,
            data: String::new(),
        })
    }

    // ── Typed Request Helpers ─────────────────────────────────────────

    /// Send a request to the file system service
    pub fn send_fs_request(&mut self, request: FileSystemRequest) -> ShellResult<ServiceResponse> {
        self.send_request(ServiceRequest::FileSystem(request))
    }

    /// Send a request to the process service
    pub fn send_process_request(&mut self, request: ProcessRequest) -> ShellResult<ServiceResponse> {
        self.send_request(ServiceRequest::Process(request))
    }

    /// Send a request to the driver service
    pub fn send_driver_request(&mut self, request: DriverRequest) -> ShellResult<ServiceResponse> {
        self.send_request(ServiceRequest::Driver(request))
    }

    // ── Health Monitoring ─────────────────────────────────────────────

    /// Perform health checks on all services.
    ///
    /// Should be called periodically from the shell main loop.
    pub fn check_service_health(&mut self) {
        let tick = self.current_tick;
        self.check_endpoint_health(&mut self.fs_endpoint.clone(), tick);
        self.check_endpoint_health(&mut self.process_endpoint.clone(), tick);
        self.check_endpoint_health(&mut self.driver_endpoint.clone(), tick);
    }

    fn check_endpoint_health(&mut self, endpoint: &mut ServiceEndpoint, tick: u64) {
        if tick - endpoint.last_health_check < HEALTH_CHECK_INTERVAL {
            return;
        }
        endpoint.last_health_check = tick;

        if !endpoint.is_available() {
            return;
        }

        // In a real implementation we would send a ping/health-check message.
        // For now, assume the service is healthy if it was connected.
    }

    /// Get the connection status of a specific service type.
    pub fn get_service_status(&self, service_type: ServiceType) -> ServiceConnectionStatus {
        self.endpoint(service_type).status
    }

    /// Get the PID of a specific service type if connected.
    pub fn get_service_pid(&self, service_type: ServiceType) -> Option<ProcessId> {
        self.endpoint(service_type).pid
    }

    // ── Reconnection Logic ────────────────────────────────────────────

    /// Attempt to reconnect a single endpoint.
    /// Returns `true` if reconnection succeeded.
    fn try_reconnect_endpoint(&mut self, endpoint: &mut ServiceEndpoint) -> bool {
        if endpoint.retry_count >= MAX_RETRY_ATTEMPTS {
            return false;
        }

        endpoint.status = ServiceConnectionStatus::Reconnecting;
        endpoint.retry_count += 1;

        // In a real implementation we would re-query the service registry.
        // For now, simulate success by re-assigning the well-known PID.
        let simulated_pid = match endpoint.service_type {
            ServiceType::FileSystem => 100,
            ServiceType::ProcessManager => 101,
            ServiceType::DriverManager => 102,
            _ => return false,
        };

        endpoint.connect(simulated_pid);
        true
    }

    /// Attempt to reconnect all unreachable services.
    /// Returns the number of services successfully reconnected.
    pub fn reconnect_all(&mut self) -> u32 {
        let mut reconnected = 0;

        // Clone to avoid borrow issues, then apply results
        let types = [ServiceType::FileSystem, ServiceType::ProcessManager, ServiceType::DriverManager];
        for st in &types {
            let status = self.endpoint(*st).status;
            if status == ServiceConnectionStatus::Unreachable
                || status == ServiceConnectionStatus::Disconnected
            {
                let ep = self.endpoint_mut(*st);
                if ShellServiceClient::try_reconnect_endpoint_inner(ep) {
                    reconnected += 1;
                }
            }
        }
        reconnected
    }

    fn try_reconnect_endpoint_inner(endpoint: &mut ServiceEndpoint) -> bool {
        if endpoint.retry_count >= MAX_RETRY_ATTEMPTS {
            return false;
        }
        endpoint.status = ServiceConnectionStatus::Reconnecting;
        endpoint.retry_count += 1;

        let simulated_pid = match endpoint.service_type {
            ServiceType::FileSystem => 100,
            ServiceType::ProcessManager => 101,
            ServiceType::DriverManager => 102,
            _ => return false,
        };

        endpoint.connect(simulated_pid);
        true
    }

    // ── Data Conversion Helpers ───────────────────────────────────────

    fn fs_request_to_service_data(&self, req: &FileSystemRequest) -> ServiceData {
        match req {
            FileSystemRequest::List { path } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::List {
                    path: path.clone(),
                })
            }
            FileSystemRequest::Read { path } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::Open {
                    path: path.clone(),
                    flags: 0, // read-only
                })
            }
            FileSystemRequest::Write { path: _, data } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::Write {
                    fd: 0,
                    data: data.clone(),
                })
            }
            FileSystemRequest::Create { path, is_directory } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::Create {
                    path: path.clone(),
                    is_directory: *is_directory,
                })
            }
            FileSystemRequest::Delete { path } => {
                ServiceData::FileSystemRequest(kosh_service::FileSystemRequest::Delete {
                    path: path.clone(),
                })
            }
        }
    }

    fn process_request_to_service_data(&self, req: &ProcessRequest) -> ServiceData {
        match req {
            ProcessRequest::List => {
                ServiceData::ProcessRequest(kosh_service::ProcessRequest::List)
            }
            ProcessRequest::Kill { pid } => {
                ServiceData::ProcessRequest(kosh_service::ProcessRequest::Kill { pid: *pid })
            }
            ProcessRequest::GetInfo { pid } => {
                ServiceData::ProcessRequest(kosh_service::ProcessRequest::GetInfo { pid: *pid })
            }
        }
    }

    fn driver_request_to_service_data(&self, req: &DriverRequest) -> ServiceData {
        match req {
            DriverRequest::List => {
                ServiceData::DriverRequest(kosh_service::DriverRequest::ListDrivers)
            }
            DriverRequest::Load { path } => {
                ServiceData::DriverRequest(kosh_service::DriverRequest::LoadDriver {
                    path: path.clone(),
                })
            }
            DriverRequest::Unload { driver_id } => {
                ServiceData::DriverRequest(kosh_service::DriverRequest::UnloadDriver {
                    driver_id: *driver_id,
                })
            }
        }
    }

    // ── Endpoint Accessors ────────────────────────────────────────────

    fn endpoint(&self, service_type: ServiceType) -> &ServiceEndpoint {
        match service_type {
            ServiceType::FileSystem => &self.fs_endpoint,
            ServiceType::ProcessManager => &self.process_endpoint,
            ServiceType::DriverManager => &self.driver_endpoint,
            _ => &self.fs_endpoint, // fallback
        }
    }

    fn endpoint_mut(&mut self, service_type: ServiceType) -> &mut ServiceEndpoint {
        match service_type {
            ServiceType::FileSystem => &mut self.fs_endpoint,
            ServiceType::ProcessManager => &mut self.process_endpoint,
            ServiceType::DriverManager => &mut self.driver_endpoint,
            _ => &mut self.fs_endpoint, // fallback
        }
    }

    // ── Accessors for backward compatibility ──────────────────────────

    /// Get the file system service PID (backward compat)
    pub fn fs_service_pid(&self) -> Option<ProcessId> {
        self.fs_endpoint.pid
    }

    /// Get the process service PID (backward compat)
    pub fn process_service_pid(&self) -> Option<ProcessId> {
        self.process_endpoint.pid
    }

    /// Get the driver service PID (backward compat)
    pub fn driver_service_pid(&self) -> Option<ProcessId> {
        self.driver_endpoint.pid
    }
}

/// Command execution context
/// This provides the runtime context for command execution
pub struct ExecutionContext {
    pub environment: Environment,
    pub service_client: ShellServiceClient,
    pub background_jobs: Vec<BackgroundJob>,
    pub next_job_id: u32,
}

impl ExecutionContext {
    pub fn new() -> Self {
        Self {
            environment: Environment::with_defaults(),
            service_client: ShellServiceClient::new(),
            background_jobs: Vec::new(),
            next_job_id: 1,
        }
    }

    /// Initialize the execution context
    pub fn initialize(&mut self) -> ShellResult<()> {
        self.service_client.discover_services()?;
        Ok(())
    }

    /// Get the current working directory
    pub fn current_directory(&self) -> &str {
        &self.environment.working_directory
    }

    /// Change the current working directory
    pub fn change_directory(&mut self, path: String) -> ShellResult<()> {
        self.environment.working_directory = path.clone();
        self.environment.set_var("PWD".to_string(), path);
        Ok(())
    }

    /// Add a background job
    pub fn add_background_job(&mut self, pid: ProcessId, command: String) -> u32 {
        let job_id = self.next_job_id;
        self.next_job_id += 1;

        let job = BackgroundJob {
            job_id,
            pid,
            command,
            status: JobStatus::Running,
        };

        self.background_jobs.push(job);
        job_id
    }

    /// Update job status
    pub fn update_job_status(&mut self, job_id: u32, status: JobStatus) {
        if let Some(job) = self.background_jobs.iter_mut().find(|j| j.job_id == job_id) {
            job.status = status;
        }
    }

    /// Get all background jobs
    pub fn get_background_jobs(&self) -> &[BackgroundJob] {
        &self.background_jobs
    }

    /// Clean up completed jobs
    pub fn cleanup_completed_jobs(&mut self) {
        self.background_jobs.retain(|job| !matches!(job.status, JobStatus::Completed(_)));
    }
}

/// Command parser infrastructure.
/// Delegates to the AdvancedParser for full pipe, redirect, and conditional support.
pub struct CommandParser {
    parser: crate::parser::AdvancedParser,
}

impl CommandParser {
    pub fn new() -> Self {
        Self {
            parser: crate::parser::AdvancedParser::new(),
        }
    }

    /// Parse a command line into a ParsedCommand with full operator support.
    pub fn parse(&self, command_line: &str) -> ShellResult<ParsedCommand> {
        self.parser.parse(command_line)
    }

    /// Parse with environment variable expansion.
    pub fn parse_with_env<F>(&self, command_line: &str, lookup: F) -> ShellResult<ParsedCommand>
    where
        F: Fn(&str) -> Option<String>,
    {
        self.parser.parse_with_env(command_line, lookup)
    }
}
