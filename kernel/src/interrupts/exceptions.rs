//! CPU exception handlers.
//!
//! Every handler prints a legible dump — vector, mnemonic, error code, faulting
//! instruction pointer, stack pointer, flags and (for page faults) CR2 — and
//! then either kills the offending process or halts. Halting rather than
//! rebooting is deliberate: a frozen machine with a readable dump on the serial
//! port is debuggable, a reboot loop is not.
//!
//! ## Whose fault it is
//!
//! The page-fault handler has terminated ring-3 threads rather than halting
//! since Phase 5, on the grounds that a userspace bug taking down the machine is
//! the exact thing a microkernel exists to avoid. Every *other* handler halted,
//! which meant the argument only applied to the one exception a program was
//! expected to cause. A `ud2` in a user program, a division by zero, or — since
//! this phase — an `in` on a port the process was not granted, all stopped the
//! system.
//!
//! [`fault`] now makes that decision for every vector, from CS's RPL. Faults in
//! ring 0 still halt: a kernel that cannot trust its own state has nothing
//! useful to do next.
//!
//! ## Why the handlers do their work through [`kosh_call_aligned`]
//!
//! An exception that pushes an error code pushes six quadwords onto a stack the
//! CPU has already aligned to 16, so the handler starts with `RSP % 16 == 0`.
//! The System V ABI says a function starts with `RSP % 16 == 8` — the state a
//! `call` leaves, having pushed a return address. Those disagree by eight bytes,
//! and LLVM's `x86-interrupt` convention does not reconcile them: everything the
//! handler calls runs with its frames eight bytes off, and the first `movaps` to
//! a 16-aligned stack slot anywhere in that subtree raises #GP.
//!
//! This was true of every error-code handler since Phase 5 and never fired,
//! because the only one that did real work was the page-fault handler and the
//! only process it ever killed was a demo with no message queue to tear down.
//! Killing a *registered* process reaches `BTreeMap::remove`, which spills SSE
//! registers to aligned slots, which faults — a second #GP, inside the handler
//! for the first, from a `movaps` in code that is entirely correct.
//!
//! So the handlers keep only the frame decoding, and hand the work to a body
//! function called with the stack put back the way the ABI expects.

use x86_64::structures::idt::{InterruptStackFrame, PageFaultErrorCode};

use crate::{println, serial_println};

/// Call `body(arg)` with the stack aligned as the System V ABI requires.
///
/// # Safety
/// `arg` must be whatever `body` expects.
extern "C" {
    fn kosh_call_aligned(body: extern "C" fn(u64), arg: u64);
}

// Written as `global_asm!` rather than inline `asm!` for a reason worth
// recording, because the inline version compiles and then breaks the machine.
//
// An inline `asm!` that contains a `call` has to declare `clobber_abi("C")`, and
// for an `x86-interrupt` function that declaration is a statement that the
// handler clobbers xmm0-15. LLVM then dutifully preserves them — with `movaps`
// into 16-byte-aligned slots, in the handler's *own prologue*, before any of
// this code runs. Those spills fault for exactly the reason this function
// exists, so the handler re-faults on entry, forever, until the kernel stack is
// gone and it comes out as a double fault in a prologue.
//
// A plain `extern "C"` call has no such effect: the handler saves what it
// actually holds, which is what it did before any of this was here.
core::arch::global_asm!(
    r#"
.section .text.kosh_call_aligned, "ax"
.global kosh_call_aligned
.type kosh_call_aligned, @function

kosh_call_aligned:
    /* rdi = body, rsi = arg. r10 and r11 are caller-saved scratch. */
    movq    %rdi, %r11
    movq    %rsi, %rdi

    movq    %rsp, %r10
    andq    $-16, %rsp          /* only ever moves down, so the caller's
                                   locals - including the context struct -
                                   stay where they are                      */
    subq    $16, %rsp           /* still 16-aligned; room to park old rsp   */
    movq    %r10, 8(%rsp)

    call    *%r11               /* rsp % 16 == 0 here, so the callee starts
                                   at 8, which is what the ABI defines      */

    movq    8(%rsp), %rsp
    ret
"#,
    options(att_syntax)
);

