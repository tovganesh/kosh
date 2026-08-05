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
const SYS_FORK: u64 = 2;
const SYS_EXEC: u64 = 3;
const SYS_WAIT: u64 = 4;
const SYS_GETPID: u64 = 5;
const SYS_MMAP: u64 = 10;
const SYS_MUNMAP: u64 = 11;
const SYS_WRITE: u64 = 23;
const SYS_CLOCK_GETTIME: u64 = 53;
const SYS_DEBUG_PRINT: u64 = 100;

/// mmap protection bits, as the kernel reads them.
const PROT_READ: u64 = 0x1;
const PROT_WRITE: u64 = 0x2;

/// mmap flags. `MAP_ANONYMOUS` is not optional: the kernel discriminates on it
/// rather than on `fd`, because `fd` lives in R8 and `syscall3` never sets it.
const MAP_PRIVATE: u64 = 0x02;
const MAP_ANONYMOUS: u64 = 0x20;

const CLOCK_MONOTONIC: u64 = 1;

/// Written before `fork` and again after it, by both parent and child, to prove
/// they are looking at different memory. A `static mut` rather than a stack
/// variable because it lives in `.bss` — the part of the address space the
/// eager copy has to duplicate.
static mut FORK_WITNESS: u64 = 0;

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

#[inline(always)]
unsafe fn syscall4(number: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") number => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        // Argument four is in R10, not RCX: `syscall` destroys RCX.
        in("r10") a4,
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

fn mmap(length: u64, prot: u64) -> i64 {
    // addr = 0: let the kernel choose.
    unsafe { syscall4(SYS_MMAP, 0, length, prot, MAP_PRIVATE | MAP_ANONYMOUS) }
}

fn munmap(addr: u64, length: u64) -> i64 {
    unsafe { syscall3(SYS_MUNMAP, addr, length, 0) }
}

fn clock_gettime(clock: u64, out: &mut [u64; 2]) -> i64 {
    unsafe { syscall3(SYS_CLOCK_GETTIME, clock, out.as_mut_ptr() as u64, 0) }
}

fn debug_print(message: &str) -> i64 {
    unsafe {
        syscall3(
            SYS_DEBUG_PRINT,
            message.as_ptr() as u64,
            message.len() as u64,
            0,
        )
    }
}

fn fork() -> i64 {
    unsafe { syscall3(SYS_FORK, 0, 0, 0) }
}

fn wait(task: i64, status: &mut i32) -> i64 {
    unsafe { syscall3(SYS_WAIT, task as u64, status as *mut i32 as u64, 0) }
}

fn exec(name: &str) -> i64 {
    unsafe { syscall3(SYS_EXEC, name.as_ptr() as u64, name.len() as u64, 0) }
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

    // mmap: memory the program was not given at load time. Until this release
    // `mmap` returned the constant 0x40000000 and reported success, so writing
    // to the result faulted.
    // A megabyte, of which this touches two pages. Under eager allocation that
    // is 256 frames for 8 KiB of use; reserved, it is 256 page table entries.
    const BIG: u64 = 1024 * 1024;
    let mapped = mmap(BIG, PROT_READ | PROT_WRITE);
    if mapped < 0 {
        print("  WARNING: mmap failed\n");
    } else {
        let p = mapped as u64 as *mut u64;

        // A page near the far end, which nothing has touched. It must read as
        // zero — a reserved page is *promised* to, and handing over a frame with
        // somebody else's data in it is the classic information leak.
        let far = unsafe { p.byte_add((BIG - 4096) as usize) };
        let far_before = unsafe { core::ptr::read_volatile(far) };

        unsafe {
            core::ptr::write_volatile(p, 0x1234_5678_9abc_def0);
            core::ptr::write_volatile(far, 0x0fed_cba9_8765_4321);
        }
        let ok = unsafe {
            core::ptr::read_volatile(p) == 0x1234_5678_9abc_def0
                && core::ptr::read_volatile(far) == 0x0fed_cba9_8765_4321
        };

        if far_before != 0 {
            print("  WARNING: an untouched page came back non-zero\n");
        } else if ok {
            print("  mmap gave me a usable megabyte at 0x");
            print_hex(mapped as u64);
            print(" (2 pages touched)\n");
        } else {
            print("  WARNING: mmap memory did not hold its contents\n");
        }

        if munmap(mapped as u64, BIG) < 0 {
            print("  WARNING: munmap failed\n");
        } else {
            print("  munmap returned the pages\n");
        }

        // Recycled memory must not carry the last tenant's data.
        //
        // Checking that a *fresh* mapping reads zero proves nothing on a mostly
        // idle system, where the allocator hands back frames that were never
        // written. So: dirty a region, give it back, take it again, and look.
        // Those are the same frames.
        const SMALL: u64 = 64 * 1024;
        const PATTERN: u64 = 0xDEAD_BEEF_CAFE_F00D;

        let first = mmap(SMALL, PROT_READ | PROT_WRITE);
        if first <= 0 {
            print("  WARNING: second mmap failed with ");
            print_i64(first);
            print("\n");
        } else {
            let q = first as u64 as *mut u64;
            let mut page = 0u64;
            while page < SMALL {
                unsafe { core::ptr::write_volatile(q.byte_add(page as usize), PATTERN) };
                page += 4096;
            }
            munmap(first as u64, SMALL);

            let second = mmap(SMALL, PROT_READ | PROT_WRITE);
            if second <= 0 {
                print("  WARNING: third mmap failed with ");
                print_i64(second);
                print("\n");
            } else {
                let r = second as u64 as *mut u64;
                let mut leaked = 0;
                let mut page = 0u64;
                while page < SMALL {
                    if unsafe { core::ptr::read_volatile(r.byte_add(page as usize)) } == PATTERN {
                        leaked += 1;
                    }
                    page += 4096;
                }
                if leaked == 0 {
                    print("  recycled pages came back zeroed\n");
                } else {
                    print("  WARNING: ");
                    print_i64(leaked);
                    print(" recycled page(s) still held the old contents\n");
                }
                munmap(second as u64, SMALL);
            }
        }
    }

    // A clock that moves. CLOCK_MONOTONIC comes from the PIT, so two reads
    // either side of some work must not go backwards.
    let mut first = [0u64; 2];
    let mut second = [0u64; 2];
    if clock_gettime(CLOCK_MONOTONIC, &mut first) == 0 {
        let mut spin = 0u64;
        for i in 0..2_000_000u64 {
            spin = spin.wrapping_add(i);
        }
        core::hint::black_box(spin);
        if clock_gettime(CLOCK_MONOTONIC, &mut second) == 0 {
            let a = first[0] * 1_000_000_000 + first[1];
            let b = second[0] * 1_000_000_000 + second[1];
            if b >= a {
                print("  CLOCK_MONOTONIC moves forwards\n");
            } else {
                print("  WARNING: CLOCK_MONOTONIC went backwards\n");
            }
        } else {
            print("  WARNING: second clock_gettime failed\n");
        }
    } else {
        print("  WARNING: clock_gettime failed\n");
    }

    // debug_print used to log the *address* of this string.
    if debug_print("hello reached the kernel log through debug_print") < 0 {
        print("  WARNING: debug_print failed\n");
    } else {
        print("  debug_print echoed my message\n");
    }

    // fork, and the copy-on-write behaviour underneath it.
    //
    // The sequencing is deliberate and not racy: `fork` returns *in the parent*
    // with the child merely Ready, so the parent's write below is guaranteed to
    // happen before the child runs. The child then reads the same address and
    // must still see the pre-fork value.
    //
    // That is what catches the classic copy-on-write bug — marking only the
    // child's page read-only and leaving the parent writable, so the parent's
    // next write lands in the page the child is about to read.
    unsafe { core::ptr::write_volatile(&raw mut FORK_WITNESS, 0x1111_1111) };

    let child = fork();
    if child < 0 {
        print("  WARNING: fork failed\n");
    } else if child == 0 {
        // Child. Its copy must still hold the pre-fork value.
        let inherited = unsafe { core::ptr::read_volatile(&raw const FORK_WITNESS) };
        if inherited == 0x1111_1111 {
            print("  child: I inherited the value my parent set before forking\n");
        } else {
            print("  WARNING: child saw ");
            print_hex(inherited);
            print(" — the parent wrote through a shared page\n");
        }

        unsafe { core::ptr::write_volatile(&raw mut FORK_WITNESS, 0x2222_2222) };
        let mine = unsafe { core::ptr::read_volatile(&raw const FORK_WITNESS) };
        if mine == 0x2222_2222 {
            print("  child: my copy of the witness is mine\n");
        } else {
            print("  WARNING: child could not write its own memory\n");
        }

        // exec never returns. If it does, it failed.
        let err = exec("hello2");
        print("  WARNING: exec returned ");
        print_i64(err);
        print("\n");
        exit(1);
    } else {
        // Parent. This write faults on a page it shares with the child, and the
        // kernel has to give the parent a private copy — not hand it the shared
        // one.
        unsafe { core::ptr::write_volatile(&raw mut FORK_WITNESS, 0x3333_3333) };

        let mut status: i32 = 0;
        wait(child, &mut status);

        let seen = unsafe { core::ptr::read_volatile(&raw const FORK_WITNESS) };
        if seen == 0x3333_3333 {
            print("  parent: the child did not touch my memory\n");
        } else {
            print("  WARNING: fork shared memory, witness reads ");
            print_hex(seen);
            print("\n");
        }

        // The child is gone, so this page has one holder again. Writing it
        // exercises the other half of the copy-on-write path: take ownership
        // rather than copy.
        unsafe { core::ptr::write_volatile(&raw mut FORK_WITNESS, 0x4444_4444) };
        if unsafe { core::ptr::read_volatile(&raw const FORK_WITNESS) } == 0x4444_4444 {
            print("  parent: still writable after the child exited\n");
        } else {
            print("  WARNING: parent lost write access to its own page\n");
        }

        if status == 7 {
            print("  child exec'd and exited 7\n");
        } else {
            print("  WARNING: unexpected child status ");
            print_i64(status as i64);
            print("\n");
        }
    }

    print("  exiting cleanly\n");
    exit(0)
}

fn print_hex(mut value: u64) {
    let mut buf = [0u8; 16];
    for i in (0..16).rev() {
        buf[i] = match (value & 0xF) as u8 {
            d @ 0..=9 => b'0' + d,
            d => b'a' + (d - 10),
        };
        value >>= 4;
    }
    write(1, &buf);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print("userspace panic\n");
    exit(1)
}
