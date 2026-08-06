//! Process 1.
//!
//! The kernel starts exactly one program and this is it. Everything else on the
//! system — the disk driver, the filesystem, the shell — is started from here,
//! in dependency order, and shut down from here when the shell exits.
//!
//! ## What this replaces
//!
//! 788 lines across four files that had never run: a `ServiceManager` with a
//! restart policy for services that could not be started, a `ProcessSpawner`
//! with a priority field nothing read, and a `syscalls.rs` whose `sys_spawn`
//! returned a fabricated pid rather than making a system call. The crate had no
//! linker script and was not on the ISO. The kernel spawned `ksh` directly and
//! this directory was decoration.
//!
//! ## Why the order matters
//!
//! `fs` cannot mount until `block` is answering, and `ksh` cannot list a
//! directory until `fs` is answering. Waiting is not optional and it is not a
//! sleep: after spawning each service, init polls `lookup_service` until the
//! name appears, so "the service is up" means the service said so.
//!
//! ## Why it does not restart anything
//!
//! It could — a driver crashing and being replaced is one of the better
//! arguments for this architecture, and the kernel already releases a dead
//! process's device claim and its registered name. What is missing is a way for
//! clients to notice: `ksh` holds a pid it looked up once, and a restarted `fs`
//! has a different one. Restart without client-side re-lookup would hand the
//! shell a dead pid and present as a hang. So this exits when the shell exits,
//! and supervision waits for reconnect.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

const SYS_EXIT: u64 = 1;
const SYS_WAIT: u64 = 4;
const SYS_YIELD: u64 = 8;
const SYS_SPAWN: u64 = 9;
const SYS_WRITE: u64 = 23;
const SYS_SEND_MESSAGE: u64 = 30;
const SYS_LOOKUP_SERVICE: u64 = 47;

#[inline(always)]
unsafe fn syscall3(number: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") number => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

fn write(bytes: &[u8]) {
    unsafe { syscall3(SYS_WRITE, 1, bytes.as_ptr() as u64, bytes.len() as u64) };
}

fn print(s: &str) {
    write(s.as_bytes());
}

fn print_i64(mut value: i64) {
    let mut buf = [0u8; 24];
    let mut i = buf.len();
    let negative = value < 0;
    if negative {
        value = -value;
    }
    if value == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while value > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    if negative {
        i -= 1;
        buf[i] = b'-';
    }
    write(&buf[i..]);
}

fn spawn(name: &str) -> i64 {
    unsafe { syscall3(SYS_SPAWN, name.as_ptr() as u64, name.len() as u64, 0) }
}

fn wait(task: i64, status: &mut i32) -> i64 {
    unsafe { syscall3(SYS_WAIT, task as u64, status as *mut i32 as u64, 0) }
}

fn lookup_service(name: &str) -> i64 {
    unsafe { syscall3(SYS_LOOKUP_SERVICE, name.as_ptr() as u64, name.len() as u64, 0) }
}

fn send_message(to: i64, bytes: &[u8]) -> i64 {
    unsafe {
        syscall3(
            SYS_SEND_MESSAGE,
            to as u64,
            bytes.as_ptr() as u64,
            bytes.len() as u64,
        )
    }
}

fn yield_now() {
    unsafe { syscall3(SYS_YIELD, 0, 0, 0) };
}

fn exit(code: u64) -> ! {
    unsafe { syscall3(SYS_EXIT, code, 0, 0) };
    loop {
        core::hint::spin_loop();
    }
}

/// How many times init yields waiting for a service to register.
///
/// Bounded, because the alternative to a bound is a system that hangs at boot
/// with no output when a service fails to start — which is precisely what an
/// init process must not do. Each iteration is a `yield`, not a spin: the
/// service is runnable and this thread has nothing useful to do.
const SERVICE_WAIT_YIELDS: u32 = 20_000;

/// Start `module` and block until it has registered `service`.
///
/// The thread id `spawn` returns and the pid `lookup_service` returns are the
/// same number — a process here *is* a ring-3 thread — but they are obtained
/// separately on purpose. A service that started and then died would give a
/// perfectly good id from `spawn` and nothing from the registry, and those two
/// answers should not be confused with each other.
fn start_service(module: &str, service: &str) -> i64 {
    print("init: starting ");
    print(module);
    print("\n");

    let thread = spawn(module);
    if thread < 0 {
        print("init: could not spawn ");
        print(module);
        print(", error ");
        print_i64(thread);
        print("\n");
        return thread;
    }

    for _ in 0..SERVICE_WAIT_YIELDS {
        let pid = lookup_service(service);
        if pid >= 0 {
            print("init: service '");
            print(service);
            print("' is up as pid ");
            print_i64(pid);
            print("\n");
            return pid;
        }
        yield_now();
    }

    print("init: '");
    print(service);
    print("' never registered\n");
    -1
}

/// The shutdown message `block` understands.
///
/// `block`'s protocol calls shutdown op 2 and `fs`'s calls it op 6, so these are
/// two messages rather than one. Sending the same bytes to both would work today
/// and break the first time either protocol moved an opcode.
fn shutdown_block(pid: i64) {
    let mut request = [0u8; 24];
    request[0..4].copy_from_slice(&0x4B42_4C4Bu32.to_le_bytes()); // "KBLK"
    request[4..8].copy_from_slice(&2u32.to_le_bytes()); // OP_SHUTDOWN
    send_message(pid, &request);
}

fn shutdown_fs(pid: i64) {
    let mut request = [0u8; 40];
    request[0..4].copy_from_slice(&0x4B46_5330u32.to_le_bytes()); // "KFS0"
    request[4..8].copy_from_slice(&6u32.to_le_bytes()); // OP_SHUTDOWN
    send_message(pid, &request);
}

core::arch::global_asm!(
    r#"
.section .text._start, "ax"
.global _start
.type _start, @function
_start:
    xorq    %rbp, %rbp
    andq    $-16, %rsp
    call    init_main
1:
    jmp     1b
"#,
    options(att_syntax)
);

#[no_mangle]
pub extern "C" fn init_main() -> ! {
    print("init: kosh userspace starting\n");

    let block = start_service("ata-driver", "block");
    if block < 0 {
        print("init: no block service; the filesystem cannot mount\n");
        exit(1);
    }

    let fs = start_service("fs-service", "fs");
    if fs < 0 {
        print("init: no fs service; the shell would have nothing to read\n");
        shutdown_block(block);
        exit(1);
    }

    print("init: userspace is up, handing the console to ksh\n");

    let shell = spawn("ksh");
    if shell < 0 {
        print("init: could not start ksh\n");
        shutdown_fs(fs);
        shutdown_block(block);
        exit(1);
    }

    let mut status: i32 = 0;
    wait(shell, &mut status);

    print("init: ksh exited with ");
    print_i64(status as i64);
    print(", shutting the services down\n");

    // Filesystem first. It is the block driver's only client, and stopping the
    // driver with a read in flight would leave `fs` blocked in a receive that
    // never completes — a hang rather than an error, and hangs are the harder
    // of the two to diagnose from a serial log.
    shutdown_fs(fs);
    let mut fs_status: i32 = 0;
    wait(fs, &mut fs_status);

    shutdown_block(block);
    let mut block_status: i32 = 0;
    wait(block, &mut block_status);

    print("init: services stopped, exiting\n");
    exit(status as u64)
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print("init panic\n");
    exit(1)
}
