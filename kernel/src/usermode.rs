//! Ring 3.
//!
//! `iretq`, `sysretq`, `swapgs` and `user_code_segment` had zero occurrences in
//! this repository before Phase 5. For a microkernel — an architecture whose
//! entire premise is that services run *outside* the kernel — that was the
//! single biggest gap in the project.
//!
//! ## Getting to ring 3
//!
//! There is no "jump to user mode" instruction. You fake a return from one:
//! push the five words `iretq` expects (SS, RSP, RFLAGS, CS, RIP) with ring-3
//! selectors, and execute it. The CPU cannot tell the difference between that
//! and returning from a genuine interrupt.
//!
//! ## The payload
//!
//! `user_program.rs` assembles a small position-independent blob into its own
//! `.user` linker section. Phase 5 maps that section's existing frames at a
//! user virtual address rather than allocating and copying — the copying
//! version is an ELF loader, which is Phase 6. The blob is written in assembly
//! with every reference RIP-relative, so it runs correctly at an address it was
//! not linked for.

use spin::Mutex;
use x86_64::structures::paging::PageTableFlags;

use crate::memory::paging::{self, PHYSMAP_BASE};
use crate::memory::PAGE_SIZE;
use crate::serial_println;

/// A boot module as GRUB placed it, copied out of the multiboot2 structure
/// before that borrow ends.
#[derive(Debug, Clone, Copy)]
pub struct BootModule {
    pub start: u64,
    pub end: u64,
    /// The module's GRUB command line, which is how `module2 /boot/ksh ksh`
    /// names it. Stored as a fixed array because this lives in a static and
    /// nothing here should allocate before the heap exists.
    name: [u8; 32],
}

impl BootModule {
    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    pub fn name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }

    /// The module's bytes, viewed through the physmap.
    ///
    /// # Safety
    /// Only valid after `paging::init`, and only while the frames remain
    /// reserved — which `physical::reserve_boot_modules` guarantees.
    pub unsafe fn bytes(&self) -> &'static [u8] {
        core::slice::from_raw_parts((PHYSMAP_BASE + self.start) as *const u8, self.len())
    }
}

const MAX_BOOT_MODULES: usize = 4;
static BOOT_MODULES: Mutex<[Option<BootModule>; MAX_BOOT_MODULES]> =
    Mutex::new([None; MAX_BOOT_MODULES]);

/// Copy the module table out of the multiboot2 info.
pub fn record_boot_modules(boot_info: &multiboot2::BootInformation) {
    let mut slots = BOOT_MODULES.lock();
    let mut n = 0;

    for module in boot_info.module_tags() {
        if n >= MAX_BOOT_MODULES {
            serial_println!("  (ignoring boot modules beyond {})", MAX_BOOT_MODULES);
            break;
        }

        let name = module.cmdline().unwrap_or("");
        let mut name_buf = [0u8; 32];
        let bytes = name.as_bytes();
        let take = core::cmp::min(bytes.len(), name_buf.len() - 1);
        name_buf[..take].copy_from_slice(&bytes[..take]);

        slots[n] = Some(BootModule {
            start: module.start_address() as u64,
            end: module.end_address() as u64,
            name: name_buf,
        });

        serial_println!(
            "Boot module {}: 0x{:x}..0x{:x} ({} bytes) '{}'",
            n,
            module.start_address(),
            module.end_address(),
            module.end_address() - module.start_address(),
            name
        );
        n += 1;
    }

    if n == 0 {
        serial_println!("No boot modules supplied by the bootloader");
    }
}

/// The nth boot module, if present.
pub fn boot_module(index: usize) -> Option<BootModule> {
    BOOT_MODULES.lock().get(index).copied().flatten()
}

/// Find a module by the name given on its `module2` line.
///
/// By name rather than by index, so adding a module to grub.cfg cannot silently
/// change which one something else loads.
pub fn boot_module_named(name: &str) -> Option<BootModule> {
    BOOT_MODULES
        .lock()
        .iter()
        .flatten()
        .find(|m| m.name() == name)
        .copied()
}

