//! Per-CPU state, reached through GS.
//!
//! ## Why this exists
//!
//! `syscall` gives the kernel no stack. It loads CS, SS and RIP from MSRs and
//! leaves RSP pointing at the *user* stack, so the first thing the entry stub
//! must do is find a kernel stack — without touching any register the caller
//! cares about, and with no stack to spill to.
//!
//! Until now the stub read a single global, `SYSCALL_KERNEL_RSP`. That worked
//! exactly as long as only one thread could ever be in ring 3: a second thread
//! entering a syscall while the first was parked inside one would take the same
//! stack from the top and overwrite its frame. Half a dozen comments in this
//! kernel said so, and `files::wait_for_key` — which enables interrupts inside a
//! syscall so a blocking `read` can be woken by the keyboard IRQ — is precisely
//! the code path that makes "parked inside one" reachable.
//!
//! The fix is two-part: the stack pointer the stub loads becomes per-thread
//! (here), and the stub parks the user's RSP on that stack instead of in a
//! global (`syscall/entry.rs`).
//!
//! ## Why GS.base, and why no `swapgs`
//!
//! The textbook mechanism is `swapgs`: user mode keeps its own GS.base, the
//! kernel's lives in `IA32_KERNEL_GS_BASE`, and each kernel entry and exit
//! exchanges them. That invariant only holds if *every* transition swaps, and
//! this kernel cannot honour that. Its interrupt handlers are Rust functions
//! using `x86-interrupt`, which gives no place to put a `swapgs` before the
//! compiler-generated prologue — so an interrupt taken from ring 3 enters the
//! kernel without swapping. As soon as a timer tick can land inside a syscall
//! (see `wait_for_key` above) the swap count stops being even, and a later
//! `swapgs` leaves GS.base holding user data. `movq %gs:0, %rsp` would then load
//! garbage as the kernel stack pointer — with no fault to point at the cause.
//!
//! So both MSRs point at the same block: GS.base for the accesses, and
//! `IA32_KERNEL_GS_BASE` so that a `swapgs` from anywhere — including one added
//! later — is a no-op rather than a bug. `gs:0` reads per-CPU data in every ring
//! and after any number of swaps.
//!
//! Ring 3 cannot subvert this. Writing GS.base needs `wrgsbase` (CR4.FSGSBASE is
//! clear) or `wrmsr` (ring 0), and the `swapgs` instruction itself is #GP in
//! ring 3. The real cost is that user-mode TLS via GS is off the table until
//! entry becomes swap-based, which needs naked interrupt stubs first.
//!
//! ## Field offsets are part of the ABI
//!
//! `syscall/entry.rs` addresses these fields as `%gs:0` and `%gs:8`. Reordering
//! them silently breaks the syscall path, so the layout is `repr(C)` and the
//! offsets are asserted at compile time.

use core::ptr::{addr_of, addr_of_mut};

use x86_64::registers::model_specific::{GsBase, KernelGsBase};
use x86_64::VirtAddr;

use crate::serial_println;

#[repr(C)]
pub struct PerCpu {
    /// Kernel stack top for the currently-running thread's system calls.
    /// Offset 0 — read by the syscall stub as `gs:0`.
    pub syscall_rsp: u64,
    /// Scratch slot where the stub parks the user's RSP for the two
    /// instructions it takes to get onto the kernel stack. Offset 8 — `gs:8`.
    ///
    /// Per-CPU rather than per-thread, which is only sound because the window it
    /// is live for runs with interrupts masked by `SFMASK`. The stub immediately
    /// pushes the value onto the kernel stack, where it *is* per-thread, and
    /// never reads this slot again.
    pub user_rsp_scratch: u64,
    /// Which thread the two fields above belong to. Diagnostics only — but it is
    /// what makes a mismatch visible. Offset 16.
    pub current_thread: u64,
}

impl PerCpu {
    const fn new() -> Self {
        Self {
            syscall_rsp: 0,
            user_rsp_scratch: 0,
            current_thread: 0,
        }
    }
}

/// The one and only CPU's block. Becomes an array indexed by APIC id if SMP
/// ever arrives — at which point `swapgs` and naked interrupt stubs become
/// mandatory, not optional.
static mut PER_CPU: PerCpu = PerCpu::new();

// The assembly addresses these by offset, so a reordering has to be a build
// error rather than a mystery at run time.
const _: () = {
    assert!(core::mem::offset_of!(PerCpu, syscall_rsp) == 0);
    assert!(core::mem::offset_of!(PerCpu, user_rsp_scratch) == 8);
    assert!(core::mem::offset_of!(PerCpu, current_thread) == 16);
};

/// Point GS.base — and `IA32_KERNEL_GS_BASE` — at the per-CPU block.
///
/// Must run before `syscall::init`, because the stub it installs dereferences
/// `gs:0` on the very first system call.
pub fn init() {
    let addr = unsafe { addr_of!(PER_CPU) as u64 };
    let va = VirtAddr::new(addr);

    unsafe {
        GsBase::write(va);
        KernelGsBase::write(va);
    }

    // Seeded, not left at zero. `task::init` overwrites this with thread 0's
    // stack — but it runs *after* `syscall::init`, and a `gs:0` of zero would
    // turn any syscall issued in between into a page fault at address 0 with no
    // frame to read. Same stack, published early.
    set_syscall_stack(crate::gdt::boot_kernel_stack_top().as_u64());

    serial_println!(
        "Per-CPU block at 0x{:x} (GS.base and IA32_KERNEL_GS_BASE, so swapgs is a no-op)",
        addr
    );
    serial_println!("  gs:0 = 0x{:x} (boot kernel stack)", syscall_stack());
}

/// Set the kernel stack the syscall stub will switch to.
///
/// Called on every context switch with the incoming thread's stack. That is the
/// whole point of the exercise: the value is per-thread, not per-kernel.
pub fn set_syscall_stack(top: u64) {
    unsafe {
        (*addr_of_mut!(PER_CPU)).syscall_rsp = top;
    }
}

pub fn set_current_thread(id: u64) {
    unsafe {
        (*addr_of_mut!(PER_CPU)).current_thread = id;
    }
}

pub fn syscall_stack() -> u64 {
    unsafe { (*addr_of!(PER_CPU)).syscall_rsp }
}

pub fn current_thread() -> u64 {
    unsafe { (*addr_of!(PER_CPU)).current_thread }
}

/// Read `gs:0` the way the syscall stub does, and compare it with the static.
///
/// A self-test rather than a getter: if the GS base were wrong, every syscall
/// would take a page fault on its first instruction with nothing but a stack
/// pointer of `0` to explain it. Cheaper to find out here.
pub fn check_gs_addressing() -> bool {
    let via_gs: u64;
    unsafe {
        core::arch::asm!("mov {}, gs:[0]", out(reg) via_gs, options(nostack, readonly));
    }
    via_gs == syscall_stack()
}
