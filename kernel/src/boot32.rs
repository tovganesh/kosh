//! 32-bit Multiboot2 entry trampoline.
//!
//! Multiboot2 hands control to the kernel in **32-bit protected mode with
//! paging disabled**. The rest of the kernel is compiled as 64-bit code, so
//! something has to bridge the gap. That is this file.
//!
//! Sequence:
//!   1. `_start32` — stash the multiboot magic/info pointer, set up a stack,
//!      bring COM1 up so we can talk even if everything else fails.
//!   2. Validate the multiboot2 magic and check the CPU actually has long mode.
//!   3. Build a bootstrap identity map of the first 1 GiB using 2 MiB pages.
//!   4. CR3 <- PML4, CR4.PAE, EFER.LME, CR0.PG, load a 64-bit GDT, far jump.
//!   5. `long_mode_start` — reload segments, enable SSE (rustc emits SSE for
//!      x86-64 by default), then call the Rust `_start(mb_info_addr)`.
//!
//! Registers at Multiboot2 hand-off: EAX = 0x36D76289, EBX = &boot_info.
//! The System V AMD64 ABI wants the first argument in RDI, so we marshal it.

core::arch::global_asm!(
    r#"
.section .text.boot32, "ax"
.code32
.global _start32
.type _start32, @function

_start32:
    cli
    cld

    /* Stash the Multiboot2 hand-off registers before anything can clobber
       them (CPUID clobbers EBX, and we need EBX free for serial output). */
    movl    %eax, mb_magic
    movl    %ebx, mb_info

    movl    $stack_top, %esp
    xorl    %ebp, %ebp

    call    serial_init32

    movl    $msg_boot32, %esi
    call    puts32

    /* Verify we were actually loaded by a Multiboot2-compliant loader. */
    movl    mb_magic, %eax
    cmpl    $0x36D76289, %eax
    jne     bad_magic

    call    check_long_mode
    call    setup_page_tables
    call    enable_paging

    lgdt    gdt64_ptr
    ljmp    $0x08, $long_mode_start

/* --- error paths ------------------------------------------------------- */

bad_magic:
    movl    $msg_badmagic, %esi
    call    puts32
    jmp     halt32

no_long_mode:
    movl    $msg_nolm, %esi
    call    puts32
    jmp     halt32

halt32:
    cli
    hlt
    jmp     halt32

/* --- long mode capability check ---------------------------------------- */

check_long_mode:
    movl    $0x80000000, %eax
    cpuid
    cmpl    $0x80000001, %eax
    jb      no_long_mode

    movl    $0x80000001, %eax
    cpuid
    testl   $(1 << 29), %edx        /* LM bit */
    jz      no_long_mode
    ret

/* --- bootstrap page tables --------------------------------------------- */
/* Identity-maps 0..1GiB with 2 MiB pages. That covers the kernel image at
   1 MiB, the frame bitmap, the heap, VGA at 0xB8000 and QEMU's default RAM. */

setup_page_tables:
    /* Zero PML4, PDPT and PD (3 * 4096 bytes = 3072 dwords). */
    movl    $p4_table, %edi
    xorl    %eax, %eax
    movl    $3072, %ecx
    rep stosl

    /* PML4[0] -> PDPT, present + writable */
    movl    $p3_table, %eax
    orl     $0x03, %eax
    movl    %eax, p4_table

    /* PDPT[0] -> PD, present + writable */
    movl    $p2_table, %eax
    orl     $0x03, %eax
    movl    %eax, p3_table

    /* PD[i] = (i * 2MiB) | present | writable | huge */
    xorl    %ecx, %ecx
1:
    movl    $0x200000, %eax
    mull    %ecx                    /* EDX:EAX = 2MiB * i */
    orl     $0x83, %eax
    movl    $p2_table, %edi
    movl    %eax, (%edi, %ecx, 8)
    movl    %edx, 4(%edi, %ecx, 8)
    incl    %ecx
    cmpl    $512, %ecx
    jne     1b
    ret

/* --- switch the CPU into long mode ------------------------------------- */

enable_paging:
    movl    $p4_table, %eax
    movl    %eax, %cr3

    movl    %cr4, %eax
    orl     $(1 << 5), %eax         /* CR4.PAE */
    movl    %eax, %cr4

    movl    $0xC0000080, %ecx       /* IA32_EFER */
    rdmsr
    orl     $(1 << 8), %eax         /* EFER.LME */
    wrmsr

    movl    %cr0, %eax
    orl     $(1 << 31), %eax        /* CR0.PG */
    movl    %eax, %cr0
    ret

/* --- early COM1 output (32-bit) ---------------------------------------- */
/* The uart_16550 crate re-initialises this later; we need output *now* so a
   failure between here and Rust is visible rather than a silent reboot. */

serial_init32:
    movw    $0x3F9, %dx             /* IER: disable interrupts */
    xorb    %al, %al
    outb    %al, %dx
    movw    $0x3FB, %dx             /* LCR: enable DLAB */
    movb    $0x80, %al
    outb    %al, %dx
    movw    $0x3F8, %dx             /* divisor low = 3 (38400 baud) */
    movb    $0x03, %al
    outb    %al, %dx
    movw    $0x3F9, %dx             /* divisor high = 0 */
    xorb    %al, %al
    outb    %al, %dx
    movw    $0x3FB, %dx             /* LCR: 8N1, DLAB off */
    movb    $0x03, %al
    outb    %al, %dx
    movw    $0x3FA, %dx             /* FCR: enable + clear FIFOs */
    movb    $0xC7, %al
    outb    %al, %dx
    movw    $0x3FC, %dx             /* MCR: DTR + RTS + OUT2 */
    movb    $0x0B, %al
    outb    %al, %dx
    ret

putc32:                             /* char in AL */
    pushl   %ebx
    pushl   %edx
    movb    %al, %bl
1:
    movw    $0x3FD, %dx             /* LSR */
    inb     %dx, %al
    testb   $0x20, %al              /* transmitter holding register empty */
    jz      1b
    movw    $0x3F8, %dx
    movb    %bl, %al
    outb    %al, %dx
    popl    %edx
    popl    %ebx
    ret

puts32:                             /* NUL-terminated string in ESI */
    pushl   %esi
1:
    movzbl  (%esi), %eax
    testb   %al, %al
    je      2f
    call    putc32
    incl    %esi
    jmp     1b
2:
    popl    %esi
    ret

/* --- 64-bit land -------------------------------------------------------- */

.code64
long_mode_start:
    movw    $0x10, %ax
    movw    %ax, %ss
    movw    %ax, %ds
    movw    %ax, %es
    movw    %ax, %fs
    movw    %ax, %gs

    movq    $stack_top, %rsp
    xorq    %rbp, %rbp

    /* rustc emits SSE instructions for x86-64 unconditionally, so the FPU/SSE
       state has to be usable before we enter any Rust code. */
    movq    %cr0, %rax
    andq    $-5, %rax               /* clear CR0.EM */
    orq     $2, %rax                /* set CR0.MP */
    movq    %rax, %cr0
    movq    %cr4, %rax
    orq     $(3 << 9), %rax         /* CR4.OSFXSR | CR4.OSXMMEXCPT */
    movq    %rax, %cr4
    fninit

    leaq    msg_boot64(%rip), %rsi
    call    puts64

    /* System V AMD64: first argument goes in RDI. */
    movl    mb_info(%rip), %edi
    call    _start

    /* _start is `-> !`, but be defensive. */
1:
    cli
    hlt
    jmp     1b

putc64:                             /* char in AL */
    movb    %al, %bl
1:
    movw    $0x3FD, %dx
    inb     %dx, %al
    testb   $0x20, %al
    jz      1b
    movw    $0x3F8, %dx
    movb    %bl, %al
    outb    %al, %dx
    ret

puts64:                             /* NUL-terminated string in RSI */
1:
    movzbl  (%rsi), %eax
    testb   %al, %al
    je      2f
    call    putc64
    incq    %rsi
    jmp     1b
2:
    ret

/* --- data --------------------------------------------------------------- */

.section .rodata.boot32, "a"
.balign 8
gdt64:
    .quad   0
    .quad   0x00AF9A000000FFFF      /* 0x08: 64-bit code, ring 0 */
    .quad   0x00CF92000000FFFF      /* 0x10: data, ring 0 */
gdt64_ptr:
    .word   gdt64_ptr - gdt64 - 1
    .quad   gdt64

msg_boot32:
    .asciz  "[boot] 32-bit protected mode entry OK\r\n"
msg_boot64:
    .asciz  "[boot] long mode OK, entering Rust\r\n"
msg_badmagic:
    .asciz  "[boot] FATAL: bad Multiboot2 magic\r\n"
msg_nolm:
    .asciz  "[boot] FATAL: CPU does not support long mode\r\n"

.section .bss.boot32, "aw", @nobits
.balign 4096
p4_table:
    .skip   4096
p3_table:
    .skip   4096
p2_table:
    .skip   4096
mb_magic:
    .skip   4
mb_info:
    .skip   4
.balign 16
stack_bottom:
    .skip   65536
stack_top:
"#,
    options(att_syntax)
);
