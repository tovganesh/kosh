//! PIT (8254) channel 0 as the system tick.
//!
//! This is the first thing in Kosh that makes time pass. Until now
//! `get_system_time()` returned a counter nobody incremented and
//! `scheduler::handle_timer_tick()` had zero callers, so nothing in the kernel
//! could ever be preempted or timed.

use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::instructions::port::Port;
use x86_64::structures::idt::InterruptStackFrame;

use crate::interrupts::{pic, InterruptIndex};
use crate::serial_println;

/// Tick rate. 100 Hz gives a 10 ms granularity — fine for a scheduler time
/// slice, cheap enough not to drown the machine in interrupts.
pub const TIMER_HZ: u32 = 100;

/// The PIT's fixed input frequency, 1.193182 MHz.
const PIT_BASE_FREQUENCY: u32 = 1_193_182;

const PIT_CHANNEL_0: u16 = 0x40;
const PIT_COMMAND: u16 = 0x43;

/// Ticks since the timer was started. Monotonic; wraps after ~5.8 billion
/// years at 100 Hz, so not a practical concern.
static TICKS: AtomicU64 = AtomicU64::new(0);

/// Program PIT channel 0 for periodic interrupts at `TIMER_HZ`.
pub fn init() {
    let divisor = PIT_BASE_FREQUENCY / TIMER_HZ;
    assert!(divisor <= u16::MAX as u32, "PIT divisor out of range");

    unsafe {
        let mut command: Port<u8> = Port::new(PIT_COMMAND);
        let mut channel0: Port<u8> = Port::new(PIT_CHANNEL_0);

        // 0b00_11_010_0:
        //   channel 0, lo/hi byte access, mode 2 (rate generator), binary.
        command.write(0b0011_0100u8);
        channel0.write((divisor & 0xFF) as u8);
        channel0.write((divisor >> 8) as u8);
    }

    serial_println!(
        "  PIT channel 0: divisor {} -> {} Hz ({} ms per tick)",
        divisor,
        TIMER_HZ,
        1000 / TIMER_HZ
    );
}

/// Ticks elapsed since boot.
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Milliseconds elapsed since the timer was started.
pub fn uptime_ms() -> u64 {
    ticks() * (1000 / TIMER_HZ as u64)
}

/// Busy-wait for `ms` milliseconds. Requires interrupts to be enabled.
pub fn sleep_ms(ms: u64) {
    let target = ticks() + (ms * TIMER_HZ as u64) / 1000;
    while ticks() < target {
        x86_64::instructions::hlt();
    }
}

pub extern "x86-interrupt" fn timer_interrupt_handler(_frame: InterruptStackFrame) {
    let tick = TICKS.fetch_add(1, Ordering::Relaxed) + 1;

    // Proof of life, once a second. This is temporary scaffolding — it comes
    // out when the scheduler takes over the tick in Phase 4.
    if tick % TIMER_HZ as u64 == 0 {
        serial_println!("[tick] uptime {}s ({} ticks)", tick / TIMER_HZ as u64, tick);
    }

    // TODO (Phase 4): drive the scheduler from here.
    //   crate::process::scheduler::handle_timer_tick();

    unsafe {
        pic::notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}
