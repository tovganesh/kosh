use core::arch::asm;
use crate::{serial_println, println};

/// CPU context for x86-64 architecture
/// This structure represents the saved state of a process
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct CpuContext {
    // General purpose registers
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    
    // Control registers
    pub rip: u64,
    pub rflags: u64,
    
    // Segment selectors
    pub cs: u16,
    pub ds: u16,
    pub es: u16,
    pub fs: u16,
    pub gs: u16,
    pub ss: u16,
    
    // Padding for alignment
    _padding: [u16; 1],
}

impl CpuContext {
    /// Create a new CPU context with default values
    pub fn new() -> Self {
        Self {
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            rsp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rip: 0,
            rflags: 0x202, // Default RFLAGS with interrupts enabled
            cs: 0x08,      // Kernel code segment
            ds: 0x10,      // Kernel data segment
            es: 0x10,
            fs: 0x10,
            gs: 0x10,
            ss: 0x10,
            _padding: [0],
        }
    }
    
    /// Create a new CPU context for a kernel thread
    pub fn new_kernel_thread(entry_point: u64, stack_pointer: u64) -> Self {
        let mut context = Self::new();
        context.rip = entry_point;
        context.rsp = stack_pointer;
        context.rbp = stack_pointer;
        context
    }
    
    /// Create a new CPU context for a user process
    pub fn new_user_process(entry_point: u64, stack_pointer: u64) -> Self {
        let mut context = Self::new();
        context.rip = entry_point;
        context.rsp = stack_pointer;
        context.rbp = stack_pointer;
        context.cs = 0x1B;  // User code segment (GDT index 3, RPL 3)
        context.ds = 0x23;  // User data segment (GDT index 4, RPL 3)
        context.es = 0x23;
        context.fs = 0x23;
        context.gs = 0x23;
        context.ss = 0x23;
        context.rflags = 0x202; // Enable interrupts
        context
    }
    
    /// Set the instruction pointer
    pub fn set_instruction_pointer(&mut self, rip: u64) {
        self.rip = rip;
    }
    
    /// Set the stack pointer
    pub fn set_stack_pointer(&mut self, rsp: u64) {
        self.rsp = rsp;
        self.rbp = rsp; // Also set base pointer for consistency
    }
    
    /// Get the instruction pointer
    pub fn instruction_pointer(&self) -> u64 {
        self.rip
    }
    
    /// Get the stack pointer
    pub fn stack_pointer(&self) -> u64 {
        self.rsp
    }
    
    /// Print context information for debugging
    pub fn print_debug(&self) {
        serial_println!("CPU Context:");
        serial_println!("  RAX: 0x{:016x}  RBX: 0x{:016x}  RCX: 0x{:016x}  RDX: 0x{:016x}", 
                       self.rax, self.rbx, self.rcx, self.rdx);
        serial_println!("  RSI: 0x{:016x}  RDI: 0x{:016x}  RBP: 0x{:016x}  RSP: 0x{:016x}", 
                       self.rsi, self.rdi, self.rbp, self.rsp);
        serial_println!("  R8:  0x{:016x}  R9:  0x{:016x}  R10: 0x{:016x}  R11: 0x{:016x}", 
                       self.r8, self.r9, self.r10, self.r11);
        serial_println!("  R12: 0x{:016x}  R13: 0x{:016x}  R14: 0x{:016x}  R15: 0x{:016x}", 
                       self.r12, self.r13, self.r14, self.r15);
        serial_println!("  RIP: 0x{:016x}  RFLAGS: 0x{:016x}", self.rip, self.rflags);
        serial_println!("  CS: 0x{:04x}  DS: 0x{:04x}  ES: 0x{:04x}  FS: 0x{:04x}  GS: 0x{:04x}  SS: 0x{:04x}", 
                       self.cs, self.ds, self.es, self.fs, self.gs, self.ss);
    }
}

impl Default for CpuContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Context switching functionality
pub struct ContextSwitcher;

impl ContextSwitcher {
    /// Perform a context switch from the current process to a new process
    /// 
    /// # Safety
    /// This function is unsafe because it directly manipulates CPU registers
    /// and stack pointers. The caller must ensure that:
    /// - The old_context pointer is valid and points to writable memory
    /// - The new_context contains valid register values
    /// - The new stack pointer is valid and properly aligned
    pub unsafe fn switch_context(old_context: *mut CpuContext, new_context: &CpuContext) {
        serial_println!("Performing context switch from 0x{:p} to RIP 0x{:016x}", 
                       old_context, new_context.rip);
        
        // For now, this is a simplified implementation that just copies the context
        // In a real implementation, this would be done in assembly with precise
        // register manipulation and stack switching
        
        // Save current context (simplified - just save what we can)
        Self::save_current_context(old_context);
        
        // Load new context (simplified - just restore registers)
        Self::restore_context(new_context);
    }
    
