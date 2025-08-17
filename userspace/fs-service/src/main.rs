#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use kosh_fs_service::{Vfs, FileSystemType};
use kosh_types::{OpenFlags, FileType, FilePermissions};
use kosh_service::{ServiceHandler, ServiceMessage, ServiceResponse, ServiceType, ServiceData, ServiceStatus, ServiceRunner, FileSystemRequest};

// Global allocator setup
use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// File System Service Handler
struct FileSystemService {
    vfs: Vfs,
}

impl FileSystemService {
    fn new() -> Self {
        Self {
            vfs: Vfs::new(),
        }
    }
}

impl ServiceHandler for FileSystemService {
    fn handle_request(&mut self, request: ServiceMessage) -> ServiceResponse {
        let response_data = match request.data {
            ServiceData::FileSystemRequest(fs_request) => {
                match fs_request {
                    FileSystemRequest::Open { path, flags } => {
                        // Convert u32 flags to OpenFlags
                        let open_flags = OpenFlags::from_bits_truncate(flags);
                        match self.vfs.open(&path, open_flags) {
                            Ok(fd) => ServiceData::Binary(fd.to_le_bytes().to_vec()),
                            Err(_) => ServiceData::Empty,
                        }
                    }
                    FileSystemRequest::Close { fd } => {
                        match self.vfs.close(fd) {
                            Ok(_) => ServiceData::Empty,
                            Err(_) => ServiceData::Empty,
                        }
                    }
                    FileSystemRequest::Read { fd, size } => {
                        let mut buffer = vec![0u8; size];
                        match self.vfs.read(fd, &mut buffer) {
                            Ok(bytes_read) => {
                                buffer.truncate(bytes_read);
                                ServiceData::Binary(buffer)
                            },
                            Err(_) => ServiceData::Empty,
                        }
                    }
                    FileSystemRequest::Write { fd, data } => {
                        match self.vfs.write(fd, &data) {
                            Ok(bytes_written) => ServiceData::Binary(bytes_written.to_le_bytes().to_vec()),
                            Err(_) => ServiceData::Empty,
                        }
                    }
                    FileSystemRequest::List { path } => {
                        // For now, return a mock directory listing
                        // In a real implementation, this would use VFS methods
                        let result = format!("Contents of {}:\n  file1.txt\n  file2.txt\n  subdir/", path);
                        ServiceData::Text(result)
                    }
                    FileSystemRequest::Create { path, is_directory } => {
                        let file_type = if is_directory { FileType::Directory } else { FileType::Regular };
                        let permissions = FilePermissions::OWNER_READ | FilePermissions::OWNER_WRITE;
                        match self.vfs.create(&path, file_type, permissions) {
                            Ok(_) => ServiceData::Empty,
                            Err(_) => ServiceData::Empty,
                        }
                    }
                    FileSystemRequest::Delete { path } => {
                        // For now, just return success
                        // In a real implementation, this would use VFS delete methods
                        ServiceData::Empty
                    }
                }
            }
            _ => ServiceData::Empty,
        };

        ServiceResponse {
            request_id: request.request_id,
            status: ServiceStatus::Success,
            data: response_data,
        }
    }

    fn get_service_type(&self) -> ServiceType {
        ServiceType::FileSystem
    }

    fn initialize(&mut self) -> Result<(), kosh_service::ServiceError> {
        // Mount root filesystem
        match self.vfs.mount("/", FileSystemType::Ext4, None, false) {
            Ok(_) => {
                debug_print(b"FS Service: Root filesystem mounted\n");
                Ok(())
            }
            Err(_) => {
                debug_print(b"FS Service: Failed to mount root filesystem\n");
                Err(kosh_service::ServiceError::InvalidRequest)
            }
        }
    }

    fn shutdown(&mut self) -> Result<(), kosh_service::ServiceError> {
        debug_print(b"FS Service: Shutting down\n");
        // Unmount filesystems and cleanup
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Initialize heap allocator
    init_heap();
    
    debug_print(b"FS Service: Starting file system service\n");
    
    // Create and start the file system service
    let fs_service = FileSystemService::new();
    let mut service_runner = ServiceRunner::new(fs_service);
    
    // Initialize the service
    if let Err(_) = service_runner.start() {
        debug_print(b"FS Service: Failed to start service\n");
        sys_exit(1);
    }
    
    debug_print(b"FS Service: Service started, entering main loop\n");
    
    // Main service loop
    loop {
        // Process incoming requests
        if let Err(_) = service_runner.run_once() {
            debug_print(b"FS Service: Error processing request\n");
        }
        
        // Yield CPU to prevent busy waiting
        yield_cpu();
    }
}

fn init_heap() {
    const HEAP_SIZE: usize = 128 * 1024; // 128KB heap for FS service
    static mut HEAP_MEMORY: [u8; 128 * 1024] = [0; 128 * 1024];
    
    unsafe {
        let heap_ptr = core::ptr::addr_of_mut!(HEAP_MEMORY);
        ALLOCATOR.lock().init((*heap_ptr).as_mut_ptr(), HEAP_SIZE);
    }
}

fn yield_cpu() {
    for _ in 0..1000 {
        core::hint::spin_loop();
    }
}

fn debug_print(message: &[u8]) {
    #[cfg(debug_assertions)]
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 100u64, // SYS_DEBUG_PRINT
            in("rdi") message.as_ptr(),
            in("rsi") message.len(),
            options(nostack, preserves_flags)
        );
    }
}

fn sys_exit(status: i32) -> ! {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 1u64, // SYS_EXIT
            in("rdi") status,
            options(noreturn)
        );
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    debug_print(b"FS Service: PANIC occurred!\n");
    sys_exit(1);
}