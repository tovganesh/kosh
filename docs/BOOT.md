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
      └─> _start32       kernel/src/boot32.rs  [32-bit, running at phys 1 MiB]
           └─> long_mode_low                   [64-bit, running at phys 1 MiB]
                └─> long_mode_start            [64-bit, 0xFFFFFFFF801.....]
                     └─> _start   kernel/src/main.rs         [Rust]
                          └─> boot::init_kernel    kernel/src/boot.rs
```

The kernel is linked at `0xFFFF_FFFF_8010_0000` and loaded at physical 1 MiB.
Which of those two a given line of `boot32.rs` is running at is the thing to
track while reading it — see "The higher half" below.

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
| `movabsq` + `jmp *%rax` | ...and this is the moment we stop running at a physical address |

The trampoline maps physical 0..1 GiB **twice**: identity, because the code doing
the mapping is executing at a low address and cannot pull the rug out from under
itself, and again at `KERNEL_VMA` (PML4[511], PDPT[510]), because that is where
the kernel is linked. Both windows point at the same PD, so the second costs one
extra page table. `paging::init` later replaces both and keeps only the higher
half.

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

Those symbols are *virtual* (`0xFFFF_FFFF_8010_0000`), and the exclusion above is
about *physical* frames, so `physical.rs` runs them through
`paging::kernel_phys`. Before the higher-half migration the two were the same
number and the conversion did not exist; forgetting it is not a subtle failure —
the comparison never matches, and the allocator hands out the running kernel.

Virtual, once `memory::paging` has installed the kernel's own tables:

```
0x0000_0000_0000_0000 ┐
        ...           │ the entire lower half belongs to userspace
0x0000_7FFF_FFFF_FFFF ┘
        ...
0xFFFF_8000_0000_0000   physmap: all of RAM, 2 MiB pages, RW+NX      PML4[256]
0xFFFF_9000_0000_0000   kernel heap window, 4 KiB pages, RW+NX       PML4[288]
        ...
0xFFFF_FFFF_8000_0000   unmapped                  null guard         PML4[511]
0xFFFF_FFFF_8000_1000 ┬ kernel window, 4 KiB pages, W^X
                      │   VGA, multiboot info, frame bitmap  RW- NX
                      │   .text                              R-X
                      │   .rodata                            R-- NX
                      │   .data/.bss                         RW- NX
0xFFFF_FFFF_8040_0000 ┘
```

The kernel window maps `KERNEL_VMA + phys` for the low few MiB — the same frames
the identity map used to cover, at addresses out of userspace's way. It exists
because the frame bitmap and the VGA buffer are touched *before* `paging::init`
builds the physmap, so they need a window that the boot trampoline can set up.

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

## Ring 3 and syscalls (Phase 5)

`iretq`, `sysretq`, `swapgs` and `user_code_segment` had **zero occurrences** in
this repository before Phase 5. For a microkernel — an architecture whose whole
premise is that services run outside the kernel — that was the biggest gap in
the project.

| File | Role |
|---|---|
| `gdt.rs` | GDT with ring-3 descriptors, TSS with RSP0 |
| `syscall/entry.rs` | MSR setup and the `syscall` assembly stub |
| `syscall/uaccess.rs` | `copy_from_user` / `copy_to_user` and range validation |
| `usermode.rs` | Maps the payload, drops to ring 3 via `iretq` |
| `user_program.rs` | The ring-3 payload, in position-independent assembly |

### The GDT order is not arbitrary

`syscall` loads CS from `STAR[47:32]` and SS from `STAR[47:32] + 8`. `sysretq`
loads CS from `STAR[63:48] + 16` and SS from `STAR[63:48] + 8`. The CPU just
adds those offsets, so the table has to match:

```
  0x00  null
  0x08  kernel code   <- STAR[47:32]
  0x10  kernel data       (= 0x08 + 8)        <- STAR[63:48]
  0x18  user data         (= 0x10 + 8)   sysret SS, RPL 3 -> 0x1B
  0x20  user code (64)    (= 0x10 + 16)  sysret CS, RPL 3 -> 0x23
  0x28  TSS (two slots)
```

User *data* comes before user *code*. That reads backwards until you write out
the `sysret` arithmetic, and getting it the intuitive way round is a classic
route to a #GP the first time a process returns to ring 3.

`TSS.RSP0` is the stack the CPU switches to when an interrupt arrives in ring 3.
Without it, the first timer tick after entering user mode is a triple fault.

### Getting to ring 3

There is no "jump to user mode" instruction. You fake a return from one: push
the five words `iretq` expects — SS, RSP, RFLAGS, CS, RIP — with ring-3
selectors, and execute it.

The user program enters with IF set, so the timer can still preempt it. A ring-3
thread that cannot be preempted is a ring-3 thread that can hang the machine
with `for {}`.

### The syscall path

`syscall` is fast because it does almost nothing: CS/SS from `STAR`, RIP from
`LSTAR`, RFLAGS masked by `SFMASK`, caller's RIP into RCX and RFLAGS into R11 —
and **RSP still points at the user stack**. The stack switch is the kernel's job
and is the first thing the stub does.

Argument 4 travels in R10 rather than RCX precisely because `syscall` destroys
RCX.

`SFMASK` masks IF, so a syscall *starts* uninterruptible. Phase 10 replaced the
single static kernel syscall stack this once justified — see
"Several ring-3 threads" below.

### Trusting nothing from ring 3

`syscall/uaccess.rs` does two checks, both necessary:

1. **Range** — the whole span must lie in the lower canonical half.
2. **Mapping** — every page must be present *and* `USER_ACCESSIBLE`. Range
   checking alone is not enough; a kernel-only page can sit at a low address.

This is what `sys_write` was missing. It used to return the byte count without
reading the buffer; `sys_read` returned a length without writing one; and
`sys_send_message` discarded the user pointer and sent a `format!` string
instead. All of them "worked" because nothing could call them.

One subtlety worth knowing: `SyscallError::to_errno()` already returns
*negative* numbers (-22 for EINVAL). Negating it in the handler — the obvious
thing to write, since the Linux convention is "negative means error" — flips
every failure positive, and userspace then reads all errors as success. The
handler normalises rather than assuming.

### A userspace fault must not be fatal

The page-fault handler checks `USER_MODE` in the error code. If the fault came
from ring 3 it terminates that thread and returns to the scheduler instead of
halting. Any other behaviour would mean a userspace bug takes the whole system
down — which is the thing a microkernel exists to prevent.

### Proving it

Two payloads run, back to back:

```
--- ring 3 output ---
hello from ring 3
Process 1 syscall write failed: InvalidArgument
kernel rejected my out-of-bounds pointer
Process 1 terminated with exit code 0
--- back in ring 0 ---

--- ring 3 output ---
about to dereference a kernel address directly

