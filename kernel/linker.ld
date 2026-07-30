/* Kosh kernel link script.
 *
 * The ELF entry point is the 32-bit trampoline in src/boot32.rs, NOT the Rust
 * `_start` — Multiboot2 hands off in 32-bit protected mode.
 *
 * ## Two addresses for everything
 *
 * The kernel is *linked* at KERNEL_VMA + 1 MiB and *loaded* at 1 MiB physical.
 * Every output section therefore carries an explicit LMA via `AT()`, so the ELF
 * program headers end up with `p_vaddr` in the higher half and `p_paddr` at
 * 1 MiB. GRUB loads by `p_paddr`, which is what puts the image where the 32-bit
 * trampoline can reach it; everything after the jump to long mode runs at
 * `p_vaddr`.
 *
 * ## Why the higher half at all
 *
 * Until now the kernel lived in PML4[0] — the same top-level entry every user
 * program needs for its own image and stack. Giving a process its own address
 * space means giving it its own PML4[0], which is impossible while the kernel
 * is in there too. Moving the kernel to PML4[511] is what makes per-process
 * address spaces possible.
 *
 * -2 GiB specifically, because that is what `-C code-model=kernel` assumes: it
 * lets the compiler use 32-bit sign-extended displacements everywhere, which is
 * why that flag was already in `.cargo/config.toml` long before it was true.
 */

KERNEL_VMA = 0xFFFFFFFF80000000;

/* `e_entry` is a *virtual* address, even though GRUB jumps to it in 32-bit
 * protected mode with paging off. GRUB finds the program header whose
 * `p_vaddr` range contains the entry and rebases it into that header's
 * `p_paddr` range, so it arrives at physical 1 MiB on its own.
 *
 * Naming the physical address here instead gets `error: entry point isn't in a
 * segment` and no boot at all — GRUB looks it up in `p_vaddr` space, where a
 * bare 0x101000 is nowhere. */
ENTRY(_start32)

SECTIONS
{
    . = 1M + KERNEL_VMA;

    __kernel_start = .;

    .multiboot2 ALIGN(8) : AT(ADDR(.multiboot2) - KERNEL_VMA)
    {
        KEEP(*(.multiboot2))
    }

    __text_start = .;
    .text ALIGN(4K) : AT(ADDR(.text) - KERNEL_VMA)
    {
        KEEP(*(.text.boot32))
        *(.text .text.*)
    }
    . = ALIGN(4K);
    __text_end = .;

    __rodata_start = .;
    .rodata ALIGN(4K) : AT(ADDR(.rodata) - KERNEL_VMA)
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
    .user ALIGN(4K) : AT(ADDR(.user) - KERNEL_VMA)
    {
        KEEP(*(.user))
    }
    . = ALIGN(4K);
    __user_end = .;

    .data ALIGN(4K) : AT(ADDR(.data) - KERNEL_VMA)
    {
        *(.data .data.*)
        *(.got .got.plt)
    }

    .bss ALIGN(4K) : AT(ADDR(.bss) - KERNEL_VMA)
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
