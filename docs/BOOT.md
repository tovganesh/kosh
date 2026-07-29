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
| Build PML4 / PDPT / PD / PT | Identity-maps the first 1 GiB (see the null guard note below) |
| CR3, CR4.PAE, EFER.LME, CR0.PG | The actual mode switch |
| `lgdt` + `ljmp $0x08` | Loads a 64-bit GDT and reloads CS — this is the moment we become 64-bit |

The identity map covers the kernel at 1 MiB, the frame bitmap, the heap, VGA at
`0xB8000` and QEMU's default RAM. It is deliberately a *bootstrap* map: the
kernel replaces it with a proper higher-half address space in Phase 3.

**Null guard page.** The first 2 MiB is mapped with 4 KiB pages rather than one
2 MiB huge page, purely so that page 0 can be left *unmapped*. With a flat
huge-page map a null pointer dereference silently reads real memory instead of
faulting — which is exactly the class of bug a kernel most needs to catch. This
was found by testing: the first version of the trampoline used a huge page for
0..2 MiB, and a deliberate `read_volatile(0)` returned successfully. Everything
from 2 MiB up still uses 2 MiB pages.

### 3. `long_mode_start`

Reloads the data segments, resets RSP, then **enables SSE** — rustc emits SSE
instructions for x86-64 unconditionally, so `CR0.EM` must be clear, `CR0.MP`
set, and `CR4.OSFXSR | CR4.OSXMMEXCPT` set before any Rust code runs. Finally it
moves the saved multiboot info pointer into RDI (System V AMD64 puts the first
argument there) and calls `_start`.

## Memory layout

Physical, at hand-off:

```
0x00000000  ─┬─ low memory, BIOS, VGA at 0xB8000    (reserved, never allocated)
0x00100000  ─┼─ __kernel_start
             │    .multiboot2
             │    .text     (.text.boot32 first)   __text_start/__text_end
             │    .rodata                          __rodata_start/__rodata_end
             │    .data
             │    .bss      ← bootstrap page tables + 64 KiB boot stack
0x00144000  ─┼─ __kernel_end
             │    physical frame bitmap
             └─ free frames
```

`__kernel_start` / `__kernel_end` are emitted by `linker.ld` and consumed by
`memory/physical.rs`, which excludes that range from the free list. Without it
the allocator would happily hand out the frames holding the running kernel — and
the boot page tables with it.

Virtual, once `memory::paging` has installed the kernel's own tables:

```
0x0000_0000_0000_0000   unmapped                    null guard
0x0000_0000_0000_1000 ┬ identity window, 4 KiB pages, W^X
                      │   .text            R-X
                      │   .rodata          R--  NX
                      │   .data/.bss       RW-  NX
                      │   VGA, frame bitmap RW- NX
0x0000_0000_0040_0000 ┘
        ...
0xFFFF_8000_0000_0000   physmap: all of RAM, 2 MiB pages, RW+NX
0xFFFF_9000_0000_0000   kernel heap window, 4 KiB pages, RW+NX
```

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

## Interrupts (Phase 2)

`kernel/src/interrupts/` splits into four pieces:

| File | Role |
|---|---|
| `mod.rs` | Builds and loads the IDT; `init()` vs `enable_hardware_interrupts()` |
| `exceptions.rs` | Vectors 0..31. Print a dump and halt. `#PF` decodes CR2 and the error code; `#BP`/`#DB` return so debuggers work |
| `pic.rs` | Remaps the 8259 pair to vectors 32..47 |
| `timer.rs` | PIT channel 0 at 100 Hz, `TICKS` counter, `uptime_ms()`, `sleep_ms()` |
| `keyboard.rs` | IRQ1 -> port 0x60 -> `pc-keyboard` decode -> SPSC ring buffer |

Two things worth knowing:

**Why the PIC remap is not optional.** On a freshly-booted PC the PICs deliver
IRQ0..15 on vectors 8..15 and 0x70..0x77. Vector 8 is `#DF Double Fault` — so
without remapping, the very first timer tick after `sti` arrives as a double
fault.

**Why `init()` and `enable_hardware_interrupts()` are separate.** The IDT is
installed immediately after the GDT, so a fault during memory-manager init is
reported rather than silently fatal. Hardware interrupts are enabled at the very
end of `init_kernel`, so a timer tick cannot arrive mid-initialisation.

Interrupt handlers use `try_lock`, never `lock`. Blocking on a spinlock held by
the code you just interrupted is a guaranteed deadlock; dropping a keystroke is
the correct trade.

Self-test: `init_kernel` raises an `int3` right after loading the IDT and expects
to resume. If the IDT were missing or malformed, that would triple-fault instead
of printing `returned from int3 handler`.

## Page tables (Phase 3)

`kernel/src/memory/paging.rs` replaces the bootstrap map with tables the kernel
builds itself. `Cr3::write` appeared nowhere in the tree before this — the
"virtual memory manager" was reading the bootloader's CR3 and describing it.

What it sets up:

- **W^X on the kernel image.** `.text` is R-X, `.rodata` is R-- NX, everything
  else is RW- NX. Nothing is both writable and executable.
- **A physmap** at `0xFFFF_8000_0000_0000` covering all of RAM in 2 MiB pages.
  `vmm.rs`'s `PHYSICAL_MEMORY_OFFSET` had always claimed this existed; now it
  does, so the constant is back to its intended value.
- **A heap window** at `0xFFFF_9000_0000_0000`. The heap used to take the raw
  physical address of a contiguous frame run and use it as a pointer.
