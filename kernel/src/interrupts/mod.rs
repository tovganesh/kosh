//! Interrupt handling: IDT, CPU exceptions, PIC, PIT timer, PS/2 keyboard.
//!
//! Before this module existed, the kernel had no IDT at all. Any fault —
//! a null dereference, a stack overflow, a bad opcode — escalated straight to
//! a triple fault and silently rebooted the machine, with no indication of
//! what went wrong. That made every bug below this layer invisible.
//!
//! Split:
//!   * `exceptions` — CPU-generated faults (vectors 0..31). Print a legible
//!     dump and halt. Installed as early as possible.
//!   * `pic`        — the legacy 8259 pair, remapped to vectors 32..47 so
//!                    hardware IRQs stop colliding with CPU exceptions.
//!   * `timer`      — PIT channel 0 at 100 Hz, driving the system tick.
//!   * `keyboard`   — PS/2 IRQ1 -> scancode -> decoded char -> ring buffer.
//!
//! `init()` installs the IDT. `enable_hardware_interrupts()` brings the PIC
//! and PIT up and executes `sti`. They are deliberately separate: exceptions
//! should be catchable from the very start of boot, but hardware interrupts
//! should not fire until the kernel is ready for them.

pub mod exceptions;
pub mod keyboard;
pub mod pic;
pub mod timer;

use lazy_static::lazy_static;
use x86_64::structures::idt::InterruptDescriptorTable;

use crate::serial_println;

/// Hardware interrupt vector numbers, after the PIC remap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = pic::PIC_1_OFFSET,
    Keyboard,
}

impl InterruptIndex {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn as_usize(self) -> usize {
        self as usize
    }
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        // --- CPU exceptions (vectors 0..31) ------------------------------
        idt.divide_error.set_handler_fn(exceptions::divide_error_handler);
        idt.debug.set_handler_fn(exceptions::debug_handler);
        idt.non_maskable_interrupt.set_handler_fn(exceptions::nmi_handler);
        idt.breakpoint.set_handler_fn(exceptions::breakpoint_handler);
        idt.overflow.set_handler_fn(exceptions::overflow_handler);
        idt.bound_range_exceeded.set_handler_fn(exceptions::bound_range_handler);
        idt.invalid_opcode.set_handler_fn(exceptions::invalid_opcode_handler);
        idt.device_not_available.set_handler_fn(exceptions::device_not_available_handler);
        idt.invalid_tss.set_handler_fn(exceptions::invalid_tss_handler);
        idt.segment_not_present.set_handler_fn(exceptions::segment_not_present_handler);
        idt.stack_segment_fault.set_handler_fn(exceptions::stack_segment_handler);
        idt.general_protection_fault.set_handler_fn(exceptions::general_protection_handler);
        idt.page_fault.set_handler_fn(exceptions::page_fault_handler);
        idt.x87_floating_point.set_handler_fn(exceptions::x87_handler);
        idt.alignment_check.set_handler_fn(exceptions::alignment_check_handler);
        idt.machine_check.set_handler_fn(exceptions::machine_check_handler);
        idt.simd_floating_point.set_handler_fn(exceptions::simd_handler);
        idt.virtualization.set_handler_fn(exceptions::virtualization_handler);

        // The double fault handler runs on its own stack via the IST. If a
        // fault is caused by a bad kernel stack (guard-page hit, stack
        // overflow), pushing the exception frame onto that same stack would
        // fault again -> triple fault. The IST swaps in a known-good stack.
        unsafe {
            idt.double_fault
                .set_handler_fn(exceptions::double_fault_handler)
                .set_stack_index(crate::boot::DOUBLE_FAULT_IST_INDEX);
        }

        // --- hardware IRQs (vectors 32..47) ------------------------------
        idt[InterruptIndex::Timer.as_usize()]
            .set_handler_fn(timer::timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_usize()]
            .set_handler_fn(keyboard::keyboard_interrupt_handler);

        // Everything else on the PIC gets a handler that acknowledges and
        // moves on. Without this, a spurious IRQ7/IRQ15 (which real hardware
        // and some QEMU configurations do generate) would hit an absent IDT
        // entry and become a general protection fault.
        idt[pic::PIC_1_OFFSET as usize + 2].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_1_OFFSET as usize + 3].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_1_OFFSET as usize + 4].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_1_OFFSET as usize + 5].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_1_OFFSET as usize + 6].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_1_OFFSET as usize + 7].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_2_OFFSET as usize].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_2_OFFSET as usize + 1].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_2_OFFSET as usize + 2].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_2_OFFSET as usize + 3].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_2_OFFSET as usize + 4].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_2_OFFSET as usize + 5].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_2_OFFSET as usize + 6].set_handler_fn(pic::spurious_handler);
        idt[pic::PIC_2_OFFSET as usize + 7].set_handler_fn(pic::spurious_handler);

        idt
    };
}

/// Install the IDT. Safe to call once, early — before memory management, so
/// that faults during memory init are reported rather than silently fatal.
pub fn init() {
    serial_println!("Installing IDT...");
    IDT.load();
    serial_println!("IDT installed ({} exception vectors, 16 IRQ vectors)", 20);
}

/// Bring up the PIC and PIT and enable interrupts.
///
/// Separate from `init()` on purpose: exceptions must be catchable from the
/// start of boot, but we do not want a timer tick arriving in the middle of
/// memory-manager initialisation.
pub fn enable_hardware_interrupts() {
    serial_println!("Initializing PIC (remapping to vectors 32..47)...");
    pic::init();

    serial_println!("Initializing PIT at {} Hz...", timer::TIMER_HZ);
    timer::init();

    keyboard::init();

    x86_64::instructions::interrupts::enable();
    serial_println!("Interrupts enabled");
}

/// Run `f` with interrupts disabled, restoring the previous state afterwards.
///
/// Use this around anything that a handler also touches — otherwise an
/// interrupt arriving mid-critical-section deadlocks against a spinlock the
/// interrupted code already holds.
pub fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    x86_64::instructions::interrupts::without_interrupts(f)
}

/// Deliberately trigger a breakpoint (`int3`) to prove the IDT is live.
///
/// The handler prints and returns, so execution continues normally. If the
/// IDT were missing or malformed this would triple-fault instead.
pub fn test_breakpoint_exception() {
    serial_println!("Testing IDT: raising int3 (execution should continue)...");
    x86_64::instructions::interrupts::int3();
    serial_println!("Testing IDT: returned from int3 handler, IDT is live");
}
