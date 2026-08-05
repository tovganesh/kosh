/* Link at 8 MiB — the *same* address as ksh (userspace/shell/user.ld).
 *
 * Deliberately the same, as a standing test. Until per-process address spaces
 * every program had to be given its own address so their images would not land
 * on top of each other in the one set of page tables, and `spawn` refused any
 * image that overlapped a resident one. Two programs sharing a link address
 * would have meant one overwriting the other's `.text` while it executed.
 *
 * With a PML4 each, 0x800000 in `hello` and 0x800000 in `ksh` are simply
 * different memory. `ksh` spawning `hello` — which the scripted console session
 * does twice — exercises exactly that, and would fail loudly if the spaces were
 * ever shared again.
 *
 * The ELF loader honours p_vaddr, so this is where the program genuinely runs.
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
