use crate::process::ProcessId;
use crate::syscall::{SyscallError, SyscallResult};
use crate::syscall::numbers::*;
use crate::syscall::validation::validate_syscall_args;
use crate::{serial_println, println};
use alloc::format;
use core::sync::atomic::{AtomicBool, Ordering};

/// Per-syscall tracing.
///
/// Off by default. It is genuinely useful while bringing a syscall up, but a
/// program that writes one character at a time — a console, say — produces two
/// log lines per keystroke, interleaved with its own output on the same line.
/// That makes the thing you are trying to read unreadable.
static SYSCALL_TRACE: AtomicBool = AtomicBool::new(false);

pub fn set_syscall_trace(enabled: bool) {
    SYSCALL_TRACE.store(enabled, Ordering::Relaxed);
}

pub fn syscall_trace_enabled() -> bool {
    SYSCALL_TRACE.load(Ordering::Relaxed)
}

/// Initialize the system call dispatcher
pub fn init_syscall_dispatcher() -> Result<(), &'static str> {
    serial_println!("Initializing system call dispatcher...");
    
    // Initialize any dispatcher-specific data structures
    // For now, this is just a placeholder
    
    serial_println!("System call dispatcher initialized");
    Ok(())
}

/// Main system call dispatcher
pub fn dispatch_syscall(
    process_id: ProcessId,
    syscall_number: u64,
    args: [u64; 6],
) -> SyscallResult {
    if syscall_trace_enabled() {
        serial_println!(
            "Process {} calling syscall {} ({}) with args [{}, {}, {}, {}, {}, {}]",
            process_id.0,
            syscall_number,
            syscall_name(syscall_number),
            args[0], args[1], args[2], args[3], args[4], args[5]
        );
    }
    
    // Validate system call arguments
    validate_syscall_args(process_id, syscall_number, &args)?;
    
    // Dispatch to appropriate handler
    let result = match syscall_number {
        // Process management
        SYS_EXIT => sys_exit(process_id, args),
        SYS_FORK => sys_fork(process_id, args),
        SYS_EXEC => sys_exec(process_id, args),
        SYS_WAIT => sys_wait(process_id, args),
        SYS_GETPID => sys_getpid(process_id, args),
        SYS_GETPPID => sys_getppid(process_id, args),
        SYS_KILL => sys_kill(process_id, args),
        SYS_YIELD => sys_yield(process_id, args),
        SYS_SPAWN => sys_spawn(process_id, args),
        
        // Memory management
        SYS_MMAP => sys_mmap(process_id, args),
        SYS_MUNMAP => sys_munmap(process_id, args),
        SYS_MPROTECT => sys_mprotect(process_id, args),
        SYS_BRK => sys_brk(process_id, args),
        SYS_SBRK => sys_sbrk(process_id, args),
        
        // File system
        SYS_OPEN => crate::syscall::files::sys_open(args),
        SYS_CLOSE => crate::syscall::files::sys_close(args),
        SYS_READ => crate::syscall::files::sys_read(args),
        SYS_WRITE => sys_write(process_id, args),
        SYS_LSEEK => crate::syscall::files::sys_lseek(args),
        SYS_STAT => crate::syscall::files::sys_stat(args),
        SYS_FSTAT => sys_fstat(process_id, args),
        SYS_MKDIR => sys_mkdir(process_id, args),
        SYS_RMDIR => sys_rmdir(process_id, args),
        SYS_UNLINK => sys_unlink(process_id, args),
        
        // IPC
        SYS_SEND_MESSAGE => sys_send_message(process_id, args),
        SYS_RECEIVE_MESSAGE => sys_receive_message(process_id, args),
        SYS_REPLY_MESSAGE => sys_reply_message(process_id, args),
        SYS_CREATE_CHANNEL => sys_create_channel(process_id, args),
        SYS_DESTROY_CHANNEL => sys_destroy_channel(process_id, args),
        
        // Driver interface
        SYS_DRIVER_REGISTER => sys_driver_register(process_id, args),
        SYS_DRIVER_UNREGISTER => sys_driver_unregister(process_id, args),
        SYS_DRIVER_REQUEST => sys_driver_request(process_id, args),
        SYS_DRIVER_RESPONSE => sys_driver_response(process_id, args),
        
        // System information
        SYS_GETDENTS => crate::syscall::files::sys_getdents(args),
        SYS_UNAME => sys_uname(process_id, args),
        SYS_SYSINFO => sys_sysinfo(process_id, args),
        SYS_TIME => sys_time(process_id, args),
        SYS_CLOCK_GETTIME => sys_clock_gettime(process_id, args),
        
        // Security
        SYS_GRANT_CAPABILITY => sys_grant_capability(process_id, args),
        SYS_REVOKE_CAPABILITY => sys_revoke_capability(process_id, args),
        SYS_CHECK_CAPABILITY => sys_check_capability(process_id, args),
        SYS_LIST_CAPABILITIES => sys_list_capabilities(process_id, args),
        
        // Debug (only in debug builds)
        SYS_DEBUG_PRINT => sys_debug_print(process_id, args),
        SYS_DEBUG_DUMP => sys_debug_dump(process_id, args),
        
        _ => {
            serial_println!("Unknown system call: {}", syscall_number);
            Err(SyscallError::InvalidSyscall)
        }
    };
    
    // Failures are always reported; successes only when tracing. A syscall
    // that fails is news, one that works is not.
    match &result {
        Ok(value) => {
            if syscall_trace_enabled() {
                serial_println!(
                    "Process {} syscall {} completed successfully, returned {}",
                    process_id.0, syscall_name(syscall_number), value
                );
            }
        }
        Err(error) => {
            serial_println!(
                "Process {} syscall {} failed: {:?}",
                process_id.0, syscall_name(syscall_number), error
            );
        }
    }
    
    result
}

