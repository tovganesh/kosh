//! x86-64 register definitions and operations

/// x86-64 CPU registers structure
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct X86_64Registers {
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
    
    // Instruction pointer and flags
    pub rip: u64,
    pub rflags: u64,
    
    // Segment registers
    pub cs: u16,
    pub ds: u16,
    pub es: u16,
    pub fs: u16,
    pub gs: u16,
    pub ss: u16,
    
    // Control registers (saved for kernel contexts)
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
}

impl Default for X86_64Registers {
    fn default() -> Self {
        Self {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, rbp: 0, rsp: 0,
            r8: 0, r9: 0, r10: 0, r11: 0,
            r12: 0, r13: 0, r14: 0, r15: 0,
            rip: 0,
            rflags: 0x202, // Default RFLAGS with interrupts enabled
            cs: 0x08,  // Kernel code segment
            ds: 0x10,  // Kernel data segment
            es: 0x10,
            fs: 0x10,
            gs: 0x10,
            ss: 0x10,
            cr0: 0, cr2: 0, cr3: 0, cr4: 0,
        }
    }
}

impl X86_64Registers {
    /// Create a new register set for user mode
    pub fn new_user_mode(entry_point: u64, stack_pointer: u64) -> Self {
        Self {
            rip: entry_point,
            rsp: stack_pointer,
            rflags: 0x202, // Interrupts enabled, reserved bit set
            cs: 0x1B,      // User code segment (GDT entry 3, RPL 3)
            ds: 0x23,      // User data segment (GDT entry 4, RPL 3)
            es: 0x23,
            fs: 0x23,
            gs: 0x23,
            ss: 0x23,
            ..Default::default()
        }
    }
    
    /// Create a new register set for kernel mode
    pub fn new_kernel_mode(entry_point: u64, stack_pointer: u64) -> Self {
        Self {
            rip: entry_point,
            rsp: stack_pointer,
            rflags: 0x202, // Interrupts enabled, reserved bit set
            cs: 0x08,      // Kernel code segment
            ds: 0x10,      // Kernel data segment
            es: 0x10,
            fs: 0x10,
            gs: 0x10,
            ss: 0x10,
            ..Default::default()
        }
    }
    
    /// Set system call arguments (following System V ABI)
    pub fn set_syscall_args(&mut self, args: &[u64]) {
        if args.len() > 0 { self.rdi = args[0]; }
        if args.len() > 1 { self.rsi = args[1]; }
        if args.len() > 2 { self.rdx = args[2]; }
        if args.len() > 3 { self.rcx = args[3]; }
        if args.len() > 4 { self.r8 = args[4]; }
        if args.len() > 5 { self.r9 = args[5]; }
    }
    
    /// Get system call arguments
    pub fn get_syscall_args(&self) -> [u64; 6] {
        [self.rdi, self.rsi, self.rdx, self.rcx, self.r8, self.r9]
    }
    
    /// Set system call return value
    pub fn set_syscall_return(&mut self, value: u64) {
        self.rax = value;
    }
    
    /// Get system call number
    pub fn get_syscall_number(&self) -> u64 {
        self.rax
    }
}