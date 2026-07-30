//! 32-bit Multiboot2 entry trampoline.
//!
//! Multiboot2 hands control to the kernel in **32-bit protected mode with
//! paging disabled**. The rest of the kernel is compiled as 64-bit code linked
//! in the higher half, so something has to bridge both gaps. That is this file.
//!
//! Sequence:
//!   1. `_start32` — stash the multiboot magic/info pointer, set up a stack,
//!      bring COM1 up so we can talk even if everything else fails.
//!   2. Validate the multiboot2 magic and check the CPU actually has long mode.
//!   3. Build a bootstrap map of the first 1 GiB, present at *two* virtual
//!      addresses: identity, and again at `KERNEL_VMA`.
//!   4. CR3 <- PML4, CR4.PAE, EFER.LME, CR0.PG, load a 64-bit GDT, far jump.
//!   5. `long_mode_low` — an absolute 64-bit jump up to the higher half.
//!   6. `long_mode_start` — reload segments, enable SSE (rustc emits SSE for
//!      x86-64 by default), then call the Rust `_start(mb_info_addr)`.
//!
//! Registers at Multiboot2 hand-off: EAX = 0x36D76289, EBX = &boot_info.
//! The System V AMD64 ABI wants the first argument in RDI, so we marshal it.
//!
//! ## Every symbol here has two addresses
//!
//! This file is linked with the rest of the kernel at `KERNEL_VMA + 1 MiB`, but
//! steps 1-4 execute at *physical* 1 MiB with paging off. So every reference to
//! a symbol in 32-bit code is written `sym - KERNEL_VMA`, which the linker
//! resolves to a value that fits in the 32-bit operand. Miss one and the failure
//! is a triple fault with no output — the exact class of bug that cost this
//! project 27 commits the first time round.
//!
//! The one that bites hardest is `movl $p4_table, %eax` before `mov %cr3`:
//! a truncated CR3 faults on the very next instruction, before any of the serial
//! output below can report anything.
//!
//! ## Two maps, briefly
//!
//! The identity map still exists here because the code doing the switch is
//! *running* at a low address; it cannot be removed until execution has moved to
//! the higher half. `paging::init` builds the real tables later and drops it.
//! Both windows point at the same PD, so they cost one extra page table between
//! them.