==================== PAGE FAULT ====================
  accessed address : 0xffff800000000000
  cause            : protection violation while reading in user mode
  error code       : PageFaultErrorCode(PROTECTION_VIOLATION | USER_MODE)
  action           : ring 3 fault — terminating the process
  rip              : 0x00000000400000c4
====================================================
--- kernel survived a ring 3 fault ---
```

The first payload asks the kernel to `write` from a kernel address and reports
back whether it was refused — the validation is tested *from the user side*, not
by the kernel checking itself. The second bypasses syscalls entirely and touches
kernel memory directly, so page protection rather than argument validation has
to stop it.

## Loading programs (Phase 6)

`kernel/src/elf.rs` parses static `ET_EXEC` ELF64: walk the program headers, map
each `PT_LOAD` at its `p_vaddr` with permissions from `p_flags`, copy
`p_filesz` bytes, leave the rest zero. No dynamic linking, no relocations, no
interpreter.

Segments are populated **through the physmap**, not through the mapping being
created — so a read-only segment never has to be temporarily writable just to be
filled in. `map_user_pages` already zeroes each frame, which makes the `.bss`
tail (`p_memsz > p_filesz`) correct without extra work.

Two things about GRUB modules that are easy to get wrong:

- **The frames must be reserved before the allocator starts.** The memory map
  reports them as available, because from the firmware's point of view they are.
  `physical.rs` claims them, or the first allocation lands on top of the binary.
- **A module can land outside the low identity window**, so its bytes are read
  through the physmap rather than by physical address.

`userspace/hello` is the payload — an ordinary Rust binary linked at 4 MiB, not
position-independent assembly. It only runs if the loader honoured `p_vaddr`,
and it self-checks the three things the loader must get right: executing at the
linked address, resolving `.rodata` through absolute addresses, and reading a
`.bss` static that exists only because the loader zeroed the tail.

**Its `_start` is assembly, exactly like a real crt0.** System V says RSP is
16-byte aligned at process entry, but a Rust `extern "C"` function is compiled
as an ordinary callee and assumes RSP is 8 *past* a boundary — the state a
`call` leaves. Wiring Rust straight to the entry point makes the first `movaps`
spill fault with #GP. That is what happened on the first run, and the symptom
(a #GP deep inside integer formatting) points nowhere near the cause.

## The console (Phase 7)

`kernel/src/console/` is an in-kernel shell: `editor.rs` does line editing and
history, `commands.rs` is the command table.

```
kosh> uname
Kosh 0.1.0 x86_64
  microkernel, Rust, multiboot2
  running in ring 0
  CR3            0x176000
  physmap base   0xffff800000000000
kosh> ps
2 thread(s):
   id  name         state        ticks
    0  kmain        ready          642
    1  console      running        616

661 context switches since boot
```

**Why in-kernel, in a microkernel.** Because a kernel you can interrogate while
it is running is worth more than architectural purity at this stage. Every
command reports live state — real frame counts, real heap statistics, the real
thread table, a real page-table walk. That makes it a debugging instrument, and
it is why serious kernels keep a debug console permanently even after userspace
exists. The userspace shell is still the goal; it needs a blocking read syscall
and a filesystem to be worth using, and this is what lets you inspect the kernel
while building those.

**What it deliberately does not have:** `ls`, `cat`, `cd`. There is no
filesystem, and inventing commands that return plausible-looking strings is
exactly the habit that let this project accumulate 27 commits of code that had
never run.

The console runs on **its own kernel thread**, so the scheduler stays live while
it blocks on the keyboard — timer ticks keep arriving, other threads keep
running, and a wedged console does not wedge the machine.

Two things it changed elsewhere:

- The keyboard ring now carries a `Key` enum rather than a byte. A line editor
  has to tell an arrow key from the character it would otherwise be conflated
  with; squeezing cursor keys into spare ASCII control codes works right up
  until something wants to type one of those codes.
- The once-a-second tick heartbeat is now off by default once the console
  starts. A line that appears in the middle of what you are typing makes the
  console unusable. `heartbeat on` brings it back.

### Testing it

```bash
./scripts/run.sh --check-cli
```

Boots, types at the `kosh>` prompt through QEMU's monitor, and asserts what the
console answers. Both checks run in CI. A console only a human can test is a
console that rots quietly.

## Storage and a filesystem (Phase 8)

```
kosh:/> df
FAT32 'KOSHDISK', 64 MB, 512 B/cluster, 2 FAT(s) of 1009 sectors, root at cluster 2
kosh:/> ls
drw         -  DOCS
-rw       199  README.TXT
-rw     22000  BIG.TXT
-rw        26  A Long File Name.txt

6 entries, 22280 bytes
kosh:/> cat lines.txt
line one
line two
line three
kosh:/> cd docs
kosh:/docs> ls
-rw        26  NOTES.TXT
```

| File | Role |
|---|---|
| `block/ata.rs` | Polled PIO driver for the legacy IDE ports |
| `block/mod.rs` | `BlockDevice` trait and the registered device |
| `fs/fat32.rs` | FAT32, read-only: mount, chain walk, directories, long names, file reads |
| `fs/mod.rs` | The mounted volume, and a self-test against the known test image |

### Why PIO, and why FAT32

**Polled PIO on the legacy ports** rather than DMA over PCI. It is the smallest
thing that really reads a disk: no PCI enumeration, no interrupt plumbing, no
bus-master buffers. It is also slow — one 16-bit port read per two bytes with the
CPU spinning on a status register — which is fine for reading a filesystem and
wrong for anything performance-sensitive. DMA and virtio-blk are for later.

**FAT32 rather than ext4.** `userspace/fs-service/src/ext4.rs` already claims to
implement ext4, and is instructive about why: `read_block` fills the buffer with
zeroes, `write_block` returns success without writing, the superblock is
fabricated in code, `mtime` is the literal `1234567890`, and `read_dir` returns
only `.` and `..`. It was never connected to a disk, because there was no disk
driver. FAT32 is a few hundred lines to read correctly, and the image can be
mounted on the development host — so every answer the kernel gives is checkable
against what is actually on the disk. ext4 is a lot of surface area to get wrong
silently.

### Things that cost time

- **The test disk is bootable, and that broke everything.** `mkfs.vfat` writes a
  `0xAA55` signature at offset 510, so SeaBIOS considered the FAT image bootable,
  tried it before the CD, and hung — producing *no serial output at all*, which
  looks exactly like a kernel that failed to start. `-boot order=d` fixes it.
- **Validate the BPB before doing arithmetic with it.** A corrupt or hostile
  boot sector otherwise turns directly into out-of-range reads. `mount` checks
  sector size, cluster size, FAT count and region sizes, and rejects a root
  cluster past the end of the volume.
- **FAT32 is identified by `root_entry_count == 0` and `sectors_per_fat_16 == 0`**,
  not by the filesystem-type string, which is informational and routinely wrong.
- **Mask the top four bits of a FAT entry.** They are reserved. A free (`0`) or
  out-of-range entry found *inside* a chain means the FAT is damaged, and saying
  so beats reading whatever sector that implies.
- **A zero-length file has no cluster allocated**, so the chain walk must not be
  attempted on it.
- **Long filenames were passing without being tested.** Every name on the first
  test image (`DOCS`, `README.TXT`) is a valid 8.3 short name, so the ~80 lines
  of long-filename assembly never ran. The image now contains
  `A Long File Name.txt`, and the self-test asserts both that it lists and that
  it opens by that name.

### The self-test

`fs::selftest` runs at boot and asserts against facts `scripts/run.sh`
guarantees when it builds the image: a file whose exact contents it controls, a
22 KB file that forces a cluster-chain walk, a subdirectory, a long filename, and
a path that must report `NotFound`. A filesystem that returned an empty root
without complaining would otherwise look like a pass.

## Userspace (Phase 9)

```
ksh: the Kosh shell, in ring 3. Type 'help'.
ksh:/$ ls
d           -  DOCS
-         199  README.TXT
-       22000  BIG.TXT
-          26  A Long File Name.txt

