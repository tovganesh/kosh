#![no_std]
#![no_main]

extern crate alloc;

use kosh_fs_service::{Vfs, FileSystemType, handle_fs_request, FsRequest};
use kosh_types::VfsError;

// Global allocator setup
use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

static mut VFS_INSTANCE: Option<Vfs> = None;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Initialize heap allocator
    unsafe {
        let heap_start = 0x_4444_4444_0000u64;
        let heap_size = 100 * 1024; // 100 KB heap
        ALLOCATOR.lock().init(heap_start as *mut u8, heap_size);
    }
    
    // Initialize VFS
    let mut vfs = Vfs::new();
    
    // Mount root filesystem
    if let Err(e) = vfs.mount("/", FileSystemType::Ext4, None, false) {
        // Handle mount error - in a real system this would be logged
        panic!("Failed to mount root filesystem: {:?}", e);
    }
    
    // Store VFS instance globally
    unsafe {
        VFS_INSTANCE = Some(vfs);
    }
    
    // File system service main loop
    loop {
        // In a real implementation, this would:
        // 1. Listen for IPC messages from other processes
        // 2. Handle file system requests (open, read, write, etc.)
        // 3. Delegate to appropriate file system implementations
        // 4. Return responses via IPC
        
        // For now, just idle
        core::hint::spin_loop();
    }
}

/// Handle file system service requests from IPC
pub fn handle_service_request(request: FsRequest) -> Result<kosh_fs_service::FsResponse, VfsError> {
    unsafe {
        let vfs = VFS_INSTANCE.as_mut().ok_or(VfsError::IoError)?;
        handle_fs_request(vfs, request)
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}