/* Kosh kernel link script.
 *
 * The ELF entry point is the 32-bit trampoline in src/boot32.rs, NOT the Rust
 * `_start` — Multiboot2 hands off in 32-bit protected mode.
 */

ENTRY(_start32)

SECTIONS
{
    . = 1M;

    __kernel_start = .;

    .multiboot2 ALIGN(8) :
    {
        KEEP(*(.multiboot2))
    }

    __text_start = .;
    .text ALIGN(4K) :
    {
        KEEP(*(.text.boot32))
        *(.text .text.*)
    }
    . = ALIGN(4K);
    __text_end = .;

    __rodata_start = .;
    .rodata ALIGN(4K) :
    {
        KEEP(*(.rodata.boot32))
        *(.rodata .rodata.*)
    }
    . = ALIGN(4K);
    __rodata_end = .;

    /* Ring-3 payload. Its own page-aligned section so it can be mapped into
       user space independently of anything the kernel keeps to itself. */
    . = ALIGN(4K);
    __user_start = .;
    .user ALIGN(4K) :
    {
        KEEP(*(.user))
    }
    . = ALIGN(4K);
    __user_end = .;

    .data ALIGN(4K) :
    {
        *(.data .data.*)
        *(.got .got.plt)
    }

    .bss ALIGN(4K) :
    {
        *(COMMON)
        *(.bss .bss.*)
    }

    . = ALIGN(4K);
    __kernel_end = .;

    /DISCARD/ :
    {
        *(.comment)
        *(.eh_frame)
        *(.eh_frame_hdr)
        *(.note .note.*)
    }
}
