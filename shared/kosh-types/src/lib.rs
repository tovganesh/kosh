#![no_std]

use bitflags::bitflags;

pub type ProcessId = u32;
pub type DriverId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Ready,
    Blocked,
    Zombie,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Priority(pub u8);

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CapabilityFlags: u64 {
        const READ_MEMORY = 1 << 0;
        const WRITE_MEMORY = 1 << 1;
        const EXECUTE = 1 << 2;
        const HARDWARE_ACCESS = 1 << 3;
        const IPC_SEND = 1 << 4;
        const IPC_RECEIVE = 1 << 5;
        const FILE_READ = 1 << 6;
        const FILE_WRITE = 1 << 7;
        const NETWORK_ACCESS = 1 << 8;
    }
}

#[derive(Debug, Clone)]
pub struct Capability {
    pub flags: CapabilityFlags,
    pub resource_id: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    SystemCall,
    DriverRequest,
    ServiceRequest,
    Signal,
}

#[derive(Debug, Clone)]
pub enum DriverError {
    InitializationFailed,
    HardwareNotFound,
    InvalidRequest,
    ResourceBusy,
    PermissionDenied,
}