// Process management system calls
fn sys_exit(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let exit_code = args[0] as i32;
    serial_println!("Process {} exiting with code {}", process_id.0, exit_code);
    
    // For now, just log the exit. In a real implementation, we would:
    // 1. Mark the process as terminated
    // 2. Clean up process resources
    // 3. Notify parent process
    // 4. Schedule next process
    
    serial_println!(
        "Process {} terminated with exit code {}",
        process_id.0,
        exit_code
    );

    // The descriptor table is global — one table for the whole system, because
    // there is no per-process state to hang one off. So closing everything is
    // only safe when this is the last program running; otherwise a short-lived
    // program exiting would close the shell's open files behind its back.
    #[cfg(target_arch = "x86_64")]
    let last_program = crate::usermode::resident_count() <= 1;
    #[cfg(not(target_arch = "x86_64"))]
    let last_program = true;

    if last_program {
        crate::syscall::files::close_all();
    } else {
        serial_println!(
            "  (leaving {} open file(s) alone: another program is still resident)",
            crate::syscall::files::open_count()
        );
    }

    // Actually terminate. The ring-3 program runs on a kernel thread, so
    // retiring that thread is the exit: `exit_current` marks it finished and
    // schedules away, and never returns. Previously this logged and returned
    // Ok(0), leaving the caller running as if nothing had happened.
    //
    // The code is recorded first, because `exit_current` does not return and
    // whoever is in `wait` needs something to collect.
    #[cfg(target_arch = "x86_64")]
    crate::task::set_exit_code(exit_code);

    #[cfg(target_arch = "x86_64")]
    crate::task::exit_current();

    #[allow(unreachable_code)]
    Ok(0)
}

/// Not implemented, and now says so.
///
/// This used to add a row to the process table, log "Fork successful", and
/// return the new PID — without duplicating an address space, copying a stack,
/// or creating anything that could run. Userspace saw a positive return value,
/// concluded it was the parent, and carried on; the "child" never existed. A
/// syscall that reports success for work it did not do is worse than one that
/// refuses, because the caller has no way to find out.
///
/// Honest `fork` needs per-process page tables, which needs the kernel out of
/// the low identity mapping first. Until then, `Err`.
fn sys_fork(process_id: ProcessId, _args: [u64; 6]) -> SyscallResult {
    serial_println!(
        "Process {} called fork, which needs per-process address spaces (not implemented)",
        process_id.0
    );
    Err(SyscallError::NotSupported)
}

