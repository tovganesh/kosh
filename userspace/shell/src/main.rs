#![no_std]
#![no_main]

extern crate alloc;

use core::panic::PanicInfo;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;
use linked_list_allocator::LockedHeap;
use kosh_service::ServiceClient;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

mod commands;
mod input;
mod output;

use commands::CommandProcessor;
use input::InputHandler;
use output::OutputHandler;

/// Basic shell for testing the Kosh operating system
#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Initialize heap allocator
    init_heap();
    
    debug_print(b"Shell: Starting Kosh shell\n");
    
    let mut shell = KoshShell::new();
    shell.run();
}

struct KoshShell {
    command_processor: CommandProcessor,
    input_handler: InputHandler,
    output_handler: OutputHandler,
    service_client: ServiceClient,
    running: bool,
}

impl KoshShell {
    fn new() -> Self {
        Self {
            command_processor: CommandProcessor::new(),
            input_handler: InputHandler::new(),
            output_handler: OutputHandler::new(),
            service_client: ServiceClient::new(),
            running: true,
        }
    }
    
    fn run(&mut self) -> ! {
        // Print welcome message
        self.output_handler.print_line("Kosh Shell v0.1.0");
        self.output_handler.print_line("Type 'help' for available commands");
        self.output_handler.print_line("Available commands: help, ls, cat, echo, ps, drivers, exit");
        
        // Main shell loop
        while self.running {
            // Print prompt
            self.output_handler.print("kosh> ");
            
            // Read command line
            let command_line = self.input_handler.read_line();
            
            // Process command
            match self.process_shell_command(&command_line) {
                Ok(output) => {
                    if !output.is_empty() {
                        self.output_handler.print_line(&output);
                    }
                }
                Err(error) => {
                    self.output_handler.print_line(&format!("Error: {}", error));
                }
            }
            
            // Check if exit was requested
            if command_line.trim() == "exit" {
                self.running = false;
            }
        }
        
        self.output_handler.print_line("Shell exiting...");
        
        // Exit the shell process
        sys_exit(0);
    }
    
    fn process_shell_command(&mut self, command_line: &str) -> Result<String, String> {
        let parts: Vec<&str> = command_line.trim().split_whitespace().collect();
        if parts.is_empty() {
            return Ok(String::new());
        }
        
        let command = parts[0];
        let args = &parts[1..];
        
        match command {
            "help" => {
                Ok(String::from("Available commands:\n  help - Show this help\n  ls [path] - List directory contents\n  cat <file> - Display file contents\n  echo <text> - Print text\n  ps - List processes\n  drivers - List loaded drivers\n  exit - Exit shell"))
            }
            "ls" => {
                let path = args.get(0).unwrap_or(&"/");
                self.list_directory(path)
            }
            "cat" => {
                if args.is_empty() {
                    Err(String::from("Usage: cat <filename>"))
                } else {
                    self.read_file(args[0])
                }
            }
            "echo" => {
                Ok(args.join(" "))
            }
            "ps" => {
                Ok(String::from("Process list not implemented yet"))
            }
            "drivers" => {
                self.list_drivers()
            }
            "exit" => {
                Ok(String::new())
            }
            _ => {
                Err(format!("Unknown command: {}", command))
            }
        }
    }
    
    fn list_directory(&mut self, path: &str) -> Result<String, String> {
        // This would send a request to the file system service
        // For now, return a mock response
        debug_print(b"Shell: Listing directory\n");
        
        // In a real implementation, this would:
        // 1. Find the file system service PID
        // 2. Send a FileSystemRequest::List message
        // 3. Wait for and parse the response
        
        Ok(format!("Contents of {}:\n  file1.txt\n  file2.txt\n  subdir/", path))
    }
    
    fn read_file(&mut self, filename: &str) -> Result<String, String> {
        debug_print(b"Shell: Reading file\n");
        
        // Mock file reading - in a real implementation this would
        // communicate with the file system service
        Ok(format!("Contents of file: {}\n(This is mock content)", filename))
    }
    
    fn list_drivers(&mut self) -> Result<String, String> {
        debug_print(b"Shell: Listing drivers\n");
        
        // This would send a request to the driver manager service
        // For now, return a mock response
        Ok(String::from("Loaded drivers:\n  graphics.ko (ID: 1)\n  keyboard.ko (ID: 2)\n  storage.ko (ID: 3)"))
    }
}

fn init_heap() {
    const HEAP_SIZE: usize = 32 * 1024; // 32KB heap for shell
    static mut HEAP_MEMORY: [u8; 32 * 1024] = [0; 32 * 1024];
    
    unsafe {
        let heap_ptr = core::ptr::addr_of_mut!(HEAP_MEMORY);
        ALLOCATOR.lock().init((*heap_ptr).as_mut_ptr(), HEAP_SIZE);
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

/// Exit system call
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

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    debug_print(b"Shell: PANIC occurred!\n");
    sys_exit(1);
}