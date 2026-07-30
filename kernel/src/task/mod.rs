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

/// Per-thread kernel stack.
///
/// This is also the stack a thread's *system calls* run on once it is in ring 3,
/// which is why it is no longer 16 KiB: the FAT32 and ATA paths buffer whole
/// sectors, and the old dedicated syscall stack was 32 KiB.
const STACK_SIZE: usize = 32 * 1024;

/// Timer ticks a thread runs before being preempted. 100 Hz tick, so 2 ticks is
/// a 20 ms slice.
const TIME_SLICE_TICKS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Ready,
    Running,
    /// Waiting for the thread with this id to finish. Skipped by the scheduler,
    /// so a `wait` costs nothing while it blocks — as opposed to spinning on
    /// `yield_now`, which is what the first version of this did and which meant
    /// a shell waiting for a child consumed half the CPU.
    Blocked { on: usize },
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
    /// Top of this thread's kernel stack, 16-aligned.
    ///
    /// Published to `TSS.RSP0` and to `gs:0` on every switch to this thread, so
    /// that an interrupt taken in ring 3 and a `syscall` from ring 3 both land
    /// on *this* thread's stack. Safe to reuse from the top because entering
    /// ring 3 is `-> !`: `enter_ring3` never returns, so nothing below is live
    /// while the thread is in user mode.
    kernel_stack_top: u64,
    state: State,
    entry: Option<(fn(usize), usize)>,
    /// Ticks this thread has been scheduled for — a cheap fairness check.
    ticks: u64,
    /// Set by `sys_exit` before the thread retires, collected by `wait_for`.
    exit_code: i32,
    /// Another thread is in `wait_for` on this one, so `reap_finished` must
    /// leave the slot alone until that waiter has collected the exit code.
    awaited: bool,
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
    ///
    /// Returns `(top, rsp)`: the 16-aligned top of the stack, and the synthetic
    /// resume point below the frame.
    fn prepare_stack(stack: &mut [u8]) -> (u64, u64) {
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

        (top, top - 72)
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
    // kmain runs on the stack `gdt::init` published as RSP0, so thread 0 adopts
    // that rather than pretending it has no kernel stack. It never enters ring 3,
    // but leaving `gs:0` at zero would turn any mistake about that into a
    // page fault at address 0 with no frame to read.
    let boot_top = crate::gdt::boot_kernel_stack_top().as_u64();

    without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        sched.threads[0] = Some(Thread {
            id: 0,
            name: "kmain",
            rsp: 0, // filled in by the first switch away from here
            _stack: None,
            kernel_stack_top: boot_top,
            state: State::Running,
            entry: None,
            ticks: 0,
            exit_code: 0,
            awaited: false,
        });
        sched.current = 0;
    });

    publish_kernel_stack(0, boot_top);

    serial_println!(
        "Scheduler: round-robin over up to {} kernel threads, {} ms slice",
        MAX_THREADS,
        TIME_SLICE_TICKS * 10
    );
}

/// Create a runnable kernel thread. It starts the next time the scheduler runs.
pub fn spawn(name: &'static str, entry: fn(usize), arg: usize) -> Result<usize, &'static str> {
    let mut stack = alloc::vec![0u8; STACK_SIZE].into_boxed_slice();
    let (kernel_stack_top, rsp) = Thread::prepare_stack(&mut stack);

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
            kernel_stack_top,
            state: State::Ready,
            entry: Some((entry, arg)),
            ticks: 0,
            exit_code: 0,
            awaited: false,
        });

        serial_println!(
            "  spawned thread {} '{}' (kernel stack 0x{:x}, resume 0x{:x})",
            slot,
            name,
            kernel_stack_top,
            rsp
        );
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
                    let next_top = sched.threads[next].as_ref().unwrap().kernel_stack_top;
                    let prev_rsp = sched.threads[current].as_mut().unwrap().rsp_ptr();

                    Some((prev_rsp, next_rsp, next, next_top))
                }
            }
        }
        // lock dropped here, before the switch
    };

    if let Some((prev_rsp, next_rsp, next, next_top)) = plan {
        // Before the switch, not after: the incoming thread may resume straight
        // into `sysretq` and be in ring 3 — able to fault or syscall — before
        // any instruction after `kosh_switch_context` in *this* frame runs.
        // Interrupts are off, so publishing early is not visible to anyone else.
        publish_kernel_stack(next as u64, next_top);

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

/// Tell the CPU and the syscall stub which kernel stack is current.
///
/// Two consumers, one value:
///
/// * `TSS.RSP0` — used by the CPU when an interrupt arrives while the thread is
///   in ring 3. Without this, two ring-3 threads would push exception frames
///   onto the same stack.
/// * `gs:0` — read by the `syscall` stub, which the CPU gives no stack at all.
///
/// Must be called with interrupts disabled, from `schedule()`, for every switch.
/// Skipping it is not a subtle bug: the next syscall or ring-3 interrupt lands on
/// the *previous* thread's stack and quietly overwrites whatever it had parked
/// there.
fn publish_kernel_stack(id: u64, top: u64) {
    crate::gdt::set_kernel_stack(x86_64::VirtAddr::new(top));
    crate::percpu::set_syscall_stack(top);
    crate::percpu::set_current_thread(id);
}

/// Kernel stack top of the running thread, for diagnostics.
pub fn current_kernel_stack_top() -> u64 {
    crate::percpu::syscall_stack()
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

/// Record the value `wait_for` will hand to whoever is waiting.
pub fn set_exit_code(code: i32) {
    without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let current = sched.current;
        if let Some(t) = sched.threads[current].as_mut() {
            t.exit_code = code;
        }
    });
}