6 entries, 22280 bytes
ksh:/$ cat lines.txt
line one
line two
line three
ksh:/$ getpid
pid 1
ksh:/$ cd docs
ksh:/docs$
```

The shell is now a ring-3 process that reaches the kernel only through system
calls. It is loaded from a GRUB module by the ELF loader, like any other program.

### File descriptors

`kernel/src/syscall/files.rs` implements `open`, `close`, `read`, `lseek`,
`getdents` and `stat` against FAT32. What was there before: `sys_open` returned
the literal `3` for every path ("for demonstration, return a dummy file
descriptor"), `sys_read` returned `min(count, 1024)` without touching the buffer
("simulate reading some data"), and `close` and `lseek` returned `NotSupported`.
There was no descriptor table because there was nothing to put in one.

`getdents` is path-based, not fd-based — opening a directory as a byte stream is
a category error that `open` already refuses — and returns fixed-size records so
userspace needs no parser.

`fat32::read_at` was added for this. Re-reading from byte 0 and discarding the
prefix, which is what `read_file` would have to do, turns reading a file in N
chunks into O(N²) disk traffic, and every one of those sectors is a polled PIO
transfer.

### The trap: a blocking syscall with interrupts masked

`SFMASK` clears the interrupt flag on syscall entry, so a syscall runs with
interrupts **masked**. `read` on fd 0 blocks on the keyboard — and halting with
interrupts masked means the keyboard IRQ that would wake it can never fire. The
machine simply stops, with the shell's prompt on screen, looking exactly like a
program waiting for input.

`wait_for_key` therefore enables interrupts while it waits (via `enable_and_hlt`,
which does the sti/hlt atomically) and masks them again before returning. That is
safe only because exactly one thread is ever in ring 3 — the same reason the
single static syscall stack works.

### Which shell has the keyboard

Both `ksh` and the in-kernel console read the same keyboard ring, so starting
them together would mean two line editors racing for every keystroke. A
supervisor thread runs `ksh`, waits for it, and then hands the console to the
kernel's own. That is also how you get a debug prompt on a system whose userspace
has died — and it means one scripted session can test both.

### What was deleted

`userspace/shell` kept `parser.rs` (952 lines: tokenizer, quotes, escapes,
variable expansion, pipes, redirects, conditionals) and `history.rs` (769 lines).
Both were real code that nothing had ever run — the old binary did not even
declare them as modules.

Removed: `service_client.rs` and `infrastructure.rs` (1,399 and ~450 lines, zero
`asm!` between them, `wait_for_response` returned a fabricated success, service
discovery was three hardcoded PIDs, and they were near-duplicates of each other);
`fs_commands.rs` (every path fell through to `simulated_listing()`);
`commands.rs` (canned strings); `input.rs` (`read_line` replayed a hardcoded
array of six commands, `read_char` always returned `None`); `output.rs` (wrote
through a `syscall` wrapped in `#[cfg(debug_assertions)]`, so release builds
printed nothing); and `tests.rs` (558 host-side tests against the above).

### Honest about what does not work

The parser understands pipes, redirection, conditionals and background jobs.
Executing any of them needs `fork` and `exec`. The shell reports that rather than
accepting the syntax and ignoring it:

```
ksh:/$ echo a > out.txt
ksh: redirection needs a writable filesystem, which does not exist yet
```

`parsetest` runs the tokenizer over sample inputs and prints what it found. It
exists because most of that grammar is otherwise unreachable — and because it is
the only practical way to test the pipe path: QEMU's `sendkey` cannot produce a
`|` through this keyboard layout, so a scripted session can never type one.

### Two more validator bugs this surfaced

- **`validate_stat_args` checked the wrong argument.** It validated `args[1]` as
  a 144-byte writable buffer, but `args[1]` is the path *length* — so it was
  asking whether the address `9` was mapped. It never is: that is page 0, the
  null guard. Every `stat` failed before its handler ran, while `open` and
  `getdents` worked because their validators happened to match their ABIs.
- **`MAX_SYSCALL_NUMBER` was 101 in debug and 63 in release.** Every syscall
  above 63 — including `SYS_DEBUG_PRINT`, which userspace calls
  unconditionally, and the new `SYS_GETDENTS` — was rejected as invalid in
  exactly the build that ships. It is now one value.

`validate_user_pointer` and `validate_user_string` also no longer consist of a
null check and two TODOs; they delegate to `syscall::uaccess`, and take a
`writable` flag so `read` and `write` are checked in the right direction.

## Several ring-3 threads, and launching programs (Phase 10)

Everything up to Phase 9 ran **one** ring-3 thread at a time, and the syscall
path was built on that assumption: a single `static SYSCALL_STACK`, a single
`SAVED_USER_RSP`, and a comment in three places saying so. Phase 10 removes the
assumption, and then uses what that buys.

### Per-thread kernel stacks

`syscall` gives the kernel no stack. It leaves RSP pointing at the *user* stack,
and no register is free — RCX and R11 carry the return state, everything else
belongs to the caller. So the hop to a kernel stack has to go through memory
without a register to address it with, which is what `gs:` addressing is for.

`kernel/src/percpu.rs` holds the per-CPU block:

```
gs:0   syscall_rsp        kernel stack top for the running thread
gs:8   user_rsp_scratch   where the stub parks the user RSP for two instructions
gs:16  current_thread
```

The offsets are asserted with `offset_of!` at compile time, because the assembly
addresses them numerically.

The stub is now:

```
    movq    %rsp, %gs:8         /* park the user RSP                    */
    movq    %gs:0, %rsp         /* this thread's own kernel stack        */
    pushq   %gs:8               /* ...and onto the stack, where it is    */
    ...                         /*    per-thread rather than per-CPU     */
```

