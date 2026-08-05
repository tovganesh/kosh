NOTE:
Initially, I built this using Kiro, using a spec based approach.
Now using Claude to build further.

Initial thoughts and ideas: https://blog.tovganesh.in/2012/01/kosh-building-mobile-user-experience.html

At this point this is my personal toy project. I currently have no idea if this will actually work. If you are interested in writing a rust based OS with or without AI tools you can poke me.

---

# Kosh

An x86-64 operating system kernel written in Rust. It boots on QEMU from a
Multiboot2 ISO, runs programs in ring 3 with per-process address spaces, reads a
FAT32 disk, and gives you a shell.

```
ksh: the Kosh shell, in ring 3. Type 'help'.
ksh:/$ ls
d           -  DOCS
-         199  README.TXT
-       22000  BIG.TXT
-          26  A Long File Name.txt

6 entries, 22280 bytes
ksh:/$ date
2026-07-30 11:42:25 UTC
ksh:/$ hello
hello from a loaded ELF binary
  my pid is 3
  mmap gave me 8192 usable bytes at 0x0000000010000000
  exiting cleanly
ksh:/$ ksh
ksh: the Kosh shell, in ring 3. Type 'help'.
ksh:/$ getpid
pid 4
ksh:/$ exit
ksh:/$ getpid
pid 3
```

## About this README

This project has spent most of its life describing things it could not do. The
kernel had a "virtual memory manager" that read the bootloader's page tables and
described them; a shell whose `read_line` replayed a hardcoded array of six
commands; a `sys_open` that returned the literal `3` for every path. For 27
commits the kernel never executed a single instruction, because Multiboot2 hands
off in 32-bit mode and nothing bridged the gap.

Unwinding that is most of the work that has happened since, so this file tries to
be accurate about what runs and blunt about what doesn't. If something is listed
under **Works**, there is a marker in `scripts/run.sh` that fails if it stops
working.

`docs/BOOT.md` is the long version: how the boot chain, paging, scheduling,
ring 3, the filesystem, the address-space work and the driver interface actually
function, including the bugs found along the way and how each was caught.

## Running it

One command builds the kernel, the four userspace programs, a GRUB ISO and a FAT32
test disk, then boots the lot:

```bash
./scripts/run.sh              # boot it, serial on stdio (ctrl-a x to quit)
./scripts/run.sh --check      # boot headless, assert 62 serial markers (CI)
./scripts/run.sh --check-cli  # drive the shell through QEMU's monitor, assert 35
./scripts/run.sh --debug      # same as plain run, plus a gdb stub on :1234
```

You need `cargo` (nightly), `grub-mkrescue`, `xorriso`, `qemu-system-x86_64`,
`mkfs.vfat` and `mcopy`. `scripts/run.sh` checks for all of them and tells you
what to install.

The other scripts in `scripts/` predate the current build and are stale —
`build.sh` and `build-iso.sh` in particular do not build the two userspace
binaries that actually ship, and do not handle the `-Z build-std` flags the
kernel now needs. `run.sh` is the only supported entry point.

`.github/workflows/boot.yml` runs `--check` on every push, so "it boots" is a
merge gate rather than a claim.

## What works

Each of these is asserted by a serial marker in `--check` or a scripted console
interaction in `--check-cli`.

**Boot**
- Multiboot2 hand-off, a 32-bit trampoline into long mode, and a **higher-half
  kernel** linked at `0xFFFFFFFF80100000` and loaded at 1 MiB
- Early COM1 output from assembly, so a failure before Rust is visible
- GRUB's own output on the serial port too, because a kernel GRUB refuses to load
  otherwise looks identical to one that hung

**Memory**
- Page tables the kernel builds itself, with **W^X** enforced — `.text` is
  read-execute, `.rodata` and everything else is NX, and `CR0.WP` plus
  `EFER.NXE` are set so those bits actually bind in ring 0
- A physmap of all RAM at `0xFFFF800000000000`, a heap window at
  `0xFFFF900000000000`, and page 0 left unmapped as a null guard
- A bitmap frame allocator that excludes the running kernel and the boot tables
- A first-fit kernel heap with coalescing, alignment handling and stats

**Processes and scheduling**
- Preemptive round-robin over kernel threads, driven by the PIT at 100 Hz
- **Per-process address spaces**: a PML4 each, kernel's upper half shared. Two
  programs can be — and `hello`, `hello2` and `ksh` all are — linked at the
  same address
- **`fork` and `exec`**: the child returns from a syscall it never executed;
  `exec` replaces the whole image
- **Copy-on-write**: a `fork` copies no page data at all. Both sides go
  read-only, the frame reference count goes up, and the first write from either
  side faults into a private copy
- **Demand paging**: `.bss`, user stacks and anonymous `mmap` are *reserved*
  rather than allocated — a page table entry with no frame behind it until the
  program touches it. A boot that runs five programs reserves 322 pages and
  touches 38