/// Id of the running thread.
pub fn current_id() -> usize {
    without_interrupts(|| SCHEDULER.lock().current)
}

/// Retire the running thread and never come back.
pub fn exit_current() -> ! {
    // Release this thread's address-space reservations before it becomes
    // unschedulable. Done here, while the thread is still current, because the
    // table keyed on thread ids is about to stop having an entry for it.
    let id = current_id();
    crate::usermode::on_thread_exit(id);

    without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let current = sched.current;
        if let Some(t) = sched.threads[current].as_mut() {
            t.state = State::Finished;
        }

        // Wake anyone blocked on us. If this is skipped the waiter is never
        // Ready again and the machine deadlocks with a live thread that the
        // scheduler will not pick — the failure mode that makes `Blocked` more
        // dangerous than a yield loop, and worth the two lines to get right.
        wake_waiters_locked(&mut sched, current);
    });

    loop {
        schedule();
        // Only reachable if nothing else is runnable; wait for a tick that
        // makes something else ready.
        x86_64::instructions::hlt();
    }
}

fn wake_waiters_locked(sched: &mut Scheduler, finished: usize) {
    for slot in sched.threads.iter_mut() {
        if let Some(t) = slot {
            if t.state == (State::Blocked { on: finished }) {
                t.state = State::Ready;
            }
        }
    }
}

/// Block until thread `id` finishes, then return its exit code.
///
/// The kernel half of `sys_wait`. Returns `Err` if `id` is not a live thread or
/// is the caller itself — waiting on yourself is a deadlock, and it is better
/// caught here than discovered as a hang.
pub fn wait_for(id: usize) -> Result<i32, &'static str> {
    if id >= MAX_THREADS {
        return Err("no such thread");
    }

    // Claim the slot first. Without this, `reap_finished` running between the
    // child's exit and this thread waking up would free the TCB and take the
    // exit code with it.
    let already_finished = without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let current = sched.current;
        if id == current {
            return Err("a thread cannot wait for itself");
        }
        match sched.threads[id].as_mut() {
            None => Err("no such thread"),
            Some(t) => {
                t.awaited = true;
                Ok(t.state == State::Finished)
            }
        }
    })?;

    if !already_finished {
        // Mark, then switch. `schedule()` will not pick a Blocked thread, so
        // this returns only once the child's `exit_current` has woken us.
        loop {
            let done = without_interrupts(|| {
                let mut sched = SCHEDULER.lock();
                let current = sched.current;
                let finished = matches!(
                    sched.threads[id].as_ref().map(|t| t.state),
                    Some(State::Finished) | None
                );
                if !finished {
                    if let Some(t) = sched.threads[current].as_mut() {
                        t.state = State::Blocked { on: id };
                    }
                }
                finished
            });

            if done {
                break;
            }

            schedule();
        }
    }

    // Collect and release. The child is Finished, so nothing is running on the
    // stack this drops.
    without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        match sched.threads[id].take() {
            Some(t) => Ok(t.exit_code),
            None => Ok(0),
        }
    })
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
            // `awaited` threads belong to whoever is in `wait_for` on them; that
            // caller frees the slot after reading the exit code.
            if matches!(slot, Some(t) if t.state == State::Finished && !t.awaited) {
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
