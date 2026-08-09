//! Interrupts, for processes that are not the kernel.
//!
//! ## Why a counter and not a message
//!
//! The obvious design is "the kernel turns an IRQ into an IPC message". It is
//! also the one that deadlocks. Sending a message takes the message-queue lock
//! and the capability manager's lock, and an interrupt can arrive while the
//! interrupted thread is holding either — the driver whose request is being
//! serviced is *exactly* the thread most likely to be in the IPC path when its
//! own device raises IRQ14.
//!
//! So an interrupt does the smallest thing that cannot deadlock: it increments a
//! counter and marks any thread waiting on that line runnable. No allocation, no
//! IPC locks, one atomic and a scan of a fixed thread table.
//!
//! ## Why the counter is monotonic, and why userspace passes back what it saw
//!
//! `wait_irq(irq, seen)` blocks only while the counter still equals `seen`. That
//! makes the interrupt-arrives-before-the-wait case — which is the common case,
//! because a disk can answer faster than a process can be scheduled — a
//! non-event: the counter has already moved, and the call returns immediately.
//!
//! A flag would lose that race. The driver would issue its command, the IRQ
//! would fire before the driver reached `wait_irq`, the flag would be cleared by
//! nobody, and the driver would wait for an interrupt that had already happened.
//! That bug is a hang, and hangs are the expensive kind.
//!
//! ## Why it has a deadline
//!
//! A driver that waits forever for an interrupt that never comes takes the
//! system with it — and "never comes" is the normal outcome of a wrong PIC mask,
//! a device that needs its status register read to lower the line, or an
//! emulator quirk. So the wait has a bound in timer ticks, and expiry is
//! reported rather than hidden: the caller learns the counter did not move and
//! falls back to polling, which it can do because it owns the ports.
//!
//! Slow beats stuck.

use core::sync::atomic::{AtomicU64, Ordering};

/// IRQ lines the PIC pair can deliver.
pub const MAX_IRQ: usize = 16;

/// One monotonically increasing count per line.
///
/// `AtomicU64` rather than a `Mutex`: this is written from an interrupt handler,
/// and a handler that blocks on a lock the interrupted code holds is a deadlock
/// no amount of care elsewhere fixes.
static COUNTS: [AtomicU64; MAX_IRQ] = [const { AtomicU64::new(0) }; MAX_IRQ];

/// Note that `irq` fired, and wake whoever is waiting for it.
///
/// # Safety
/// Called from an interrupt handler. Must not be called with the scheduler lock
/// held by this CPU — `wake_for_irq` takes it.
pub fn fired(irq: u8) {
    if (irq as usize) < MAX_IRQ {
        COUNTS[irq as usize].fetch_add(1, Ordering::Release);
        crate::task::wake_for_irq(irq);
    }
}

/// How many times `irq` has fired since boot.
pub fn count(irq: u8) -> u64 {
    if (irq as usize) < MAX_IRQ {
        COUNTS[irq as usize].load(Ordering::Acquire)
    } else {
        0
    }
}
