//! Preemptive kernel threads.
//!
//! This is the first thing in Kosh that actually schedules. Through Phase 3,
//! `process::scheduler` implemented round-robin, priority and CFS *policy* over
//! a table of processes — but "scheduling" ended at writing a PID into a field.
//! `handle_timer_tick()` had no callers, `ContextSwitcher::switch_context()`
//! had no callers, and no stack was ever switched.
//!
//! ## Scope: kernel threads, not processes
//!
//! A [`Thread`] here shares the kernel's address space and runs in ring 0. It
//! has its own stack and nothing else. That is deliberately *not* the same
//! abstraction as `process::Process`, which is meant to be a userspace process
//! with its own address space and ring-3 context — that arrives in Phase 5,
//! and will build on this switch primitive rather than replace it. Most kernels
//! carry both; conflating them now would mean pretending an address-space
//! switch exists before one does.
//!
//! ## Locking
//!
//! Every scheduler access runs with interrupts disabled. The timer handler
//! takes the same lock, so touching it with interrupts on means a tick landing
//! mid-update deadlocks against a lock the interrupted code already holds.
//!
//! The lock is always released *before* the switch. Holding a spinlock across a
//! context switch parks it in the outgoing thread, and the incoming thread
//! spins on it forever.

pub mod switch;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, Ordering};

use spin::Mutex;

use crate::interrupts::without_interrupts;
use crate::serial_println;
use switch::kosh_switch_context;

/// Fixed thread table. A fixed array rather than a `Vec` on purpose: the
/// scheduler hands out a raw pointer into a TCB and then drops its lock before
/// switching, so a concurrent `spawn` reallocating the backing store would
/// leave that pointer dangling.
const MAX_THREADS: usize = 16;

/// Per-thread kernel stack. Generous — these are debug-friendly, not tight.
const STACK_SIZE: usize = 16 * 1024;

/// Timer ticks a thread runs before being preempted. 100 Hz tick, so 2 ticks is
/// a 20 ms slice.
const TIME_SLICE_TICKS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Ready,
    Running,
    Finished,
}

pub struct Thread {
    pub id: usize,
    pub name: &'static str,
    /// Saved stack pointer while this thread is not running.
    rsp: u64,
    /// Owning handle to the stack. `None` for the bootstrap thread, which runs
    /// on the stack the boot trampoline set up.
    _stack: Option<Box<[u8]>>,
    state: State,
    entry: Option<(fn(usize), usize)>,
    /// Ticks this thread has been scheduled for — a cheap fairness check.
    ticks: u64,
}

impl Thread {
    /// Lay out a synthetic stack that [`kosh_switch_context`] can "resume" into
    /// even though this thread has never run.
    ///
    /// The frame must mirror the pop sequence exactly. From low address to
    /// high: RFLAGS, R15, R14, R13, R12, RBX, RBP, return address.
    ///
    /// The trailing dummy word exists for ABI alignment: after `ret` pops the
    /// return address, RSP must be congruent to 8 mod 16, which is the state a
    /// function sees immediately after a real `call`.
    fn prepare_stack(stack: &mut [u8]) -> u64 {
        let top = (stack.as_mut_ptr() as u64 + stack.len() as u64) & !0xF;

        unsafe {
            let put = |offset: u64, value: u64| {
                core::ptr::write((top - offset) as *mut u64, value);
            };

            put(8, 0); // alignment dummy
            put(16, thread_bootstrap as usize as u64); // return address
            put(24, 0); // rbp
            put(32, 0); // rbx
            put(40, 0); // r12
            put(48, 0); // r13
            put(56, 0); // r14
            put(64, 0); // r15

            // RFLAGS with IF clear: a new thread is first entered from inside
            // the timer interrupt handler, where interrupts are off. It enables
            // them itself in `thread_bootstrap`, once it is on its own stack
            // and no longer touching scheduler state.
            put(72, 0x0000_0002);
        }

        top - 72
    }
}