`user_rsp` is a field of `SyscallFrame` now. That is the part that actually
matters: the scratch slot at `gs:8` is only live while `SFMASK` has interrupts
masked, and one instruction later the value is on a stack nobody else can touch.

`task::schedule()` publishes the incoming thread's stack to two places on every
switch — `TSS.RSP0`, used by the CPU when an interrupt arrives in ring 3, and
`gs:0`, read by the syscall stub. `gdt.rs` stopped being a `lazy_static` for
this: `set_kernel_stack` used to be `unimplemented!()` because a `lazy_static`
hands out `&TSS` and no way to mutate it.

**Why there is no `swapgs`.** The textbook mechanism swaps GS.base with
`IA32_KERNEL_GS_BASE` on each kernel entry and exit. That invariant only holds if
*every* transition swaps, and this kernel cannot honour it: its interrupt
handlers are Rust functions using `x86-interrupt`, which offers no place to put a
`swapgs` before the compiler's prologue. Once a timer tick can land inside a
syscall — which `files::wait_for_key` makes reachable — the swap count stops
being even, and a later `swapgs` leaves GS.base holding user data. So both MSRs
point at the same block, and any `swapgs` is a no-op. Ring 3 cannot subvert this:
`wrgsbase` needs CR4.FSGSBASE (clear) and `swapgs` itself is #GP in ring 3. The
cost is that user-mode TLS through GS is unavailable until interrupt entry gets
naked stubs.

### Proving it needed doing

A test that cannot fail proves nothing, so this one was checked against a
deliberately broken kernel.

`SYS_YIELD` (syscall 8) gives up the rest of a slice. It is the only way for
userspace to force a context switch *from inside a system call* — which is
exactly the situation per-thread stacks exist for. `kosh_user_pingpong_entry` in
`user_program.rs` loops twelve times over "yield, then write one byte", and two
threads run it concurrently with different tag bytes:

```
Two ring-3 threads, each yielding from inside a syscall:
  ring 3 'A': entry 0x400000e7, user stack 0x58000000, kernel stack 0xffff900000054da0
  ring 3 'B': entry 0x400000e7, user stack 0x57f00000, kernel stack 0xffff90000005cdd0
ABABABABABABABABABABAB
A: survived 12 yields inside a syscall
B: survived 12 yields inside a syscall
Concurrent ring 3: PASS — 52 syscalls across 39 context switches
```

With `publish_kernel_stack` patched to hand every thread the *same* stack — the
Phase 9 behaviour — the same test produces:

```
BBBBBBBBBBBBB: survived 12 yields inside a syscall
[#DB debug at 0x0000000000101216] — resuming
...
==================== PAGE FAULT ====================
  cause : protection violation while fetching an instruction in kernel mode
  rip   : 0x0000000000175df4
```

Thread A never finishes. B's syscall entry took the shared stack from the top and
overwrote A's parked frame, so A's `sysretq` returned to a garbage RIP with
garbage RFLAGS — TF among them, hence the single-step storm — and then tried to
execute its own kernel stack.

The completion line is assembled in one buffer and written with a single
`write`, because two writes would let the other thread's byte land between the
tag and the message.

### spawn and wait

With more than one ring-3 thread possible, a shell can launch a program:

```
ksh:/$ hello
hello from a loaded ELF binary
  my pid is 3
  .bss was zeroed correctly
  stack is writable and readable
  exiting cleanly
ksh:/$ ksh
ksh: ksh: could not start (error -98)
```

`spawn(path, len)` (syscall 9) loads a named boot module and runs it on a new
kernel thread in ring 3, returning the task id. `wait(task, status)` (syscall 4,
which used to be a TODO and `Err(NotSupported)`) blocks until that task exits and
writes its exit code.

Three things are worth spelling out.

**It is `spawn`, not `fork`+`exec`.** `fork` duplicates an address space; there
is one address space. Calling it `spawn` keeps the name honest. `sys_fork` itself
now returns `NotSupported` — it used to add a row to the process table, log "Fork
successful", and return a PID for a child that did not exist, which userspace read
as success.

**Blocking is real blocking.** `State::Blocked { on }` is skipped by
`next_runnable`, and `exit_current` wakes anyone waiting on the exiting thread.
The alternative — a `yield_now` loop — would mean a shell waiting for a child
consumed half the CPU. Getting the wake-up wrong is worse than a spin, though:
the waiter is simply never Ready again and the machine deadlocks with a live
thread the scheduler refuses to pick.

**The ELF is loaded in the caller's thread, not the new one.** A load failure is
then a synchronous error the shell can report, instead of a thread that starts
and dies with the reason only in the kernel log.

### Programs have to be unmapped

The second `spawn` of the same binary used to be impossible: `map_user_pages`
returns `PageAlreadyMapped`, after having already allocated a frame for that
page, so every attempt also leaked one. `paging::unmap_user_pages` is the
counterpart that was missing, and `usermode::RESIDENT` tracks what each program
mapped so `task::exit_current` can hand it back:

```
  released 'hello': 19 page(s) returned, 1560 used system-wide
  released 'hello': 19 page(s) returned, 1560 used system-wide
```

Two runs, identical used-page count. That number is printed for exactly this
reason — it is the only way to see from the log alone that running a program
twice does not cost twice the memory.

The same table is what makes the overlap check possible. Every userspace program
is linked at a fixed address, so `ksh` spawning `ksh` would overwrite the `.text`
it is currently executing. `elf::extent_of` answers "where would this go" without
mapping anything, which is the only moment a conflict can be refused cheaply.

### Loose ends this exposed

`sys_exit` called `files::close_all()`, which closes the *system's* descriptor
table — there is only one, because there is no per-process state to hang one off.
With two programs resident, a short-lived one exiting would close the shell's
open files behind its back. It now does that only when it is the last program
running, and says so when it declines.

`kosh_syscall_handler` used a hardcoded `ProcessId::new(1)`. It uses
`task::current_id()` now, so `getpid`, the log lines, and the task id `spawn`
returns all agree.

## The higher half (Phase 11)

Until this, the kernel lived at physical 1 MiB and ran there, identity-mapped
through PML4[0]. That is the same top-level entry a user process needs for its
own image and stack, so "give each process its own address space" was blocked on
"get the kernel out of the way first". This is that.

The kernel is now linked at `0xFFFF_FFFF_8000_0000 + 1 MiB` and loaded at
physical 1 MiB. `linker.ld` gives every section an explicit `AT()`, so the ELF
program headers carry `p_vaddr` in the higher half and `p_paddr` at 1 MiB:

```
  Type   VirtAddr           PhysAddr           Flg
  LOAD   0xffffffff80100000 0x0000000000100000 R
  LOAD   0xffffffff80101000 0x0000000000101000 R E
  ...
```

-2 GiB specifically, because that is what `-C code-model=kernel` assumes — a
flag `.cargo/config.toml` had been setting since long before it was true.

### The entry point is a virtual address