- **Per-thread kernel stacks**, so more than one thread can be inside a syscall
- Real blocking: `wait` parks a thread in `State::Blocked` rather than spinning

**Ring 3**
- `SYSCALL`/`SYSRET` with a `swapgs`-free per-CPU block reached through `gs:`
- User-pointer validation that checks range *and* page permissions, tested from
  the user side by a payload that deliberately passes a kernel address
- A ring-3 fault of *any* kind kills the process and the kernel carries on —
  not just a page fault, which is all it used to be
- A static ELF64 loader: `PT_LOAD` segments mapped at their `p_vaddr` with
  per-segment permissions and a zeroed `.bss` tail

**Devices and storage**
- 8259 PIC remap, PIT timer, PS/2 keyboard on IRQ1
- ATA PIO driver on the legacy IDE ports — IDENTIFY, LBA28 reads
- **A disk driver in ring 3.** `userspace/ata-driver` reads the same disk from
  an unprivileged process, using `in` and `out` directly — no syscall per port.
  Its ports come from the TSS I/O permission bitmap, 9 bits granted to that
  thread and denied to every other; a process without the grant that touches
  0x1F7 is killed and the system carries on
- Devices are named, not addressed: `request_device("ata0")` is checked against a
  `DeviceAccess` capability, and the kernel decides which ports the name means —
  so a disk driver cannot ask for the interrupt controller
- One driver per device at a time: while a ring-3 driver holds `ata0`, the
  kernel's own block layer refuses that channel, and the claim is released even
  if the driver crashes
- Read-only FAT32: BPB validation, cluster-chain walking, long filenames
- CMOS RTC, so `date` prints the real date

**Userspace**
- `hello` — a static ELF that proves the loader, and exercises `mmap`,
  `munmap`, `clock_gettime` and `debug_print`
- `ksh` — a shell in ring 3 with line editing, history, arrow keys, a
  recursive-descent parser, and `ls`/`cat`/`cd`/`stat`/`date`/`getpid`
- `ksh` can launch programs: unknown commands become `spawn` + `wait`, and
  `cmd &` starts one in the background
- `ata-driver` — the ATA driver as an ordinary ring-3 process, driving a real
  disk with `in` and `out` and answering block reads over IPC

**Processes and IPC**
- A process *is* a ring-3 thread with an address space, registered in the
  process table at the same id — one namespace, not two
- `send_message`/`receive_message` carry real bytes, with a blocking receive
  that parks the thread rather than spinning
- **Capabilities that can refuse**: `fork` and `spawn` open a channel between a
  process and its parent, and nothing else. A message to any other process is
  `PermissionDenied`, and the test checks it

**In-kernel console**
- A fallback shell on the same keyboard, which takes over when `ksh` exits — so
  there is a prompt on a system whose userspace has died

## The syscall surface

46 numbers are defined; **25 do the work** and the other 21 return an error
saying so. Nothing returns success for work it did not do.

**Working** (all 25 exercised from ring 3, not just by the kernel checking
itself):

`exit` `fork` `exec` `wait` `getpid` `getppid` `yield` `spawn` · `mmap`
`munmap` · `open` `close` `read` `write` `lseek` `stat` `getdents` · `time`
`clock_gettime` · `send_message` `receive_message` · `request_device`
`release_device` · `debug_print` `debug_dump`

**Refuses with `NotSupported`, honestly:**

`kill` · `mprotect` `brk` `sbrk` · `fstat` `mkdir` `rmdir` `unlink` ·
`reply_message` `create_channel` `destroy_channel` · all 4 driver calls · all 4
capability calls · `uname` `sysinfo`

## What does not work

Being explicit about this, because the earlier version of this file claimed most
of it.

- **Demand paging is anonymous-only.** A reserved page is always filled with
  zeros. File-backed mappings would need the fault handler to read through the
  VFS, so `mmap` still refuses anything but `MAP_ANONYMOUS`. There is also no
  reclaim: nothing ever takes a page *back*, so the only pressure valve is a
  process exiting.
- **No `argv`.** `exec` takes a program name and nothing else; passing arguments
  needs somewhere to put the strings in the new address space.
- **The filesystem is read-only.** `ksh` refuses redirection rather than
  pretending; there is no `mkdir`, `unlink` or write path.
- **IPC is send/receive only.** `reply_message`, `create_channel` and
  `destroy_channel` still refuse, and there is no timeout on a blocking receive:
  a process waiting for a message that never comes waits forever.
- **Capabilities are not exposed to userspace.** They are enforced on the IPC
  path — a process may message its parent and its children, and nothing else —
  but the four capability syscalls still return `NotSupported`, so a program
  cannot inspect or delegate what it holds.
- **Only the disk driver is out of the kernel, and there are two of it.** The
  filesystem and the keyboard driver are still in-kernel, and `block/ata.rs` is
  still there too — it stands aside while the ring-3 driver holds the channel
  rather than being gone. It can be deleted once `fat32` moves out and stops
  needing it, which is the next piece of work.
