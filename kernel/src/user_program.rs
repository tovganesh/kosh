//! A ring-3 payload, in assembly.
//!
//! Assembly rather than Rust for one reason: this blob is *linked* inside the
//! kernel image at ~1 MiB but *runs* at `usermode::USER_CODE_BASE`. Every
//! reference here is RIP-relative, so it does not care. Rust compiled with
//! `-C code-model=kernel` would happily emit an absolute reference to a static
//! and fault the moment it ran at the wrong address.
//!
//! It exercises four things:
//!
//! 1. A syscall that works — `write(1, ...)`.
//! 2. A syscall that returns a value — `getpid()`.
//! 3. A syscall the kernel must *refuse* — `write(1, <kernel address>, 16)`.
//!    That one is the interesting case: it proves the user-pointer validation in
//!    `syscall::uaccess` actually rejects a hostile pointer, tested from the user
//!    side rather than by the kernel checking itself.
//! 4. Two ring-3 threads yielding from inside a system call
//!    (`kosh_user_pingpong_entry`), which is what per-thread kernel stacks are
//!    for and what a single shared syscall stack cannot survive.

core::arch::global_asm!(
    r#"
.section .user, "ax"
.balign 4096

.global kosh_user_entry
.type kosh_user_entry, @function

kosh_user_entry:
    /* write(1, hello, len) */
    movq    $1, %rdi
    leaq    msg_hello(%rip), %rsi
    movq    $msg_hello_len, %rdx
    movq    $23, %rax                   /* SYS_WRITE */
    syscall

    /* getpid() */
    movq    $5, %rax                    /* SYS_GETPID */
    syscall
    movq    %rax, %r12                  /* keep it; callee-saved across syscall */

    /* write(1, <physmap base>, 16) -- the kernel must refuse this. */
    movq    $1, %rdi
    movabsq $0xFFFF800000000000, %rsi
    movq    $16, %rdx
    movq    $23, %rax
    syscall

    /* Linux convention: errors come back as a negative value. */
    testq   %rax, %rax
    jns     .Lnot_rejected

    movq    $1, %rdi
    leaq    msg_rejected(%rip), %rsi
    movq    $msg_rejected_len, %rdx
    movq    $23, %rax
    syscall
    jmp     .Ldone

.Lnot_rejected:
    movq    $1, %rdi
    leaq    msg_leaked(%rip), %rsi
    movq    $msg_leaked_len, %rdx
    movq    $23, %rax
    syscall

.Ldone:
    /* exit(0) */
    xorq    %rdi, %rdi
    movq    $1, %rax                    /* SYS_EXIT */
    syscall

    /* exit does not return. If it somehow does, spin rather than run off the
       end of the mapping. */
.Lhang:
    jmp     .Lhang

/* A second payload: touch kernel memory directly, with no syscall involved.
   Page protection — not argument validation — has to stop this one. The kernel
   must kill the process and carry on. */
.global kosh_user_fault_entry
.type kosh_user_fault_entry, @function
kosh_user_fault_entry:
    movq    $1, %rdi
    leaq    msg_faulting(%rip), %rsi
    movq    $msg_faulting_len, %rdx
    movq    $23, %rax
    syscall

    movabsq $0xFFFF800000000000, %rax
    movq    (%rax), %rbx                /* #PF: kernel page, no USER bit */

    /* Never reached. */
    movq    $1, %rdi
    leaq    msg_survived(%rip), %rsi
    movq    $msg_survived_len, %rdx
    movq    $23, %rax
    syscall
.Lfault_hang:
    jmp     .Lfault_hang

/* A third payload, run as two concurrent ring-3 threads.
   It exists to make per-thread kernel stacks *observably* necessary: each
   iteration calls SYS_YIELD, which context-switches while this thread's
   SyscallFrame is still live on its kernel stack, and then writes one byte. If
   both threads shared one syscall stack, the second thread's entry would take
   that stack from the top and overwrite the first thread's parked frame,
   including its return RIP and user RSP.

   The tag byte arrives in RDI — the kernel sets it before iretq — and is copied
   onto the user stack, because `write` needs an address it can read and this
   payload has no writable data section of its own. */
.global kosh_user_pingpong_entry
.type kosh_user_pingpong_entry, @function
kosh_user_pingpong_entry:
    andq    $-16, %rsp
    subq    $64, %rsp
    movq    %rsp, %r13                  /* callee-saved: survives syscalls */
    movb    %dil, (%r13)                /* tag byte, now in user memory */

    /* Assemble "<tag>: survived ...\n" in one buffer, so the completion line
       goes out as a single write. Two writes would let the other thread's byte
       land between the tag and the message. */
    leaq    1(%r13), %rdi
    leaq    msg_pp_done(%rip), %rsi
    movq    $msg_pp_done_len, %rcx
    rep     movsb
    movq    $msg_pp_done_len + 1, %r15

    movq    $12, %r14                   /* iterations */

.Lpp_loop:
    movq    $8, %rax                    /* SYS_YIELD, from inside a syscall */
    syscall
    testq   %rax, %rax
    js      .Lpp_fail

    movq    $1, %rdi                    /* write(1, &tag, 1) */
    movq    %r13, %rsi
    movq    $1, %rdx
    movq    $23, %rax
    syscall
    testq   %rax, %rax
    js      .Lpp_fail

    decq    %r14
    jnz     .Lpp_loop

    /* Reaching here at all is the result: 12 round trips through a syscall that
       gave up the CPU with its frame still on this thread's kernel stack. */
    movq    $1, %rdi
    movq    %r13, %rsi
    movq    %r15, %rdx
    movq    $23, %rax
    syscall

    xorq    %rdi, %rdi
    movq    $1, %rax                    /* SYS_EXIT */
    syscall

.Lpp_fail:
    movq    $1, %rdi
    leaq    msg_pp_fail(%rip), %rsi
    movq    $msg_pp_fail_len, %rdx
    movq    $23, %rax
    syscall

    movq    $1, %rdi
    movq    $1, %rax                    /* exit(1) */
    syscall

.Lpp_hang:
    jmp     .Lpp_hang

msg_pp_done:
    .ascii  ": survived 12 yields inside a syscall\n"
    .set    msg_pp_done_len, . - msg_pp_done

msg_pp_fail:
    .ascii  "FAIL: a syscall failed across a yield\n"
    .set    msg_pp_fail_len, . - msg_pp_fail

msg_faulting:
    .ascii  "about to dereference a kernel address directly\n"
    .set    msg_faulting_len, . - msg_faulting

msg_survived:
    .ascii  "WARNING: ring 3 read kernel memory without faulting\n"
    .set    msg_survived_len, . - msg_survived

msg_hello:
    .ascii  "hello from ring 3\n"
    .set    msg_hello_len, . - msg_hello

msg_rejected:
    .ascii  "kernel rejected my out-of-bounds pointer\n"
    .set    msg_rejected_len, . - msg_rejected

msg_leaked:
    .ascii  "WARNING: kernel accepted a kernel-half pointer from ring 3\n"
    .set    msg_leaked_len, . - msg_leaked

.balign 4096
"#,
    options(att_syntax)
);
