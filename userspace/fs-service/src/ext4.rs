use kosh_types::{
    FileDescriptor, InodeNumber, FileOffset, FileType, FilePermissions,
    OpenFlags, FileMetadata, VfsError, DirectoryEntry, FileSize
};
use crate::vfs::FileSystem;
use alloc::{vec, vec::Vec, string::{String, ToString}, collections::BTreeMap};
use core::{result::Result, mem};

/// ext4 file system implementation
pub struct Ext4FileSystem {
    superblock: Option<Ext4Superblock>,
    block_size: u32,
    inode_size: u16,
    device_id: Option<u32>,
    mounted: bool,
    inode_cache: BTreeMap<InodeNumber, Ext4Inode>,
    path_to_inode: BTreeMap<String, InodeNumber>,
    next_inode: InodeNumber,
}

/// ext4 superblock structure (simplified)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Ext4Superblock {
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub r_blocks_count: u32,
    pub free_blocks_count: u32,
    pub free_inodes_count: u32,
    pub first_data_block: u32,
    pub log_block_size: u32,
    pub log_cluster_size: u32,
    pub blocks_per_group: u32,
    pub clusters_per_group: u32,
    pub inodes_per_group: u32,
    pub mtime: u32,
    pub wtime: u32,
    pub mnt_count: u16,
    pub max_mnt_count: u16,
    pub magic: u16,
    pub state: u16,
    pub errors: u16,
    pub minor_rev_level: u16,
    pub lastcheck: u32,
    pub checkinterval: u32,
    pub creator_os: u32,
    pub rev_level: u32,
    pub def_resuid: u16,
    pub def_resgid: u16,
    // Extended fields for ext4
    pub first_ino: u32,
    pub inode_size: u16,
    pub block_group_nr: u16,
    pub feature_compat: u32,
    pub feature_incompat: u32,
    pub feature_ro_compat: u32,
    pub uuid: [u8; 16],
    pub volume_name: [u8; 16],
    pub last_mounted: [u8; 64],
    pub algorithm_usage_bitmap: u32,
}

/// ext4 inode structure (simplified)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Ext4Inode {
    pub mode: u16,
    pub uid: u16,
    pub size_lo: u32,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub gid: u16,
    pub links_count: u16,
    pub blocks_lo: u32,
    pub flags: u32,
    pub osd1: u32,
    pub block: [u32; 15], // Direct and indirect block pointers
    pub generation: u32,
    pub file_acl_lo: u32,
    pub size_high: u32,
    pub obso_faddr: u32,
    pub osd2: [u8; 12],
    pub extra_isize: u16,
    pub checksum_hi: u16,
    pub ctime_extra: u32,
    pub mtime_extra: u32,
    pub atime_extra: u32,
    pub crtime: u32,
    pub crtime_extra: u32,
    pub version_hi: u32,
    pub projid: u32,
}

/// ext4 directory entry structure
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Ext4DirEntry {
    pub inode: u32,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: u8,
    // name follows this structure
}

/// ext4 constants
const EXT4_SUPER_MAGIC: u16 = 0xEF53;
const EXT4_SUPERBLOCK_OFFSET: u64 = 1024;
const EXT4_MIN_BLOCK_SIZE: u32 = 1024;
const EXT4_MAX_BLOCK_SIZE: u32 = 65536;

// File type constants for ext4 directory entries
const EXT4_FT_UNKNOWN: u8 = 0;
const EXT4_FT_REG_FILE: u8 = 1;
const EXT4_FT_DIR: u8 = 2;
const EXT4_FT_CHRDEV: u8 = 3;
const EXT4_FT_BLKDEV: u8 = 4;
const EXT4_FT_FIFO: u8 = 5;
const EXT4_FT_SOCK: u8 = 6;
const EXT4_FT_SYMLINK: u8 = 7;

// Inode mode constants
const EXT4_S_IFREG: u16 = 0x8000;  // Regular file
const EXT4_S_IFDIR: u16 = 0x4000;  // Directory
const EXT4_S_IFLNK: u16 = 0xA000;  // Symbolic link
const EXT4_S_IFBLK: u16 = 0x6000;  // Block device
const EXT4_S_IFCHR: u16 = 0x2000;  // Character device
const EXT4_S_IFIFO: u16 = 0x1000;  // FIFO
const EXT4_S_IFSOCK: u16 = 0xC000; // Socket