/// Where the user blob is mapped. 1 GiB — clear of the kernel's low identity
/// window and nowhere near the higher half.
pub const USER_CODE_BASE: u64 = 0x0000_0000_4000_0000;

/// Top of the user stack, growing down.
pub const USER_STACK_TOP: u64 = 0x0000_0000_5000_0000;

/// Pages of user stack.
const USER_STACK_PAGES: usize = 4;

extern "C" {
    static __user_start: u8;
    static __user_end: u8;
    fn kosh_user_entry();
    fn kosh_user_fault_entry();
    fn kosh_user_pingpong_entry();
}

/// Which payload to run.
#[derive(Debug, Clone, Copy)]
pub enum Demo {
    /// Syscalls, including one the kernel must refuse.
    Syscalls,
    /// Dereference a kernel address directly — must be killed, not survived.
    Fault,
}

static CODE_MAPPED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

fn sym(s: &u8) -> u64 {
    s as *const u8 as u64
}

/// Map the user blob and its stack, then drop to ring 3.
///
/// Runs as a kernel thread, so `sys_exit` can retire it through
/// `task::exit_current()` and hand control back to the rest of the kernel —
/// no unwinding required.
pub fn run_user_demo(which: usize) {
    use core::sync::atomic::Ordering;

    let demo = if which == 0 { Demo::Syscalls } else { Demo::Fault };

    let blob_start = sym(unsafe { &__user_start });
    let blob_end = sym(unsafe { &__user_end });
    let blob_pages = ((blob_end - blob_start) as usize).div_ceil(PAGE_SIZE).max(1);

    // Offset of the entry point within the section, so we can find it again at
    // the address the blob is actually mapped to.
    let entry_fn = match demo {
        Demo::Syscalls => kosh_user_entry as usize as u64,
        Demo::Fault => kosh_user_fault_entry as usize as u64,
    };
    let user_entry = USER_CODE_BASE + (entry_fn - blob_start);

    serial_println!("Preparing ring 3 ({:?}):", demo);

    // Read + execute for the user, and NOT writable — W^X applies to userspace
    // too.
    let code_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if !CODE_MAPPED.swap(true, Ordering::SeqCst) {
        serial_println!(
            "  .user section  : 0x{:x}..0x{:x} ({} page(s))",
            blob_start,
            blob_end,
            blob_pages
        );
        if let Err(e) = paging::map_user_range(USER_CODE_BASE, blob_start, blob_pages, code_flags) {
            serial_println!("  failed to map user code: {}", e);
            return;
        }
        serial_println!("  code mapped at : 0x{:x} (R-X, user)", USER_CODE_BASE);
    }
    serial_println!("  entry          : 0x{:x}", user_entry);

    // Each run gets its own stack region so the second demo cannot observe the
    // first one's leftovers.
    let stack_top = USER_STACK_TOP - (which as u64 * 0x10_0000);
    let stack_bottom = stack_top - (USER_STACK_PAGES * PAGE_SIZE) as u64;
    let stack_flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;
    if let Err(e) = paging::map_user_pages(stack_bottom, USER_STACK_PAGES, stack_flags) {
        serial_println!("  failed to map user stack: {}", e);
        return;
    }
    serial_println!(
        "  stack          : 0x{:x}..0x{:x} (RW-, user, NX)",
        stack_bottom,
        stack_top
    );

    serial_println!("Entering ring 3 via iretq...");
    serial_println!("--- ring 3 output ---");

    unsafe { enter_ring3(user_entry, stack_top) }
}

// ---------------------------------------------------------------------------
// Resident programs
// ---------------------------------------------------------------------------

/// How many loaded programs can be resident at once. Bounded by the thread
/// table, and small because they all share one address space.
const MAX_PROGRAMS: usize = 8;

/// Marker for a reservation whose thread has not claimed it yet.
const NO_THREAD: usize = usize::MAX;

/// Top of the stack region handed to spawned programs. Each slot gets 1 MiB of
/// address space, well clear of the fixed stacks the boot demos use.
const SPAWN_STACK_REGION_TOP: u64 = 0x0000_0000_3000_0000;