/// What [`fault_body`] needs, gathered so it can be passed as one pointer.
#[repr(C)]
struct FaultContext<'a> {
    vector: u8,
    name: &'a str,
    error_code: Option<u64>,
    frame: &'a InterruptStackFrame,
}

/// Print a uniform exception banner, then kill the process or hang.
fn fault(vector: u8, name: &str, error_code: Option<u64>, frame: &InterruptStackFrame) -> ! {
    let context = FaultContext {
        vector,
        name,
        error_code,
        frame,
    };

    unsafe { kosh_call_aligned(fault_body, &context as *const FaultContext as u64) };

    // `fault_body` does not return. Reached only if it somehow did.
    halt()
}

extern "C" fn fault_body(context: u64) {
    let context = unsafe { &*(context as *const FaultContext) };

    dump(context.vector, context.name, context.error_code, context.frame);

    if from_ring3(context.frame) {
        serial_println!("  action      : ring 3 fault — terminating the process, not the system");
        crate::task::kill_current(crate::task::EXIT_KILLED);
    }

    halt()
}

/// Whether the fault happened in ring 3.
///
/// From the saved CS's requested-privilege-level bits, which the CPU pushed as
/// part of the exception frame. Reading the *current* CS would be wrong — by the
/// time a handler runs, CS is the kernel's.
fn from_ring3(frame: &InterruptStackFrame) -> bool {
    frame.code_segment & 3 == 3
}

fn dump(vector: u8, name: &str, error_code: Option<u64>, frame: &InterruptStackFrame) {
    serial_println!();
    serial_println!("==================== CPU EXCEPTION ====================");
    serial_println!("  vector      : {} (0x{:02x})  {}", vector, vector, name);

    match error_code {
        Some(code) => serial_println!("  error code  : 0x{:x}", code),
        None => serial_println!("  error code  : (none)"),
    }

    serial_println!("  rip         : 0x{:016x}", frame.instruction_pointer.as_u64());
    serial_println!("  cs          : 0x{:x}", frame.code_segment);
    serial_println!("  rflags      : 0x{:016x}", frame.cpu_flags);
    serial_println!("  rsp         : 0x{:016x}", frame.stack_pointer.as_u64());
    serial_println!("  ss          : 0x{:x}", frame.stack_segment);
    serial_println!("=======================================================");

    // Mirror a one-liner to VGA so a headless-serial mistake still shows
    // something on screen.
    println!("CPU EXCEPTION {} ({}) at 0x{:x}", vector, name, frame.instruction_pointer.as_u64());
}

fn halt() -> ! {
    serial_println!("System halted.");
    loop {
        x86_64::instructions::interrupts::disable();
        x86_64::instructions::hlt();
    }
}

// --- faults that cannot be resumed ---------------------------------------

pub extern "x86-interrupt" fn divide_error_handler(frame: InterruptStackFrame) {
    fault(0, "#DE Divide Error", None, &frame)
}

pub extern "x86-interrupt" fn nmi_handler(frame: InterruptStackFrame) {
    // An NMI is usually a hardware problem (parity error, watchdog). Report it
    // but do not pretend we can recover — and unlike the rest, do not blame the
    // running process for it, whichever ring it was in.
    dump(2, "NMI Non-Maskable Interrupt", None, &frame);
    halt()
}

pub extern "x86-interrupt" fn overflow_handler(frame: InterruptStackFrame) {
    fault(4, "#OF Overflow", None, &frame)
}

pub extern "x86-interrupt" fn bound_range_handler(frame: InterruptStackFrame) {
    fault(5, "#BR Bound Range Exceeded", None, &frame)
}

pub extern "x86-interrupt" fn invalid_opcode_handler(frame: InterruptStackFrame) {
    fault(6, "#UD Invalid Opcode", None, &frame)
}