impl Ext4FileSystem {
    /// Create a new ext4 file system instance
    pub fn new() -> Self {
        Self {
            superblock: None,
            block_size: 0,
            inode_size: 0,
            device_id: None,
            mounted: false,
            inode_cache: BTreeMap::new(),
            path_to_inode: BTreeMap::new(),
            next_inode: 100, // Start from inode 100 for user files
        }
    }

    /// Parse the ext4 superblock from raw bytes
    fn parse_superblock(&mut self, data: &[u8]) -> Result<(), VfsError> {
        if data.len() < mem::size_of::<Ext4Superblock>() {
            return Err(VfsError::IoError);
        }

        // Safety: We've verified the buffer is large enough
        let superblock = unsafe {
            core::ptr::read_unaligned(data.as_ptr() as *const Ext4Superblock)
        };

        // Verify ext4 magic number
        if superblock.magic != EXT4_SUPER_MAGIC {
            return Err(VfsError::IoError);
        }

        // Calculate block size
        self.block_size = EXT4_MIN_BLOCK_SIZE << superblock.log_block_size;
        if self.block_size < EXT4_MIN_BLOCK_SIZE || self.block_size > EXT4_MAX_BLOCK_SIZE {
            return Err(VfsError::IoError);
        }

        // Validate inode size
        self.inode_size = if superblock.rev_level >= 1 {
            superblock.inode_size
        } else {
            128 // Default inode size for revision 0
        };

        if self.inode_size < 128 || self.inode_size > self.block_size as u16 {
            return Err(VfsError::IoError);
        }

        self.superblock = Some(superblock);
        Ok(())
    }

    /// Convert ext4 file type to VFS file type
    fn ext4_to_vfs_file_type(ext4_type: u8) -> FileType {
        match ext4_type {
            EXT4_FT_REG_FILE => FileType::Regular,
            EXT4_FT_DIR => FileType::Directory,
            EXT4_FT_SYMLINK => FileType::SymbolicLink,
            EXT4_FT_BLKDEV => FileType::BlockDevice,
            EXT4_FT_CHRDEV => FileType::CharacterDevice,
            EXT4_FT_FIFO => FileType::Fifo,
            EXT4_FT_SOCK => FileType::Socket,
            _ => FileType::Regular,
        }
    }

    /// Convert ext4 inode mode to VFS file type
    fn inode_mode_to_file_type(mode: u16) -> FileType {
        match mode & 0xF000 {
            EXT4_S_IFREG => FileType::Regular,
            EXT4_S_IFDIR => FileType::Directory,
            EXT4_S_IFLNK => FileType::SymbolicLink,
            EXT4_S_IFBLK => FileType::BlockDevice,
            EXT4_S_IFCHR => FileType::CharacterDevice,
            EXT4_S_IFIFO => FileType::Fifo,
            EXT4_S_IFSOCK => FileType::Socket,
            _ => FileType::Regular,
        }
    }

    /// Convert ext4 inode mode to VFS permissions
    fn inode_mode_to_permissions(mode: u16) -> FilePermissions {
        FilePermissions::from_bits_truncate(mode)
    }

    /// Read a block from the device (placeholder implementation)
    fn read_block(&self, block_num: u32, buffer: &mut [u8]) -> Result<(), VfsError> {
        // In a real implementation, this would read from the actual storage device
        // For now, we'll return an error to indicate this is not implemented
        if buffer.len() < self.block_size as usize {
            return Err(VfsError::IoError);
        }
        
        // Placeholder: fill with zeros
        buffer[..self.block_size as usize].fill(0);
        Ok(())
    }

    /// Write a block to the device (placeholder implementation)
    fn write_block(&self, _block_num: u32, _buffer: &[u8]) -> Result<(), VfsError> {
        // In a real implementation, this would write to the actual storage device
        // For now, we'll return success as a placeholder
        Ok(())
    }

