//! `fork` and `exec`.
//!
//! ## What was here before
//!
//! `sys_fork` used to add a row to the process table, log "Fork successful", and
//! return the new PID — for a child with no address space, no stack, no context
//! and no way to run. Userspace read the positive return value, concluded it was
//! the parent, and carried on. It was replaced by an honest `NotSupported` three
//! phases ago; this is the implementation that refusal was waiting for.
//!
//! ## The two halves of fork
//!
//! **Memory** is [`AddressSpace::fork`]: a copy of the parent's lower half, same
//! contents at the same addresses in different frames.
//!
//! **Control flow** is [`crate::task::spawn_forked`]. The child has to appear in
//! ring 3 at the parent's RIP with the parent's registers and `rax = 0`, without
//! ever having executed the `syscall` instruction. That is arranged by writing a
//! copy of the parent's `SyscallFrame` onto the child's fresh kernel stack and
//! pointing the context switch's `ret` at the syscall stub's return path.
//!
//! The parent's user RSP is reused unchanged, which is only correct because the
//! child has its own address space: the same number names the child's own copy
//! of that stack.
//!
//! ## Why `spawn` still exists
//!
//! `spawn` is `fork`+`exec` fused, and it is what a shell should keep using when
//! it has nothing to do between the two. The pair earns its place when there
//! *is* something to do in the child before the new image arrives — setting up
//! redirections, closing descriptors — which is the shape a shell needs and
//! which `spawn` cannot express.

use crate::serial_println;
use crate::syscall::entry::SyscallFrame;
use crate::syscall::{SyscallError, SyscallResult};

/// `fork()` -> child task id in the parent, 0 in the child
///
/// Reached directly from `kosh_syscall_handler` rather than through the
/// dispatcher, because it is the one syscall that needs the caller's whole
/// register frame and not just its arguments.
pub fn sys_fork(frame: &SyscallFrame) -> SyscallResult {
    let parent = crate::task::current_id();

    // A thread with no address space of its own is a kernel thread. Forking one
    // would produce a child with a user register frame and nowhere to run it.
    let space = match crate::task::current_address_space_pml4() {
        Some(pml4) => pml4,
        None => {
            serial_println!("Thread {} called fork without an address space", parent);
            return Err(SyscallError::NotSupported);
        }
    };

    // Takes the PML4 by value rather than an `&AddressSpace`, because the space
    // is owned by the thread table and borrowing it across a lock we would have
    // to hold for the whole copy is worse than passing the one number the copy
    // needs.
    let (child_space, copied) = crate::memory::address_space::fork_from(space).map_err(|e| {
        serial_println!("fork failed: {}", e);
        SyscallError::OutOfMemory
    })?;

    serial_println!(
        "Thread {} forking: {} page(s) copied into PML4 0x{:x}",
        parent,
        copied,
        child_space.pml4_phys()
    );

    match crate::task::spawn_forked("forked", frame, child_space) {
        Ok(child) => {
            crate::usermode::register_forked(child, parent);
            Ok(child as u64)
        }
        Err(e) => {
            serial_println!("fork could not create a thread: {}", e);
            Err(SyscallError::ResourceExhausted)
        }
    }
}

/// `exec(path_ptr, path_len)` — never returns on success
///
/// Replaces the calling thread's program: a new address space with the named
/// boot module loaded, swapped in, and entered. The old address space is freed
/// once the new one is live.
///
/// Unlike `fork`, this does not need the caller's register frame — there is
/// nothing to return to. The syscall frame on the kernel stack is simply
/// abandoned when `enter_ring3` takes over.
pub fn sys_exec(args: [u64; 6]) -> SyscallResult {
    let name = crate::syscall::files::path_from_user(args[0], args[1])?;
    let name = name.trim_start_matches('/');

    let thread = crate::task::current_id();

    if crate::task::current_address_space_pml4().is_none() {
        serial_println!("Thread {} called exec without an address space", thread);
        return Err(SyscallError::NotSupported);
    }

    serial_println!("Thread {} exec '{}':", thread, name);

    // Build the replacement completely before disturbing anything. A failure
    // here has to leave the caller running its current program — an `exec` that
    // half-succeeds has destroyed the only thing it could return to.
    let (space, entry) = crate::usermode::prepare_named_program(name)?;

    crate::usermode::rename_program(thread, name);

    // Swap, then free. The new space is in CR3 before the old one is touched;
    // the reverse order frees the page tables the CPU is walking.
    if let Some(old) = crate::task::replace_address_space(space) {
        let freed = unsafe { old.free() };
        serial_println!("  old image released: {} frame(s)", freed);
    }

    serial_println!("  entering '{}' at 0x{:x}", name, entry);
    unsafe { crate::usermode::enter_ring3_at(entry, crate::usermode::USER_STACK_TOP) }
}
