//! The context switch itself.
//!
//! Written as a `global_asm!` symbol rather than inline `asm!` inside a Rust
//! function, because a context switch has to control the stack precisely and a
//! compiler-generated prologue would get in the way.
//!
//! ## Why this is a plain `extern "C"` call and not interrupt magic
//!
//! `kosh_switch_context` follows the System V AMD64 C ABI, so the *caller*
//! has already spilled anything it cares about in caller-saved registers. All
//! this function must preserve is the callee-saved set — RBX, RBP, R12-R15 —
//! plus RFLAGS. It pushes those onto the outgoing thread's stack, records RSP
//! in the outgoing TCB, loads RSP from the incoming TCB, pops the same set back
//! and returns.
//!
//! The consequence is the part worth internalising: **the incoming thread
//! resumes by returning out of its own earlier call to this function.** Thread B
//! returns into B's `schedule()`, which returns into B's timer handler, whose
//! `iretq` resumes whatever B was doing when it was preempted. Nothing has to
//! reconstruct an interrupt frame, because B's frame never left B's stack.
//!
//! ## What the previous implementation did
//!
//! `process/context.rs` had `save_current_context` / `restore_context` with 18
//! identical `in(reg) context` operands, saved `[rsp]` as the resume address,
//! never touched segment selectors — and had zero callers. Its
//! "context switching test" built two `CpuContext` structs, printed them, and
//! asserted the fields it had just set. No switch ever happened.

/// Save the current thread's callee-saved state, then resume `next_rsp`.
///
/// # Safety
/// `prev_rsp` must point at the outgoing thread's saved-RSP slot, and
/// `next_rsp` must be a stack prepared either by a previous call to this
/// function or by [`super::Thread::prepare_stack`]. Interrupts must be
/// disabled: the scheduler's lock is released before the switch, so a timer
/// tick landing here would re-enter the scheduler with half-updated state.
extern "C" {
    pub fn kosh_switch_context(prev_rsp: *mut u64, next_rsp: u64);
}

core::arch::global_asm!(
    r#"
.section .text.kosh_switch, "ax"
.global kosh_switch_context
.type kosh_switch_context, @function

kosh_switch_context:
    /* rdi = &prev.rsp, rsi = next.rsp */

    pushq   %rbp
    pushq   %rbx
    pushq   %r12
    pushq   %r13
    pushq   %r14
    pushq   %r15
    pushfq

    movq    %rsp, (%rdi)        /* outgoing thread: remember where we parked */
    movq    %rsi, %rsp          /* incoming thread: adopt its stack           */

    popfq
    popq    %r15
    popq    %r14
    popq    %r13
    popq    %r12
    popq    %rbx
    popq    %rbp
    ret
"#,
    options(att_syntax)
);
