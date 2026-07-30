/* Link userspace programs at 4 MiB.
 *
 * Clear of the kernel's low identity window (which ends around 4 MiB) and far
 * below the higher half. The ELF loader honours p_vaddr, so this is where the
 * program genuinely runs — unlike the Phase 5 payload, which was
 * position-independent assembly mapped wherever we liked.
 */

ENTRY(_start)

SECTIONS
{
    . = 0x400000;

    .text ALIGN(4K) : { *(.text .text.*) }
    .rodata ALIGN(4K) : { *(.rodata .rodata.*) }
    .data ALIGN(4K) : { *(.data .data.*) }
    .bss ALIGN(4K) : { *(COMMON) *(.bss .bss.*) }

    /DISCARD/ : { *(.comment) *(.eh_frame) *(.eh_frame_hdr) *(.note .note.*) }
}