/// A program currently occupying part of the address space.
///
/// This table is what makes `spawn` safe in a single address space. Every
/// userspace program here is linked at a fixed `p_vaddr` (`userspace/*/user.ld`),
/// so two programs whose images overlap cannot both be resident — and `ksh`
/// spawning `ksh` would overwrite its own `.text` while executing it. The table
/// is also what lets a program's pages be freed when it exits, which is what
/// makes running one twice possible.
#[derive(Clone, Copy)]
struct Resident {
    /// Thread that owns this program, or [`NO_THREAD`] until it claims it.
    thread: usize,
    name: [u8; 32],
    lo: u64,
    hi: u64,
    entry: u64,
    stack_top: u64,
    ranges: [Option<crate::elf::MappedRange>; crate::elf::MAX_SEGMENTS],
    stack: crate::elf::MappedRange,
}

impl Resident {
    fn name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

static RESIDENT: Mutex<[Option<Resident>; MAX_PROGRAMS]> = Mutex::new([None; MAX_PROGRAMS]);

/// Why a `spawn` could not happen.
#[derive(Debug)]
pub enum SpawnError {
    /// No boot module by that name. This is what a shell turns into
    /// "command not found", so it must be distinguishable from the rest.
    NotFound,
    /// The image overlaps a program that is already resident.
    AddressConflict,
    TooManyPrograms,
    Load(crate::elf::ElfError),
    Map(&'static str),
    NoThread(&'static str),
}

/// Load a boot module and run it on a new kernel thread in ring 3.
///
/// Returns the thread id, which is what `wait` takes.
///
/// The ELF is loaded here, in the *caller's* thread, rather than in the new one.
/// That is deliberate: a load failure is then a synchronous error the caller can
/// act on, instead of a thread that starts and immediately dies with the reason
/// only in the kernel log.
pub fn spawn_program(name: &str) -> Result<usize, SpawnError> {
    let module = boot_module_named(name).ok_or(SpawnError::NotFound)?;
    let image = unsafe { module.bytes() };

    let (lo, hi) = crate::elf::extent_of(image).map_err(SpawnError::Load)?;

    // Reserve a slot and check for conflicts under one lock, so two concurrent
    // spawns cannot both pass the check and then both load.
    let slot = {
        let mut table = RESIDENT.lock();

        for existing in table.iter().flatten() {
            if lo < existing.hi && existing.lo < hi {
                serial_println!(
                    "spawn '{}' refused: 0x{:x}..0x{:x} overlaps resident '{}' at 0x{:x}..0x{:x}",
                    name,
                    lo,
                    hi,
                    existing.name(),
                    existing.lo,
                    existing.hi
                );
                return Err(SpawnError::AddressConflict);
            }
        }

        let slot = table
            .iter()
            .position(|s| s.is_none())
            .ok_or(SpawnError::TooManyPrograms)?;

        let mut name_buf = [0u8; 32];
        let bytes = name.as_bytes();
        let take = core::cmp::min(bytes.len(), name_buf.len() - 1);
        name_buf[..take].copy_from_slice(&bytes[..take]);

        // Placeholder, so the range is claimed against other spawns while this
        // one loads. Filled in properly below.
        table[slot] = Some(Resident {
            thread: NO_THREAD,
            name: name_buf,
            lo,
            hi,
            entry: 0,
            stack_top: 0,
            ranges: [None; crate::elf::MAX_SEGMENTS],
            stack: crate::elf::MappedRange { start: 0, pages: 0 },
        });

        slot
    };

    match load_into_slot(slot, image) {
        Ok(()) => {}
        Err(e) => {
            release_slot(slot);
            return Err(e);
        }
    }

    match crate::task::spawn("spawned", enter_spawned, slot) {
        Ok(thread) => {
            serial_println!(
                "spawn '{}': thread {}, entry 0x{:x}, image 0x{:x}..0x{:x}",
                name,
                thread,
                RESIDENT.lock()[slot].as_ref().map(|r| r.entry).unwrap_or(0),
                lo,
                hi
            );
            Ok(thread)
        }
        Err(e) => {
            release_slot(slot);
            Err(SpawnError::NoThread(e))
        }
    }
}

fn load_into_slot(slot: usize, image: &[u8]) -> Result<(), SpawnError> {
    let loaded = unsafe { crate::elf::load(image) }.map_err(SpawnError::Load)?;

    let stack_top = SPAWN_STACK_REGION_TOP - (slot as u64 * 0x10_0000);
    let stack_bottom = stack_top - (SHELL_STACK_PAGES * PAGE_SIZE) as u64;
    let stack_flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;

    paging::map_user_pages(stack_bottom, SHELL_STACK_PAGES, stack_flags)
        .map_err(SpawnError::Map)?;

    let mut table = RESIDENT.lock();
    if let Some(r) = table[slot].as_mut() {
        r.entry = loaded.entry;
        r.stack_top = stack_top;
        r.ranges = loaded.ranges;
        r.stack = crate::elf::MappedRange {
            start: stack_bottom,
            pages: SHELL_STACK_PAGES,
        };
    }

    Ok(())
}

/// Entry point of a spawned thread: claim the slot, then drop to ring 3.
fn enter_spawned(slot: usize) {
    let target = {
        let mut table = RESIDENT.lock();
        match table.get_mut(slot).and_then(|s| s.as_mut()) {
            None => None,
            Some(r) => {
                // Claimed by the thread itself, as its first action. Doing it in
                // the parent would race: the child can be scheduled — and can
                // exit — between `task::spawn` returning and the parent writing
                // the id, and then nothing would free its pages.
                r.thread = crate::task::current_id();
                Some((r.entry, r.stack_top))
            }
        }
    };

    match target {
        Some((entry, stack_top)) => unsafe { enter_ring3(entry, stack_top) },
        None => serial_println!("spawned thread has no program slot {}", slot),
    }
}

/// Record an image loaded by the running thread, so its pages are freed when it
/// exits. Used by the boot-time demos, which load in their own thread.
fn adopt_resident(name: &str, loaded: &crate::elf::LoadedImage, stack: crate::elf::MappedRange) {
    let (lo, hi) = loaded.extent();
    let mut table = RESIDENT.lock();

    let Some(slot) = table.iter().position(|s| s.is_none()) else {
        serial_println!("  (no resident slot for '{}'; its pages will not be freed)", name);
        return;
    };

    let mut name_buf = [0u8; 32];
    let bytes = name.as_bytes();
    let take = core::cmp::min(bytes.len(), name_buf.len() - 1);
    name_buf[..take].copy_from_slice(&bytes[..take]);

    table[slot] = Some(Resident {
        thread: crate::task::current_id(),
        name: name_buf,
        lo,
        hi,
        entry: loaded.entry,
        stack_top: stack.start + (stack.pages * PAGE_SIZE) as u64,
        ranges: loaded.ranges,
        stack,
    });
}

fn release_slot(slot: usize) {
    let entry = RESIDENT.lock().get_mut(slot).and_then(|s| s.take());
    if let Some(r) = entry {
        unmap_resident(&r);
    }
}

fn unmap_resident(r: &Resident) {
    let mut freed = 0;
    for range in r.ranges.iter().flatten() {
        freed += unsafe { paging::unmap_user_pages(range.start, range.pages) };
    }
    if r.stack.pages > 0 {
        freed += unsafe { paging::unmap_user_pages(r.stack.start, r.stack.pages) };
    }

    // The used-page count is printed on purpose: it is the only way to see, from
    // the log alone, that running a program twice does not cost twice the memory.
    // Two `hello` runs should report the same number here.
    let used = crate::memory::physical::memory_stats()
        .map(|s| s.used_pages)
        .unwrap_or(0);

    serial_println!(
        "  released '{}': {} page(s) returned, {} used system-wide",
        r.name(),
        freed,
        used
    );
}

/// Free the address space a finished thread was using.
///
/// Called from `task::exit_current`, while the exiting thread is still current
/// and still on its *kernel* stack — so unmapping its user pages cannot pull the
/// ground out from under it. A thread with no program registered is a plain
/// kernel thread and this does nothing.
pub fn on_thread_exit(thread: usize) {
    let entry = {
        let mut table = RESIDENT.lock();
        let found = table.iter().position(|s| matches!(s, Some(r) if r.thread == thread));
        found.and_then(|i| table[i].take())
    };

    if let Some(r) = entry {
        unmap_resident(&r);
    }
}

/// Names of resident programs, for diagnostics.
pub fn resident_count() -> usize {
    RESIDENT.lock().iter().flatten().count()
}

/// Top of the ping-pong payload's stacks. Each thread gets its own 1 MiB below
/// this, so neither can blame the other for a corrupted stack.
const PINGPONG_STACK_TOP: u64 = 0x0000_0000_5800_0000;

/// Ring-3 half of the per-thread-kernel-stack test. Runs as a kernel thread;
/// `which` is 0 or 1 and picks both the tag byte and the stack.
///
/// Two of these run concurrently. Each iteration calls `SYS_YIELD`, so the
/// thread is sitting *inside* a system call — with a `SyscallFrame` live on its
/// kernel stack — at the moment the scheduler hands the CPU to the other one.
/// Under the old single static syscall stack, the second thread's entry stub
/// would reset RSP to the same top and overwrite the first thread's frame; the
/// first would then `sysretq` to whatever landed where its return RIP had been.
pub fn run_pingpong(which: usize) {
    use core::sync::atomic::Ordering;

    let blob_start = sym(unsafe { &__user_start });
    let blob_end = sym(unsafe { &__user_end });
    let blob_pages = ((blob_end - blob_start) as usize).div_ceil(PAGE_SIZE).max(1);
    let entry = USER_CODE_BASE + (kosh_user_pingpong_entry as usize as u64 - blob_start);

    let code_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if !CODE_MAPPED.swap(true, Ordering::SeqCst) {
        if let Err(e) = paging::map_user_range(USER_CODE_BASE, blob_start, blob_pages, code_flags) {
            serial_println!("  failed to map user code: {}", e);
            return;
        }
    }

    let stack_top = PINGPONG_STACK_TOP - (which as u64 * 0x10_0000);
    let stack_bottom = stack_top - (USER_STACK_PAGES * PAGE_SIZE) as u64;
    let stack_flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;
    if let Err(e) = paging::map_user_pages(stack_bottom, USER_STACK_PAGES, stack_flags) {
        serial_println!("  failed to map ping-pong stack: {}", e);
        return;
    }

    let tag = if which == 0 { b'A' } else { b'B' };
    serial_println!(
        "  ring 3 '{}': entry 0x{:x}, user stack 0x{:x}, kernel stack 0x{:x}",
        tag as char,
        entry,
        stack_top,
        crate::task::current_kernel_stack_top()
    );

    unsafe { enter_ring3_with_arg(entry, stack_top, tag as u64) }
}

/// Load boot module 0 as an ELF and run it in ring 3.
///
/// This is the real path: a program the kernel was not compiled with, parsed
/// out of an ELF, mapped at the addresses it was linked for, and entered.
pub fn run_boot_module(_arg: usize) {
    run_module("hello", ELF_USER_STACK_TOP)
}

/// Load the userspace shell and hand it the console.
pub fn run_shell(_arg: usize) {
    run_module("ksh", SHELL_USER_STACK_TOP)
}

/// Stack for the shell. Distinct from the loader demo's so a fault dump makes it
/// obvious which program was running.
const SHELL_USER_STACK_TOP: u64 = 0x0000_0000_7000_0000;

fn run_module(name: &str, stack_top: u64) {
    let Some(module) = boot_module_named(name) else {
        serial_println!("No boot module named '{}'", name);
        serial_println!("  (add `module2 /boot/{} {}` to grub.cfg)", name, name);
        return;
    };

    serial_println!("Loading boot module '{}' as ELF:", name);
    let image = unsafe { module.bytes() };
    crate::elf::describe(image);

    let loaded = match unsafe { crate::elf::load(image) } {
        Ok(l) => l,
        Err(e) => {
            serial_println!("  ELF load FAILED: {:?}", e);
            return;
        }
    };

    serial_println!(
        "  loaded {} segment(s), {} bytes, entry 0x{:x}",
        loaded.segments,
        loaded.bytes_mapped,
        loaded.entry
    );

    // A fresh stack, well clear of the image's own segments.
    let stack_bottom = stack_top - (SHELL_STACK_PAGES * PAGE_SIZE) as u64;
    let stack_flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;

    if let Err(e) = paging::map_user_pages(stack_bottom, SHELL_STACK_PAGES, stack_flags) {
        serial_println!("  failed to map stack for loaded image: {}", e);
        return;
    }
    serial_println!(
        "  stack          : 0x{:x}..0x{:x} (RW-, user, NX)",
        stack_bottom,
        stack_top
    );

    // Register before entering ring 3, so `task::exit_current` frees these pages
    // when the program exits. Without this the boot-time load of `hello` stayed
    // mapped forever, and `ksh` spawning `hello` later failed with
    // `PageAlreadyMapped` — leaking a frame per attempt.
    adopt_resident(
        name,
        &loaded,
        crate::elf::MappedRange {
            start: stack_bottom,
            pages: SHELL_STACK_PAGES,
        },
    );

    serial_println!("Entering loaded ELF in ring 3...");
    serial_println!("--- loaded program output ---");

    unsafe { enter_ring3(loaded.entry, stack_top) }
}

/// Stack for the ELF-loaded program. Separate from the built-in payload's, so
/// the two demos cannot interfere.
const ELF_USER_STACK_TOP: u64 = 0x0000_0000_6000_0000;

/// Loaded programs get more stack than the hand-written payload: the shell has a
/// recursive-descent parser and formats strings on the stack.
const SHELL_STACK_PAGES: usize = 16;

/// Drop to ring 3 at `entry` with `stack_top`.
///
/// `stack_top` is passed through as RSP unchanged, so the contract with
/// userspace is the System V one: **RSP is 16-byte aligned at process entry**.
/// It is the program's `_start` that must then establish the call-boundary
/// alignment its compiled code expects — which is exactly what a real crt0
/// does, and what `userspace/hello` does. Getting this wrong shows up as a #GP
/// on the first `movaps` spill, not as anything obviously stack-related.
///
/// # Safety
/// `entry` and `stack_top` must be mapped `USER_ACCESSIBLE`, and the GDT must
/// carry ring-3 code and data descriptors.
unsafe fn enter_ring3(entry: u64, stack_top: u64) -> ! {
    enter_ring3_with_arg(entry, stack_top, 0)
}

/// As [`enter_ring3`], but with `arg` in RDI at entry.
///
/// `iretq` only replaces SS, RSP, RFLAGS, CS and RIP, so every other register
/// crosses the ring boundary untouched — which makes RDI a perfectly good place
/// to hand a starting value to a payload. That is how `argc`/`argv` will
/// eventually arrive; for now it is how the ping-pong payload learns which of the
/// two threads it is.
///
/// # Safety
/// As [`enter_ring3`].
unsafe fn enter_ring3_with_arg(entry: u64, stack_top: u64, arg: u64) -> ! {
    let sel = crate::gdt::selectors();
    let user_cs = (sel.user_code.0 | 3) as u64;
    let user_ss = (sel.user_data.0 | 3) as u64;

    // Reserved bit 1 is always set; IF on so the timer can still preempt the
    // user program. A ring-3 thread that cannot be preempted is a ring-3 thread
    // that can hang the machine with `for {}`.
    let rflags: u64 = 0x202;

    core::arch::asm!(
        "push {ss}",
        "push {rsp}",
        "push {rflags}",
        "push {cs}",
        "push {rip}",
        "iretq",
        ss = in(reg) user_ss,
        rsp = in(reg) stack_top,
        rflags = in(reg) rflags,
        cs = in(reg) user_cs,
        rip = in(reg) entry,
        in("rdi") arg,
        options(noreturn)
    )
}
