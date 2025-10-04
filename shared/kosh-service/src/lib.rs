#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;
use kosh_types::ProcessId;
use kosh_ipc::{Message, MessageData, IpcError};

/// Service communication framework for Kosh OS
/// Provides standardized communication between system services

#[derive(Debug, Clone)]
pub struct ServiceMessage {
    pub service_type: ServiceType,
    pub request_id: u64,
    pub data: ServiceData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    FileSystem,
    DriverManager,
    ProcessManager,
    MemoryManager,
    NetworkManager,
    DisplayManager,
    InputManager,
}

#[derive(Debug, Clone)]
pub enum ServiceData {
    Empty,
    Text(String),
    Binary(Vec<u8>),
    FileSystemRequest(FileSystemRequest),
    DriverRequest(DriverRequest),
    ProcessRequest(ProcessRequest),
}

#[derive(Debug, Clone)]
pub enum FileSystemRequest {
    Open { path: String, flags: u32 },
    Close { fd: u32 },
    Read { fd: u32, size: usize },
    Write { fd: u32, data: Vec<u8> },
    List { path: String },
    Create { path: String, is_directory: bool },
    Delete { path: String },
}

#[derive(Debug, Clone)]
pub enum DriverRequest {
    LoadDriver { path: String },
    UnloadDriver { driver_id: u32 },
    ListDrivers,
    SendToDriver { driver_id: u32, data: Vec<u8> },
}

#[derive(Debug, Clone)]
pub enum ProcessRequest {
    Spawn { program: String, args: Vec<String> },
    Kill { pid: ProcessId },
    List,
    GetInfo { pid: ProcessId },
}

#[derive(Debug, Clone)]
pub struct ServiceResponse {
    pub request_id: u64,
    pub status: ServiceStatus,
    pub data: ServiceData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Success,
    Error,
    NotFound,
    PermissionDenied,
    InvalidRequest,
    ServiceUnavailable,
}

/// Service registry for tracking available services
pub struct ServiceRegistry {
    services: Vec<ServiceInfo>,
}

#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub service_type: ServiceType,
    pub pid: ProcessId,
    pub name: String,
    pub status: ServiceStatus,
}

impl ServiceRegistry {
    pub fn new() -> Self {
        Self {
            services: Vec::new(),
        }
    }
    
    pub fn register_service(&mut self, service_type: ServiceType, pid: ProcessId, name: String) {
        let service_info = ServiceInfo {
            service_type,
            pid,
            name,
            status: ServiceStatus::Success,
        };
        
        self.services.push(service_info);
    }
    
    pub fn unregister_service(&mut self, pid: ProcessId) {
        self.services.retain(|service| service.pid != pid);
    }
    
    pub fn find_service(&self, service_type: ServiceType) -> Option<&ServiceInfo> {
        self.services.iter().find(|service| service.service_type == service_type)
    }
    
    pub fn list_services(&self) -> &[ServiceInfo] {
        &self.services
    }
    
    pub fn update_service_status(&mut self, pid: ProcessId, status: ServiceStatus) {
        if let Some(service) = self.services.iter_mut().find(|s| s.pid == pid) {
            service.status = status;
        }
    }
}

/// Service client for communicating with services
pub struct ServiceClient {
    next_request_id: u64,
}

impl ServiceClient {
    pub fn new() -> Self {
        Self {
            next_request_id: 1,
        }
    }
    
    pub fn send_request(&mut self, service_pid: ProcessId, service_type: ServiceType, data: ServiceData) -> Result<u64, ServiceError> {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        
        let service_message = ServiceMessage {
            service_type,
            request_id,
            data,
        };
        
        // Convert to IPC message
        let _ipc_message = self.service_message_to_ipc(service_pid, service_message)?;
        
        // Send via IPC (this would use actual IPC system calls)
        // For now, just return the request ID
        Ok(request_id)
    }
    
    pub fn receive_response(&self) -> Result<ServiceResponse, ServiceError> {
        // In a real implementation, this would receive IPC messages
        // and convert them back to service responses
        Err(ServiceError::NotImplemented)
    }
    
    fn service_message_to_ipc(&self, receiver: ProcessId, _message: ServiceMessage) -> Result<Message, ServiceError> {
        // Convert ServiceMessage to IPC Message
        // This is a simplified conversion
        let message_data = MessageData::Bytes(&[]); // Would serialize the actual data
        
        Ok(Message {
            sender: 0, // Would be filled by IPC system
            receiver,
            message_type: kosh_types::MessageType::ServiceRequest,
            data: message_data,
            capabilities: &[],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceError {
    NotFound,
    PermissionDenied,
    InvalidRequest,
    CommunicationError,
    Timeout,
    NotImplemented,
}

impl From<IpcError> for ServiceError {
    fn from(error: IpcError) -> Self {
        match error {
            IpcError::InvalidReceiver => ServiceError::NotFound,
            IpcError::PermissionDenied => ServiceError::PermissionDenied,
            IpcError::Timeout => ServiceError::Timeout,
            _ => ServiceError::CommunicationError,
        }
    }
}

/// Trait for implementing service handlers
pub trait ServiceHandler {
    fn handle_request(&mut self, request: ServiceMessage) -> ServiceResponse;
    fn get_service_type(&self) -> ServiceType;
    fn initialize(&mut self) -> Result<(), ServiceError>;
    fn shutdown(&mut self) -> Result<(), ServiceError>;
}

/// Service runner for managing service lifecycle
pub struct ServiceRunner<T: ServiceHandler> {
    handler: T,
    running: bool,
}

impl<T: ServiceHandler> ServiceRunner<T> {
    pub fn new(handler: T) -> Self {
        Self {
            handler,
            running: false,
        }
    }
    
    pub fn start(&mut self) -> Result<(), ServiceError> {
        self.handler.initialize()?;
        self.running = true;
        Ok(())
    }
    
    pub fn stop(&mut self) -> Result<(), ServiceError> {
        self.running = false;
        self.handler.shutdown()
    }
    
    pub fn run_once(&mut self) -> Result<(), ServiceError> {
        if !self.running {
            return Err(ServiceError::InvalidRequest);
        }
        
        // In a real implementation, this would:
        // 1. Check for incoming IPC messages
        // 2. Convert them to ServiceMessage
        // 3. Call handler.handle_request()
        // 4. Send response back via IPC
        
        Ok(())
    }
    
    pub fn is_running(&self) -> bool {
        self.running
    }
}