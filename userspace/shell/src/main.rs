#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::format;
use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

mod commands;
mod input;
mod output;
mod error;
mod types;

use commands::CommandProcessor;
use input::InputHandler;
use output::OutputHandler;
use error::ShellResult;

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
    input_handler: InputHandler,
    output_handler: OutputHandler,
    running: bool,
}

impl KoshShell {
    fn new() -> Self {
        Self {
            input_handler: InputHandler::new(),
            output_handler: OutputHandler::new(),
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
                    self.output_handler.print_line(&error.user_message());
                    if let Some(suggestion) = error.suggest_fix() {
                        self.output_handler.print_line(&format!("Suggestion: {}", suggestion));
                    }
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
    
    fn process_shell_command(&mut self, command_line: &str) -> ShellResult<String> {
        let mut command_processor = CommandProcessor::new();
        command_processor.process_command(command_line)
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

// Conditional panic handler - only when not testing
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    debug_print(b"Shell: PANIC occurred!\n");
    sys_exit(1);
}