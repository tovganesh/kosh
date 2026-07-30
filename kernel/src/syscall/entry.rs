//! The `syscall`/`sysret` fast-path entry point.
//!
//! Before Phase 5 there was no way into the kernel from user mode at all.
//! `syscall/mod.rs` had `setup_syscall_interrupt()` — a `serial_println!`, a
//! `// TODO: Set up IDT entry for interrupt 0x80`, and `Ok(())` — and
//! `syscall_entry()`, documented as "called from assembly interrupt handler",
//! with no assembly interrupt handler anywhere in the tree. The dispatcher
//! below it was real and well-validated, and completely unreachable.
//!
//! ## What the instruction does and does not do
//!
//! `syscall` is fast because it does almost nothing: it loads CS/SS from
//! `STAR`, RIP from `LSTAR`, masks RFLAGS with `SFMASK`, stashes the caller's
//! RIP in RCX and RFLAGS in R11 — and *leaves RSP pointing at the user stack*.
//! Everything else is the kernel's job. In particular the stack switch is
//! manual, which is the first thing the stub below does.
//!
//! ## Register ABI
//!
//! Linux's convention, which the existing userspace code already assumes:
//!
//! | register | meaning |
//! |---|---|
//! | RAX | syscall number, and the return value |
//! | RDI RSI RDX R10 R8 R9 | arguments 1-6 |
//! | RCX R11 | clobbered by the instruction itself |
//!
//! Argument 4 lives in R10 rather than RCX precisely because `syscall`
//! destroys RCX.

use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

use crate::process::ProcessId;
use crate::serial_println;

/// Registers the stub pushes, in push order (so the first field is at the
/// lowest address — the layout the stub builds by pushing RAX last).
#[repr(C)]
pub struct SyscallFrame {
    pub rax: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub r10: u64,
    pub r8: u64,
    pub r9: u64,
    /// User RIP, delivered by the CPU in RCX. `sysretq` needs it back there.
    pub rip: u64,
    /// User RFLAGS, delivered in R11. `sysretq` needs it back there.
    pub rflags: u64,
    /// The caller's RSP, saved *on this thread's kernel stack* rather than in a
    /// global. Pushed first, so it sits at the highest address in the frame.
    ///
    /// This is what makes a preempted syscall safe. It used to live in a static
    /// `SAVED_USER_RSP`, which meant a second thread entering a syscall while
    /// the first was blocked inside one would overwrite the first thread's user
    /// stack pointer — and `sysretq` would then return it to ring 3 with
    /// somebody else's RSP.
    pub user_rsp: u64,
}

extern "C" {
    fn kosh_syscall_entry();
}

core::arch::global_asm!(
    r#"
.section .text.kosh_syscall, "ax"
.global kosh_syscall_entry
.type kosh_syscall_entry, @function

kosh_syscall_entry:
    /* On entry: RSP is still the *user* stack, RCX = user RIP,
       R11 = user RFLAGS, RAX = syscall number. Interrupts are already masked
       by SFMASK, which is what makes the scratch slot below safe.

       No register is free — RCX and R11 carry the return state, everything
       else is the caller's — so the hop from the user stack to the kernel one
       has to go through memory. gs: addressing needs no register at all. */

    movq    %rsp, %gs:8         /* park the user RSP in the per-CPU scratch */
    movq    %gs:0, %rsp         /* this thread's own kernel stack           */

    /* Build SyscallFrame. Pushed high field first so RAX lands lowest.
       The scratch value moves onto the kernel stack immediately: from here on
       it is per-thread, so a preemption cannot lose it. */
    pushq   %gs:8               /* user_rsp */
    pushq   %r11                /* rflags   */
    pushq   %rcx                /* rip      */
    pushq   %r9
    pushq   %r8
    pushq   %r10
    pushq   %rdx
    pushq   %rsi
    pushq   %rdi
    pushq   %rax

    movq    %rsp, %rdi          /* &mut SyscallFrame */

    /* SysV wants RSP 16-byte aligned at the call. Ten pushes from a 16-aligned
       top land back on 16, so there is nothing to correct — unlike the nine
       pushes this had before user_rsp joined the frame. */
    call    kosh_syscall_handler

    movq    %rax, (%rsp)        /* return value into frame.rax */

    popq    %rax
    popq    %rdi
    popq    %rsi
    popq    %rdx
    popq    %r10
    popq    %r8
    popq    %r9
    popq    %rcx                /* sysretq takes the target RIP here    */
    popq    %r11                /* ...and the target RFLAGS here        */

    movq    (%rsp), %rsp        /* frame.user_rsp — the last word we own */
    sysretq
"#,
    options(att_syntax)
);

