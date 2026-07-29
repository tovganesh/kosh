//! The legacy 8259 PIC pair.
//!
//! On a freshly-booted PC the two PICs deliver IRQ0..15 on vectors 8..15 and
//! 0x70..0x77. Vectors 8..15 collide head-on with CPU exceptions — IRQ0 (the
//! timer) arrives as vector 8, which is #DF Double Fault. So the very first
//! timer tick after `sti` would look like a double fault.
//!
//! Remapping is therefore not optional. We move them to 32..47, immediately
//! after the 32 architecturally-reserved exception vectors.

use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::structures::idt::InterruptStackFrame;

use crate::serial_println;

/// First vector used by the primary PIC (IRQ0..7 -> 32..39).
pub const PIC_1_OFFSET: u8 = 32;
/// First vector used by the secondary PIC (IRQ8..15 -> 40..47).
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// Remap both PICs and unmask only the IRQs we actually handle.
pub fn init() {
    unsafe {
        let mut pics = PICS.lock();
        pics.initialize();

        // Mask everything except IRQ0 (timer) and IRQ1 (keyboard). IRQ2 is the
        // cascade line to the secondary PIC and must stay unmasked for IRQ8..15
        // to work at all — but we mask the secondary entirely for now.
        //
        // primary mask: bit set = masked. 0b1111_1000 leaves IRQ0,1,2 enabled.
        pics.write_masks(0b1111_1000, 0b1111_1111);
    }

    serial_println!(
        "  PIC remapped: IRQ0..7 -> vectors {}..{}, IRQ8..15 -> vectors {}..{}",
        PIC_1_OFFSET,
        PIC_1_OFFSET + 7,
        PIC_2_OFFSET,
        PIC_2_OFFSET + 7
    );
    serial_println!("  IRQ0 (timer) and IRQ1 (keyboard) unmasked, rest masked");
}

/// Acknowledge an interrupt so the PIC will deliver the next one.
///
/// Forgetting this is the classic "the timer fires exactly once" bug.
///
/// # Safety
/// Must be called with the vector of the interrupt currently being serviced.
pub unsafe fn notify_end_of_interrupt(vector: u8) {
    PICS.lock().notify_end_of_interrupt(vector);
}

/// Catch-all for IRQs we do not handle yet.
///
/// Real hardware and some QEMU configurations generate spurious IRQ7/IRQ15.
/// Without an IDT entry those become a #GP; with one, we simply acknowledge
/// and carry on.
pub extern "x86-interrupt" fn spurious_handler(_frame: InterruptStackFrame) {
    // Nothing sensible to do but acknowledge. Deliberately silent: logging
    // here would flood the console if a device latched an IRQ line high.
    unsafe {
        // IRQ7 is the usual spurious vector on the primary PIC.
        notify_end_of_interrupt(PIC_1_OFFSET + 7);
    }
}
