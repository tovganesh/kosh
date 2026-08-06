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

use crate::memory::address_space::AddressSpace;
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

const MAX_BOOT_MODULES: usize = 8;
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

/// Where the user blob is mapped. 1 GiB into the lower half.
///
/// It used to have to dodge the kernel's low identity window; there isn't one
/// any more, so the whole lower half belongs to userspace and this is just a
/// round number that keeps the payload clear of the loaded ELFs at 4 and 8 MiB.
pub const USER_CODE_BASE: u64 = 0x0000_0000_4000_0000;

/// Top of the built-in payload's stack, growing down.
const BLOB_STACK_TOP: u64 = 0x0000_0000_5000_0000;

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

fn sym(s: &u8) -> u64 {
    s as *const u8 as u64
}

/// Physical frame the `.user` section starts at.
///
/// The section is part of the kernel image, so its link-time address is
/// higher-half; `map_user_range` needs the frame GRUB actually loaded it into.
/// Passing the virtual address here used to work because they were the same
/// number — now it would hand `PhysAddr::new` a value with bits above 52 set,
/// which panics rather than silently mapping the wrong thing.
fn user_blob_phys() -> u64 {
    crate::memory::paging::kernel_phys(sym(unsafe { &__user_start }))
}

/// Map the user blob and its stack, then drop to ring 3.
///
/// Runs as a kernel thread, so `sys_exit` can retire it through
/// `task::exit_current()` and hand control back to the rest of the kernel —
/// no unwinding required.
pub fn run_user_demo(which: usize) {
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

    // Its own address space, like every other ring-3 program. This demo used to
    // map the payload into the *kernel's* PML4[0] and leave it there, which
    // quietly contradicted the previous phase's claim that PML4[0] belongs to
    // userspace — and meant the two demos shared a lower half and had to be
    // given stacks a megabyte apart.
    let space = match AddressSpace::new_user() {
        Ok(s) => s,
        Err(e) => {
            serial_println!("  no address space for the payload: {}", e);
            return;
        }
    };

    // Read + execute for the user, and NOT writable — W^X applies to userspace
    // too. These frames are borrowed from the kernel image, so they are mapped
    // with `map_user_range_in`, which does not tag them as owned; freeing this
    // space must not hand the kernel's own `.user` section to the allocator.
    let code_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    serial_println!(
        "  .user section  : 0x{:x}..0x{:x} ({} page(s), phys 0x{:x})",
        blob_start,
        blob_end,
        blob_pages,
        user_blob_phys()
    );
    if let Err(e) = paging::map_user_range_in(
        unsafe { paging::mapper_for(space.pml4_phys()) },
        USER_CODE_BASE,
        user_blob_phys(),
        blob_pages,
        code_flags,
    ) {
        serial_println!("  failed to map user code: {}", e);
        unsafe { space.free() };
        return;
    }
    serial_println!("  code mapped at : 0x{:x} (R-X, user)", USER_CODE_BASE);
    serial_println!("  entry          : 0x{:x}", user_entry);

    // Both demos use the same stack address now; they are in different spaces.
    let stack_top = BLOB_STACK_TOP;
    let stack_bottom = stack_top - (USER_STACK_PAGES * PAGE_SIZE) as u64;
    let stack_flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;
    if let Err(e) = paging::map_user_pages_in(
        unsafe { paging::mapper_for(space.pml4_phys()) },
        stack_bottom,
        USER_STACK_PAGES,
        stack_flags,
    ) {
        serial_println!("  failed to map user stack: {}", e);
        unsafe { space.free() };
        return;
    }
    serial_println!(
        "  stack          : 0x{:x}..0x{:x} (RW-, user, NX), PML4 0x{:x}",
        stack_bottom,
        stack_top,
        space.pml4_phys()
    );

    crate::task::adopt_address_space(space);

    serial_println!("Entering ring 3 via iretq...");
    serial_println!("--- ring 3 output ---");

    unsafe { enter_ring3(user_entry, stack_top) }
}