The first attempt set `ENTRY()` to the *physical* address of the trampoline,
reasoning that GRUB jumps there in 32-bit protected mode with paging off. GRUB
disagreed, and said so:

```
error: entry point isn't in a segment.
```

GRUB looks `e_entry` up in `p_vaddr` space, finds the program header containing
it, and rebases it into that header's `p_paddr` range itself. So `e_entry` is the
higher-half address and GRUB arrives at physical 1 MiB on its own.

Worth recording *how* that was found: the message goes to the VGA console, and
`--check` only captures serial — so the symptom was a completely empty log,
indistinguishable from a kernel that died on its first instruction. `run.sh` now
writes a `grub.cfg` that puts GRUB's own output on COM1 for exactly this reason.

### Two addresses for every symbol in the trampoline

Steps 1-4 of `boot32.rs` execute at physical 1 MiB with paging off, but the file
is linked with everything else. Every reference to a symbol in 32-bit code is
therefore written `sym - KERNEL_VMA`:

```asm
    movl    %eax, (mb_magic - KERNEL_VMA)
    movl    $(stack_top - KERNEL_VMA), %esp
    movl    $(p4_table - KERNEL_VMA), %eax
    movl    %eax, %cr3
```

Miss one and the failure is a triple fault with no output. The `%cr3` line is the
one that bites hardest: a truncated CR3 faults on the very next instruction,
before any of the serial output in that file can report anything.

Three more places where 32 bits is not enough:

- **`lgdt`.** A 32-bit LGDT consumes a 6-byte operand — 2-byte limit, 4-byte
  base. Sharing one descriptor between the two modes silently loads a GDTR
  pointing at the *low 32 bits* of a higher-half address, so there are now two:
  `gdt64_ptr32` with the physical base and `gdt64_ptr` with the virtual one.
- **The far jump.** `ljmp` takes a 32-bit offset and cannot reach the higher
  half. It lands at the physical address of `long_mode_low`, two instructions
  whose only job is `movabsq $long_mode_start, %rax; jmp *%rax`.
- **`movq $stack_top, %rsp`.** `mov r64, imm32` sign-extends, which happens to
  produce the right answer for a -2 GiB address — it worked by luck before and
  is a `movabsq` now.

### Virtual is not physical any more

The migration's real cost is every place that used to get away with treating the
two as the same number:

| site | before | after |
|---|---|---|
| `physical::kernel_image_range` | linker symbols as physical | `paging::kernel_phys(sym)` |
| the frame bitmap | placed and dereferenced at one address | placed physical, written through `kernel_virt` |
| `vga_buffer` | `0xb8000 as *mut Buffer` | `kernel_virt(0xb8000)` |
| the multiboot info pointer | dereferenced directly | `kernel_virt(phys)` |
| `usermode`'s `.user` blob | `__user_start` passed as a frame address | `kernel_phys(__user_start)` |
| `paging::identity_map_4k` | `PhysAddr::new(virt)` | `PhysAddr::new(kernel_phys(virt))` |

`kernel_image_range` is the one that would have been worst. It feeds the check
that keeps the allocator away from the running kernel's own frames; with virtual
symbols the comparison simply never matches, and the failure is the allocator
handing out the page tables it is executing on.

The `.user` blob fails loudly instead, which is a mercy: `PhysAddr::new` panics
on a value with bits above 52 set.

### The bootstrap window

There is an ordering problem underneath all of this. The frame bitmap is placed
and zeroed, and the first `println!` reaches VGA, *before* `paging::init` builds
the physmap. Neither can use `PHYSMAP_BASE`, and the identity map they used to
rely on is what is being removed.

The answer is that the trampoline's higher-half window covers a full 1 GiB, and
`paging::init` deliberately keeps the low few MiB of it. `kernel_virt(phys)` is
therefore valid from the first instruction of long mode through to shutdown, and
neither the bitmap pointer nor the VGA pointer ever needs rebasing.

Building the new tables has the same problem one level up: the mapper has to
write page-table frames it just allocated, and the physmap it is constructing
does not exist yet. So construction runs with the mapper offset set to
`KERNEL_VMA` rather than `0`, and a `BootstrapFrameAllocator` refuses any frame
above the 1 GiB window instead of returning a pointer that faults on first write.

`phys_to_virt` was deleted rather than fixed. It answered "the kernel-virtual
address of this frame" with either the identity or the physmap depending on a
flag — and after the migration that question has two right answers depending on
*when* you ask. Hiding that behind one function invites using the wrong one; the
call sites pick `kernel_virt` or `PHYSMAP_BASE` deliberately now. It had no
callers anyway.

### Proving it

`paging::self_test` gained a check, and `--check` gained a marker:

```
  kernel window   : 0xffffffff80000000 -> 0x0..0x400000 (4 KiB pages, W^X)
  physmap         : 0xffff800000000000 -> 0x0..0x1ffe0000 (2 MiB pages, RW+NX)
  PML4[0]         : empty — reserved for user address spaces
  translate(0xffffffff80100000) -> 0x100000: OK
  kernel out of PML4[0]: OK (0x100000 is unmapped, was the kernel image)
```

It checks that the kernel *image address* is gone from the lower half rather than
that PML4[0] is empty, because PML4[0] is exactly where user mappings live — it
is populated, just not by the kernel.

Restoring the low mapping in `init` turns that line into:

```
  WARNING: 0x100000 still maps to 0x100000 — the low identity map survived
```

and the marker fails, which is the only evidence that it is checking anything.

### One more thing this cleaned up

`memory/vmm.rs` carried a `kernel_layout` module describing kernel code at
`0xFFFFFFFF80000000`, data 16 MiB above it and a heap 16 MiB above that — three
invented numbers for a layout nothing had ever built, printed in the boot log
next to the real one. They are derived from `memory::paging` now, and the dump
reports what the page tables actually contain:

```
  Kernel Window: 0xffffffff80000000 - 0xffffffff80400000 (4096 KB) [R-XK]
  Physmap: 0xffff800000000000 - 0xffff80001ffe0000 (524160 KB) [RW-K]
  Kernel Heap: 0xffff900000000000 - 0xffff900000400000 (4096 KB) [RW-K]
```

`KERNEL_CODE_START` finally being right is not a coincidence — the migration
moved the kernel to the address that constant had been claiming all along.

## Per-process address spaces (Phase 12)

Phase 11 got the kernel out of PML4[0]. This puts a process in it.

An address space is a PML4 of its own: entries 0..256 — the whole lower half —
are the process's alone, and 256..512 are copied from the kernel's table at
creation. It copies the *entries*, not the tables under them, so every process
walks the same PDPTs for the physmap, the heap window and the kernel image. That
is what makes it safe for a syscall to allocate: a page added to the kernel heap
after a process was created is already visible to it.

```
  0x10000000 -> frame 0x5a4000 in one space, 0x5a8000 in the other
  same address 0x10000000: 0xaaaaaaaaaaaaaaaa in one space, 0xbbbbbbbbbbbbbbbb in the other
  Address spaces: PASS — 3 kernel PML4 entries shared, lower half private
```

