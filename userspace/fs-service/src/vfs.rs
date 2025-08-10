use kosh_types::{
    FileDescriptor, InodeNumber, FileOffset, FileType, FilePermissions,
    OpenFlags, FileMetadata, VfsError, DirectoryEntry
};
use crate::ext4::Ext4FileSystem;
use alloc::{vec, vec::Vec, string::{String, ToString}, collections::BTreeMap, boxed::Box};
use core::result::Result;

/// Virtual File System abstraction layer
pub struct Vfs {
    mount_points: BTreeMap<String, MountPoint>,
    file_systems: BTreeMap<String, Box<dyn FileSystem>>,
    open_files: BTreeMap<FileDescriptor, OpenFile>,
    next_fd: FileDescriptor,
}

/// Mount point information
#[derive(Debug, Clone)]
pub struct MountPoint {
    pub path: String,
    pub filesystem: FileSystemType,
    pub read_only: bool,
    pub device_id: Option<u32>,
}

/// File system type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSystemType {
    Ext4,
    TmpFs,
    ProcFs,
    DevFs,
}

/// Open file descriptor information
#[derive(Debug, Clone)]
pub struct OpenFile {
    pub inode: InodeNumber,
    pub mount_point: String,
    pub flags: OpenFlags,
    pub offset: FileOffset,
    pub metadata: FileMetadata,
}

/// File system interface trait that all file systems must implement
pub trait FileSystem {
    /// Initialize the file system
    fn init(&mut self) -> Result<(), VfsError>;
    
    /// Mount the file system at the given path
    fn mount(&mut self, device_id: Option<u32>) -> Result<(), VfsError>;
    
    /// Unmount the file system
    fn unmount(&mut self) -> Result<(), VfsError>;
    
    /// Open a file and return its metadata
    fn open(&mut self, path: &str, flags: OpenFlags) -> Result<(InodeNumber, FileMetadata), VfsError>;
    
    /// Close a file
    fn close(&mut self, inode: InodeNumber) -> Result<(), VfsError>;
    
    /// Read data from a file
    fn read(&mut self, inode: InodeNumber, offset: FileOffset, buffer: &mut [u8]) -> Result<usize, VfsError>;
    
    /// Write data to a file
    fn write(&mut self, inode: InodeNumber, offset: FileOffset, buffer: &[u8]) -> Result<usize, VfsError>;
    
    /// Create a new file
    fn create(&mut self, path: &str, file_type: FileType, permissions: FilePermissions) -> Result<InodeNumber, VfsError>;
    
    /// Delete a file
    fn unlink(&mut self, path: &str) -> Result<(), VfsError>;
    
    /// Get file metadata
    fn stat(&mut self, path: &str) -> Result<FileMetadata, VfsError>;
    
    /// Read directory entries
    fn readdir(&mut self, path: &str) -> Result<Vec<DirectoryEntry>, VfsError>;
    
    /// Create a directory
    fn mkdir(&mut self, path: &str, permissions: FilePermissions) -> Result<(), VfsError>;
    
    /// Remove a directory
    fn rmdir(&mut self, path: &str) -> Result<(), VfsError>;
    
    /// Sync file system data to storage
    fn sync(&mut self) -> Result<(), VfsError>;
}

impl Vfs {
    /// Create a new VFS instance
    pub fn new() -> Self {
        Self {
            mount_points: BTreeMap::new(),
            file_systems: BTreeMap::new(),
            open_files: BTreeMap::new(),
            next_fd: 1, // Start from 1, 0 is reserved
        }
    }
    
    /// Mount a file system at the specified path
    pub fn mount(&mut self, path: &str, fs_type: FileSystemType, device_id: Option<u32>, read_only: bool) -> Result<(), VfsError> {
        // Validate mount path
        if path.is_empty() || !path.starts_with('/') {
            return Err(VfsError::InvalidPath);
        }
        
        // Check if already mounted
        if self.mount_points.contains_key(path) {
            return Err(VfsError::MountPointBusy);
        }
        
        // Create the appropriate file system instance
        let mut filesystem: Box<dyn FileSystem> = match fs_type {
            FileSystemType::Ext4 => Box::new(Ext4FileSystem::new()),
            _ => return Err(VfsError::IoError), // Other file systems not implemented yet
        };
        
        // Initialize and mount the file system
        filesystem.init()?;
        filesystem.mount(device_id)?;
        
        let mount_point = MountPoint {
            path: path.to_string(),
            filesystem: fs_type,
            read_only,
            device_id,
        };
        
        // Store both the mount point and the file system instance
        self.mount_points.insert(path.to_string(), mount_point);
        self.file_systems.insert(path.to_string(), filesystem);
        
        Ok(())
    }
    