    /// Read an inode from disk
    fn read_inode(&mut self, inode_num: InodeNumber) -> Result<Ext4Inode, VfsError> {
        // Check cache first
        if let Some(inode) = self.inode_cache.get(&inode_num) {
            return Ok(*inode);
        }

        let superblock = self.superblock.ok_or(VfsError::NotMounted)?;
        
        // Calculate inode location
        let group = (inode_num - 1) / superblock.inodes_per_group as u64;
        let index = (inode_num - 1) % superblock.inodes_per_group as u64;
        
        // In a real implementation, we would:
        // 1. Read the block group descriptor to get the inode table location
        // 2. Calculate the exact block and offset within the block
        // 3. Read the block and extract the inode
        
        // For now, return a placeholder inode
        let inode = Ext4Inode {
            mode: EXT4_S_IFREG | 0o644,
            uid: 0,
            size_lo: 0,
            atime: 0,
            ctime: 0,
            mtime: 0,
            dtime: 0,
            gid: 0,
            links_count: 1,
            blocks_lo: 0,
            flags: 0,
            osd1: 0,
            block: [0; 15],
            generation: 0,
            file_acl_lo: 0,
            size_high: 0,
            obso_faddr: 0,
            osd2: [0; 12],
            extra_isize: 0,
            checksum_hi: 0,
            ctime_extra: 0,
            mtime_extra: 0,
            atime_extra: 0,
            crtime: 0,
            crtime_extra: 0,
            version_hi: 0,
            projid: 0,
        };

        // Cache the inode
        self.inode_cache.insert(inode_num, inode);
        Ok(inode)
    }

    /// Convert ext4 inode to VFS metadata
    fn inode_to_metadata(&self, inode_num: InodeNumber, inode: &Ext4Inode) -> FileMetadata {
        let file_size = (inode.size_high as u64) << 32 | inode.size_lo as u64;
        
        FileMetadata {
            inode: inode_num,
            file_type: Self::inode_mode_to_file_type(inode.mode),
            permissions: Self::inode_mode_to_permissions(inode.mode),
            size: file_size,
            uid: inode.uid as u32,
            gid: inode.gid as u32,
            created_time: inode.crtime as u64,
            modified_time: inode.mtime as u64,
            accessed_time: inode.atime as u64,
        }
    }

    /// Resolve a path to an inode number
    fn resolve_path(&mut self, path: &str) -> Result<InodeNumber, VfsError> {
        if path == "/" {
            return Ok(2); // Root inode is typically 2 in ext4
        }

        // Check if we have this path in our path-to-inode mapping
        if let Some(&inode_num) = self.path_to_inode.get(path) {
            return Ok(inode_num);
        }
        
        // If not found, return NotFound error
        Err(VfsError::NotFound)
    }
}

impl FileSystem for Ext4FileSystem {
    /// Initialize the ext4 file system
    fn init(&mut self) -> Result<(), VfsError> {
        // Reset state
        self.superblock = None;
        self.block_size = 0;
        self.inode_size = 0;
        self.mounted = false;
        self.inode_cache.clear();
        self.path_to_inode.clear();
        self.next_inode = 100;
        Ok(())
    }

    /// Mount the ext4 file system
    fn mount(&mut self, device_id: Option<u32>) -> Result<(), VfsError> {
        if self.mounted {
            return Err(VfsError::MountPointBusy);
        }

        self.device_id = device_id;

        // Read and parse the superblock
        let mut superblock_data = vec![0u8; mem::size_of::<Ext4Superblock>()];
        
        // In a real implementation, we would read from the actual device at offset 1024
        // For now, we'll create a minimal valid superblock for testing
        let test_superblock = Ext4Superblock {
            inodes_count: 1000,
            blocks_count: 10000,
            r_blocks_count: 500,
            free_blocks_count: 8000,
            free_inodes_count: 900,
            first_data_block: 1,
            log_block_size: 0, // 1024 byte blocks
            log_cluster_size: 0,
            blocks_per_group: 8192,
            clusters_per_group: 8192,
            inodes_per_group: 256,
            mtime: 0,
            wtime: 0,
            mnt_count: 1,
            max_mnt_count: 20,
            magic: EXT4_SUPER_MAGIC,
            state: 1, // Clean
            errors: 1, // Continue on errors
            minor_rev_level: 0,
            lastcheck: 0,
            checkinterval: 0,
            creator_os: 0,
            rev_level: 1,
            def_resuid: 0,
            def_resgid: 0,
            first_ino: 11,
            inode_size: 256,
            block_group_nr: 0,
            feature_compat: 0,
            feature_incompat: 0,
            feature_ro_compat: 0,
            uuid: [0; 16],
            volume_name: [0; 16],
            last_mounted: [0; 64],
            algorithm_usage_bitmap: 0,
        };

        // Copy test superblock to buffer
        unsafe {
            core::ptr::copy_nonoverlapping(
                &test_superblock as *const _ as *const u8,
                superblock_data.as_mut_ptr(),
                mem::size_of::<Ext4Superblock>()
            );
        }

        self.parse_superblock(&superblock_data)?;
        self.mounted = true;
        Ok(())
    }