// ---------------------------------------------------------------------------
// Programs
// ---------------------------------------------------------------------------

/// How many programs can be pending or running at once. Bounded by the thread
/// table.
const MAX_PROGRAMS: usize = 8;

/// Marker for a reservation whose thread has not claimed it yet.
const NO_THREAD: usize = usize::MAX;

/// Top of a user program's stack.
///
/// One constant, used by every program, because every program now has its own
/// address space. Phase 10 had to hand out a distinct 1 MiB slot per spawn and
/// keep the boot demos clear of the shell — all of that was bookkeeping around
/// a shared lower half.
pub const USER_STACK_TOP: u64 = 0x0000_0000_3000_0000;

/// A program that has been loaded and is waiting for its thread, or is running.
///
/// The predecessor of this was a table of *address ranges*, used to refuse a
/// `spawn` whose image would land on top of a resident program's, and to unmap
/// that program's pages when it exited. Both jobs are gone: a program that has
/// its own PML4 cannot collide with anything, and freeing its address space
/// frees its pages. What is left is the hand-off from the spawning thread to the
/// spawned one, plus a name for the log.
struct Program {
    /// Thread that owns this program, or [`NO_THREAD`] until it claims it.
    thread: usize,
    name: [u8; 32],
    entry: u64,
    stack_top: u64,
    /// Moved into the thread by [`enter_spawned`]. `None` afterwards.
    space: Option<AddressSpace>,
}