struct Scheduler {
    threads: [Option<Thread>; MAX_THREADS],
    current: usize,
    slice_remaining: u32,
    switches: u64,
}

impl Scheduler {
    const fn new() -> Self {
        Self {
            threads: [const { None }; MAX_THREADS],
            current: 0,
            slice_remaining: TIME_SLICE_TICKS,
            switches: 0,
        }
    }

    /// Next runnable thread after `current`, wrapping. Plain round-robin.
    fn next_runnable(&self) -> Option<usize> {
        for step in 1..=MAX_THREADS {
            let idx = (self.current + step) % MAX_THREADS;
            if let Some(t) = &self.threads[idx] {
                if t.state == State::Ready {
                    return Some(idx);
                }
            }
        }
        None
    }
}

static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Register the currently-executing context as thread 0 and start scheduling.
pub fn init() {
    without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        sched.threads[0] = Some(Thread {
            id: 0,
            name: "kmain",
            rsp: 0, // filled in by the first switch away from here
            _stack: None,
            state: State::Running,
            entry: None,
            ticks: 0,
        });
        sched.current = 0;
    });

    serial_println!(
        "Scheduler: round-robin over up to {} kernel threads, {} ms slice",
        MAX_THREADS,
        TIME_SLICE_TICKS * 10
    );
}

/// Create a runnable kernel thread. It starts the next time the scheduler runs.
pub fn spawn(name: &'static str, entry: fn(usize), arg: usize) -> Result<usize, &'static str> {
    let mut stack = alloc::vec![0u8; STACK_SIZE].into_boxed_slice();
    let rsp = Thread::prepare_stack(&mut stack);

    without_interrupts(|| {
        let mut sched = SCHEDULER.lock();

        let slot = sched
            .threads
            .iter()
            .position(|t| t.is_none())
            .ok_or("thread table full")?;

        sched.threads[slot] = Some(Thread {
            id: slot,
            name,
            rsp,
            _stack: Some(stack),
            state: State::Ready,
            entry: Some((entry, arg)),
            ticks: 0,
        });

        serial_println!("  spawned thread {} '{}' (stack top 0x{:x})", slot, name, rsp);
        Ok(slot)
    })
}

/// Begin preempting. Called once the demo threads exist.
pub fn start() {
    ENABLED.store(true, Ordering::SeqCst);
}

/// Called from the timer interrupt, after the EOI.
///
/// This is the wiring that did not exist: a periodic interrupt that actually
/// reaches the scheduler.
pub fn on_tick() {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let should_switch = {
        let mut sched = SCHEDULER.lock();
        let current = sched.current;
        if let Some(t) = sched.threads[current].as_mut() {
            t.ticks += 1;
        }

        if sched.slice_remaining > 1 {
            sched.slice_remaining -= 1;
            false
        } else {
            sched.slice_remaining = TIME_SLICE_TICKS;
            true
        }
    };

    if should_switch {
        schedule();
    }
}

/// Pick the next runnable thread and switch to it.
///
/// Safe to call from a thread as well as from the timer handler; it is a no-op
/// if nothing else is runnable.
pub fn schedule() {
    // Interrupts must stay off from here until the switch completes: the lock
    // is released before `kosh_switch_context`, and a tick arriving in that
    // window would re-enter the scheduler with `current` already advanced.
    let saved = crate::interrupts::interrupts_enabled();
    x86_64::instructions::interrupts::disable();

    let plan = {
        let mut sched = SCHEDULER.lock();

        match sched.next_runnable() {
            None => None,
            Some(next) => {
                let current = sched.current;
                if next == current {
                    None
                } else {
                    if let Some(t) = sched.threads[current].as_mut() {
                        if t.state == State::Running {
                            t.state = State::Ready;
                        }
                    }
                    if let Some(t) = sched.threads[next].as_mut() {
                        t.state = State::Running;
                    }

                    sched.current = next;
                    sched.switches += 1;

                    let next_rsp = sched.threads[next].as_ref().unwrap().rsp;
                    let prev_rsp = sched.threads[current].as_mut().unwrap().rsp_ptr();

                    Some((prev_rsp, next_rsp))
                }
            }
        }
        // lock dropped here, before the switch
    };

    if let Some((prev_rsp, next_rsp)) = plan {
        unsafe { kosh_switch_context(prev_rsp, next_rsp) };
    }

    if saved {
        x86_64::instructions::interrupts::enable();
    }
}

impl Thread {
    fn rsp_ptr(&mut self) -> *mut u64 {
        &mut self.rsp as *mut u64
    }
}

/// First thing a new thread executes.
///
/// It reads its own entry point out of the thread table rather than having it
/// passed in a register, which keeps the assembly in `switch.rs` down to the
/// callee-saved set and nothing thread-specific.
extern "C" fn thread_bootstrap() -> ! {
    let entry = without_interrupts(|| {
        let sched = SCHEDULER.lock();
        sched.threads[sched.current].as_ref().and_then(|t| t.entry)
    });

    // We arrived here from inside the timer interrupt handler, so interrupts
    // are still masked. Now that we are on our own stack and hold no locks, we
    // can take part in preemption.
    x86_64::instructions::interrupts::enable();

    if let Some((func, arg)) = entry {
        func(arg);
    }

    exit_current()
}

/// Retire the running thread and never come back.
pub fn exit_current() -> ! {
    without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let current = sched.current;
        if let Some(t) = sched.threads[current].as_mut() {
            t.state = State::Finished;
        }
    });

    loop {
        schedule();
        // Only reachable if nothing else is runnable; wait for a tick that
        // makes something else ready.
        x86_64::instructions::hlt();
    }
}

