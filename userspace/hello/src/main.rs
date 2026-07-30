//! A minimal userspace program, loaded from an ELF module by the kernel.
//!
//! Unlike the Phase 5 payload — hand-written position-independent assembly that
//! the kernel mapped wherever it liked — this is an ordinary Rust binary linked
//! at a fixed address (see `user.ld`). It only runs if the loader actually
//! parsed the ELF headers and honoured `p_vaddr`, so its output is evidence
//! that the loader works rather than that a memcpy worked.
//!
//! It deliberately exercises three things the loader has to get right:
//!
//! * `.text` — executing at the linked address at all.
//! * `.rodata` — string literals resolved through absolute addresses.
//! * `.bss` — a zero-initialised static, which lives in `p_memsz` beyond
//!   `p_filesz` and therefore exists only if the loader zeroed the tail.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

const SYS_EXIT: u64 = 1;
const SYS_GETPID: u64 = 5;
const SYS_WRITE: u64 = 23;

/// Zero-initialised, so it occupies no space in the file. If the loader forgets
/// to zero the gap between `p_filesz` and `p_memsz`, this reads as garbage.
static mut BSS_CANARY: [u64; 64] = [0; 64];

#[inline(always)]
unsafe fn syscall3(number: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") number => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        // The instruction clobbers these; the compiler has to know.
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

fn write(fd: u64, bytes: &[u8]) -> i64 {
    unsafe { syscall3(SYS_WRITE, fd, bytes.as_ptr() as u64, bytes.len() as u64) }
}

fn getpid() -> i64 {
    unsafe { syscall3(SYS_GETPID, 0, 0, 0) }
}

fn exit(code: u64) -> ! {
    unsafe { syscall3(SYS_EXIT, code, 0, 0) };
    // The kernel does not return from exit, but the type system does not know.
    loop {
        core::hint::spin_loop();
    }
}

fn print(s: &str) {
    write(1, s.as_bytes());
}

/// Print a signed integer without an allocator or `core::fmt`.
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

    write(1, &buf[i..]);
}

// The process entry point.
//
// System V says RSP is 16-byte aligned when the kernel enters a new process.
// But a Rust `extern "C"` function is compiled as an ordinary callee, so LLVM
// assumes RSP is 8 *past* a 16-byte boundary — the state left by a `call`. Wire
// a Rust function straight to the entry point and its first SSE spill
// (`movaps`, which requires 16-byte alignment) faults with #GP.
//
// So `_start` is asm, exactly like a real crt0: normalise the alignment, then
// `call` the Rust entry so it sees the boundary it was compiled for.
core::arch::global_asm!(
    r#"
.section .text._start, "ax"
.global _start
.type _start, @function
_start:
    xorq    %rbp, %rbp          /* end of the frame-pointer chain */
    andq    $-16, %rsp          /* System V: 16-byte aligned at process entry */
    call    kosh_main           /* ...and `call` makes it 8 past, as ABI wants */
1:
    jmp     1b                  /* kosh_main does not return */
"#,
    options(att_syntax)
);

#[no_mangle]
pub extern "C" fn kosh_main() -> ! {
    print("hello from a loaded ELF binary\n");

    print("  my pid is ");
    print_i64(getpid());
    print("\n");

    // .bss check: every byte should be zero because the loader zeroed it, not
    // because anything in the file said so.
    let mut nonzero = 0usize;
    for i in 0..64 {
        if unsafe { core::ptr::read_volatile(&raw const BSS_CANARY[i]) } != 0 {
            nonzero += 1;
        }
    }
    if nonzero == 0 {
        print("  .bss was zeroed correctly\n");
    } else {
        print("  WARNING: .bss contains garbage\n");
    }

    // Stack check: the kernel has to have given us a mapped, writable stack.
    let mut on_stack = [0u64; 32];
    for (i, slot) in on_stack.iter_mut().enumerate() {
        *slot = (i as u64) * 7 + 1;
    }
    let mut sum = 0u64;
    for slot in on_stack.iter() {
        sum += unsafe { core::ptr::read_volatile(slot) };
    }
    if sum == (0..32u64).map(|i| i * 7 + 1).sum() {
        print("  stack is writable and readable\n");
    } else {
        print("  WARNING: stack check failed\n");
    }

    print("  exiting cleanly\n");
    exit(0)
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print("userspace panic\n");
    exit(1)
}
