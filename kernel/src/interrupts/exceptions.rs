//! CPU exception handlers.
//!
//! Every handler prints a legible dump — vector, mnemonic, error code, faulting
//! instruction pointer, stack pointer, flags and (for page faults) CR2 — and
//! then halts. Halting rather than rebooting is deliberate: a frozen machine
//! with a readable dump on the serial port is debuggable, a reboot loop is not.

use x86_64::structures::idt::{InterruptStackFrame, PageFaultErrorCode};

use crate::{println, serial_println};

/// Print a uniform exception banner, then hang.
fn fault(vector: u8, name: &str, error_code: Option<u64>, frame: &InterruptStackFrame) -> ! {
    dump(vector, name, error_code, frame);
    halt()
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
    // but do not pretend we can recover.
    fault(2, "NMI Non-Maskable Interrupt", None, &frame)
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
    fault(18, "#MC Machine Check", None, &frame)
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
    use x86_64::registers::control::Cr2;

    let accessed = Cr2::read();

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

    fault(14, "#PF Page Fault", Some(error_code.bits()), &frame)
}

// --- double fault ----------------------------------------------------------

pub extern "x86-interrupt" fn double_fault_handler(
    frame: InterruptStackFrame,
    error_code: u64,
) -> ! {
    serial_println!();
    serial_println!("A second exception fired while handling the first.");
    serial_println!("Common causes: kernel stack overflow, or a fault inside a handler.");
    fault(8, "#DF Double Fault", Some(error_code), &frame)
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