core::arch::global_asm!(
    r#"
.section .text.boot32, "ax"
.code32
.global _start32
.type _start32, @function

/* Link-time constant, so `sym - KERNEL_VMA` is resolved by the linker into
   something that fits a 32-bit operand. The linker script defines the same
   value; it is repeated here because the assembler needs it too. */
.set KERNEL_VMA, 0xFFFFFFFF80000000

_start32:
    cli
    cld

    /* Stash the Multiboot2 hand-off registers before anything can clobber
       them (CPUID clobbers EBX, and we need EBX free for serial output). */
    movl    %eax, (mb_magic - KERNEL_VMA)
    movl    %ebx, (mb_info - KERNEL_VMA)

    movl    $(stack_top - KERNEL_VMA), %esp
    xorl    %ebp, %ebp

    call    serial_init32

    movl    $(msg_boot32 - KERNEL_VMA), %esi
    call    puts32

    /* Verify we were actually loaded by a Multiboot2-compliant loader. */
    movl    (mb_magic - KERNEL_VMA), %eax
    cmpl    $0x36D76289, %eax
    jne     bad_magic

    call    check_long_mode
    call    setup_page_tables
    call    enable_paging

    /* A 32-bit LGDT takes a 6-byte operand: 2-byte limit, 4-byte *linear* base.
       Paging is on by now and only the identity window is usable from here, so
       the base recorded in the descriptor is physical. This GDT is replaced by
       `gdt::init` long before the identity map goes away. */
    lgdt    (gdt64_ptr32 - KERNEL_VMA)

    /* The far jump takes a 32-bit offset, so it cannot reach the higher half.
       Land at the physical address of a two-instruction stub and jump the rest
       of the way with a full 64-bit immediate. */
    ljmp    $0x08, $(long_mode_low - KERNEL_VMA)

/* --- error paths ------------------------------------------------------- */

bad_magic:
    movl    $(msg_badmagic - KERNEL_VMA), %esi
    call    puts32
    jmp     halt32

no_long_mode:
    movl    $(msg_nolm - KERNEL_VMA), %esi
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
/* Maps physical 0..1 GiB twice: identity, and again at KERNEL_VMA.
 *
 * Both windows share one PD, so the second costs a single extra PDPT. The
 * identity half is what the code below is executing from and cannot be dropped
 * here; the higher half is where the kernel is linked and where it runs from
 * `long_mode_start` onwards. `paging::init` replaces both with real tables.
 *
 * KERNEL_VMA = 0xFFFFFFFF80000000 decomposes as PML4[511], PDPT[510] — the last
 * 2 GiB of the address space, which is exactly the window `-C code-model=kernel`
 * assumes.
 *
 * The first 2 MiB uses 4 KiB pages rather than one huge page, purely so that
 * page 0 can be left unmapped as a null guard. With a flat huge-page map, a
 * null pointer dereference silently reads real memory instead of faulting —
 * which is precisely the class of bug an OS most needs to catch. Everything
 * from 2 MiB up uses 2 MiB pages. */

setup_page_tables:
    /* Zero PML4, PDPT-low, PDPT-high, PD and the first-2MiB PT
       (5 * 4096 bytes = 5120 dwords). */
    movl    $(p4_table - KERNEL_VMA), %edi
    xorl    %eax, %eax
    movl    $5120, %ecx
    rep stosl

    /* PML4[0] -> low PDPT, present + writable */
    movl    $(p3_table - KERNEL_VMA), %eax
    orl     $0x03, %eax
    movl    %eax, (p4_table - KERNEL_VMA)

    /* PML4[511] -> high PDPT, present + writable. Entry 511 is byte 4088. */
    movl    $(p3_high_table - KERNEL_VMA), %eax
    orl     $0x03, %eax
    movl    %eax, (p4_table - KERNEL_VMA + 4088)

    /* PDPT[0] -> PD, present + writable */
    movl    $(p2_table - KERNEL_VMA), %eax
    orl     $0x03, %eax
    movl    %eax, (p3_table - KERNEL_VMA)

    /* high PDPT[510] -> the *same* PD. Entry 510 is byte 4080. */
    movl    $(p2_table - KERNEL_VMA), %eax
    orl     $0x03, %eax
    movl    %eax, (p3_high_table - KERNEL_VMA + 4080)

    /* PD[0] -> PT (4 KiB pages), present + writable, NOT huge */
    movl    $(p1_table - KERNEL_VMA), %eax
    orl     $0x03, %eax
    movl    %eax, (p2_table - KERNEL_VMA)

    /* PT[i] = (i * 4KiB) | present | writable, for i = 1..511.
       PT[0] is deliberately left zero: unmapped null guard page. */
    movl    $1, %ecx
2:
    movl    %ecx, %eax
    shll    $12, %eax
    orl     $0x03, %eax
    movl    $(p1_table - KERNEL_VMA), %edi
    movl    %eax, (%edi, %ecx, 8)
    movl    $0, 4(%edi, %ecx, 8)
    incl    %ecx
    cmpl    $512, %ecx
    jne     2b

    /* PD[i] = (i * 2MiB) | present | writable | huge, for i = 1..511. */
    movl    $1, %ecx
1:
    movl    $0x200000, %eax
    mull    %ecx                    /* EDX:EAX = 2MiB * i */
    orl     $0x83, %eax
    movl    $(p2_table - KERNEL_VMA), %edi
    movl    %eax, (%edi, %ecx, 8)
    movl    %edx, 4(%edi, %ecx, 8)
    incl    %ecx
    cmpl    $512, %ecx
    jne     1b
    ret

/* --- switch the CPU into long mode ------------------------------------- */

enable_paging:
    movl    $(p4_table - KERNEL_VMA), %eax
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

/* Reached by the far jump, still executing at a physical address. The only job
   here is to get RIP into the higher half; `movabsq` is the only form that can
   name a 64-bit target. */
long_mode_low:
    movabsq $long_mode_start, %rax
    jmp     *%rax

long_mode_start:
    movw    $0x10, %ax
    movw    %ax, %ss
    movw    %ax, %ds
    movw    %ax, %es
    movw    %ax, %fs
    movw    %ax, %gs

    movabsq $stack_top, %rsp
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
gdt64_end:

/* Two descriptors for one table. The 32-bit LGDT above consumes 6 bytes and
   needs the physical base; the 64-bit form consumes 10 and takes the higher-half
   one. Sharing a single descriptor between the two modes is a silent way to load
   a GDTR pointing at the low 32 bits of a higher-half address. */
gdt64_ptr32:
    .word   gdt64_end - gdt64 - 1
    .long   gdt64 - KERNEL_VMA

.balign 8
gdt64_ptr:
    .word   gdt64_end - gdt64 - 1
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
p3_high_table:
    .skip   4096
p1_table:
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
