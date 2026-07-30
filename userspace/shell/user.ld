/* Link ksh at 8 MiB.
 *
 * `userspace/hello` is linked at the same address on purpose — see the comment
 * in its own `user.ld`. Two programs at one address is a thing per-process page
 * tables make unremarkable and a shared address space made impossible.
 *
 * The ELF loader honours p_vaddr, so this is where it genuinely runs.
 */

ENTRY(_start)

SECTIONS
{
    . = 0x800000;

    .text ALIGN(4K) : { *(.text .text.*) }
    .rodata ALIGN(4K) : { *(.rodata .rodata.*) }
    .data ALIGN(4K) : { *(.data .data.*) }
    .bss ALIGN(4K) : { *(COMMON) *(.bss .bss.*) }

    /DISCARD/ : { *(.comment) *(.eh_frame) *(.eh_frame_hdr) *(.note .note.*) }
}