/// Rust side of the syscall entry. Called by the stub with the saved frame.
#[no_mangle]
pub extern "C" fn kosh_syscall_handler(frame: &mut SyscallFrame) -> u64 {
    let number = frame.rax;
    let args = [frame.rdi, frame.rsi, frame.rdx, frame.r10, frame.r8, frame.r9];

    SYSCALL_COUNT.fetch_add(1, Ordering::Relaxed);

    // The calling thread's id, which is now a real answer rather than the
    // hardcoded `1` this used while only one thread could be in ring 3. It is
    // also what `spawn` returns and what `wait` takes, so a log line naming a
    // process can be matched against one naming a task.
    let pid = ProcessId::new(crate::task::current_id() as u32);

    match crate::syscall::dispatcher::dispatch_syscall(pid, number, args) {
        Ok(value) => value,
        Err(err) => {
            if crate::syscall::dispatcher::syscall_trace_enabled() {
                serial_println!("syscall {} returning error {:?}", number, err);
            }

            // Linux convention: errors come back as a negative value in RAX,
            // successes as a non-negative one, and userspace tests the sign.
            //
            // Note `SyscallError::to_errno()` already returns *negative*
            // numbers (-22 for EINVAL, and so on), which is unusual for
            // something named "errno". Negating it here — the obvious thing to
            // write — flips errors positive, and userspace then reads every
            // failure as success. Normalise instead of assuming.
            let errno = err.to_errno() as i64;
            let negative = if errno > 0 { -errno } else { errno };
            negative as u64
        }
    }
}

static SYSCALL_COUNT: AtomicU64 = AtomicU64::new(0);

/// Syscalls serviced since boot.
pub fn syscall_count() -> u64 {
    SYSCALL_COUNT.load(Ordering::Relaxed)
}

/// Program the MSRs that make `syscall` work.
pub fn init() {
    let sel = crate::gdt::selectors();

    unsafe {
        // Without SCE, `syscall` is an invalid opcode.
        Efer::update(|flags| flags.insert(EferFlags::SYSTEM_CALL_EXTENSIONS));

        // The crate checks the descriptor arithmetic described in `gdt.rs` and
        // refuses selectors that would make `sysret` land somewhere absurd.
        Star::write(sel.user_code, sel.user_data, sel.kernel_code, sel.kernel_data)
            .expect("GDT selector layout is incompatible with sysret");

        LStar::write(VirtAddr::new(kosh_syscall_entry as usize as u64));

        // Bits cleared in RFLAGS on entry. Masking IF means a syscall starts
        // uninterruptible, which is what makes the per-CPU scratch slot in the
        // stub safe; a handler that wants to block re-enables it deliberately.
        // DF is masked so string instructions behave; TF so a single-stepping
        // debugger in userspace does not trap into the kernel.
        SFMask::write(
            RFlags::INTERRUPT_FLAG | RFlags::DIRECTION_FLAG | RFlags::TRAP_FLAG,
        );
    }

    serial_println!("Syscall interface (SYSCALL/SYSRET):");
    serial_println!("  EFER.SCE enabled");
    serial_println!(
        "  STAR: syscall CS 0x{:x}/SS 0x{:x}, sysret CS 0x{:x}/SS 0x{:x}",
        sel.kernel_code.0,
        sel.kernel_data.0,
        sel.user_code.0 | 3,
        sel.user_data.0 | 3
    );
    serial_println!("  LSTAR -> 0x{:x}", kosh_syscall_entry as usize as u64);
    serial_println!(
        "  kernel stack from gs:0 = 0x{:x} (per-thread), gs addressing {}",
        crate::percpu::syscall_stack(),
        if crate::percpu::check_gs_addressing() {
            "OK"
        } else {
            "BROKEN"
        }
    );
    serial_println!("  SFMASK masks IF, DF, TF");
}
