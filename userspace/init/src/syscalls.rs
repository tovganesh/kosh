use kosh_types::ProcessId;

/// System call wrapper functions for the init process

/// Exit the current process with the given status code
pub fn sys_exit(status: i32) -> ! {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 1u64, // SYS_EXIT
            in("rdi") status,
            options(noreturn)
        );
    }
}

/// Get the current process ID
pub fn sys_getpid() -> ProcessId {
    let pid: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 5u64, // SYS_GETPID
            lateout("rax") pid,
            options(nostack, preserves_flags)
        );
    }
    pid as ProcessId
}

/// Fork the current process, creating a new child process
pub fn sys_fork() -> Result<ProcessId, i32> {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 2u64, // SYS_FORK
            lateout("rax") result,
            options(nostack, preserves_flags)
        );
    }
    
    if result < 0 {
        Err(result as i32)
    } else {
        Ok(result as ProcessId)
    }
}

/// Execute a new program in the current process
pub fn sys_exec(path: &str, args: &[&str]) -> Result<(), i32> {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 3u64, // SYS_EXEC
            in("rdi") path.as_ptr(),
            in("rsi") path.len(),
            in("rdx") args.as_ptr(),
            in("r10") args.len(),
            lateout("rax") result,
            options(nostack, preserves_flags)
        );
    }
    
    if result < 0 {
        Err(result as i32)
    } else {
        Ok(())
    }
}

/// Wait for a child process to exit
pub fn sys_wait() -> Result<(ProcessId, i32), i32> {
    let pid: i64;
    let status: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 4u64, // SYS_WAIT
            lateout("rax") pid,
            lateout("rdx") status,
            options(nostack, preserves_flags)
        );
    }
    
    if pid < 0 {
        Err(pid as i32)
    } else {
        Ok((pid as ProcessId, status as i32))
    }
}

/// Send a signal to a process
pub fn sys_kill(pid: ProcessId, signal: i32) -> Result<(), i32> {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 7u64, // SYS_KILL
            in("rdi") pid,
            in("rsi") signal,
            lateout("rax") result,
            options(nostack, preserves_flags)
        );
    }
    
    if result < 0 {
        Err(result as i32)
    } else {
        Ok(())
    }
}

/// Debug print function (only available in debug builds)
#[cfg(debug_assertions)]
pub fn sys_debug_print(message: &[u8]) {
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

/// Send an IPC message to another process
pub fn sys_send_message(receiver: ProcessId, data: &[u8]) -> Result<(), i32> {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 30u64, // SYS_SEND_MESSAGE
            in("rdi") receiver,
            in("rsi") data.as_ptr(),
            in("rdx") data.len(),
            lateout("rax") result,
            options(nostack, preserves_flags)
        );
    }
    
    if result < 0 {
        Err(result as i32)
    } else {
        Ok(())
    }
}

/// Receive an IPC message (blocking)
pub fn sys_receive_message(buffer: &mut [u8]) -> Result<(ProcessId, usize), i32> {
    let sender: i64;
    let length: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 31u64, // SYS_RECEIVE_MESSAGE
            in("rdi") buffer.as_mut_ptr(),
            in("rsi") buffer.len(),
            lateout("rax") sender,
            lateout("rdx") length,
            options(nostack, preserves_flags)
        );
    }
    
    if sender < 0 {
        Err(sender as i32)
    } else {
        Ok((sender as ProcessId, length as usize))
    }
}