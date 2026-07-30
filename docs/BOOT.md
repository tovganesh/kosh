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

`SFMASK` masks IF, so a syscall cannot be interrupted. That is what makes the
single static kernel syscall stack safe for now; multiple user threads will need
`swapgs` and a per-CPU block.

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

## What is deliberately still missing
- **The filesystem is read-only.** Allocating clusters and keeping both FAT
  copies consistent is a separate problem, and a read-only filesystem that is
  correct beats a read-write one that is not.
- **No partition table support.** The BPB is read from LBA 0, so the image is a
  bare filesystem rather than a partitioned disk.
- **The VFS is still unwired.** `userspace/fs-service/src/vfs.rs` has a mount
  table; it has never had a real filesystem underneath it, and connecting the
  two is separate work from making FAT32 correct.
- **No `fork`/`exec`.** `userspace/init` cannot do its job until those exist, so
  the ELF payload is `userspace/hello` rather than init.
- **One address space.** Loaded programs share the kernel's page tables; they
  are protected by page permissions, not by separation. Per-process address
  spaces come with `fork`.
- **One ring-3 thread at a time.** The syscall path uses a single static kernel
  stack, which is only safe because `SFMASK` masks IF and nothing else is in
  user mode. Several user threads need `swapgs` and per-CPU data.
- **Swap and power management are disabled** in `init_kernel`. Both were
  simulated, and swap tried to allocate 8 MiB from a 1 MiB heap.
- **Only IRQ0 and IRQ1 are unmasked.** Everything else on the PIC has a
  handler that acknowledges and returns.
