//! The `syscall`/`sysret` fast-path entry point.
//!
//! Before Phase 5 there was no way into the kernel from user mode at all.
//! `syscall/mod.rs` had `setup_syscall_interrupt()` — a `serial_println!`, a
//! `// TODO: Set up IDT entry for interrupt 0x80`, and `Ok(())` — and
//! `syscall_entry()`, documented as "called from assembly interrupt handler",
//! with no assembly interrupt handler anywhere in the tree. The dispatcher
//! below it was real and well-validated, and completely unreachable.
//!
//! ## What the instruction does and does not do
//!
//! `syscall` is fast because it does almost nothing: it loads CS/SS from
//! `STAR`, RIP from `LSTAR`, masks RFLAGS with `SFMASK`, stashes the caller's
//! RIP in RCX and RFLAGS in R11 — and *leaves RSP pointing at the user stack*.
//! Everything else is the kernel's job. In particular the stack switch is
//! manual, which is the first thing the stub below does.
//!
//! ## Register ABI
//!
//! Linux's convention, which the existing userspace code already assumes:
//!
//! | register | meaning |
//! |---|---|
//! | RAX | syscall number, and the return value |
//! | RDI RSI RDX R10 R8 R9 | arguments 1-6 |
//! | RCX R11 | clobbered by the instruction itself |
//!
//! Argument 4 lives in R10 rather than RCX precisely because `syscall`
//! destroys RCX.

use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

use crate::process::ProcessId;
use crate::serial_println;

/// Kernel stack used while servicing a syscall.
///
/// One static stack is correct only because exactly one thread can be in ring 3
/// in Phase 5, and `SFMASK` masks IF so a syscall cannot be interrupted into a
/// second syscall. Multiple user threads need this to become per-thread, reached
/// through `swapgs` and a per-CPU block — see the note on `SAVED_USER_RSP`.
const SYSCALL_STACK_SIZE: usize = 32 * 1024;

#[repr(align(16))]
struct SyscallStack([u8; SYSCALL_STACK_SIZE]);

static mut SYSCALL_STACK: SyscallStack = SyscallStack([0; SYSCALL_STACK_SIZE]);

/// Top of the syscall stack, read by the assembly stub.
#[no_mangle]
static mut SYSCALL_KERNEL_RSP: u64 = 0;

/// Where the stub parks the user's RSP for the duration of the call.
///
/// A plain static rather than `swapgs` + per-CPU data: single CPU, single
/// ring-3 thread, and interrupts masked during the call, so nothing can
/// re-enter. The moment any of those three stops being true this must become
/// `swapgs`-based, or two concurrent syscalls will trample each other's stacks.
#[no_mangle]
static mut SAVED_USER_RSP: u64 = 0;

/// Registers the stub pushes, in push order (so the first field is at the
/// lowest address — the layout the stub builds by pushing RAX last).
#[repr(C)]
pub struct SyscallFrame {
    pub rax: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub r10: u64,
    pub r8: u64,
    pub r9: u64,
    /// User RIP, delivered by the CPU in RCX. `sysretq` needs it back there.
    pub rip: u64,
    /// User RFLAGS, delivered in R11. `sysretq` needs it back there.
    pub rflags: u64,
}

extern "C" {
    fn kosh_syscall_entry();
}

core::arch::global_asm!(
    r#"
.section .text.kosh_syscall, "ax"
.global kosh_syscall_entry
.type kosh_syscall_entry, @function

kosh_syscall_entry:
    /* On entry: RSP is still the *user* stack, RCX = user RIP,
       R11 = user RFLAGS, RAX = syscall number. Interrupts are already masked
       by SFMASK. */

    movq    %rsp, SAVED_USER_RSP(%rip)
    movq    SYSCALL_KERNEL_RSP(%rip), %rsp

    /* Build SyscallFrame. Pushed high field first so RAX lands lowest. */
    pushq   %r11                /* rflags */
    pushq   %rcx                /* rip    */
    pushq   %r9
    pushq   %r8
    pushq   %r10
    pushq   %rdx
    pushq   %rsi
    pushq   %rdi
    pushq   %rax

    movq    %rsp, %rdi          /* &mut SyscallFrame */

    /* SysV wants RSP 16-byte aligned at the call. Nine pushes from a
       16-aligned top leaves it 8 past, so nudge it. */
    subq    $8, %rsp
    call    kosh_syscall_handler
    addq    $8, %rsp

    movq    %rax, (%rsp)        /* return value into frame.rax */

    popq    %rax
    popq    %rdi
    popq    %rsi
    popq    %rdx
    popq    %r10
    popq    %r8
    popq    %r9
    popq    %rcx                /* sysretq takes the target RIP here    */
    popq    %r11                /* ...and the target RFLAGS here        */

    movq    SAVED_USER_RSP(%rip), %rsp
    sysretq
"#,
    options(att_syntax)
);