pub extern "x86-interrupt" fn device_not_available_handler(frame: InterruptStackFrame) {
    // Almost always means CR0.TS/EM is wrong, i.e. the SSE setup in boot32.rs
    // did not take effect.
    fault(7, "#NM Device Not Available (FPU/SSE)", None, &frame)
}

pub extern "x86-interrupt" fn invalid_tss_handler(frame: InterruptStackFrame, error_code: u64) {
    fault(10, "#TS Invalid TSS", Some(error_code), &frame)
}

pub extern "x86-interrupt" fn segment_not_present_handler(
    frame: InterruptStackFrame,
    error_code: u64,
) {
    fault(11, "#NP Segment Not Present", Some(error_code), &frame)
}

pub extern "x86-interrupt" fn stack_segment_handler(frame: InterruptStackFrame, error_code: u64) {
    fault(12, "#SS Stack Segment Fault", Some(error_code), &frame)
}

pub extern "x86-interrupt" fn general_protection_handler(
    frame: InterruptStackFrame,
    error_code: u64,
) {
    serial_println!();
    serial_println!("(#GP error code 0x{:x}: {})", error_code, gp_source(error_code));
    fault(13, "#GP General Protection Fault", Some(error_code), &frame)
}

fn gp_source(error_code: u64) -> &'static str {
    if error_code == 0 {
        "not segment-related"
    } else if error_code & 0b10 != 0 {
        "IDT descriptor"
    } else if error_code & 0b100 != 0 {
        "LDT descriptor"
    } else {
        "GDT descriptor"
    }
}

pub extern "x86-interrupt" fn x87_handler(frame: InterruptStackFrame) {
    fault(16, "#MF x87 Floating-Point Error", None, &frame)
}

pub extern "x86-interrupt" fn alignment_check_handler(
    frame: InterruptStackFrame,
    error_code: u64,
) {
    fault(17, "#AC Alignment Check", Some(error_code), &frame)
}

pub extern "x86-interrupt" fn machine_check_handler(frame: InterruptStackFrame) -> ! {
    // Hardware said something is wrong with the hardware. Not the process's
    // doing, and not survivable by killing it.
    dump(18, "#MC Machine Check", None, &frame);
    halt()
}

pub extern "x86-interrupt" fn simd_handler(frame: InterruptStackFrame) {
    fault(19, "#XM SIMD Floating-Point Exception", None, &frame)
}

pub extern "x86-interrupt" fn virtualization_handler(frame: InterruptStackFrame) {
    fault(20, "#VE Virtualization Exception", None, &frame)
}

// --- page fault: the one worth decoding properly --------------------------

pub extern "x86-interrupt" fn page_fault_handler(
    frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    // Every page fault goes through the realigning shim, not just the ones that
    // kill something: `resolve_cow` and `resolve_demand` run on the common path,
    // several times per program, and they are ordinary kernel code entitled to
    // an ABI-conformant stack.
    let mut context = PageFaultContext {
        frame: &frame,
        error_code,
        resolved: false,
    };

    unsafe {
        kosh_call_aligned(
            page_fault_body,
            &mut context as *mut PageFaultContext as u64,
        )
    };

    // `resolved` is the return, because the shim's body cannot have one: the
    // handler returns normally so the CPU retries the faulting instruction.
    let _ = context.resolved;
}

#[repr(C)]
struct PageFaultContext<'a> {
    frame: &'a InterruptStackFrame,
    error_code: PageFaultErrorCode,
    resolved: bool,
}