/// Yield the rest of this thread's slice.
pub fn yield_now() {
    schedule();
}

/// Number of threads not yet finished, excluding thread 0.
pub fn live_threads() -> usize {
    without_interrupts(|| {
        SCHEDULER
            .lock()
            .threads
            .iter()
            .skip(1)
            .filter(|t| matches!(t, Some(t) if t.state != State::Finished))
            .count()
    })
}

/// Total context switches performed.
pub fn switch_count() -> u64 {
    without_interrupts(|| SCHEDULER.lock().switches)
}

/// Free the stacks of finished threads.
pub fn reap_finished() {
    without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let current = sched.current;
        for (i, slot) in sched.threads.iter_mut().enumerate() {
            if i == current {
                continue;
            }
            if matches!(slot, Some(t) if t.state == State::Finished) {
                *slot = None;
            }
        }
    });
}

/// A snapshot of one thread, for callers that want to format it themselves
/// (the console renders to VGA as well as serial, which `print_threads` does
/// not).
#[derive(Debug, Clone, Copy)]
pub struct ThreadInfo {
    pub id: usize,
    pub name: &'static str,
    pub state: State,
    pub ticks: u64,
}

/// Copy the thread table out under the lock, so callers can print without
/// holding it. Returns how many entries were filled.
pub fn snapshot(out: &mut [Option<ThreadInfo>; MAX_THREADS]) -> usize {
    without_interrupts(|| {
        let sched = SCHEDULER.lock();
        let mut n = 0;
        for (i, slot) in sched.threads.iter().enumerate() {
            if let Some(t) = slot {
                out[i] = Some(ThreadInfo {
                    id: t.id,
                    name: t.name,
                    state: t.state,
                    ticks: t.ticks,
                });
                n += 1;
            } else {
                out[i] = None;
            }
        }
        n
    })
}

/// Maximum threads, for callers sizing a snapshot buffer.
pub const fn max_threads() -> usize {
    MAX_THREADS
}

pub fn print_threads() {
    without_interrupts(|| {
        let sched = SCHEDULER.lock();
        serial_println!("Thread table:");
        for slot in sched.threads.iter() {
            if let Some(t) = slot {
                serial_println!(
                    "  [{}] {:<10} {:?}  {} ticks",
                    t.id,
                    t.name,
                    t.state,
                    t.ticks
                );
            }
        }
        serial_println!("  context switches: {}", sched.switches);
    });
}
