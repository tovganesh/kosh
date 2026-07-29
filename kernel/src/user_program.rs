//! A ring-3 payload, in assembly.
//!
//! Assembly rather than Rust for one reason: this blob is *linked* inside the
//! kernel image at ~1 MiB but *runs* at `usermode::USER_CODE_BASE`. Every
//! reference here is RIP-relative, so it does not care. Rust compiled with
//! `-C code-model=kernel` would happily emit an absolute reference to a static
//! and fault the moment it ran at the wrong address.
//!
//! It exercises three things:
//!
//! 1. A syscall that works — `write(1, ...)`.
//! 2. A syscall that returns a value — `getpid()`.
//! 3. A syscall the kernel must *refuse* — `write(1, <kernel address>, 16)`.
//!    That last one is the interesting case: it proves the user-pointer
//!    validation in `syscall::uaccess` actually rejects a hostile pointer,
//!    tested from the user side rather than by the kernel checking itself.

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