    /// Unmount the ext4 file system
    fn unmount(&mut self) -> Result<(), VfsError> {
        if !self.mounted {
            return Err(VfsError::NotMounted);
        }

        // Sync any pending changes
        self.sync()?;
        
        // Clear state
        self.superblock = None;
        self.block_size = 0;
        self.inode_size = 0;
        self.device_id = None;
        self.mounted = false;
        self.inode_cache.clear();
        self.path_to_inode.clear();
        self.next_inode = 100;
        
        Ok(())
    }

    /// Open a file and return its inode and metadata
    fn open(&mut self, path: &str, _flags: OpenFlags) -> Result<(InodeNumber, FileMetadata), VfsError> {
        if !self.mounted {
            return Err(VfsError::NotMounted);
        }

        let inode_num = self.resolve_path(path)?;
        let inode = self.read_inode(inode_num)?;
        let metadata = self.inode_to_metadata(inode_num, &inode);
        
        Ok((inode_num, metadata))
    }

    /// Close a file
    fn close(&mut self, _inode: InodeNumber) -> Result<(), VfsError> {
        if !self.mounted {
            return Err(VfsError::NotMounted);
        }
        
        // In a real implementation, we might flush any cached data
        // and update access times
        Ok(())
    }

    /// Read data from a file
    fn read(&mut self, inode_num: InodeNumber, offset: FileOffset, buffer: &mut [u8]) -> Result<usize, VfsError> {
        if !self.mounted {
            return Err(VfsError::NotMounted);
        }

        let inode = self.read_inode(inode_num)?;
        let file_size = (inode.size_high as u64) << 32 | inode.size_lo as u64;
        
        // Check if offset is beyond file size
        if offset >= file_size {
            return Ok(0);
        }

        // Calculate how much we can actually read
        let bytes_to_read = core::cmp::min(buffer.len() as u64, file_size - offset) as usize;
        
        // In a real implementation, we would:
        // 1. Calculate which blocks contain the requested data
        // 2. Read the blocks from disk
        // 3. Copy the relevant portions to the buffer
        
        // For now, fill with placeholder data
        buffer[..bytes_to_read].fill(0x42); // Fill with 'B' for testing
        
        Ok(bytes_to_read)
    }

    /// Write data to a file
    fn write(&mut self, inode_num: InodeNumber, offset: FileOffset, buffer: &[u8]) -> Result<usize, VfsError> {
        if !self.mounted {
            return Err(VfsError::NotMounted);
        }

        let mut inode = self.read_inode(inode_num)?;
        
        // Check if this is a regular file
        if Self::inode_mode_to_file_type(inode.mode) != FileType::Regular {
            return Err(VfsError::PermissionDenied);
        }

        // In a real implementation, we would:
        // 1. Allocate new blocks if needed
        // 2. Update the inode's block pointers
        // 3. Write the data to the allocated blocks
        // 4. Update the inode size and modification time
        // 5. Write the updated inode back to disk

        // For now, just update the size if we're extending the file
        let new_size = offset + buffer.len() as u64;
        let current_size = (inode.size_high as u64) << 32 | inode.size_lo as u64;
        
        if new_size > current_size {
            inode.size_lo = new_size as u32;
            inode.size_high = (new_size >> 32) as u32;
            
            // Update modification time (placeholder)
            inode.mtime = 1234567890; // Placeholder timestamp
            
            // Update cache
            self.inode_cache.insert(inode_num, inode);
        }

        Ok(buffer.len())
    }

