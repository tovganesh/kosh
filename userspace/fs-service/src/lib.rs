#![no_std]

extern crate alloc;

use alloc::{vec::Vec, string::String};
use kosh_types::{OpenFlags, FileType, FilePermissions, VfsError};

pub mod vfs;
pub mod ext4;
pub use vfs::{Vfs, FileSystemType};

/// File system service request types
#[derive(Debug, Clone)]
pub enum FsRequest {
    Open { path: String, flags: OpenFlags },
    Close { fd: kosh_types::FileDescriptor },
    Read { fd: kosh_types::FileDescriptor, size: usize },
    Write { fd: kosh_types::FileDescriptor, data: Vec<u8> },
    Stat { path: String },
    Create { path: String, file_type: FileType, permissions: FilePermissions },
    Unlink { path: String },
    ReadDir { path: String },
    MkDir { path: String, permissions: FilePermissions },
    RmDir { path: String },
}

/// File system service response types
#[derive(Debug, Clone)]
pub enum FsResponse {
    Success,
    FileDescriptor(kosh_types::FileDescriptor),
    Data(Vec<u8>),
    BytesWritten(usize),
    Metadata(kosh_types::FileMetadata),
    DirectoryEntries(Vec<kosh_types::DirectoryEntry>),
}

/// Handle file system service requests
pub fn handle_fs_request(vfs: &mut Vfs, request: FsRequest) -> Result<FsResponse, VfsError> {
    match request {
        FsRequest::Open { path, flags } => {
            let fd = vfs.open(&path, flags)?;
            Ok(FsResponse::FileDescriptor(fd))
        }
        FsRequest::Close { fd } => {
            vfs.close(fd)?;
            Ok(FsResponse::Success)
        }
        FsRequest::Read { fd, size } => {
            let mut buffer = Vec::with_capacity(size);
            buffer.resize(size, 0);
            let bytes_read = vfs.read(fd, &mut buffer)?;
            buffer.truncate(bytes_read);
            Ok(FsResponse::Data(buffer))
        }
        FsRequest::Write { fd, data } => {
            let bytes_written = vfs.write(fd, &data)?;
            Ok(FsResponse::BytesWritten(bytes_written))
        }
        FsRequest::Stat { path } => {
            let metadata = vfs.stat(&path)?;
            Ok(FsResponse::Metadata(metadata))
        }
        FsRequest::Create { path, file_type, permissions } => {
            vfs.create(&path, file_type, permissions)?;
            Ok(FsResponse::Success)
        }
        FsRequest::Unlink { path } => {
            vfs.unlink(&path)?;
            Ok(FsResponse::Success)
        }
        FsRequest::ReadDir { path } => {
            let entries = vfs.readdir(&path)?;
            Ok(FsResponse::DirectoryEntries(entries))
        }
        FsRequest::MkDir { path, permissions } => {
            vfs.mkdir(&path, permissions)?;
            Ok(FsResponse::Success)
        }
        FsRequest::RmDir { path } => {
            vfs.rmdir(&path)?;
            Ok(FsResponse::Success)
        }
    }
}