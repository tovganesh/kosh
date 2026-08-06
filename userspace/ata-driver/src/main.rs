//! The ATA disk driver, in ring 3.
//!
//! `kernel/src/block/ata.rs` does the same job inside the kernel, and this is
//! close to a transcription of it on purpose. A microkernel's claim is that a
//! driver does not need privilege to drive hardware — only *access* to that
//! hardware — and the way to test that claim is to move a driver across the
//! boundary and see what the move costs. The answer here is: two system calls at
//! startup and an IPC round trip per request. The register programming, the
//! status polling and the 256 `in` instructions per sector are identical, and
//! they are ordinary unprivileged instructions that the CPU permits because the
//! TSS I/O permission bitmap has 9 bits cleared for this thread and no others.
//!
//! What it cannot do is exactly what a driver should not be able to do: touch
//! the interrupt controller, the timer, the CMOS or the serial port. `ata0` is a
//! name in a kernel table, and the table decides what ports the name means.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

const SYS_EXIT: u64 = 1;
const SYS_WRITE: u64 = 23;
const SYS_SEND_MESSAGE: u64 = 30;
const SYS_RECEIVE_MESSAGE: u64 = 31;
const SYS_REQUEST_DEVICE: u64 = 44;
const SYS_REGISTER_SERVICE: u64 = 46;

// --- the block protocol ----------------------------------------------------
//
// Fixed-layout little-endian records rather than anything self-describing. Both
// ends are in this repository and a parser is a thing that can be wrong.

const REQ_MAGIC: u32 = 0x4B42_4C4B; // "KBLK"
const REP_MAGIC: u32 = 0x4B52_504C; // "KRPL"

const OP_READ: u32 = 0;
const OP_INFO: u32 = 1;
const OP_SHUTDOWN: u32 = 2;

/// Request: magic, op, lba, count. 24 bytes.
const REQ_BYTES: usize = 24;
/// Reply header: magic, status, len, reserved. 16 bytes, then the payload.
const REP_HEADER: usize = 16;

/// A message is capped at 4096 bytes by the kernel, so a reply carries at most
/// (4096 - 16) / 512 = 7 sectors. Four is the advertised limit, which leaves the
/// header room and makes the arithmetic obvious.
const MAX_SECTORS: usize = 4;
const SECTOR: usize = 512;

const STATUS_OK: i32 = 0;
const STATUS_BAD_REQUEST: i32 = -1;
const STATUS_DEVICE_ERROR: i32 = -2;
const STATUS_TIMEOUT: i32 = -3;
const STATUS_OUT_OF_RANGE: i32 = -4;

// --- syscalls --------------------------------------------------------------

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

fn print_u64(mut value: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    if value == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while value > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    unsafe { syscall3(SYS_WRITE, 1, buf[i..].as_ptr() as u64, (buf.len() - i) as u64) };
}

fn exit(code: u64) -> ! {
    unsafe { syscall3(SYS_EXIT, code, 0, 0) };
    loop {
        core::hint::spin_loop();
    }
}

// --- port I/O --------------------------------------------------------------
//
// The whole point. These are the same instructions the kernel's driver uses, in
// a process with no privilege whatsoever; the CPU checks the bitmap, finds the
// bit clear, and lets them run. Every one of them is a #GP in any other process
// on the system.

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack, preserves_flags));
    value
}

#[inline(always)]
unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    core::arch::asm!("in ax, dx", out("ax") value, in("dx") port, options(nomem, nostack, preserves_flags));
    value
}

// --- the drive -------------------------------------------------------------

const IO_BASE: u16 = 0x1F0;
const CONTROL_BASE: u16 = 0x3F6;

const STATUS_ERR: u8 = 0x01;
const STATUS_DRQ: u8 = 0x08;
const STATUS_DF: u8 = 0x20;
const STATUS_BSY: u8 = 0x80;

const CMD_READ_SECTORS: u8 = 0x20;
const CMD_IDENTIFY: u8 = 0xEC;

const POLL_LIMIT: u32 = 10_000_000;

static mut BLOCKS: u64 = 0;
static mut MODEL: [u8; 41] = [0; 41];

fn status() -> u8 {
    unsafe { inb(IO_BASE + 7) }
}

/// ~400 ns, via four reads of the *alternate* status register. Reading the
/// ordinary one acknowledges a pending interrupt, which a delay should not do.
fn delay_400ns() {
    for _ in 0..4 {
        unsafe {
            let _ = inb(CONTROL_BASE);
        }
    }
}

fn wait_not_busy() -> Result<(), i32> {
    for _ in 0..POLL_LIMIT {
        if status() & STATUS_BSY == 0 {
            return Ok(());
        }
    }
    Err(STATUS_TIMEOUT)
}