Three entries: 256 (physmap), 288 (heap), 511 (kernel image). The copy happens
once, at creation, so a *new* top-level kernel entry appearing later would be
invisible to processes that already exist — a syscall would fault on a valid
kernel address, intermittently, depending on which process was running. Nothing
creates one; `check_kernel_half` says so rather than assuming it.

### Switching

`task::schedule` reloads CR3 when the incoming thread lives somewhere else. It
can do that in the middle of the scheduler only because everything it touches
from that instruction until the incoming thread is running — RIP, RSP, the
scheduler's statics, the GDT, the IDT, the per-CPU block — is in the shared upper
half. That was not true one phase ago, which is why this could not be written
until now.

CR3 is compared before being written, because writing it flushes the TLB and
most switches here are between kernel threads.

### Teardown, and the order that matters

`exit_current` returns CR3 to the kernel's tables **first**, then frees the
space. The other order hands the frame holding the live top-level table to the
allocator, and the next allocation overwrites the address space out from under
the CPU walking it.

Freeing walks the lower half and returns every page table it visits — but a leaf
frame only if the entry carries `OWNED_BY_ADDRESS_SPACE`. `map_user_pages_in`
allocates fresh frames and sets that bit; `map_user_range_in` maps frames the
space does not own and does not. The distinction is not theoretical: the built-in
ring-3 payload lives inside the kernel image and is mapped straight into user
space, so an untagged teardown would hand the kernel's own `.user` section to the
frame allocator.

```
  address space of thread 3 released: 98 frame(s)
```

### What this deletes

The interesting part of the diff is the removals.

| gone | why it existed |
|---|---|
| the `Resident` address-range table | to refuse a `spawn` whose image overlapped a resident one |
| `elf::extent_of` | to answer "where would this go" before mapping it |
| `LoadedImage::ranges`, `MappedRange`, `MAX_SEGMENTS`, `TooManySegments` | so somebody could unmap exactly what the loader had mapped |
| `SpawnError::AddressConflict` | there is nothing left to conflict with |
| `paging::unmap_user_pages` (whole-process form) | teardown walks the tables instead |
| `SPAWN_STACK_REGION_TOP`, `ELF_USER_STACK_TOP`, `SHELL_USER_STACK_TOP`, the per-slot 1 MiB stack stride | to keep programs' stacks from colliding |

All of it was bookkeeping around a shared lower half. Every program now loads at
its own `p_vaddr` and gets a stack at one `USER_STACK_TOP`.

### Two programs at one address

`userspace/hello/user.ld` is now `. = 0x800000` — the same address as `ksh`. That
is deliberate, and it is the test: `ksh` executing at 0x800000 spawns `hello`,
which is also linked at 0x800000, twice in the scripted session.

`ksh` can also spawn `ksh`:

```
ksh:/$ ksh
Loading 'ksh' into a new address space:
  segment: vaddr 0x800000 filesz 39813 memsz 39813 [r-x] -> 10 page(s)
spawn 'ksh': thread 3, entry 0x800000

ksh: the Kosh shell, in ring 3. Type 'help'.
ksh:/$ getpid
pid 3
ksh:/$ exit
  address space of thread 3 released: 98 frame(s)
ksh:/$ getpid
pid 2
```

Two shells, both executing at 0x800000, one nested inside the other. The previous
phase refused this with `error -98` because a second copy would have overwritten
the `.text` the first one was running.

### Proving it can fail

`address_space::self_test` maps one page at the same user virtual address in two
spaces, writes a different value into each through the physmap, then activates
each in turn and reads that address.

Pointing both maps at the *same* space — which is what the kernel did until now —
produces:

```
  Address spaces: FAIL — could not map the probe page: failed to map user page
```

`PageAlreadyMapped`, which is exactly the error `spawn` used to pre-empt with its
overlap check.

### One inconsistency this surfaced

`run_user_demo` — the built-in payload — mapped its code and stack into the
*kernel's* PML4[0] and left them there. That quietly contradicted the previous
phase's "PML4[0] is empty, reserved for user address spaces": it was empty right
up until the first ring-3 demo ran. It gets its own address space now, like
everything else, and the two demos share a stack address instead of being handed
regions a megabyte apart.

## Telling the truth about the syscall surface (Phase 13)

An audit of every syscall against what its handler actually does turned up **six
that reported success for work they had not done**. That is the specific failure
mode this whole series has been unwinding, and it was still present in the ABI.

| syscall | what it did | now |
|---|---|---|
| `mmap` | returned the constant `0x40000000` and logged "mmap successful" | allocates and maps real pages |
| `debug_print` | printed the *address* of the string under a `// TODO: Read string from user space` | prints the message |
| `debug_dump` | printed `DEBUG DUMP[1]: type 0` and dumped nothing | dumps memory, threads, syscall count or open files |
| `time` | returned `Ok(0)` — a valid timestamp, 1 Jan 1970 | reads the CMOS RTC |
| `getppid` | returned `Ok(0)` under a TODO, indistinguishable from "no parent" | `NotSupported` |
| `send_message` | substituted a synthetic string for the caller's payload | `NotSupported` |
| `receive_message` | dequeued for real, then dropped the payload | `NotSupported` |

`mmap` is the clearest case. It validated `length`, built a `MemoryProtection`
struct, threw it away, and returned a hardcoded address — then logged
`mmap successful: mapped at 0x40000000`. A caller that wrote to the pointer took
a page fault, having been told the mapping existed.

### mmap, for real

Per-process address spaces are what made this easy: the pages go into the calling
process's own tables, so there is no shared region to arbitrate. Anonymous
private mappings only, in a 256 MiB window at `0x10000000`, with W^X enforced —
a request for write+execute is refused rather than quietly granted.

Address selection is a linear scan of the process's own page tables for a hole,
rather than a bump pointer. A bump pointer would need somewhere per-process to
live and would leak address space across `munmap`.

`munmap` works too, and refuses anything outside the mmap window: letting a
process unmap its own `.text` turns a userspace bug into an unexplainable fault.

### A bug the test found immediately

The first version discriminated file-backed mappings on `fd`:

```rust
if args[4] >= 0 { return Err(NotSupported); }   // wrong
```

`args[4]` arrives in **R8**, and a caller using a three-argument syscall wrapper
never sets R8 — so the check read whatever was left in that register and refused
every mapping. `hello` printed `WARNING: mmap failed` on the first run.

The fix is to discriminate on `MAP_ANONYMOUS`, a flag the caller definitely
passed. The general lesson is worth stating: **a syscall must not read an
argument the ABI does not require the caller to set.** There is no way to tell a
deliberate zero from a stale register.

### The RTC

`platform/rtc.rs`, and the two things that make it more than a port read:

The RTC updates its registers once a second, and a read part-way through an
update returns a mix of old and new fields — 10:59:59 can read as 11:59:59. So it
waits for the update-in-progress flag, reads everything, reads everything again,
and only accepts values the two agree on. The wait is bounded, because a machine
with no RTC leaves that flag set forever and hanging the kernel on `time()` would
be worse than having no clock.

Status register B says whether the values are BCD or binary, and whether the hour
is 12- or 24-hour. Both are checked rather than assumed: a kernel that assumes
BCD on a binary RTC reports plausible-looking nonsense.

Verified against the host: the kernel logged 1785411504 while `date -u +%s` on
the host said 1785411523, and the difference is the boot delay.

### Why the IPC calls became errors instead of working

Copying the payload is the easy part; `uaccess::copy_from_user` already exists.
The blocker is that there are **two unrelated id namespaces**. `syscall/entry.rs`
passes the calling *thread's* id as a `ProcessId`, while
`ipc::message::send_message` looks the sender up in `process::ProcessTable`, whose
only entries are the three synthetic ones the boot self-test creates. A ring-3
caller is not in that table, so every send would fail `SenderNotFound` even with
a real payload.

Making `send_message` copy the buffer would have turned a fabrication into a
different fabrication. One process table that `spawn` registers into — with a
thread as a *member* of a process rather than a synonym for one — is the actual
work, and `kernel/src/ipc/` is ~85 KB of implemented queueing and capability
machinery waiting on it.

### Testing

`hello` gained the new calls, so each is exercised from ring 3 rather than by the
kernel checking itself:

```
hello from a loaded ELF binary
  mmap gave me 8192 usable bytes at 0x0000000010000000
  munmap returned the pages
  CLOCK_MONOTONIC moves forwards
DEBUG[1]: hello reached the kernel log through debug_print
  debug_print echoed my message
```

It writes to the first and last word of the mapping and reads both back, so a
mapping that is present but wrong fails rather than passing. `ksh` gained `date`,
which is the RTC end to end: RTC → `sys_time` → ring 3 → civil date.

## fork and exec (Phase 14)

Every process so far started from an ELF, via `spawn`. `fork` produces a process
from a *running* one, which is a different problem: the child has to appear in
ring 3 at the parent's next instruction, with the parent's registers and its own
copy of the parent's memory, without ever having executed the `syscall`
instruction that created it.

```
Thread 1 forking: 20 page(s) copied into PML4 0x5c5000
  forked thread 2 (kernel stack 0xffff90000005cdd0, resumes in ring 3 at 0x800f5c)
  child: my copy of the witness is mine
Thread 2 exec 'hello2':
  old image released: 26 frame(s)
  entering 'hello2' at 0x800000
  hello2 here: exec replaced the whole image
  parent: the child did not touch my memory
  child exec'd and exited 7
```

### The control-flow half

`0x800f5c` above is the instruction after the parent's `syscall`. The child gets
there entirely through the layout of its kernel stack, written by
`task::spawn_forked`. From the top down:

```text
  top-8    frame.user_rsp        a copy of the parent's SyscallFrame,
  ...        ...                 highest field first
  top-80   frame.rax  = 0
  top-88   return address = kosh_syscall_return
  top-96   rbp                   the switch frame kosh_switch_context pops
  ...        ...
  top-144  rflags
```

`kosh_switch_context` pops the seven-word switch frame and `ret`s, which leaves
RSP at `top-80` — exactly the base of the `SyscallFrame` — with RIP at
`kosh_syscall_return`. That is the tail of the syscall stub, newly given a label:
it pops the frame and `sysretq`s. The child runs no kernel code written for it.

`rax = 0` is the entire fork convention. The parent's `sys_fork` returns the
child's id; the child's frame carries a zero.

The parent's user RSP is reused unchanged, which is correct *only* because the
child has its own address space: the same number names the child's own copy of
that stack. Under the shared lower half of two phases ago, both would have been
using one stack.

### The memory half

`AddressSpace::fork` copies the parent's lower half — same contents at the same
addresses, in different frames. Leaves without `OWNED_BY_ADDRESS_SPACE` are
frames the parent borrowed (the built-in payload lives in the kernel image) and
are mapped into the child by reference; copying one would give the child a
private duplicate of the kernel's `.user` section.

**Eager, not copy-on-write, deliberately.** The textbook implementation marks
both copies read-only, shares the frames, and copies one page at a time in the
page-fault handler. That needs a per-frame reference count, and a reference count
that is wrong produces double frees and use-after-free of page tables — memory
corruption that surfaces somewhere unrelated, much later. An eager copy is
*correct* fork semantics at a cost of one memcpy per page; forking `ksh` copies
about 400 KiB. The cost is real and worth stating plainly: without an `exec`
following, a fork is pure waste, and with demand paging an untouched `.bss` would
not need copying at all. Both are the next piece of work.

### exec

A fresh address space with the new image, swapped in, and the old one freed —
in that order, because freeing first hands the page tables the CPU is walking to
the allocator.

The new space is built *completely* before anything is disturbed. An `exec` that
fails half-way has destroyed the only thing it could return to, so a load failure
has to leave the caller running its current program.

`exec` does not need the caller's register frame; there is nothing to return to.
The syscall frame on the kernel stack is simply abandoned when `enter_ring3`
takes over.

### Where fork had to be intercepted

`dispatch_syscall` takes `(pid, number, args)` — arguments, not registers. `fork`
is the one syscall that needs the whole frame, so `kosh_syscall_handler` handles
it before dispatching rather than threading a `&mut SyscallFrame` through a
signature every other handler would ignore. The dispatcher's `sys_fork` still
exists, and logs `BUG:` if it is ever reached.

### Proving it

`hello` writes a witness value, forks, and both sides write a different one. If
the address spaces were shared, the child's write would be visible to the parent.
`hello2` is a separate program linked at the same address as `hello` and `ksh`,
so the only evidence `exec` worked is that something else is running there.

Making `fork_from` share the lower half instead of copying it — one line — is
more instructive than expected:

```
Thread 1 forking: 0 page(s) copied into PML4 0x5c4000
  child: my copy of the witness is mine
Thread 2 exec 'hello2':
  old image released: 26 frame(s)
...
Process 1 syscall wait failed: InvalidArgument

==================== PAGE FAULT ====================
  accessed address : 0x0000000000800f84
  cause            : page not present while fetching an instruction in user mode
```

The child's `exec` freed the *parent's* pages, because they were the same pages,
and the parent then faulted fetching its own `.text`. Five markers fail.

### One refusal that had outlived its reason

`ksh` refused `cmd &` with "background jobs need fork". They do not, quite: what
a background job needs is a way to start a program and *not* block, which `spawn`
has provided since Phase 10. The refusal was written when it was true and never
revisited.

```
ksh:/$ hello2 &
[3] hello2
ksh:/$   hello2 here: exec replaced the whole image
```