    /// Create a new file
    fn create(&mut self, path: &str, file_type: FileType, permissions: FilePermissions) -> Result<InodeNumber, VfsError> {
        if !self.mounted {
            return Err(VfsError::NotMounted);
        }

        // Check if the path already exists
        if self.path_to_inode.contains_key(path) {
            return Err(VfsError::AlreadyExists);
        }

        // In a real implementation, we would:
        // 1. Find a free inode
        // 2. Initialize the inode with the specified type and permissions
        // 3. Add a directory entry in the parent directory
        // 4. Update the superblock free inode count

        // Allocate a new inode number
        let new_inode_num = self.next_inode;
        self.next_inode += 1;

        // Create a basic inode structure
        let mode = match file_type {
            FileType::Regular => EXT4_S_IFREG,
            FileType::Directory => EXT4_S_IFDIR,
            FileType::SymbolicLink => EXT4_S_IFLNK,
            FileType::BlockDevice => EXT4_S_IFBLK,
            FileType::CharacterDevice => EXT4_S_IFCHR,
            FileType::Fifo => EXT4_S_IFIFO,
            FileType::Socket => EXT4_S_IFSOCK,
        } | permissions.bits();

        let new_inode = Ext4Inode {
            mode,
            uid: 0, // Current user ID (placeholder)
            size_lo: 0,
            atime: 1234567890, // Placeholder timestamp
            ctime: 1234567890,
            mtime: 1234567890,
            dtime: 0,
            gid: 0, // Current group ID (placeholder)
            links_count: 1,
            blocks_lo: 0,
            flags: 0,
            osd1: 0,
            block: [0; 15],
            generation: 1,
            file_acl_lo: 0,
            size_high: 0,
            obso_faddr: 0,
            osd2: [0; 12],
            extra_isize: 0,
            checksum_hi: 0,
            ctime_extra: 0,
            mtime_extra: 0,
            atime_extra: 0,
            crtime: 1234567890,
            crtime_extra: 0,
            version_hi: 0,
            projid: 0,
        };

        // Cache the new inode and add path mapping
        self.inode_cache.insert(new_inode_num, new_inode);
        self.path_to_inode.insert(path.to_string(), new_inode_num);

        Ok(new_inode_num)
    }

    /// Delete a file
    fn unlink(&mut self, path: &str) -> Result<(), VfsError> {
        if !self.mounted {
            return Err(VfsError::NotMounted);
        }

        let inode_num = self.resolve_path(path)?;
        let inode = self.read_inode(inode_num)?;

        // Check if it's a directory
        if Self::inode_mode_to_file_type(inode.mode) == FileType::Directory {
            return Err(VfsError::IsDirectory);
        }

        // In a real implementation, we would:
        // 1. Remove the directory entry from the parent directory
        // 2. Decrement the inode link count
        // 3. If link count reaches 0, free the inode and its blocks
        // 4. Update the superblock free counts

        // For now, just remove from cache
        self.inode_cache.remove(&inode_num);

        Ok(())
    }

    /// Get file metadata
    fn stat(&mut self, path: &str) -> Result<FileMetadata, VfsError> {
        if !self.mounted {
            return Err(VfsError::NotMounted);
        }

        let inode_num = self.resolve_path(path)?;
        let inode = self.read_inode(inode_num)?;
        Ok(self.inode_to_metadata(inode_num, &inode))
    }

    /// Read directory entries
    fn readdir(&mut self, path: &str) -> Result<Vec<DirectoryEntry>, VfsError> {
        if !self.mounted {
            return Err(VfsError::NotMounted);
        }

        let inode_num = self.resolve_path(path)?;
        let inode = self.read_inode(inode_num)?;

        // Check if it's a directory
        if Self::inode_mode_to_file_type(inode.mode) != FileType::Directory {
            return Err(VfsError::NotDirectory);
        }

        // In a real implementation, we would:
        // 1. Read the directory's data blocks
        // 2. Parse the directory entries
        // 3. Convert them to VFS DirectoryEntry format

        // For now, return some placeholder entries
        let mut entries = Vec::new();
        
        // Add "." entry
        let mut dot_name = [0u8; 256];
        dot_name[0] = b'.';
        entries.push(DirectoryEntry {
            name: dot_name,
            name_len: 1,
            inode: inode_num,
            file_type: FileType::Directory,
        });

        // Add ".." entry
        let mut dotdot_name = [0u8; 256];
        dotdot_name[0] = b'.';
        dotdot_name[1] = b'.';
        entries.push(DirectoryEntry {
            name: dotdot_name,
            name_len: 2,
            inode: 2, // Parent directory (root for now)
            file_type: FileType::Directory,
        });

        Ok(entries)
    }