/// Rust side of the syscall entry. Called by the stub with the saved frame.
#[no_mangle]
pub extern "C" fn kosh_syscall_handler(frame: &mut SyscallFrame) -> u64 {
    let number = frame.rax;
    let args = [frame.rdi, frame.rsi, frame.rdx, frame.r10, frame.r8, frame.r9];

    SYSCALL_COUNT.fetch_add(1, Ordering::Relaxed);

    // Phase 5 runs a single ring-3 thread; a real current-process lookup lands
    // with the process table in Phase 6.
    let pid = ProcessId::new(1);

    match crate::syscall::dispatcher::dispatch_syscall(pid, number, args) {
        Ok(value) => value,
        Err(err) => {
            serial_println!("syscall {} failed: {:?}", number, err);

            // Linux convention: errors come back as a negative value in RAX,
            // successes as a non-negative one, and userspace tests the sign.
            //
            // Note `SyscallError::to_errno()` already returns *negative*
            // numbers (-22 for EINVAL, and so on), which is unusual for
            // something named "errno". Negating it here — the obvious thing to
            // write — flips errors positive, and userspace then reads every
            // failure as success. Normalise instead of assuming.
            let errno = err.to_errno() as i64;
            let negative = if errno > 0 { -errno } else { errno };
            negative as u64
        }
    }
}

static SYSCALL_COUNT: AtomicU64 = AtomicU64::new(0);

/// Syscalls serviced since boot.
pub fn syscall_count() -> u64 {
    SYSCALL_COUNT.load(Ordering::Relaxed)
}

/// Program the MSRs that make `syscall` work.
pub fn init() {
    let sel = crate::gdt::selectors();

    unsafe {
        SYSCALL_KERNEL_RSP = {
            let base = &raw const SYSCALL_STACK as u64;
            (base + SYSCALL_STACK_SIZE as u64) & !0xF
        };
    }

    unsafe {
        // Without SCE, `syscall` is an invalid opcode.
        Efer::update(|flags| flags.insert(EferFlags::SYSTEM_CALL_EXTENSIONS));

        // The crate checks the descriptor arithmetic described in `gdt.rs` and
        // refuses selectors that would make `sysret` land somewhere absurd.
        Star::write(sel.user_code, sel.user_data, sel.kernel_code, sel.kernel_data)
            .expect("GDT selector layout is incompatible with sysret");

        LStar::write(VirtAddr::new(kosh_syscall_entry as usize as u64));

        // Bits cleared in RFLAGS on entry. Masking IF means a syscall cannot be
        // interrupted — which is what makes the single static kernel stack
        // above safe. DF is masked so string instructions behave; TF so a
        // single-stepping debugger in userspace does not trap in the kernel.
        SFMask::write(
            RFlags::INTERRUPT_FLAG | RFlags::DIRECTION_FLAG | RFlags::TRAP_FLAG,
        );
    }

    serial_println!("Syscall interface (SYSCALL/SYSRET):");
    serial_println!("  EFER.SCE enabled");
    serial_println!(
        "  STAR: syscall CS 0x{:x}/SS 0x{:x}, sysret CS 0x{:x}/SS 0x{:x}",
        sel.kernel_code.0,
        sel.kernel_data.0,
        sel.user_code.0 | 3,
        sel.user_data.0 | 3
    );
    serial_println!(
        "  LSTAR -> 0x{:x}, kernel stack 0x{:x}",
        kosh_syscall_entry as usize as u64,
        unsafe { SYSCALL_KERNEL_RSP }
    );
    serial_println!("  SFMASK masks IF, DF, TF");
}
