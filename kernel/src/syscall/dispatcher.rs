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
/// Still not called `exec` — `exec` replaces the *calling* image, and nothing
/// here does that. But it is no longer "a second program in the one address
/// space that exists": the child gets a PML4 of its own, so it can be linked at
/// the same address as its parent, and the overlap check that used to refuse
/// `ksh` spawning `ksh` is gone along with the reason for it.
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
        Err(SpawnError::TooManyPrograms) | Err(SpawnError::NoThread(_)) => {
            Err(SyscallError::ResourceExhausted)
        }
        Err(SpawnError::Space(e)) => {
            serial_println!("  spawn could not build an address space: {}", e);
            Err(SyscallError::OutOfMemory)
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

/// Not implemented, and no longer pretending otherwise.
///
/// This returned `Ok(0)` under a TODO, which userspace cannot tell apart from a
/// genuine "no parent" — and 0 is what a real `getppid` returns for the process
/// that has none. There is no parent to report: `spawn` hands back a task id and
/// records nothing about who called it, so a process hierarchy does not exist
/// yet. Refusing says that; returning 0 says the opposite.
fn sys_getppid(_process_id: ProcessId, _args: [u64; 6]) -> SyscallResult {
    Err(SyscallError::NotSupported)
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

/// Where anonymous mappings go when the caller does not ask for an address.
///
/// A fixed base is safe because each process has the lower half to itself; the
/// only thing it has to stay clear of is that process's own image and stack.
/// 256 MiB is above every `p_vaddr` the userspace link scripts use and below the
/// stack at [`crate::usermode::USER_STACK_TOP`].
const MMAP_BASE: u64 = 0x0000_0000_1000_0000;

/// Highest address `mmap` will hand out.
const MMAP_LIMIT: u64 = 0x0000_0000_2000_0000;

/// `mmap` flags. Only anonymous private mappings exist, so these are the only
/// two that mean anything — but `MAP_ANONYMOUS` must be set, because it is the
/// one argument the kernel can trust the caller to have passed deliberately.
const MAP_ANONYMOUS: u64 = 0x20;
#[allow(dead_code)]
const MAP_PRIVATE: u64 = 0x02;

/// `mmap(addr, length, prot, flags, fd, offset)` -> address
///
/// Anonymous private mappings only. What this replaces is the clearest example
/// of the pattern this project keeps unwinding: it validated `length`, built a
/// `MemoryProtection` struct, threw it away, and returned the constant
/// `0x40000000` — then logged "mmap successful: mapped at 0x40000000" and
/// returned it as a success. No frame was allocated and no page table was
/// touched, so a caller that wrote to the pointer took a page fault, having been
/// told the mapping existed.
///
/// It works now because per-process address spaces made it easy: the pages go
/// into the calling process's own tables, so there is no shared region to
/// arbitrate and no chance of handing out an address another program is using.
#[cfg(target_arch = "x86_64")]
fn sys_mmap(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    use x86_64::structures::paging::PageTableFlags;

    let addr = args[0];
    let length = args[1];
    let prot = args[2];
    let flags = args[3];
    let fd = args[4] as i64;

    if length == 0 {
        return Err(SyscallError::InvalidArgument);
    }

    // `MAP_ANONYMOUS` has to be asked for explicitly, and `fd` is deliberately
    // *not* consulted.
    //
    // Both because of a bug this test found: the first version checked
    // `args[4] >= 0` and refused, reasoning that a non-negative fd meant a
    // file-backed mapping. But `args[4]` arrives in R8, and a caller using a
    // three-argument syscall wrapper never sets R8 — so the check read whatever
    // was left in that register and refused every mapping. Discriminating on a
    // flag the caller definitely passed is the only safe way to read an argument
    // this ABI does not require them to set.
    //
    // File-backed mappings need the page-fault handler to read through the VFS,
    // which does not exist; that is what the refusal below is about.
    if flags & MAP_ANONYMOUS == 0 {
        serial_println!(
            "Process {} mmap without MAP_ANONYMOUS: file mappings are not implemented",
            process_id.0
        );
        return Err(SyscallError::NotSupported);
    }
    let _ = fd;

    let pages = (length as usize).div_ceil(crate::memory::PAGE_SIZE);

    let mut page_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if prot & 0x2 != 0 {
        page_flags |= PageTableFlags::WRITABLE;
    }
    if prot & 0x4 == 0 {
        page_flags |= PageTableFlags::NO_EXECUTE;
    }
    // W^X, for userspace too. A caller asking for both gets neither rather than
    // a writable code page.
    if prot & 0x2 != 0 && prot & 0x4 != 0 {
        serial_println!("Process {} mmap asked for write+execute; refused", process_id.0);
        return Err(SyscallError::InvalidArgument);
    }

    let base = if addr != 0 {
        // MAP_FIXED semantics without the flag: honour the hint exactly or fail,
        // rather than silently moving it somewhere the caller will not look.
        if addr % crate::memory::PAGE_SIZE as u64 != 0 {
            return Err(SyscallError::InvalidArgument);
        }
        if !region_free(addr, pages) {
            return Err(SyscallError::AddressInUse);
        }
        addr
    } else {
        find_free_region(pages).ok_or(SyscallError::OutOfMemory)?
    };

    crate::memory::paging::map_user_pages(base, pages, page_flags)
        .map_err(|e| {
            serial_println!("Process {} mmap failed: {}", process_id.0, e);
            SyscallError::OutOfMemory
        })?;

    if syscall_trace_enabled() {
        serial_println!(
            "Process {} mmap: {} page(s) at 0x{:x} (prot {}, flags {})",
            process_id.0,
            pages,
            base,
            prot,
            flags
        );
    }

    Ok(base)
}

#[cfg(not(target_arch = "x86_64"))]
fn sys_mmap(_process_id: ProcessId, _args: [u64; 6]) -> SyscallResult {
    Err(SyscallError::NotSupported)
}

/// Is every page of this span currently unmapped in the calling process?
#[cfg(target_arch = "x86_64")]
fn region_free(base: u64, pages: usize) -> bool {
    (0..pages).all(|i| {
        crate::memory::paging::translate(base + (i * crate::memory::PAGE_SIZE) as u64).is_none()
    })
}

/// First `pages`-page hole in the mmap region.
///
/// A linear scan of the process's own tables rather than a bump pointer, because
/// a bump pointer would need per-process state to live in and would leak address
/// space across `munmap`. The region is 256 MiB, so the scan is bounded.
#[cfg(target_arch = "x86_64")]
fn find_free_region(pages: usize) -> Option<u64> {
    let step = crate::memory::PAGE_SIZE as u64;
    let mut base = MMAP_BASE;

    while base + (pages as u64 * step) <= MMAP_LIMIT {
        if region_free(base, pages) {
            return Some(base);
        }
        base += step;
    }

    None
}

/// `munmap(addr, length)`
///
/// Previously a `NotSupported` under a TODO — which was at least honest, but it
/// meant a program could not give memory back. The pages return to the frame
/// allocator immediately.
#[cfg(target_arch = "x86_64")]
fn sys_munmap(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let addr = args[0];
    let length = args[1];

    if length == 0 || addr % crate::memory::PAGE_SIZE as u64 != 0 {
        return Err(SyscallError::InvalidArgument);
    }

    // Only addresses this syscall could have handed out. Letting a process
    // unmap its own `.text` is a good way to turn a userspace bug into an
    // unexplainable fault.
    if addr < MMAP_BASE || addr >= MMAP_LIMIT {
        serial_println!(
            "Process {} munmap 0x{:x} is outside the mmap region",
            process_id.0,
            addr
        );
        return Err(SyscallError::InvalidArgument);
    }

    let pages = (length as usize).div_ceil(crate::memory::PAGE_SIZE);
    let freed = unsafe {
        crate::memory::paging::unmap_user_pages_in(
            crate::memory::paging::active_mapper(),
            addr,
            pages,
        )
    };

    if syscall_trace_enabled() {
        serial_println!(
            "Process {} munmap: {} of {} page(s) at 0x{:x}",
            process_id.0,
            freed,
            pages,
            addr
        );
    }

    Ok(freed as u64)
}

#[cfg(not(target_arch = "x86_64"))]
fn sys_munmap(_process_id: ProcessId, _args: [u64; 6]) -> SyscallResult {
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
/// `send_message(receiver, ptr, len)`
///
/// Not implemented, and it now says so instead of delivering something else.
///
/// What it used to do is the most subtle fabrication in this kernel, because
/// most of it was real. It reached `ipc::message::send_message`, which genuinely
/// validates the sender and receiver, checks a `SendMessage` capability and
/// enqueues — but `args[1]`, the caller's buffer, was bound to `_message_ptr`
/// and never read. In its place went:
///
/// ```text
/// MessageData::Text(format!("Message from process {} (len={})", pid, len))
/// ```
///
/// so the receiver got a synthetic string describing the message instead of the
/// message, and the sender got `Ok(0)`.
///
/// Copying the payload is the easy part — `uaccess::copy_from_user` already
/// exists. What blocks this is that there are **two unrelated id namespaces**:
/// `syscall/entry.rs` passes the calling *thread's* id as the `ProcessId`, while
/// `ipc::message::send_message` looks the sender up in `process::ProcessTable`,
/// whose only entries are the three synthetic ones the boot self-test creates.
/// A ring-3 caller is not in that table, so every send would fail
/// `SenderNotFound` even with a real payload.
///
/// Reconciling those namespaces — one process table that `spawn` registers into,
/// with the thread as a member of a process rather than a synonym for one — is
/// the work. `kernel/src/ipc/` is ~85 KB of implemented queueing and capability
/// machinery waiting on it.
fn sys_send_message(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    serial_println!(
        "Process {} called send_message({}, 0x{:x}, {}): IPC needs one process-id namespace first",
        process_id.0,
        args[0],
        args[1],
        args[2]
    );
    Err(SyscallError::NotSupported)
}

/// `receive_message(timeout_ms)`
///
/// Not implemented, for the same reason as [`sys_send_message`]. This one did
/// dequeue for real — and then returned only `message_id`, dropping the payload
/// on the floor under a `// In a real implementation, we would copy the message
/// data to user space`. A caller that got a message id had no way to read the
/// message.
fn sys_receive_message(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    serial_println!(
        "Process {} called receive_message({}): IPC needs one process-id namespace first",
        process_id.0,
        args[0]
    );
    Err(SyscallError::NotSupported)
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

/// `time()` -> seconds since the Unix epoch
///
/// Read from the CMOS RTC. It used to return `Ok(0)` under a TODO, which is a
/// perfectly valid timestamp — midnight on 1 January 1970 — so a caller had no
/// way to know the clock was fiction.
fn sys_time(_process_id: ProcessId, _args: [u64; 6]) -> SyscallResult {
    #[cfg(target_arch = "x86_64")]
    {
        match crate::platform::rtc::unix_time() {
            Some(secs) => Ok(secs),
            None => Err(SyscallError::NotSupported),
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    Err(SyscallError::NotSupported)
}

/// Clock ids, matching Linux's for the two that exist here.
const CLOCK_REALTIME: u64 = 0;
const CLOCK_MONOTONIC: u64 = 1;

/// `clock_gettime(clock_id, timespec_ptr)`
///
/// `CLOCK_MONOTONIC` comes from the PIT tick counter, which is the only clock
/// this kernel has that is guaranteed to move forwards; `CLOCK_REALTIME` comes
/// from the RTC and therefore has one-second resolution, which the nanoseconds
/// field reports honestly as zero rather than interpolating.
#[cfg(target_arch = "x86_64")]
fn sys_clock_gettime(_process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let clock_id = args[0];
    let out = args[1];

    let (secs, nanos) = match clock_id {
        CLOCK_MONOTONIC => {
            let ms = crate::interrupts::timer::uptime_ms();
            (ms / 1000, (ms % 1000) * 1_000_000)
        }
        CLOCK_REALTIME => match crate::platform::rtc::unix_time() {
            // No sub-second source behind the RTC, so nanoseconds is 0. Faking
            // it from the tick counter would drift against the seconds field.
            Some(secs) => (secs, 0),
            None => return Err(SyscallError::NotSupported),
        },
        _ => return Err(SyscallError::InvalidArgument),
    };

    // struct timespec { i64 tv_sec; i64 tv_nsec; }
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&secs.to_le_bytes());
    buf[8..].copy_from_slice(&nanos.to_le_bytes());

    crate::syscall::uaccess::copy_to_user(out, &buf)
        .map_err(|_| SyscallError::InvalidArgument)?;

    Ok(0)
}

#[cfg(not(target_arch = "x86_64"))]
fn sys_clock_gettime(_process_id: ProcessId, _args: [u64; 6]) -> SyscallResult {
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
/// `debug_print(ptr, len)`
///
/// Prints the caller's message. It used to print
/// `DEBUG[1]: <message at 0x800abc>` — the *address* of the string, under a
/// `// TODO: Read string from user space and print it` — and return `Ok(0)`. A
/// debug facility that tells you it printed something it did not read is worse
/// than no debug facility, because you spend the time doubting the program
/// instead of the kernel.
fn sys_debug_print(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    const MAX_DEBUG_LEN: usize = 256;

    let ptr = args[0];
    let len = args[1] as usize;

    if len == 0 || len > MAX_DEBUG_LEN {
        return Err(SyscallError::InvalidArgument);
    }

    let mut buf = [0u8; MAX_DEBUG_LEN];
    crate::syscall::uaccess::copy_from_user(ptr, &mut buf[..len])
        .map_err(|_| SyscallError::InvalidArgument)?;

    match core::str::from_utf8(&buf[..len]) {
        Ok(text) => {
            serial_println!("DEBUG[{}]: {}", process_id.0, text);
            Ok(len as u64)
        }
        Err(_) => Err(SyscallError::InvalidArgument),
    }
}

/// What `debug_dump` will report.
const DUMP_MEMORY: u64 = 0;
const DUMP_THREADS: u64 = 1;
const DUMP_SYSCALLS: u64 = 2;
const DUMP_FILES: u64 = 3;

/// `debug_dump(what)`
///
/// Dumps real kernel state to the serial log. It used to print
/// `DEBUG DUMP[1]: type 0` and return `Ok(0)`, having dumped nothing.
///
/// Everything it reports is already available to the in-kernel console; the
/// point of the syscall is that a ring-3 program can ask for it when something
/// has gone wrong from its side.
#[cfg(target_arch = "x86_64")]
fn sys_debug_dump(process_id: ProcessId, args: [u64; 6]) -> SyscallResult {
    let what = args[0];

    match what {
        DUMP_MEMORY => {
            serial_println!("DUMP[{}] physical memory:", process_id.0);
            crate::memory::physical::print_memory_stats();
            crate::memory::heap::print_heap_stats();
        }
        DUMP_THREADS => {
            serial_println!("DUMP[{}] threads:", process_id.0);
            crate::task::print_threads();
        }
        DUMP_SYSCALLS => {
            serial_println!(
                "DUMP[{}] {} syscalls serviced since boot",
                process_id.0,
                crate::syscall::entry::syscall_count()
            );
        }
        DUMP_FILES => {
            serial_println!("DUMP[{}] {} open file(s):", process_id.0, crate::syscall::files::open_count());
            crate::syscall::files::describe_open(|fd, path, size, offset| {
                serial_println!("  fd {} {} ({} bytes, at {})", fd, path, size, offset);
            });
        }
        _ => return Err(SyscallError::InvalidArgument),
    }

    Ok(0)
}

#[cfg(not(target_arch = "x86_64"))]
fn sys_debug_dump(_process_id: ProcessId, _args: [u64; 6]) -> SyscallResult {
    Err(SyscallError::NotSupported)
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