No job control — no `jobs`, no `fg`, no notification on exit. The task id is
printed so a `wait` is at least possible by hand.

## Copy-on-write (Phase 15)

The previous phase's `fork` copied every page eagerly. That is correct, and it
cost about 400 KiB per fork of `ksh` — most of it a `.bss` the child `exec`s away
microseconds later. This is the optimisation, and the reference counting it
needs.

```
Thread 1 forking: 20 page(s) shared copy-on-write into PML4 0x5e9000 (20 shared system-wide)
  child: I inherited the value my parent set before forking
  child: my copy of the witness is mine
  parent: the child did not touch my memory
  parent: still writable after the child exited
Copy-on-write: 3 fault(s) resolved, 2 needed a copy, 0 frame(s) still shared
```

Twenty pages shared, zero copied at `fork` time, three copies made lazily —
exactly the pages that were actually written.

### Both sides, not just the child

`fork` shares every owned leaf: the frame's reference count goes up, and *both*
the parent's entry and the child's lose `WRITABLE` and gain `COPY_ON_WRITE`.

Marking only the child is the classic bug, and it is silent: the parent's next
write lands in the page the child is about to read, and the child sees a value
its parent wrote *after* the fork. The test is built to catch precisely that, and
the sequencing is not racy — `fork` returns in the parent with the child merely
`Ready`, so the parent's write is guaranteed to happen first:

```
  WARNING: child saw 0000000033333333 — the parent wrote through a shared page
```

That is what removing one line — `source_mut[i].set_flags(shared)` — produces.

Pages that were *already* read-only (`.text`, `.rodata`) are shared without the
marker. Nothing can write them, so there is nothing to copy on; they still need
the reference count, and they now have it.

`COPY_ON_WRITE` is a separate bit from `WRITABLE` being clear, because a
genuinely read-only page must keep faulting rather than being handed a writable
copy.

### The parent's TLB

The parent's own entries just lost `WRITABLE`, and the CPU has the old ones
cached. `fork_from` reloads CR3, which flushes every non-global entry. Blunt, and
correct; the alternative is one `invlpg` per shared page, and there are as many
of those as pages. It is only valid because the parent is the *current* address
space, which it is — `fork` runs in the caller.

### Resolving

`resolve_cow` has two cases, and the second matters more than it looks:

* **The frame has other holders.** Allocate one, copy 4 KiB through the physmap,
  repoint the entry, drop a reference to the original.
* **This is the last holder.** Nothing to copy — clear the marker and restore
  `WRITABLE`. Without this, a process whose child has already exited would keep
  paying a full copy for every page it writes, forever, with nobody to copy away
  from.

Both are exercised: `2 needed a copy` out of `3 fault(s) resolved`. The third is
the parent writing after the child exited.

### Two syscall-path hazards

**`copy_to_user` writes at the user address.** With `CR0.WP` set, a kernel write
to a read-only page traps — from ring 0, in the middle of a syscall that may be
holding locks the fault handler wants. So `validate_user_range` resolves
copy-on-write *up front* when the caller asks for write access, turning what
would be a kernel-mode fault into an ordinary function call. It also means a
failure comes back as an error return rather than an exception.

**`validate_user_range` would have rejected the write.** A shared page is not
`WRITABLE`, so `read(fd, buf)` into freshly-forked memory would have failed with
`NotWritable` before the copy could happen. It treats `COPY_ON_WRITE` as
writable-after-resolution.

### The reference count, and two bugs finding a home for it

One byte per frame — the sharers of a frame are address spaces, and the thread
table holds 16, so 255 is a long way past enough. `allocate_frame` sets it to 1;
`deallocate_frame` decrements and only really frees at zero. A frame the
allocator never handed out has a count of 0, and freeing one is refused with a
warning rather than putting the running kernel's own pages on the free list.

Placing the table found two bugs in the same afternoon, both of the same shape —
**a rule that was correct until the thing it described changed size**:

1. The metadata went "immediately after the kernel image", which was fine while
   it was a 16 KiB bitmap. The refcount table is 128 KiB for 512 MiB of RAM, and
   that reaches into the module GRUB loaded at `0x195000`. The symptom was both
   boot modules reading as all zeros and the ELF loader reporting `BadMagic`,
   several subsystems from the cause. It is now computed from the kernel *and*
   every module end — the only version without a size at which it silently
   breaks. (A hardcoded 2 MiB was the rule before that, and failed the same way
   when the kernel grew.)

2. The reservation loop excluded `bitmap_start .. bitmap_start + bitmap.len()`,
   using the bitmap's length as a proxy for "the metadata" — true when the bitmap
   *was* the metadata. So the allocator handed out the frames holding the
   refcount table, which promptly filled with page data:

   ```
   Thread 1 forking: 20 page(s) shared copy-on-write (112969 shared system-wide)
   ```

   112,969 frames shared after a fork of twenty pages. Worth logging a number you
   can sanity-check by eye.

### No leak

Two consecutive `hello` runs from `ksh` — each a fork, an exec, and two
teardowns — both end at 1613 used pages.

## What is deliberately still missing
- **The filesystem is read-only.** Allocating clusters and keeping both FAT
  copies consistent is a separate problem, and a read-only filesystem that is
  correct beats a read-write one that is not.
- **No partition table support.** The BPB is read from LBA 0, so the image is a
  bare filesystem rather than a partitioned disk.
- **The VFS is still unwired.** `userspace/fs-service/src/vfs.rs` has a mount
  table; it has never had a real filesystem underneath it, and connecting the
  two is separate work from making FAT32 correct.
- **No `fork`/`exec`.** `spawn` loads a *second* program rather than duplicating
  the caller, so there is still no way for a program to replace its own image or
  to inherit one. `userspace/init` cannot do its job until `fork` exists.
- **No `fork`.** Every process starts from an ELF. Duplicating a *running*
  address space needs the page tables copied and every writable page marked
  copy-on-write, plus a `#PF` handler that does the copying — none of which
  exists. The address space is now the right shape for it.
- **No demand paging.** Every page of a program is allocated and populated at
  load time, so a 256 KiB `.bss` costs 65 frames whether or not it is touched.
- **The descriptor table is still global.** One table for the system, because
  `Program` has nowhere to hang a per-process one yet. `sys_exit` works around
  it by only closing everything when it is the last program running.
- **No IPC or capabilities from ring 3.** Both subsystems are implemented
  in-kernel and unreachable, waiting on a single process-id namespace — see
  Phase 13 above.
- **No pipes or background jobs.** `ksh` parses them; running them needs two
  programs connected by a shared descriptor, which needs per-process descriptor
  tables. The table is currently global — see the note in `sys_exit`.
- **Swap and power management are disabled** in `init_kernel`. Both were
  simulated, and swap tried to allocate 8 MiB from a 1 MiB heap.
- **Only IRQ0 and IRQ1 are unmasked.** Everything else on the PIC has a
  handler that acknowledges and returns.