impl Program {
    fn name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

static PROGRAMS: Mutex<[Option<Program>; MAX_PROGRAMS]> =
    Mutex::new([const { None }; MAX_PROGRAMS]);

/// Why a `spawn` could not happen.
#[derive(Debug)]
pub enum SpawnError {
    /// No boot module by that name. This is what a shell turns into
    /// "command not found", so it must be distinguishable from the rest.
    NotFound,
    TooManyPrograms,
    Load(crate::elf::ElfError),
    Map(&'static str),
    Space(&'static str),
    NoThread(&'static str),
}

fn name_bytes(name: &str) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let bytes = name.as_bytes();
    let take = core::cmp::min(bytes.len(), buf.len() - 1);
    buf[..take].copy_from_slice(&bytes[..take]);
    buf
}

/// Build a fresh address space with `image` loaded into it and a stack mapped.
///
/// The caller does not have to be running in the resulting space — the ELF
/// loader writes through the physmap, which every address space shares. That is
/// what lets `spawn_program` do this work in the *parent*, so a load failure is
/// a synchronous error the shell can report rather than a thread that starts and
/// immediately dies with the reason only in the kernel log.
fn prepare_program(image: &[u8]) -> Result<(AddressSpace, u64), SpawnError> {
    let space = AddressSpace::new_user().map_err(SpawnError::Space)?;

    let loaded = match unsafe { crate::elf::load_into(space.pml4_phys(), image) } {
        Ok(l) => l,
        Err(e) => {
            unsafe { space.free() };
            return Err(SpawnError::Load(e));
        }
    };

    let stack_bottom = USER_STACK_TOP - (USER_STACK_PAGES_MAX * PAGE_SIZE) as u64;
    let stack_flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;

    // Reserved, not allocated. A program that uses one page of stack should not
    // pay for sixteen — and the sixteen exist precisely because `ksh`'s parser
    // occasionally needs them, not because anything uses them on the way to the
    // first prompt.
    let mapped = paging::reserve_user_pages_in(
        space.pml4_phys(),
        stack_bottom,
        USER_STACK_PAGES_MAX,
        stack_flags,
    );

    if let Err(e) = mapped {
        unsafe { space.free() };
        return Err(SpawnError::Map(e));
    }
    paging::note_reserved(USER_STACK_PAGES_MAX);

    serial_println!(
        "  loaded {} segment(s), {} bytes, entry 0x{:x}, stack 0x{:x}..0x{:x}",
        loaded.segments,
        loaded.bytes_mapped,
        loaded.entry,
        stack_bottom,
        USER_STACK_TOP
    );

    Ok((space, loaded.entry))
}

/// Stack pages every program gets. Generous because `ksh` has a
/// recursive-descent parser and formats strings on the stack.
const USER_STACK_PAGES_MAX: usize = 16;

/// Load a boot module and run it in ring 3 on a new thread with its own address
/// space. Returns the thread id, which is what `wait` takes.
pub fn spawn_program(name: &str) -> Result<usize, SpawnError> {
    let module = boot_module_named(name).ok_or(SpawnError::NotFound)?;
    let image = unsafe { module.bytes() };

    serial_println!("Loading '{}' into a new address space:", name);
    let (space, entry) = prepare_program(image)?;

    let slot = {
        let mut table = PROGRAMS.lock();
        let Some(slot) = table.iter().position(|s| s.is_none()) else {
            unsafe { space.free() };
            return Err(SpawnError::TooManyPrograms);
        };

        table[slot] = Some(Program {
            thread: NO_THREAD,
            name: name_bytes(name),
            entry,
            stack_top: USER_STACK_TOP,
            space: Some(space),
        });
        slot
    };

    match crate::task::spawn("spawned", enter_spawned, slot) {
        Ok(thread) => {
            register_process(thread, Some(crate::task::current_id()), name);
            serial_println!("spawn '{}': thread {} (pid {}), entry 0x{:x}", name, thread, thread, entry);
            Ok(thread)
        }
        Err(e) => {
            release_slot(slot);
            Err(SpawnError::NoThread(e))
        }
    }
}

/// Build an address space with the named boot module loaded, ready to enter.
///
/// The `exec` half of the loader path: same work `spawn_program` does, without
/// creating a thread, because `exec` reuses the caller's.
pub fn prepare_named_program(
    name: &str,
) -> Result<(AddressSpace, u64), crate::syscall::SyscallError> {
    use crate::syscall::SyscallError;

    let module = boot_module_named(name).ok_or(SyscallError::NotFound)?;
    let image = unsafe { module.bytes() };

    prepare_program(image).map_err(|e| match e {
        SpawnError::NotFound => SyscallError::NotFound,
        SpawnError::Space(_) | SpawnError::Map(_) => SyscallError::OutOfMemory,
        SpawnError::Load(_) => SyscallError::InvalidArgument,
        _ => SyscallError::ResourceExhausted,
    })
}

/// Register a ring-3 thread as a process, and give it what IPC needs.
///
/// A process here *is* a thread with an address space — there is no separate
/// abstraction, and pretending otherwise is what left the process table holding
/// three synthetic rows while every real program ran outside it. So the pid is
/// the thread id, and this is the one place a process comes into existence.
///
/// Three things are set up together, because a process missing any of them
/// fails somewhere unhelpful:
///
/// * the process table entry, without which `send_message` reports
///   `SenderNotFound`;
/// * a message queue, without which a `receive` before the first `send` reports
///   `ReceiverNotFound` rather than "no message";
/// * a capability to exchange messages with its parent, and the parent's to
///   reply — which is the whole of the security policy, and is deliberately
///   narrow. A process can talk to its parent and its children. Anything else
///   is `PermissionDenied`, and that is testable rather than assumed.
pub fn register_process(thread: usize, parent: Option<usize>, name: &str) {
    use crate::process::{ProcessId, ProcessPriority};

    let pid = ProcessId::new(thread as u32);
    let parent_pid = parent.map(|p| ProcessId::new(p as u32));

    if let Err(e) = crate::process::create_process_with_pid(
        pid,
        parent_pid,
        alloc::string::String::from(name),
        ProcessPriority::Normal,
    ) {
        serial_println!("  could not register process {}: {:?}", thread, e);
        return;
    }

    if let Err(e) = crate::ipc::queue::create_message_queue(pid) {
        serial_println!("  could not create a message queue for {}: {:?}", thread, e);
    }

    // A channel with the parent, in both directions. `create_secure_ipc_channel`
    // grants each side `SendMessage` scoped to the other specifically — not
    // `ResourceId::Any`, which is what `grant_system_process_capabilities` hands
    // out and which would make the capability check unable to refuse anything.
    if let Some(parent_pid) = parent_pid {
        if let Err(e) = crate::ipc::security::create_secure_ipc_channel(pid, parent_pid) {
            serial_println!("  could not open an IPC channel {}<->{}: {:?}", thread, parent_pid.0, e);
        }
    }

    grant_driver_capabilities(pid, name);
}

/// Boot modules the kernel is willing to treat as device drivers, and what each
/// may drive.
///
/// This is the weakest link in the chain and it is worth being plain about it.
/// A capability system wants a *delegation* story: `init` holds the hardware,
/// grants each driver exactly its device, and the kernel makes no policy
/// decisions about which program is trusted. What is here instead is the kernel
/// recognising a boot module by the name on its `module2` line — which is to say
/// the trust root is "GRUB loaded it from the ISO", the same root that already
/// decides what code the kernel will run at all.
///
/// That is defensible for a boot-time driver and useless for anything loaded
/// later, which is exactly why it is a table with two entries rather than a rule.
/// `userspace/init` taking this over is what makes it a real chain.
const DRIVER_IMAGES: &[(&str, &str)] = &[("ata-driver", "ata0")];

fn grant_driver_capabilities(pid: crate::process::ProcessId, name: &str) {
    use crate::ipc::capability::{create_capability, CapabilityType, ResourceId};

    for (image, device) in DRIVER_IMAGES {
        if *image != name {
            continue;
        }
        match create_capability(
            pid,
            CapabilityType::DeviceAccess,
            ResourceId::Device(alloc::string::String::from(*device)),
            None,
        ) {
            Ok(_) => serial_println!("  granted {} DeviceAccess for '{}'", pid.0, device),
            Err(e) => serial_println!("  could not grant DeviceAccess for '{}': {:?}", device, e),
        }
    }
}

/// Retire a process when its thread exits.
fn deregister_process(thread: usize) {
    use crate::process::ProcessId;
    let pid = ProcessId::new(thread as u32);

    // Hardware first. A driver that faults still holds its disk otherwise, and
    // nothing — not the kernel's own block layer, not a replacement driver —
    // could touch it again until reboot. Surviving a driver crash is most of the
    // argument for running drivers in ring 3 at all.
    crate::platform::devports::release_all(thread);
    crate::ipc::services::unregister_pid(pid);

    let _ = crate::ipc::queue::remove_message_queue(pid);
    let _ = crate::process::remove_process(pid);
}

/// Record a forked child in the program table, inheriting its parent's name.
///
/// Not a new program — a second thread running the same one — so it gets the
/// parent's name with a marker, which is what makes a `fork` visible in the log
/// without pretending a new image was loaded.
pub fn register_forked(child: usize, parent: usize) {
    register_process(child, Some(parent), "forked");

    let mut table = PROGRAMS.lock();

    let parent_name = table
        .iter()
        .flatten()
        .find(|p| p.thread == parent)
        .map(|p| p.name)
        .unwrap_or_else(|| name_bytes("forked"));

    let Some(slot) = table.iter().position(|s| s.is_none()) else {
        serial_println!("  (no program slot for the fork of thread {})", parent);
        return;
    };

    table[slot] = Some(Program {
        thread: child,
        name: parent_name,
        entry: 0,
        stack_top: 0,
        space: None,
    });
}

/// Point a thread's program entry at a different name, after `exec`.
pub fn rename_program(thread: usize, name: &str) {
    let mut table = PROGRAMS.lock();
    if let Some(p) = table.iter_mut().flatten().find(|p| p.thread == thread) {
        p.name = name_bytes(name);
    } else if let Some(slot) = table.iter().position(|s| s.is_none()) {
        table[slot] = Some(Program {
            thread,
            name: name_bytes(name),
            entry: 0,
            stack_top: 0,
            space: None,
        });
    }
}

/// Entry point of a spawned thread: take the address space, switch to it, and
/// drop to ring 3.
fn enter_spawned(slot: usize) {
    let target = {
        let mut table = PROGRAMS.lock();
        match table.get_mut(slot).and_then(|s| s.as_mut()) {
            None => None,
            Some(p) => {
                // Claimed by the thread itself, as its first action. Doing it in
                // the parent would race: the child can be scheduled — and can
                // exit — between `task::spawn` returning and the parent writing
                // the id, and then nothing would free its address space.
                p.thread = crate::task::current_id();
                p.space.take().map(|space| (space, p.entry, p.stack_top))
            }
        }
    };

    match target {
        Some((space, entry, stack_top)) => {
            crate::task::adopt_address_space(space);
            unsafe { enter_ring3(entry, stack_top) }
        }
        None => serial_println!("spawned thread has no program in slot {}", slot),
    }
}

/// Register a program the running thread loaded for itself.
fn register_running(name: &str) {
    let mut table = PROGRAMS.lock();
    let Some(slot) = table.iter().position(|s| s.is_none()) else {
        serial_println!("  (no program slot for '{}'; it will not appear in the count)", name);
        return;
    };
    table[slot] = Some(Program {
        thread: crate::task::current_id(),
        name: name_bytes(name),
        entry: 0,
        stack_top: 0,
        space: None,
    });
}

fn release_slot(slot: usize) {
    let taken = PROGRAMS.lock().get_mut(slot).and_then(|s| s.take());
    if let Some(p) = taken {
        if let Some(space) = p.space {
            unsafe { space.free() };
        }
    }
}

/// Forget a finished thread's program.
///
/// The address space itself is freed by `task::exit_current`, which owns it —
/// this only clears the registry. Before per-process address spaces this
/// function did the unmapping too, walking a recorded list of ranges; that list
/// existed because there was nothing else that knew what a program had mapped.
pub fn on_thread_exit(thread: usize) {
    deregister_process(thread);

    let taken = {
        let mut table = PROGRAMS.lock();
        let found = table.iter().position(|s| matches!(s, Some(p) if p.thread == thread));
        found.and_then(|i| table[i].take())
    };

    if let Some(mut p) = taken {
        // A program whose thread never claimed its space — `spawn` failed after
        // loading — still owns one.
        if let Some(space) = p.space.take() {
            unsafe { space.free() };
        }

        // The used-page count is printed on purpose: it is the only way to see,
        // from the log alone, that running a program twice does not cost twice
        // the memory. Two `hello` runs report the same number.
        let used = crate::memory::physical::memory_stats()
            .map(|s| s.used_pages)
            .unwrap_or(0);
        serial_println!("  released '{}': {} used system-wide", p.name(), used);
    }
}

/// Programs currently loaded.
pub fn resident_count() -> usize {
    PROGRAMS.lock().iter().flatten().count()
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

    let space = match AddressSpace::new_user() {
        Ok(s) => s,
        Err(e) => {
            serial_println!("  no address space for the ping-pong payload: {}", e);
            return;
        }
    };

    // The payload lives in the kernel image, so these frames are *borrowed*:
    // `map_user_range_in` does not tag them, and tearing the space down leaves
    // them alone. Both threads map the same frames into their own tables — the
    // same read-only text at the same address in two address spaces, which is
    // what shared libraries look like from the kernel's side.
    let code_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if let Err(e) = paging::map_user_range_in(
        unsafe { paging::mapper_for(space.pml4_phys()) },
        USER_CODE_BASE,
        user_blob_phys(),
        blob_pages,
        code_flags,
    ) {
        serial_println!("  failed to map user code: {}", e);
        unsafe { space.free() };
        return;
    }

    // Both threads use the *same* stack address now. Under one address space
    // they had to be given a megabyte apart.
    let stack_top = PINGPONG_STACK_TOP;
    let stack_bottom = stack_top - (USER_STACK_PAGES * PAGE_SIZE) as u64;
    let stack_flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;
    if let Err(e) = paging::map_user_pages_in(
        unsafe { paging::mapper_for(space.pml4_phys()) },
        stack_bottom,
        USER_STACK_PAGES,
        stack_flags,
    ) {
        serial_println!("  failed to map ping-pong stack: {}", e);
        unsafe { space.free() };
        return;
    }

    let tag = if which == 0 { b'A' } else { b'B' };
    serial_println!(
        "  ring 3 '{}': entry 0x{:x}, user stack 0x{:x}, PML4 0x{:x}, kernel stack 0x{:x}",
        tag as char,
        entry,
        stack_top,
        space.pml4_phys(),
        crate::task::current_kernel_stack_top()
    );

    crate::task::adopt_address_space(space);

    unsafe { enter_ring3_with_arg(entry, stack_top, tag as u64) }
}

/// Load boot module 0 as an ELF and run it in ring 3.
///
/// This is the real path: a program the kernel was not compiled with, parsed
/// out of an ELF, mapped at the addresses it was linked for, and entered.
pub fn run_boot_module(_arg: usize) {
    run_module("hello")
}

/// Load process 1.
///
/// The only program the kernel starts. Everything else — the block driver, the
/// filesystem, the shell — is `init`'s to spawn, which is what makes the process
/// tree a tree rather than a list of things the kernel happened to know about.
pub fn run_init(_arg: usize) {
    run_module("init")
}

/// Load a boot module into a fresh address space and enter it.
///
/// Runs *in* the thread that will execute the program, so it adopts the address
/// space rather than handing it over. There is no `stack_top` parameter any
/// more: each program has the lower half to itself, so they all use the same
/// one, and the three distinct constants that used to keep the boot demos, the
/// ELF demo and the shell from colliding are gone.
fn run_module(name: &str) {
    let Some(module) = boot_module_named(name) else {
        serial_println!("No boot module named '{}'", name);
        serial_println!("  (add `module2 /boot/{} {}` to grub.cfg)", name, name);
        return;
    };

    serial_println!("Loading boot module '{}' as ELF:", name);
    let image = unsafe { module.bytes() };
    crate::elf::describe(image);

    let (space, entry) = match prepare_program(image) {
        Ok(v) => v,
        Err(e) => {
            serial_println!("  load FAILED: {:?}", e);
            return;
        }
    };

    if !crate::memory::address_space::check_kernel_half(&space) {
        serial_println!("  FATAL: new address space does not share the kernel's upper half");
        unsafe { space.free() };
        return;
    }

    register_running(name);
    // A program the kernel started for itself has no parent process, so no IPC
    // channel: nothing has a capability to message it and it has none to
    // message anything. `ksh` gets one the moment it spawns a child.
    register_process(crate::task::current_id(), None, name);
    crate::task::adopt_address_space(space);

    serial_println!("Entering loaded ELF in ring 3...");
    serial_println!("--- loaded program output ---");

    unsafe { enter_ring3(entry, USER_STACK_TOP) }
}

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

/// [`enter_ring3`], for `exec`, which lives in another module.
///
/// # Safety
/// As [`enter_ring3`]: the address space holding `entry` and `stack_top` must
/// already be in CR3.
pub unsafe fn enter_ring3_at(entry: u64, stack_top: u64) -> ! {
    enter_ring3(entry, stack_top)
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