- **Drivers are trusted by boot-module name.** `DRIVER_IMAGES` in `usermode.rs`
  maps `ata-driver` to `ata0`, so the trust root is "GRUB loaded it from the
  ISO". A real capability system delegates from `init`; this is a two-entry table
  standing in for one.
- **Ring 3 cannot receive interrupts.** The userspace driver polls, exactly as
  the in-kernel one does. Turning IRQ14 into a message is not implemented.
- **`userspace/init`, `fs-service`, `driver-manager` do not run.** They are not
  built, not on the ISO, have no linker scripts, and depend on `fork`/`exec`.
- **`drivers/*` are not drivers.** `storage` and `network` are trait skeletons
  whose `init` sets a bool and returns `Ok(())`; `keyboard` declares the PS/2
  ports and then returns `0` from `read_data` with a comment saying it would use
  them in a real implementation; `graphics` does touch `0xB8000` but is built for
  the host and never loaded. The kernel's own ATA and keyboard drivers are the
  real ones.
- **`shared/kosh-ipc` and `shared/kosh-driver` are used by nothing that ships.**
- **No ARM64.** `aarch64-kosh.json` is a valid target spec and nothing more: the
  build does not compile for it (the panic handler, `serial`, `vga_buffer` and
  `memory` are all x86-only and not gated), there is no boot path, and
  `kernel/src/platform/aarch64/` says "stub implementations" in its own module
  docs.
- **No network, no graphics beyond VGA text, no UEFI, no SMP.**
- **Swap and power management are disabled** in `boot.rs`, with the reason
  in-line: swap tried to allocate 8 MiB from a 1 MiB heap, and power management
  was entirely simulated.

## Layout

```
kernel/src/
  boot32.rs        32-bit Multiboot2 trampoline; builds the bootstrap map
  boot.rs          init_kernel: brings each subsystem up, in order
  gdt.rs           GDT/TSS, with the descriptor order sysret forces
  percpu.rs        per-CPU block reached through gs:, for the syscall stub
  interrupts/      IDT, exception handlers, PIC, PIT, keyboard
  memory/
    physical.rs      bitmap frame allocator
    paging.rs        the kernel's own page tables, W^X, physmap
    address_space.rs per-process PML4s
    heap.rs          first-fit allocator with coalescing
  task/            preemptive kernel threads and the context switch
  syscall/         SYSCALL entry, dispatcher, user-pointer checks, files
  elf.rs           static ELF64 loader
  block/ata.rs     ATA PIO driver (stands aside for the ring-3 one)
  fs/fat32.rs      read-only FAT32
  console/         the in-kernel fallback shell
  platform/rtc.rs      CMOS real-time clock
  platform/devports.rs which ports a named device is, and who holds it
userspace/
  hello/           static ELF that proves the loader and the newer syscalls
  hello2/          what hello execs into — a different image at the same address
  ata-driver/      the ATA driver, in ring 3, talking to the disk over IPC
  shell/           ksh
docs/BOOT.md       how all of the above actually works
scripts/run.sh     build, ISO, disk image, boot, and the two CI gates
```

Everything else in the tree — `drivers/`, `userspace/init`, `fs-service`,
`driver-manager`, `shared/`, and the other scripts — is either vestigial or not
yet wired up. See **What does not work**.

## Where this is going

Roughly in order, each unblocking the next:

- [x] Boot to long mode, higher-half kernel
- [x] Page tables, W^X, physmap, heap
- [x] Preemptive scheduling
- [x] Ring 3, `SYSCALL`/`SYSRET`, ELF loading
- [x] ATA + read-only FAT32
- [x] A shell in userspace that can launch programs
- [x] Per-process address spaces
- [x] `fork` and `exec`
- [x] Copy-on-write, so a `fork` costs a page table rather than a program
- [x] Demand paging, so an untouched `.bss` costs nothing at all
- [x] One process-id namespace, so IPC and capabilities became reachable
- [x] The disk driver out of the kernel, with port permissions to make it possible
- [ ] The filesystem out of the kernel, so `block/ata.rs` can be deleted
- [ ] `argv` for `exec`, so a driver can be told what to serve
- [ ] FAT32 writes
- [ ] `userspace/init` doing its job

## Screenshots

<img width="1456" height="812" alt="image" src="https://github.com/user-attachments/assets/5ed82d95-751e-447d-9177-4d340a2eed97" />

(The first boot)

## Why Rust

Memory safety without a runtime, and `unsafe` as a marker rather than a mode:
the places where this kernel talks to hardware are visible in the source because
they have to be spelled out. It has not prevented a single one of the bugs in
`docs/BOOT.md` — those were all logic, not memory safety — but it did keep them
to logic.

## License

MIT. See [LICENSE](LICENSE).
