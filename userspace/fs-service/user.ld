/* Link at 8 MiB, like every other program here.
 *
 * A filesystem server is an ordinary process. It gets no special address, no
 * reserved region and no mapping the kernel would not give `hello`. It does not
 * even hold a device capability the way the ATA driver does — it reaches the
 * disk only by asking another unprivileged process for sectors, so the set of
 * things it can touch is strictly smaller than the driver's. That is the
 * property worth preserving: if a filesystem needed a special link address,
 * "filesystem" would be a kind of program rather than a job an ordinary program
 * can do.
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