    /// Unmount a file system
    pub fn unmount(&mut self, path: &str) -> Result<(), VfsError> {
        // Check if any files are still open from this mount point
        for open_file in self.open_files.values() {
            if open_file.mount_point == path {
                return Err(VfsError::MountPointBusy);
            }
        }
        
        // Unmount the file system
        if let Some(mut filesystem) = self.file_systems.remove(path) {
            filesystem.unmount()?;
        }
        
        self.mount_points.remove(path)
            .ok_or(VfsError::NotMounted)?;
        
        Ok(())
    }
    
    /// Find the mount point for a given path
    fn find_mount_point(&self, path: &str) -> Result<&MountPoint, VfsError> {
        let mut best_match = "";
        let mut best_mount = None;
        
        for (mount_path, mount_point) in &self.mount_points {
            if path.starts_with(mount_path) && mount_path.len() > best_match.len() {
                best_match = mount_path;
                best_mount = Some(mount_point);
            }
        }
        
        best_mount.ok_or(VfsError::NotMounted)
    }
    
    /// Open a file and return a file descriptor
    pub fn open(&mut self, path: &str, flags: OpenFlags) -> Result<FileDescriptor, VfsError> {
        let mount_point = self.find_mount_point(path)?;
        
        // Check read-only mount for write operations
        if mount_point.read_only && (flags == OpenFlags::WRITE_ONLY || flags == OpenFlags::READ_WRITE) {
            return Err(VfsError::ReadOnlyFileSystem);
        }
        
        // Clone the mount point path to avoid borrowing issues
        let mount_path = mount_point.path.clone();
        
        // Get the file system and delegate the open operation
        let filesystem = self.file_systems.get_mut(&mount_path)
            .ok_or(VfsError::NotMounted)?;
        
        // Convert absolute path to relative path within the file system
        let relative_path = if path == &mount_path {
            "/"
        } else if path.starts_with(&mount_path) {
            &path[mount_path.len()..]
        } else {
            path
        };
        
        let (inode, metadata) = filesystem.open(relative_path, flags)?;
        
        let fd = self.next_fd;
        self.next_fd += 1;
        
        let open_file = OpenFile {
            inode,
            mount_point: mount_path,
            flags,
            offset: 0,
            metadata,
        };
        
        self.open_files.insert(fd, open_file);
        Ok(fd)
    }
    
    /// Close a file descriptor
    pub fn close(&mut self, fd: FileDescriptor) -> Result<(), VfsError> {
        let open_file = self.open_files.remove(&fd)
            .ok_or(VfsError::InvalidFileDescriptor)?;
        
        // Delegate to the file system
        let filesystem = self.file_systems.get_mut(&open_file.mount_point)
            .ok_or(VfsError::NotMounted)?;
        
        filesystem.close(open_file.inode)?;
        Ok(())
    }
    
    /// Read from a file descriptor
    pub fn read(&mut self, fd: FileDescriptor, buffer: &mut [u8]) -> Result<usize, VfsError> {
        let open_file = self.open_files.get_mut(&fd)
            .ok_or(VfsError::InvalidFileDescriptor)?;
        
        // Check if file is open for reading
        if open_file.flags == OpenFlags::WRITE_ONLY {
            return Err(VfsError::PermissionDenied);
        }
        
        // Get the file system and delegate the read operation
        let filesystem = self.file_systems.get_mut(&open_file.mount_point)
            .ok_or(VfsError::NotMounted)?;
        
        let bytes_read = filesystem.read(open_file.inode, open_file.offset, buffer)?;
        
        // Update the file offset
        open_file.offset += bytes_read as u64;
        
        Ok(bytes_read)
    }
    