fn wait_ready_for_data() -> Result<(), i32> {
    for _ in 0..POLL_LIMIT {
        let s = status();
        if s & (STATUS_ERR | STATUS_DF) != 0 {
            return Err(STATUS_DEVICE_ERROR);
        }
        if s & STATUS_BSY == 0 && s & STATUS_DRQ != 0 {
            return Ok(());
        }
    }
    Err(STATUS_TIMEOUT)
}

/// IDENTIFY the primary master, filling in [`BLOCKS`] and [`MODEL`].
fn identify() -> Result<(), i32> {
    unsafe {
        // A floating bus reads 0xFF: nothing is driving the lines.
        if inb(IO_BASE + 7) == 0xFF {
            return Err(STATUS_DEVICE_ERROR);
        }

        outb(IO_BASE + 6, 0xA0); // master
        outb(IO_BASE + 2, 0);
        outb(IO_BASE + 3, 0);
        outb(IO_BASE + 4, 0);
        outb(IO_BASE + 5, 0);
        outb(IO_BASE + 7, CMD_IDENTIFY);

        if inb(IO_BASE + 7) == 0 {
            return Err(STATUS_DEVICE_ERROR);
        }

        wait_not_busy()?;

        // Non-zero LBA mid/high is an ATAPI or SATA device answering a command
        // it does not implement.
        if inb(IO_BASE + 4) != 0 || inb(IO_BASE + 5) != 0 {
            return Err(STATUS_DEVICE_ERROR);
        }

        wait_ready_for_data()?;

        let mut data = [0u16; 256];
        for word in data.iter_mut() {
            *word = inw(IO_BASE);
        }

        BLOCKS = (data[60] as u64) | ((data[61] as u64) << 16);
        if BLOCKS == 0 {
            return Err(STATUS_DEVICE_ERROR);
        }

        // Words 27..46, byte-swapped within each word.
        for i in 0..20 {
            let w = data[27 + i];
            MODEL[i * 2] = (w >> 8) as u8;
            MODEL[i * 2 + 1] = (w & 0xFF) as u8;
        }
        let mut end = 40;
        while end > 0 && (MODEL[end - 1] == b' ' || MODEL[end - 1] == 0) {
            end -= 1;
        }
        MODEL[end] = 0;
    }

    Ok(())
}

/// Read `count` sectors from `lba` into `out`. One sector per command, as the
/// kernel's driver does, so a mid-transfer failure names a sector.
fn read_sectors(lba: u64, count: usize, out: &mut [u8]) -> Result<(), i32> {
    if unsafe { lba + count as u64 > BLOCKS } {
        return Err(STATUS_OUT_OF_RANGE);
    }

    for sector in 0..count {
        let this = lba + sector as u64;
        wait_not_busy()?;

        unsafe {
            // 0xE0 selects LBA mode; the low nibble is LBA bits 24..27.
            outb(IO_BASE + 6, 0xE0 | (((this >> 24) & 0x0F) as u8));
            outb(IO_BASE + 1, 0); // features
            outb(IO_BASE + 2, 1); // one sector
            outb(IO_BASE + 3, (this & 0xFF) as u8);
            outb(IO_BASE + 4, ((this >> 8) & 0xFF) as u8);
            outb(IO_BASE + 5, ((this >> 16) & 0xFF) as u8);
            delay_400ns();
            outb(IO_BASE + 7, CMD_READ_SECTORS);
        }

        wait_ready_for_data()?;

        let base = sector * SECTOR;
        let mut i = 0;
        while i < SECTOR {
            let word = unsafe { inw(IO_BASE) };
            out[base + i] = (word & 0xFF) as u8;
            out[base + i + 1] = (word >> 8) as u8;
            i += 2;
        }
    }

    Ok(())
}

// --- serving ---------------------------------------------------------------

static mut REQUEST: [u8; 64] = [0; 64];
static mut REPLY: [u8; REP_HEADER + MAX_SECTORS * SECTOR] = [0; REP_HEADER + MAX_SECTORS * SECTOR];

