# How Kosh boots

## The problem this solves

Multiboot2 hands control to the kernel in **32-bit protected mode, paging
disabled, EFER.LME clear**. The rest of Kosh is compiled as 64-bit code. Before
this was fixed, GRUB jumped straight into the Rust `_start`, the CPU decoded
64-bit opcodes as 32-bit instructions, and the machine wandered off into an
infinite loop with no output — for 27 commits.

Nothing in the kernel had ever executed. Everything below is what closed that gap.

## The chain

```
BIOS
 └─> GRUB2  (reads the multiboot2 header in .multiboot2)
      └─> _start32          kernel/src/boot32.rs   [32-bit protected mode]
           └─> long_mode_start                     [64-bit long mode]
                └─> _start   kernel/src/main.rs    [Rust]
                     └─> boot::init_kernel         kernel/src/boot.rs
```

### 1. `.multiboot2` header — `kernel/src/main.rs`

A 24-byte header (magic `0xE85250D6`, architecture 0, length, checksum, END
tag), placed in its own section which `linker.ld` puts first, at 1 MiB. GRUB
scans the first 32 KiB of the file for it.

`header_length` must cover the END tag — it is `size_of::<Multiboot2Header>()`,
not a hardcoded constant. Verify with `grub-file --is-x86-multiboot2`.

### 2. `_start32` — the trampoline, `kernel/src/boot32.rs`

The ELF entry point (`ENTRY(_start32)` in `linker.ld`). In order:

| Step | Why |
|---|---|
| Stash EAX/EBX into memory | CPUID clobbers EBX, and we need EBX free for serial output |
| Set up a 64 KiB boot stack | GRUB's stack is not ours to keep |
| Bring COM1 up by hand | So a failure *before* Rust is visible instead of a silent reboot |
| Check magic == `0x36D76289` | Confirms a Multiboot2-compliant loader |
| CPUID leaf `0x80000001`, EDX bit 29 | Confirms the CPU has long mode at all |
| Build PML4 / PDPT / PD | Identity-maps the first 1 GiB with 2 MiB pages |
| CR3, CR4.PAE, EFER.LME, CR0.PG | The actual mode switch |
| `lgdt` + `ljmp $0x08` | Loads a 64-bit GDT and reloads CS — this is the moment we become 64-bit |

The identity map covers the kernel at 1 MiB, the frame bitmap, the heap, VGA at
`0xB8000` and QEMU's default RAM. It is deliberately a *bootstrap* map: the
kernel replaces it with a proper higher-half address space in Phase 3.

### 3. `long_mode_start`

Reloads the data segments, resets RSP, then **enables SSE** — rustc emits SSE
instructions for x86-64 unconditionally, so `CR0.EM` must be clear, `CR0.MP`
set, and `CR4.OSFXSR | CR4.OSXMMEXCPT` set before any Rust code runs. Finally it
moves the saved multiboot info pointer into RDI (System V AMD64 puts the first
argument there) and calls `_start`.

## Memory layout at hand-off

```
0x00000000  ─┬─ low memory, BIOS, VGA at 0xB8000    (reserved, never allocated)
0x00100000  ─┼─ __kernel_start
             │    .multiboot2
             │    .text     (.text.boot32 first)
             │    .rodata
             │    .data
             │    .bss      ← PML4/PDPT/PD + 64 KiB boot stack live here
0x00144000  ─┼─ __kernel_end
             │    physical frame bitmap
             ├─ kernel heap (1 MiB)
             └─ free frames
```

`__kernel_start` / `__kernel_end` are emitted by `linker.ld` and consumed by
`memory/physical.rs`, which excludes that range from the free list. Without it
the allocator would happily hand out the frames holding the running kernel — and
the boot page tables with it.

## Running it

```bash
./scripts/run.sh           # build, ISO, boot, stream serial to your terminal
./scripts/run.sh --debug   # same, plus a gdb stub on :1234
./scripts/run.sh --check   # headless, asserts the boot markers (this is CI)
```

Expected first two lines:

```
[boot] 32-bit protected mode entry OK
[boot] long mode OK, entering Rust
```

If you see the first and not the second, the mode switch failed — check
`build/qemu.log` for `Triple fault`. If you see neither, GRUB never reached the
kernel; check the multiboot2 header.

## Debugging

```bash
./scripts/run.sh --debug     # terminal 1
gdb target/x86_64-kosh/release/kosh-kernel
  (gdb) target remote :1234
  (gdb) break long_mode_start
  (gdb) continue
```

QEMU's `-d int,cpu_reset` output lands in `build/qemu.log`. A `Triple fault`
line there means an exception fired with no IDT to catch it — which is every
exception, until Phase 2 installs one.

## What is deliberately still missing

- **No IDT.** Any fault is an instant triple fault and reboot. Phase 2.
- **No timer, no interrupts enabled.** The kernel halts after init. Phase 2.
- **`PHYSICAL_MEMORY_OFFSET` is 0**, because the map is flat identity. It
  becomes `0xFFFF_8000_0000_0000` when the kernel builds its own page tables in
  Phase 3.
- **Swap and power management are disabled** in `init_kernel`. Both were
  simulated, and swap tried to allocate 8 MiB from a 1 MiB heap.