    /// Write to a file descriptor
    pub fn write(&mut self, fd: FileDescriptor, buffer: &[u8]) -> Result<usize, VfsError> {
        let open_file = self.open_files.get_mut(&fd)
            .ok_or(VfsError::InvalidFileDescriptor)?;
        
        // Check if file is open for writing
        if open_file.flags == OpenFlags::READ_ONLY {
            return Err(VfsError::PermissionDenied);
        }
        
        // Get the file system and delegate the write operation
        let filesystem = self.file_systems.get_mut(&open_file.mount_point)
            .ok_or(VfsError::NotMounted)?;
        
        let bytes_written = filesystem.write(open_file.inode, open_file.offset, buffer)?;
        
        // Update the file offset
        open_file.offset += bytes_written as u64;
        
        Ok(bytes_written)
    }
    
    /// Get file metadata
    pub fn stat(&mut self, path: &str) -> Result<FileMetadata, VfsError> {
        let mount_point = self.find_mount_point(path)?;
        let mount_path = mount_point.path.clone();
        
        // Get the file system and delegate the stat operation
        let filesystem = self.file_systems.get_mut(&mount_path)
            .ok_or(VfsError::NotMounted)?;
        
        // Convert absolute path to relative path within the file system
        let relative_path = if path == &mount_path {
            "/"
        } else if path.starts_with(&mount_path) {
            &path[mount_path.len()..]
        } else {
            path
        };
        
        filesystem.stat(relative_path)
    }
    
    /// Create a new file
    pub fn create(&mut self, path: &str, file_type: FileType, permissions: FilePermissions) -> Result<(), VfsError> {
        let mount_point = self.find_mount_point(path)?;
        
        if mount_point.read_only {
            return Err(VfsError::ReadOnlyFileSystem);
        }
        
        let mount_path = mount_point.path.clone();
        
        // Get the file system and delegate the create operation
        let filesystem = self.file_systems.get_mut(&mount_path)
            .ok_or(VfsError::NotMounted)?;
        
        // Convert absolute path to relative path within the file system
        let relative_path = if path == &mount_path {
            "/"
        } else if path.starts_with(&mount_path) {
            &path[mount_path.len()..]
        } else {
            path
        };
        
        filesystem.create(relative_path, file_type, permissions)?;
        Ok(())
    }
    
    /// Delete a file
    pub fn unlink(&mut self, path: &str) -> Result<(), VfsError> {
        let mount_point = self.find_mount_point(path)?;
        
        if mount_point.read_only {
            return Err(VfsError::ReadOnlyFileSystem);
        }
        
        let mount_path = mount_point.path.clone();
        
        // Get the file system and delegate the unlink operation
        let filesystem = self.file_systems.get_mut(&mount_path)
            .ok_or(VfsError::NotMounted)?;
        
        // Convert absolute path to relative path within the file system
        let relative_path = if path == &mount_path {
            "/"
        } else if path.starts_with(&mount_path) {
            &path[mount_path.len()..]
        } else {
            path
        };
        
        filesystem.unlink(relative_path)
    }
    
    /// Read directory entries
    pub fn readdir(&mut self, path: &str) -> Result<Vec<DirectoryEntry>, VfsError> {
        let mount_point = self.find_mount_point(path)?;
        let mount_path = mount_point.path.clone();
        
        // Get the file system and delegate the readdir operation
        let filesystem = self.file_systems.get_mut(&mount_path)
            .ok_or(VfsError::NotMounted)?;
        
        // Convert absolute path to relative path within the file system
        let relative_path = if path == &mount_path {
            "/"
        } else if path.starts_with(&mount_path) {
            &path[mount_path.len()..]
        } else {
            path
        };
        
        filesystem.readdir(relative_path)
    }
    
    /// Create a directory
    pub fn mkdir(&mut self, path: &str, permissions: FilePermissions) -> Result<(), VfsError> {
        let mount_point = self.find_mount_point(path)?;
        
        if mount_point.read_only {
            return Err(VfsError::ReadOnlyFileSystem);
        }
        
        let mount_path = mount_point.path.clone();
        
        // Get the file system and delegate the mkdir operation
        let filesystem = self.file_systems.get_mut(&mount_path)
            .ok_or(VfsError::NotMounted)?;
        
        // Convert absolute path to relative path within the file system
        let relative_path = if path == &mount_path {
            "/"
        } else if path.starts_with(&mount_path) {
            &path[mount_path.len()..]
        } else {
            path
        };
        
        filesystem.mkdir(relative_path, permissions)
    }
    