- **Page 0 still unmapped.**

Two flags that are easy to miss, both of which this code got wrong first time:

| Flag | Without it |
|---|---|
| `EFER.NXE` | The NX bit is *reserved*. Setting it turns every NX mapping into a reserved-bit page fault instead of a protection. |
| `CR0.WP` | Read-only pages do not apply to ring 0. The kernel can write straight through a read-only PTE, and W^X is decorative. A deliberate write to `.text` succeeded until this was set. |

`paging::self_test()` checks that the physmap aliases the identity map, that
`translate()` round-trips, and that page 0 is unmapped.

## Heap (Phase 3)

Three fixes in `memory/heap.rs`, all verified by `heap::stress_test()` — 2000
mixed-size, mixed-alignment allocations, freed out of order:

- **Alignment is honoured.** `find_free_block` ignored `layout.align()`
  entirely: it aligned to `PAGE_SIZE` regardless of the request, then returned
  the *unaligned* pointer anyway. It now carves a leading free block off the
  front when the payload needs to move.
- **Every block base is 16-aligned.** `BlockHeader` is `#[repr(C, align(16))]`
  and allocation sizes round up to 16, so splitting preserves the invariant.
  Without this, payloads drifted onto odd offsets and even an 8-byte alignment
  request failed after the first split.
- **Coalescing exists.** `coalesce_free_blocks` was an empty function with a
  comment describing what it would do, so the heap fragmented monotonically. It
  now walks the block chain in address order and merges adjacent free runs.

The stress test asserts the heap collapses back to exactly one free block of its
original size — which is the only way to prove coalescing works.

## Scheduling (Phase 4)

`kernel/src/task/` is the first thing in Kosh that actually schedules.

| File | Role |
|---|---|
| `switch.rs` | `kosh_switch_context(prev_rsp, next_rsp)` — the whole switch, in `global_asm!` |
| `mod.rs` | Thread table, round-robin picker, `spawn` / `schedule` / `on_tick` / `exit_current` |

**The switch is a plain `extern "C"` call, not interrupt machinery.** It follows
the System V AMD64 ABI, so the caller has already spilled its caller-saved
registers; the function only preserves RBX, RBP, R12-R15 and RFLAGS. It pushes
those, stores RSP in the outgoing TCB, loads RSP from the incoming TCB, pops
them back, and returns.

The consequence is the part worth internalising: **the incoming thread resumes
by returning out of its own earlier call to this function.** Thread B returns
into B's `schedule()`, which returns into B's timer handler, whose `iretq`
resumes whatever B was doing. Nothing reconstructs an interrupt frame, because
B's frame never left B's stack.

A brand-new thread has no such history, so `Thread::prepare_stack` fabricates
one. From low address to high: RFLAGS, R15, R14, R13, R12, RBX, RBP, return
address, and one dummy word. The dummy is for ABI alignment — after `ret` pops
the return address, RSP must be 8 mod 16, which is the state a function sees
right after a real `call`. The synthesised RFLAGS has IF *clear*, because a new
thread is first entered from inside the timer handler; `thread_bootstrap`
enables interrupts itself once it is on its own stack and holds no locks.

Three rules the code follows, each of which is a hang if broken:

1. **Every scheduler access runs with interrupts disabled.** The timer handler
   takes the same lock.
2. **The lock is released before the switch.** Holding a spinlock across a
   context switch parks it in the outgoing thread; the incoming thread spins on
   it forever.
3. **EOI before scheduling.** `on_tick` may not return to the handler for a long
   time, and until the PIC is acknowledged it delivers no further timer
   interrupts to anybody.

`serial::_print` and `vga_buffer::_print` now disable interrupts while holding
their port locks, for the same reason as rule 2 — a thread preempted mid-print
would otherwise deadlock the next thread that prints.

### Proving it

`init_scheduler` spawns three threads that each print a letter and then busy-wait
on the tick counter. They never yield, never sleep, never block. The only thing
that can interleave them is the timer taking the CPU away:

```
Expect A/B/C to interleave — none of them ever yields:
  A B C A B C A B C A B C A B C A B C A B C A B C
Thread table:
  [0] kmain      Running  18 ticks
  [1] worker-A   Finished  16 ticks
  [2] worker-B   Finished  16 ticks
  [3] worker-C   Finished  17 ticks
  context switches: 36
Scheduler: PASS — 36 real context switches
```

If those letters ever come out as `A A A ... B B B ...`, scheduling has become
cooperative and something is broken.

### What about `process/scheduler.rs`?

It stays. It is scheduling *policy* — round-robin, priorities, a simplified CFS —
over a process table, and it is real code. What it is not is wired to anything:
`handle_timer_tick` still has no caller.

`task` covers **kernel** threads, which share the kernel address space and run
in ring 0. `process` is meant to govern **userspace** processes, which do not
exist until ring 3 arrives in Phase 5. When they do, the intended shape is for
`task` to ask `process::scheduler` which process to run next — not for
`process::scheduler` to grow its own switch.

## What is deliberately still missing

- **No ring 3, no syscall entry.** Phase 5.
- **`PHYSICAL_MEMORY_OFFSET` is 0**, because the map is flat identity. It
  becomes `0xFFFF_8000_0000_0000` when the kernel builds its own page tables in
  Phase 3.
- **Swap and power management are disabled** in `init_kernel`. Both were
  simulated, and swap tried to allocate 8 MiB from a 1 MiB heap.
- **Only IRQ0 and IRQ1 are unmasked.** Everything else on the PIC has a
  handler that acknowledges and returns.