    /// Create a directory
    fn mkdir(&mut self, path: &str, permissions: FilePermissions) -> Result<(), VfsError> {
        if !self.mounted {
            return Err(VfsError::NotMounted);
        }

        // Create the directory inode
        let _inode_num = self.create(path, FileType::Directory, permissions)?;

        // In a real implementation, we would also:
        // 1. Initialize the directory with "." and ".." entries
        // 2. Update the parent directory's link count

        Ok(())
    }

    /// Remove a directory
    fn rmdir(&mut self, path: &str) -> Result<(), VfsError> {
        if !self.mounted {
            return Err(VfsError::NotMounted);
        }

        let inode_num = self.resolve_path(path)?;
        let inode = self.read_inode(inode_num)?;

        // Check if it's a directory
        if Self::inode_mode_to_file_type(inode.mode) != FileType::Directory {
            return Err(VfsError::NotDirectory);
        }

        // In a real implementation, we would:
        // 1. Check if the directory is empty (only contains "." and "..")
        // 2. Remove the directory entry from the parent
        // 3. Free the directory inode and blocks
        // 4. Update parent directory's link count

        // For now, just remove from cache
        self.inode_cache.remove(&inode_num);

        Ok(())
    }

    /// Sync file system data to storage
    fn sync(&mut self) -> Result<(), VfsError> {
        if !self.mounted {
            return Err(VfsError::NotMounted);
        }

        // In a real implementation, we would:
        // 1. Write all dirty inodes to disk
        // 2. Write all dirty data blocks to disk
        // 3. Update the superblock
        // 4. Ensure all writes are committed to storage

        // For now, this is a no-op
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ext4_creation() {
        let fs = Ext4FileSystem::new();
        assert!(!fs.mounted);
        assert!(fs.superblock.is_none());
        assert_eq!(fs.block_size, 0);
    }

    #[test]
    fn test_ext4_mount() {
        let mut fs = Ext4FileSystem::new();
        assert!(fs.mount(Some(1)).is_ok());
        assert!(fs.mounted);
        assert!(fs.superblock.is_some());
        assert_eq!(fs.block_size, 1024);
    }

    #[test]
    fn test_ext4_unmount() {
        let mut fs = Ext4FileSystem::new();
        assert!(fs.mount(Some(1)).is_ok());
        assert!(fs.unmount().is_ok());
        assert!(!fs.mounted);
        assert!(fs.superblock.is_none());
    }

    #[test]
    fn test_file_type_conversion() {
        assert_eq!(Ext4FileSystem::ext4_to_vfs_file_type(EXT4_FT_REG_FILE), FileType::Regular);
        assert_eq!(Ext4FileSystem::ext4_to_vfs_file_type(EXT4_FT_DIR), FileType::Directory);
        assert_eq!(Ext4FileSystem::ext4_to_vfs_file_type(EXT4_FT_SYMLINK), FileType::SymbolicLink);
    }

    #[test]
    fn test_inode_mode_conversion() {
        assert_eq!(Ext4FileSystem::inode_mode_to_file_type(EXT4_S_IFREG), FileType::Regular);
        assert_eq!(Ext4FileSystem::inode_mode_to_file_type(EXT4_S_IFDIR), FileType::Directory);
        assert_eq!(Ext4FileSystem::inode_mode_to_file_type(EXT4_S_IFLNK), FileType::SymbolicLink);
    }

    #[test]
    fn test_create_file() {
        let mut fs = Ext4FileSystem::new();
        assert!(fs.mount(Some(1)).is_ok());
        
        let inode_num = fs.create("/test.txt", FileType::Regular, FilePermissions::OWNER_READ | FilePermissions::OWNER_WRITE);
        assert!(inode_num.is_ok());
        
        let inode = fs.read_inode(inode_num.unwrap());
        assert!(inode.is_ok());
    }

    #[test]
    fn test_read_write() {
        let mut fs = Ext4FileSystem::new();
        assert!(fs.mount(Some(1)).is_ok());
        
        let inode_num = fs.create("/test.txt", FileType::Regular, FilePermissions::OWNER_READ | FilePermissions::OWNER_WRITE).unwrap();
        
        // Test write
        let data = b"Hello, ext4!";
        let written = fs.write(inode_num, 0, data);
        assert!(written.is_ok());
        assert_eq!(written.unwrap(), data.len());
        
        // Test read
        let mut buffer = vec![0u8; data.len()];
        let read = fs.read(inode_num, 0, &mut buffer);
        assert!(read.is_ok());
        assert_eq!(read.unwrap(), data.len());
    }
}