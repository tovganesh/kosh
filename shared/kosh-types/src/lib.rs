#![no_std]

use bitflags::bitflags;

extern crate alloc;

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

// File System Types
pub type FileDescriptor = u32;
pub type InodeNumber = u64;
pub type FileSize = u64;
pub type FileOffset = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
    SymbolicLink,
    BlockDevice,
    CharacterDevice,
    Fifo,
    Socket,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FilePermissions: u16 {
        const OWNER_READ = 0o400;
        const OWNER_WRITE = 0o200;
        const OWNER_EXECUTE = 0o100;
        const GROUP_READ = 0o040;
        const GROUP_WRITE = 0o020;
        const GROUP_EXECUTE = 0o010;
        const OTHER_READ = 0o004;
        const OTHER_WRITE = 0o002;
        const OTHER_EXECUTE = 0o001;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct OpenFlags: u32 {
        const READ_ONLY = 0o0;
        const WRITE_ONLY = 0o1;
        const READ_WRITE = 0o2;
        const CREATE = 0o100;
        const EXCLUSIVE = 0o200;
        const TRUNCATE = 0o1000;
        const APPEND = 0o2000;
    }
}

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub inode: InodeNumber,
    pub file_type: FileType,
    pub permissions: FilePermissions,
    pub size: FileSize,
    pub uid: u32,
    pub gid: u32,
    pub created_time: u64,
    pub modified_time: u64,
    pub accessed_time: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VfsError {
    NotFound,
    PermissionDenied,
    AlreadyExists,
    NotDirectory,
    IsDirectory,
    InvalidPath,
    IoError,
    NoSpace,
    ReadOnlyFileSystem,
    TooManyOpenFiles,
    InvalidFileDescriptor,
    NotMounted,
    MountPointBusy,
}

#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    pub name: [u8; 256], // Fixed-size name buffer
    pub name_len: u8,
    pub inode: InodeNumber,
    pub file_type: FileType,
}