extern "C" fn page_fault_body(context: u64) {
    use x86_64::registers::control::Cr2;

    let context = unsafe { &mut *(context as *mut PageFaultContext) };
    let frame = context.frame;
    let error_code = context.error_code;

    let accessed = Cr2::read();

    // Copy-on-write, before anything is printed.
    //
    // A write to a page shared by `fork` is not an error — it is the mechanism
    // working. `resolve_cow` gives this address space a private copy and the
    // faulting instruction is retried, so the common case produces no output at
    // all. Everything below is for faults that are genuinely faults.
    if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION)
        && error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE)
        && accessed.as_u64() < crate::syscall::uaccess::USER_ADDRESS_LIMIT
    {
        match crate::memory::paging::resolve_cow(accessed.as_u64()) {
            Ok(true) => {
                context.resolved = true;
                return;
            }
            Ok(false) => {}
            Err(e) => {
                serial_println!();
                serial_println!("Copy-on-write fault at 0x{:x} could not be resolved: {}", accessed.as_u64(), e);
            }
        }
    }

    // Demand-zero, likewise before anything is printed. A first touch of a
    // reserved page — a program's `.bss`, its stack, an anonymous `mmap` — is
    // the mechanism working, not an error.
    if !error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION)
        && accessed.as_u64() < crate::syscall::uaccess::USER_ADDRESS_LIMIT
    {
        match crate::memory::paging::resolve_demand(accessed.as_u64()) {
            Ok(true) => {
                context.resolved = true;
                return;
            }
            Ok(false) => {}
            Err(e) => {
                serial_println!();
                serial_println!(
                    "Demand-zero fault at 0x{:x} could not be serviced: {}",
                    accessed.as_u64(),
                    e
                );
            }
        }
    }

    serial_println!();
    serial_println!("==================== PAGE FAULT ====================");
    serial_println!("  accessed address : 0x{:016x}", accessed.as_u64());
    serial_println!(
        "  cause            : {} while {} in {} mode",
        if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
            "protection violation"
        } else {
            "page not present"
        },
        if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE) {
            "writing"
        } else if error_code.contains(PageFaultErrorCode::INSTRUCTION_FETCH) {
            "fetching an instruction"
        } else {
            "reading"
        },
        if error_code.contains(PageFaultErrorCode::USER_MODE) {
            "user"
        } else {
            "kernel"
        }
    );
    serial_println!("  error code       : {:?}", error_code);

    if accessed.as_u64() < 0x1000 {
        serial_println!("  note             : address is in the first page — this looks");
        serial_println!("                     like a null pointer dereference.");
    }

    // A fault in ring 3 is the process's problem, not the kernel's. Killing the
    // offending thread and carrying on is the entire point of running services
    // outside the kernel — halting here would mean any userspace bug takes the
    // whole system down, which is what a microkernel exists to avoid.
    if error_code.contains(PageFaultErrorCode::USER_MODE) {
        serial_println!("  action           : ring 3 fault — terminating the process");
        serial_println!("  rip              : 0x{:016x}", frame.instruction_pointer.as_u64());
        serial_println!("====================================================");
        // Same exit code every killed process gets, so a waiting parent sees one
        // thing for "the kernel stopped it" regardless of which vector did.
        crate::task::kill_current(crate::task::EXIT_KILLED);
    }

    dump(14, "#PF Page Fault", Some(error_code.bits()), frame);
    halt()
}

// --- double fault ----------------------------------------------------------

pub extern "x86-interrupt" fn double_fault_handler(
    frame: InterruptStackFrame,
    error_code: u64,
) -> ! {
    serial_println!();
    serial_println!("A second exception fired while handling the first.");
    serial_println!("Common causes: kernel stack overflow, or a fault inside a handler.");
    // Deliberately halts even from ring 3. A double fault means the CPU could
    // not deliver the *first* exception, so the machinery that would kill the
    // process is exactly the machinery that has already failed once.
    dump(8, "#DF Double Fault", Some(error_code), &frame);
    halt()
}

// --- resumable -------------------------------------------------------------

/// `int3`. Prints and returns, so debuggers and self-tests can use it.
pub extern "x86-interrupt" fn breakpoint_handler(frame: InterruptStackFrame) {
    serial_println!(
        "[#BP breakpoint at 0x{:016x}] — resuming",
        frame.instruction_pointer.as_u64()
    );
}

/// `#DB`. Single-step / hardware watchpoints. Resumable.
pub extern "x86-interrupt" fn debug_handler(frame: InterruptStackFrame) {
    serial_println!(
        "[#DB debug at 0x{:016x}] — resuming",
        frame.instruction_pointer.as_u64()
    );
}