fn read_u32(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

fn read_u64(buf: &[u8], at: usize) -> u64 {
    let mut v = [0u8; 8];
    v.copy_from_slice(&buf[at..at + 8]);
    u64::from_le_bytes(v)
}

fn write_u32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(buf: &mut [u8], at: usize, value: u64) {
    buf[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// Fill the reply header and send `payload_len` bytes after it.
fn reply(to: u32, status: i32, payload_len: usize) {
    unsafe {
        let reply = &mut *core::ptr::addr_of_mut!(REPLY);
        write_u32(reply, 0, REP_MAGIC);
        write_u32(reply, 4, status as u32);
        write_u32(reply, 8, payload_len as u32);
        write_u32(reply, 12, 0);

        syscall3(
            SYS_SEND_MESSAGE,
            to as u64,
            reply.as_ptr() as u64,
            (REP_HEADER + payload_len) as u64,
        );
    }
}

/// Handle one request. Returns false when told to shut down.
fn serve_one() -> bool {
    let received = unsafe {
        let request = &mut *core::ptr::addr_of_mut!(REQUEST);
        syscall3(
            SYS_RECEIVE_MESSAGE,
            request.as_mut_ptr() as u64,
            request.len() as u64,
            1, // blocking: park the thread, do not spin
        )
    };

    if received < 0 {
        print("  ata-driver: receive failed, stopping\n");
        return false;
    }

    let sender = (received as u64 >> 32) as u32;
    let len = (received as u64 & 0xFFFF_FFFF) as usize;

    let request = unsafe { &*core::ptr::addr_of!(REQUEST) };

    if len < REQ_BYTES || read_u32(request, 0) != REQ_MAGIC {
        reply(sender, STATUS_BAD_REQUEST, 0);
        return true;
    }

    let op = read_u32(request, 4);
    let lba = read_u64(request, 8);
    let count = read_u32(request, 16) as usize;

    match op {
        OP_INFO => {
            unsafe {
                let reply_buf = &mut *core::ptr::addr_of_mut!(REPLY);
                write_u64(reply_buf, REP_HEADER, BLOCKS);
                let model = &*core::ptr::addr_of!(MODEL);
                reply_buf[REP_HEADER + 8..REP_HEADER + 8 + 41].copy_from_slice(model);
            }
            reply(sender, STATUS_OK, 8 + 41);
            true
        }
        OP_READ => {
            if count == 0 || count > MAX_SECTORS {
                reply(sender, STATUS_BAD_REQUEST, 0);
                return true;
            }
            let result = unsafe {
                let reply_buf = &mut *core::ptr::addr_of_mut!(REPLY);
                read_sectors(lba, count, &mut reply_buf[REP_HEADER..])
            };
            match result {
                Ok(()) => reply(sender, STATUS_OK, count * SECTOR),
                Err(status) => reply(sender, status, 0),
            }
            true
        }
        OP_SHUTDOWN => {
            reply(sender, STATUS_OK, 0);
            false
        }
        _ => {
            reply(sender, STATUS_BAD_REQUEST, 0);
            true
        }
    }
}

// --- entry -----------------------------------------------------------------

core::arch::global_asm!(
    r#"
.section .text._start, "ax"
.global _start
.type _start, @function
_start:
    xorq    %rbp, %rbp
    andq    $-16, %rsp
    call    ata_driver_main
1:
    jmp     1b
"#,
    options(att_syntax)
);

#[no_mangle]
pub extern "C" fn ata_driver_main() -> ! {
    print("  ata-driver: starting in ring 3\n");

    // The two system calls that make the rest of this program legal. Everything
    // after them is `in` and `out` on a disk, executed by an unprivileged
    // process with its own page tables and no way to reach the kernel's.
    let device = "ata0";
    let granted = unsafe {
        syscall3(
            SYS_REQUEST_DEVICE,
            device.as_ptr() as u64,
            device.len() as u64,
            0,
        )
    };

    if granted < 0 {
        print("  ata-driver: request_device('ata0') was refused\n");
        exit(1);
    }
    print("  ata-driver: got the ata0 ports\n");

    if let Err(_) = identify() {
        print("  ata-driver: IDENTIFY failed\n");
        exit(2);
    }

    print("  ata-driver: IDENTIFY succeeded from ring 3\n");

    // Claim the name clients look this driver up by.
    //
    // After the IDENTIFY, not before: a name in the registry is a promise that
    // requests will be answered, and a driver that registered first and then
    // failed to find a disk would have clients blocking on a server that is
    // about to exit.
    let service = "block";
    if unsafe {
        syscall3(
            SYS_REGISTER_SERVICE,
            service.as_ptr() as u64,
            service.len() as u64,
            0,
        )
    } < 0
    {
        print("  ata-driver: could not register as 'block'\n");
        exit(4);
    }
    print("  ata-driver: registered as the 'block' service\n");

    // Serve until told to stop. `init` sends OP_SHUTDOWN when the shell exits,
    // which releases `ata0` and lets the kernel's own block layer have the disk
    // back — see `platform::devports`.
    while serve_one() {}

    print("  ata-driver: shutting down, releasing ata0\n");
    exit(0)
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print("  ata-driver panic\n");
    exit(3)
}