    /// Save the current CPU context
    /// 
    /// # Safety
    /// This function is unsafe because it directly accesses CPU registers.
    /// The caller must ensure that the context pointer is valid and writable.
    pub unsafe fn save_current_context(context: *mut CpuContext) {
        asm!(
            "mov [{}], rax",
            "mov [{}+8], rbx",
            "mov [{}+16], rcx",
            "mov [{}+24], rdx",
            "mov [{}+32], rsi",
            "mov [{}+40], rdi",
            "mov [{}+48], rbp",
            "mov [{}+56], rsp",
            "mov [{}+64], r8",
            "mov [{}+72], r9",
            "mov [{}+80], r10",
            "mov [{}+88], r11",
            "mov [{}+96], r12",
            "mov [{}+104], r13",
            "mov [{}+112], r14",
            "mov [{}+120], r15",
            
            // Save return address as RIP
            "mov rax, [rsp]",
            "mov [{}+128], rax",
            
            // Save RFLAGS
            "pushfq",
            "pop rax",
            "mov [{}+136], rax",
            
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            in(reg) context,
            out("rax") _,
        );
    }
    
    /// Restore a CPU context
    /// 
    /// # Safety
    /// This function is unsafe because it directly modifies CPU registers
    /// and may jump to arbitrary code. The caller must ensure that the
    /// context contains valid values.
    pub unsafe fn restore_context(context: &CpuContext) -> ! {
        asm!(
            // Load general purpose registers
            "mov rax, [{}]",
            "mov rbx, [{}+8]",
            "mov rcx, [{}+16]",
            "mov rdx, [{}+24]",
            "mov rsi, [{}+32]",
            "mov rdi, [{}+40]",
            "mov rbp, [{}+48]",
            "mov r8, [{}+64]",
            "mov r9, [{}+72]",
            "mov r10, [{}+80]",
            "mov r11, [{}+88]",
            "mov r12, [{}+96]",
            "mov r13, [{}+104]",
            "mov r14, [{}+112]",
            "mov r15, [{}+120]",
            
            // Load RFLAGS
            "mov rax, [{}+136]",
            "push rax",
            "popfq",
            
            // Load stack pointer
            "mov rsp, [{}+56]",
            
            // Jump to instruction pointer
            "jmp [{}+128]",
            
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            in(reg) context as *const CpuContext,
            options(noreturn)
        );
    }
}

/// Test function for context switching (for debugging)
pub fn test_context_switching() {
    serial_println!("Testing context switching functionality...");
    
    // Create test contexts
    let mut context1 = CpuContext::new_kernel_thread(test_function1 as u64, 0x100000);
    let mut context2 = CpuContext::new_kernel_thread(test_function2 as u64, 0x200000);
    
    serial_println!("Created test contexts:");
    context1.print_debug();
    context2.print_debug();
    
    // In a real implementation, we would perform actual context switches here
    // For now, just verify that the contexts were created correctly
    assert_eq!(context1.rip, test_function1 as u64);
    assert_eq!(context2.rip, test_function2 as u64);
    assert_eq!(context1.rsp, 0x100000);
    assert_eq!(context2.rsp, 0x200000);
    
    serial_println!("Context switching test completed successfully");
    println!("Context switching: Test completed");
}

/// Test function 1 for context switching
extern "C" fn test_function1() {
    serial_println!("Executing test function 1");
}

/// Test function 2 for context switching
extern "C" fn test_function2() {
    serial_println!("Executing test function 2");
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test_case]
    fn test_cpu_context_creation() {
        let context = CpuContext::new();
        assert_eq!(context.rax, 0);
        assert_eq!(context.rflags, 0x202);
        assert_eq!(context.cs, 0x08);
        assert_eq!(context.ds, 0x10);
    }
    
    #[test_case]
    fn test_kernel_thread_context() {
        let entry_point = 0x1000;
        let stack_pointer = 0x2000;
        let context = CpuContext::new_kernel_thread(entry_point, stack_pointer);
        
        assert_eq!(context.rip, entry_point);
        assert_eq!(context.rsp, stack_pointer);
        assert_eq!(context.rbp, stack_pointer);
        assert_eq!(context.cs, 0x08); // Kernel code segment
    }
    
    #[test_case]
    fn test_user_process_context() {
        let entry_point = 0x1000;
        let stack_pointer = 0x2000;
        let context = CpuContext::new_user_process(entry_point, stack_pointer);
        
        assert_eq!(context.rip, entry_point);
        assert_eq!(context.rsp, stack_pointer);
        assert_eq!(context.rbp, stack_pointer);
        assert_eq!(context.cs, 0x1B); // User code segment
        assert_eq!(context.ds, 0x23); // User data segment
    }
    
    #[test_case]
    fn test_context_setters() {
        let mut context = CpuContext::new();
        
        context.set_instruction_pointer(0x5000);
        context.set_stack_pointer(0x6000);
        
        assert_eq!(context.instruction_pointer(), 0x5000);
        assert_eq!(context.stack_pointer(), 0x6000);
        assert_eq!(context.rbp, 0x6000); // Base pointer should also be set
    }
}