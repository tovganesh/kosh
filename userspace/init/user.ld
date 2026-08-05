/* Link at 8 MiB, like every other program here.
 *
 * init is an ordinary process. It gets no special address, no reserved
 * region and no mapping the kernel would not give `hello` — the only thing it
 * has that others do not is a set of bits in the TSS I/O permission bitmap,
 * which is invisible from the linker's point of view. That is the property
 * worth preserving: if a driver needed a special link address, "driver" would
 * be a kind of program rather than a capability a program holds.
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
