//! The program `hello` execs into.
//!
//! Deliberately minimal, and deliberately linked at the same address as `hello`
//! and `ksh`. `exec` builds a fresh address space, loads this image into it,
//! swaps CR3 and frees the old one — so the only evidence that any of that
//! happened is that a *different* program is now running at the same address,
//! having inherited nothing but its thread.
//!
//! It exits with 7, which the parent checks. An `exec` that silently failed and
//! let the caller carry on would otherwise be indistinguishable from one that
//! worked, since the caller is the same program either way.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

const SYS_EXIT: u64 = 1;
const SYS_WRITE: u64 = 23;

/// Distinct from anything `hello` writes, so a stale image would be obvious.
static GREETING: &str = "  hello2 here: exec replaced the whole image\n";

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

fn print(s: &str) {
    unsafe { syscall3(SYS_WRITE, 1, s.as_ptr() as u64, s.len() as u64) };
}

// System V says RSP is 16-byte aligned at process entry, but a Rust
// `extern "C"` function assumes RSP is 8 *past* a boundary — the state a `call`
// leaves. Wiring Rust straight to the entry point makes the first SSE spill #GP.
core::arch::global_asm!(
    r#"
.section .text._start, "ax"
.global _start
.type _start, @function
_start:
    xorq    %rbp, %rbp
    andq    $-16, %rsp
    call    hello2_main
1:
    jmp     1b
"#,
    options(att_syntax)
);

#[no_mangle]
pub extern "C" fn hello2_main() -> ! {
    print(GREETING);

    // A .bss touch, because exec has to have zeroed this image's own tail rather
    // than leaving whatever the previous program had at the same address.
    static mut CANARY: [u64; 16] = [0; 16];
    let mut nonzero = 0;
    for i in 0..16 {
        if unsafe { core::ptr::read_volatile(&raw const CANARY[i]) } != 0 {
            nonzero += 1;
        }
    }
    if nonzero == 0 {
        print("  hello2: my .bss is mine and it is zero\n");
    } else {
        print("  WARNING: hello2 .bss is not zero\n");
    }

    unsafe { syscall3(SYS_EXIT, 7, 0, 0) };
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print("hello2 panic\n");
    unsafe { syscall3(SYS_EXIT, 1, 0, 0) };
    loop {
        core::hint::spin_loop();
    }
}
