/* Link ksh at 8 MiB — clear of the kernel's low identity window and of
 * userspace/hello at 4 MiB, so the two are easy to tell apart in a fault dump.
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