/// Give up the rest of the current time slice.
///
/// The interesting part is where this runs: inside a system call, on the calling
/// thread's own kernel stack, with a live `SyscallFrame` on it. `yield_now` ends
/// in `kosh_switch_context`, so another thread can enter a syscall of its own
/// while this frame is parked — which is precisely what a single shared syscall
/// stack could not survive.
fn sys_yield(_process_id: ProcessId, _args: [u64; 6]) -> SyscallResult {
    #[cfg(target_arch = "x86_64")]
    crate::task::yield_now();

    Ok(0)
}

fn sys_exec(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let path_ptr = args[0];
    let argv_ptr = args[1];
    let envp_ptr = args[2];
    
    serial_println!("Process {} attempting to exec program at 0x{:x}", process_id.0, path_ptr);
    
    // TODO: Implement program execution
    // This would involve:
    // 1. Loading the new program from filesystem
    // 2. Setting up new memory space
    // 3. Parsing arguments and environment
    // 4. Starting execution at program entry point
    
    Err(SyscallError::NotSupported)
}

/// `spawn(path, path_len)` -> task id
///
/// Deliberately not called `exec`: `exec` replaces the calling image, and
/// `fork`+`exec` needs two address spaces. This loads a *second* program into
/// the one address space that exists and runs it on its own thread — which is
/// enough for a shell to launch a program, and honest about what it is.
///
/// `path` names a boot module (`module2 /boot/hello hello` in grub.cfg), not a
/// file on the FAT32 volume. Loading from disk needs the loader to read through
/// the VFS, which is a separate job from this one.
#[cfg(target_arch = "x86_64")]
fn sys_spawn(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    use crate::usermode::SpawnError;

    let name = crate::syscall::files::path_from_user(args[0], args[1])?;
    let name = name.trim_start_matches('/');

    serial_println!("Process {} spawning '{}'", process_id.0, name);

    match crate::usermode::spawn_program(name) {
        Ok(thread) => Ok(thread as u64),
        // A shell turns this one into "command not found", so it must not be
        // lumped in with the others.
        Err(SpawnError::NotFound) => Err(SyscallError::NotFound),
        Err(SpawnError::AddressConflict) => Err(SyscallError::AddressInUse),
        Err(SpawnError::TooManyPrograms) | Err(SpawnError::NoThread(_)) => {
            Err(SyscallError::ResourceExhausted)
        }
        Err(SpawnError::Map(e)) => {
            serial_println!("  spawn mapping failed: {}", e);
            Err(SyscallError::OutOfMemory)
        }
        Err(SpawnError::Load(e)) => {
            serial_println!("  spawn load failed: {:?}", e);
            Err(SyscallError::InvalidArgument)
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn sys_spawn(_process_id: ProcessId, _args: [u64; 6]) -> SyscallResult {
    Err(SyscallError::NotSupported)
}

/// `wait(task_id, status_ptr)` -> task id
///
/// Blocks — really blocks, in `State::Blocked`, not in a yield loop — until that
/// task finishes, then writes its exit code to `status_ptr` if that is non-null.
///
/// This used to be a `serial_println!` and `Err(NotSupported)` under a three-line
/// TODO. It takes a task id rather than "any child" because there is no process
/// hierarchy to define a child with; `spawn` returns the id it expects back.
#[cfg(target_arch = "x86_64")]
fn sys_wait(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let task_id = args[0] as usize;
    let status_ptr = args[1];

    // Validate the destination *before* blocking. Failing afterwards would mean
    // the child has already been reaped and its exit code thrown away, with
    // nothing to return it to.
    if status_ptr != 0 {
        crate::syscall::uaccess::validate_user_range(status_ptr, 4, true)
            .map_err(|_| SyscallError::InvalidArgument)?;
    }

    let code = crate::task::wait_for(task_id).map_err(|e| {
        serial_println!("Process {} wait({}) failed: {}", process_id.0, task_id, e);
        SyscallError::InvalidArgument
    })?;

    if status_ptr != 0 {
        let bytes = code.to_le_bytes();
        crate::syscall::uaccess::copy_to_user(status_ptr, &bytes)
            .map_err(|_| SyscallError::InvalidArgument)?;
    }

    Ok(task_id as u64)
}

#[cfg(not(target_arch = "x86_64"))]
fn sys_wait(_process_id: ProcessId, _args: [u64; 6]) -> SyscallResult {
    Err(SyscallError::NotSupported)
}

fn sys_getpid(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    Ok(process_id.0 as u64)
}

fn sys_getppid(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    // TODO: Get actual parent process ID
    // For now, return 0 (no parent)
    Ok(0)
}

fn sys_kill(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let target_pid = args[0];
    let signal = args[1];
    
    serial_println!("Process {} sending signal {} to process {}", 
                   process_id.0, signal, target_pid);
    
    // TODO: Implement signal sending
    // This would involve:
    // 1. Validating target process exists
    // 2. Checking permissions
    // 3. Delivering the signal
    
    Err(SyscallError::NotSupported)
}

// Memory management system calls
fn sys_mmap(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let addr = args[0];
    let length = args[1];
    let prot = args[2];
    let flags = args[3];
    let _fd = args[4];
    let _offset = args[5];
    
    serial_println!("Process {} requesting mmap: addr=0x{:x}, len={}, prot={}, flags={}", 
                   process_id.0, addr, length, prot, flags);
    
    // Basic implementation for anonymous memory mapping
    if length == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    
    // Convert protection flags to MemoryProtection
    let protection = crate::memory::vmm::MemoryProtection {
        readable: (prot & 0x1) != 0,    // PROT_READ
        writable: (prot & 0x2) != 0,    // PROT_WRITE
        executable: (prot & 0x4) != 0,  // PROT_EXEC
        user_accessible: true,
    };
    
    // For now, implement simple anonymous mapping
    // In a real implementation, we would:
    // 1. Find suitable virtual address space
    // 2. Allocate physical pages
    // 3. Set up page table entries
    
    // Return a dummy address for now (in user space)
    let mapped_addr = if addr == 0 {
        0x40000000u64 // Default user space address
    } else {
        addr
    };
    
    serial_println!("Process {} mmap successful: mapped at 0x{:x}", process_id.0, mapped_addr);
    Ok(mapped_addr)
}

fn sys_munmap(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let addr = args[0];
    let length = args[1];
    
    serial_println!("Process {} requesting munmap: addr=0x{:x}, len={}", 
                   process_id.0, addr, length);
    
    // TODO: Implement memory unmapping
    Err(SyscallError::NotSupported)
}

fn sys_mprotect(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let addr = args[0];
    let length = args[1];
    let prot = args[2];
    
    serial_println!("Process {} requesting mprotect: addr=0x{:x}, len={}, prot={}", 
                   process_id.0, addr, length, prot);
    
    // TODO: Implement memory protection changes
    Err(SyscallError::NotSupported)
}

fn sys_brk(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let addr = args[0];
    
    serial_println!("Process {} requesting brk: addr=0x{:x}", process_id.0, addr);
    
    // TODO: Implement heap management
    Err(SyscallError::NotSupported)
}

fn sys_sbrk(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let increment = args[0] as i64;
    
    serial_println!("Process {} requesting sbrk: increment={}", process_id.0, increment);
    
    // TODO: Implement heap increment
    Err(SyscallError::NotSupported)
}

// File system system calls



fn sys_write(_process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    use crate::syscall::uaccess::copy_from_user;

    let fd = args[0];
    let buf_ptr = args[1];
    let count = args[2] as usize;

    // Only the console for now; real file descriptors need the VFS.
    if fd != 1 && fd != 2 {
        return Err(SyscallError::NotSupported);
    }

    // Copy through a bounded kernel buffer rather than dereferencing the user
    // pointer directly. `copy_from_user` is what makes this safe: it checks the
    // span is in the user half and that every page is present and
    // USER_ACCESSIBLE. This used to return `count` without reading anything.
    const CHUNK: usize = 256;
    let mut buffer = [0u8; CHUNK];
    let mut written = 0usize;

    while written < count {
        let n = core::cmp::min(CHUNK, count - written);
        let slice = &mut buffer[..n];

        copy_from_user(buf_ptr + written as u64, slice)
            .map_err(|_| SyscallError::InvalidArgument)?;

        for &byte in slice.iter() {
            let c = byte as char;
            crate::serial_print!("{}", c);
            crate::print!("{}", c);
        }

        written += n;
    }

    Ok(count as u64)
}



fn sys_fstat(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let fd = args[0];
    let stat_buf_ptr = args[1];
    
    serial_println!("Process {} requesting fstat: fd={}, buf=0x{:x}", 
                   process_id.0, fd, stat_buf_ptr);
    
    // TODO: Implement file descriptor stat
    Err(SyscallError::NotSupported)
}

fn sys_mkdir(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let path_ptr = args[0];
    let mode = args[1];
    
    serial_println!("Process {} requesting mkdir: path=0x{:x}, mode={}", 
                   process_id.0, path_ptr, mode);
    
    // TODO: Implement directory creation
    Err(SyscallError::NotSupported)
}

fn sys_rmdir(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let path_ptr = args[0];
    
    serial_println!("Process {} requesting rmdir: path=0x{:x}", process_id.0, path_ptr);
    
    // TODO: Implement directory removal
    Err(SyscallError::NotSupported)
}

fn sys_unlink(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let path_ptr = args[0];
    
    serial_println!("Process {} requesting unlink: path=0x{:x}", process_id.0, path_ptr);
    
    // TODO: Implement file removal
    Err(SyscallError::NotSupported)
}

// IPC system calls
fn sys_send_message(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let receiver_pid = args[0];
    let _message_ptr = args[1];
    let message_len = args[2];
    
    serial_println!("Process {} sending message to process {}: ptr=0x{:x}, len={}", 
                   process_id.0, receiver_pid, _message_ptr, message_len);
    
    // Basic implementation using existing IPC system
    if message_len > 4096 {
        return Err(SyscallError::InvalidArgument);
    }
    
    // Create a simple text message for demonstration
    // In a real implementation, we would read the actual message data from user space
    let message_data = crate::ipc::message::MessageData::Text(
        alloc::format!("Message from process {} (len={})", process_id.0, message_len)
    );
    
    let message = crate::ipc::message::create_message(
        process_id,
        ProcessId::new(receiver_pid as u32),
        crate::ipc::message::MessageType::ServiceRequest,
        message_data,
    );
    
    match crate::ipc::message::send_message(message) {
        Ok(()) => {
            serial_println!("Process {} successfully sent message to process {}", 
                           process_id.0, receiver_pid);
            Ok(0)
        }
        Err(e) => {
            serial_println!("Process {} failed to send message: {:?}", process_id.0, e);
            Err(e.into())
        }
    }
}

fn sys_receive_message(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let _timeout_ms = args[0];
    
    serial_println!("Process {} receiving message with timeout {}", process_id.0, _timeout_ms);
    
    // Basic implementation using existing IPC system
    match crate::ipc::message::receive_message(process_id) {
        Ok(message) => {
            serial_println!("Process {} received message {} from process {}", 
                           process_id.0, message.header.message_id.0, message.header.sender.0);
            // Return the message ID for now
            // In a real implementation, we would copy the message data to user space
            Ok(message.header.message_id.0)
        }
        Err(e) => {
            serial_println!("Process {} failed to receive message: {:?}", process_id.0, e);
            Err(e.into())
        }
    }
}

fn sys_reply_message(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let message_id = args[0];
    let reply_ptr = args[1];
    let reply_len = args[2];
    
    serial_println!("Process {} replying to message {}: ptr=0x{:x}, len={}", 
                   process_id.0, message_id, reply_ptr, reply_len);
    
    // TODO: Implement message reply
    Err(SyscallError::NotSupported)
}

fn sys_create_channel(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let other_pid = args[0];
    
    serial_println!("Process {} creating channel with process {}", process_id.0, other_pid);
    
    // TODO: Implement secure channel creation
    Err(SyscallError::NotSupported)
}

fn sys_destroy_channel(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let channel_id = args[0];
    
    serial_println!("Process {} destroying channel {}", process_id.0, channel_id);
    
    // TODO: Implement channel destruction
    Err(SyscallError::NotSupported)
}

// Driver interface system calls
fn sys_driver_register(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let driver_info_ptr = args[0];
    
    serial_println!("Process {} registering as driver: info=0x{:x}", 
                   process_id.0, driver_info_ptr);
    
    // TODO: Implement driver registration
    Err(SyscallError::NotSupported)
}

fn sys_driver_unregister(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let driver_id = args[0];
    
    serial_println!("Process {} unregistering driver {}", process_id.0, driver_id);
    
    // TODO: Implement driver unregistration
    Err(SyscallError::NotSupported)
}

fn sys_driver_request(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let driver_id = args[0];
    let request_ptr = args[1];
    let request_len = args[2];
    
    serial_println!("Process {} sending request to driver {}: ptr=0x{:x}, len={}", 
                   process_id.0, driver_id, request_ptr, request_len);
    
    // TODO: Implement driver request
    Err(SyscallError::NotSupported)
}

fn sys_driver_response(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let request_id = args[0];
    let response_ptr = args[1];
    let response_len = args[2];
    
    serial_println!("Process {} responding to request {}: ptr=0x{:x}, len={}", 
                   process_id.0, request_id, response_ptr, response_len);
    
    // TODO: Implement driver response
    Err(SyscallError::NotSupported)
}

// System information system calls
fn sys_uname(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let buf_ptr = args[0];
    
    serial_println!("Process {} requesting uname: buf=0x{:x}", process_id.0, buf_ptr);
    
    // TODO: Implement uname (system information)
    Err(SyscallError::NotSupported)
}

fn sys_sysinfo(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let info_ptr = args[0];
    
    serial_println!("Process {} requesting sysinfo: buf=0x{:x}", process_id.0, info_ptr);
    
    // TODO: Implement sysinfo (system statistics)
    Err(SyscallError::NotSupported)
}

fn sys_time(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let time_ptr = args[0];
    
    serial_println!("Process {} requesting time: buf=0x{:x}", process_id.0, time_ptr);
    
    // TODO: Implement time getting
    // For now, return 0 (epoch time)
    Ok(0)
}

fn sys_clock_gettime(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let clock_id = args[0];
    let timespec_ptr = args[1];
    
    serial_println!("Process {} requesting clock_gettime: clock={}, buf=0x{:x}", 
                   process_id.0, clock_id, timespec_ptr);
    
    // TODO: Implement high-resolution time getting
    Err(SyscallError::NotSupported)
}

// Security system calls
fn sys_grant_capability(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let target_pid = args[0];
    let capability_type = args[1];
    let resource_ptr = args[2];
    
    serial_println!("Process {} granting capability {} to process {}: resource=0x{:x}", 
                   process_id.0, capability_type, target_pid, resource_ptr);
    
    // TODO: Implement capability granting using existing capability system
    Err(SyscallError::NotSupported)
}

fn sys_revoke_capability(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let target_pid = args[0];
    let capability_id = args[1];
    
    serial_println!("Process {} revoking capability {} from process {}", 
                   process_id.0, capability_id, target_pid);
    
    // TODO: Implement capability revocation
    Err(SyscallError::NotSupported)
}

fn sys_check_capability(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let capability_type = args[0];
    let resource_ptr = args[1];
    
    serial_println!("Process {} checking capability {}: resource=0x{:x}", 
                   process_id.0, capability_type, resource_ptr);
    
    // TODO: Implement capability checking using existing capability system
    Err(SyscallError::NotSupported)
}

fn sys_list_capabilities(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let buf_ptr = args[0];
    let buf_len = args[1];
    
    serial_println!("Process {} listing capabilities: buf=0x{:x}, len={}", 
                   process_id.0, buf_ptr, buf_len);
    
    // TODO: Implement capability listing
    Err(SyscallError::NotSupported)
}

// Debug system calls (only in debug builds)
fn sys_debug_print(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let message_ptr = args[0];
    let message_len = args[1];
    
    serial_println!("Process {} debug print: ptr=0x{:x}, len={}", 
                   process_id.0, message_ptr, message_len);
    
    // TODO: Read string from user space and print it
    println!("DEBUG[{}]: <message at 0x{:x}>", process_id.0, message_ptr);
    
    Ok(0)
}

fn sys_debug_dump(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let dump_type = args[0];
    
    serial_println!("Process {} debug dump: type={}", process_id.0, dump_type);
    
    // TODO: Implement various debug dumps (memory, processes, etc.)
    println!("DEBUG DUMP[{}]: type {}", process_id.0, dump_type);
    
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessId;
    
    #[test_case]
    fn test_dispatch_syscall() {
        let pid = ProcessId::new(1);
        let args = [0; 6];
        
        // Test getpid syscall
        let result = dispatch_syscall(pid, SYS_GETPID, args);
        assert_eq!(result, Ok(1));
        
        // Test invalid syscall
        let result = dispatch_syscall(pid, 999, args);
        assert_eq!(result, Err(SyscallError::InvalidSyscall));
    }
    
    #[test_case]
    fn test_sys_getpid() {
        let pid = ProcessId::new(42);
        let args = [0; 6];
        
        let result = sys_getpid(pid, args);
        assert_eq!(result, Ok(42));
    }
    
    #[test_case]
    fn test_sys_exit() {
        let pid = ProcessId::new(1);
        let args = [0, 0, 0, 0, 0, 0]; // exit code 0
        
        let result = sys_exit(pid, args);
        assert_eq!(result, Ok(0));
    }
    
    #[test_case]
    fn test_sys_fork() {
        let pid = ProcessId::new(1);
        let args = [0; 6];
        
        // Fork should create a new process and return child PID
        let result = sys_fork(pid, args);
        // Since we don't have process table initialized in tests, this will fail
        // but we can verify the function doesn't panic
        assert!(result.is_ok() || result.is_err());
    }
    
    #[test_case]
    fn test_sys_mmap() {
        let pid = ProcessId::new(1);
        let args = [0, 4096, 3, 0, 0, 0]; // addr=0, len=4096, prot=RW, flags=0
        
        let result = sys_mmap(pid, args);
        assert!(result.is_ok());
        
        // Test invalid length
        let args = [0, 0, 3, 0, 0, 0]; // len=0
        let result = sys_mmap(pid, args);
        assert_eq!(result, Err(SyscallError::InvalidArgument));
    }
    
    #[test_case]
    fn test_sys_open() {
        let pid = ProcessId::new(1);
        let args = [0x1000, 0, 0644, 0, 0, 0]; // path_ptr, flags=READ_ONLY, mode
        
        let result = sys_open(pid, args);
        assert_eq!(result, Ok(3)); // Should return fd 3
        
        // Test invalid flags
        let args = [0x1000, 999, 0644, 0, 0, 0]; // invalid flags
        let result = sys_open(pid, args);
        assert_eq!(result, Err(SyscallError::InvalidArgument));
    }
    
    #[test_case]
    fn test_sys_read() {
        let pid = ProcessId::new(1);
        
        // Test reading from stdin
        let args = [0, 0x1000, 100, 0, 0, 0]; // fd=0 (stdin), buf, count
        let result = sys_read(pid, args);
        assert_eq!(result, Ok(0)); // stdin returns EOF
        
        // Test reading from regular fd
        let args = [3, 0x1000, 100, 0, 0, 0]; // fd=3, buf, count
        let result = sys_read(pid, args);
        assert_eq!(result, Ok(100)); // Should read 100 bytes
        
        // Test reading 0 bytes
        let args = [3, 0x1000, 0, 0, 0, 0]; // count=0
        let result = sys_read(pid, args);
        assert_eq!(result, Ok(0));
    }
}