    /// Remove a directory
    pub fn rmdir(&mut self, path: &str) -> Result<(), VfsError> {
        let mount_point = self.find_mount_point(path)?;
        
        if mount_point.read_only {
            return Err(VfsError::ReadOnlyFileSystem);
        }
        
        let mount_path = mount_point.path.clone();
        
        // Get the file system and delegate the rmdir operation
        let filesystem = self.file_systems.get_mut(&mount_path)
            .ok_or(VfsError::NotMounted)?;
        
        // Convert absolute path to relative path within the file system
        let relative_path = if path == &mount_path {
            "/"
        } else if path.starts_with(&mount_path) {
            &path[mount_path.len()..]
        } else {
            path
        };
        
        filesystem.rmdir(relative_path)
    }
    
    /// Get list of mount points
    pub fn get_mount_points(&self) -> Vec<&MountPoint> {
        self.mount_points.values().collect()
    }
    
    /// Get information about an open file descriptor
    pub fn get_fd_info(&self, fd: FileDescriptor) -> Result<&OpenFile, VfsError> {
        self.open_files.get(&fd)
            .ok_or(VfsError::InvalidFileDescriptor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vfs_creation() {
        let vfs = Vfs::new();
        assert_eq!(vfs.mount_points.len(), 0);
        assert_eq!(vfs.open_files.len(), 0);
        assert_eq!(vfs.next_fd, 1);
    }
    
    #[test]
    fn test_mount_unmount() {
        let mut vfs = Vfs::new();
        
        // Test mounting
        assert!(vfs.mount("/", FileSystemType::Ext4, None, false).is_ok());
        assert_eq!(vfs.mount_points.len(), 1);
        
        // Test mounting at same path fails
        assert_eq!(vfs.mount("/", FileSystemType::TmpFs, None, false), Err(VfsError::MountPointBusy));
        
        // Test unmounting
        assert!(vfs.unmount("/").is_ok());
        assert_eq!(vfs.mount_points.len(), 0);
        
        // Test unmounting non-existent mount point
        assert_eq!(vfs.unmount("/nonexistent"), Err(VfsError::NotMounted));
    }
    
    #[test]
    fn test_invalid_mount_paths() {
        let mut vfs = Vfs::new();
        
        // Empty path
        assert_eq!(vfs.mount("", FileSystemType::Ext4, None, false), Err(VfsError::InvalidPath));
        
        // Relative path
        assert_eq!(vfs.mount("relative", FileSystemType::Ext4, None, false), Err(VfsError::InvalidPath));
    }

    #[test]
    fn test_ext4_integration() {
        let mut vfs = Vfs::new();
        
        // Mount ext4 file system
        assert!(vfs.mount("/", FileSystemType::Ext4, Some(1), false).is_ok());
        
        // Test file operations through VFS
        assert!(vfs.create("/test.txt", FileType::Regular, FilePermissions::OWNER_READ | FilePermissions::OWNER_WRITE).is_ok());
        
        // Test opening a file
        let fd = vfs.open("/test.txt", OpenFlags::READ_WRITE);
        assert!(fd.is_ok());
        let fd = fd.unwrap();
        
        // Test writing to the file
        let data = b"Hello, ext4 through VFS!";
        let written = vfs.write(fd, data);
        assert!(written.is_ok());
        assert_eq!(written.unwrap(), data.len());
        
        // Test reading from the file (need to reset offset for this test)
        let mut buffer = vec![0u8; data.len()];
        // Reset the file offset to 0 for reading
        if let Some(open_file) = vfs.open_files.get_mut(&fd) {
            open_file.offset = 0;
        }
        let read = vfs.read(fd, &mut buffer);
        assert!(read.is_ok());
        assert_eq!(read.unwrap(), data.len());
        
        // Test closing the file
        assert!(vfs.close(fd).is_ok());
        
        // Test stat
        let metadata = vfs.stat("/test.txt");
        assert!(metadata.is_ok());
        let metadata = metadata.unwrap();
        assert_eq!(metadata.file_type, FileType::Regular);
        
        // Test directory operations
        assert!(vfs.mkdir("/testdir", FilePermissions::OWNER_READ | FilePermissions::OWNER_WRITE | FilePermissions::OWNER_EXECUTE).is_ok());
        
        let entries = vfs.readdir("/testdir");
        assert!(entries.is_ok());
        let entries = entries.unwrap();
        assert_eq!(entries.len(), 2); // Should contain "." and ".."
        
        // Test unmounting
        assert!(vfs.unmount("/").is_ok());
    }
}