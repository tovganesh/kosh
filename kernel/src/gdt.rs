//! GDT and TSS.
//!
//! ## Why the descriptor order is forced
//!
//! `syscall` loads CS from `STAR[47:32]` and SS from `STAR[47:32] + 8`.
//! `sysretq` loads CS from `STAR[63:48] + 16` and SS from `STAR[63:48] + 8`.
//! The CPU does not consult the GDT for the *contents* — it just adds those
//! offsets — so the descriptors have to be laid out to match:
//!
//! ```text
//!   0x00  null
//!   0x08  kernel code   <- STAR[47:32]
//!   0x10  kernel data       (= 0x08 + 8)          <- STAR[63:48]
//!   0x18  user data         (= 0x10 + 8)   sysret SS, RPL 3 -> 0x1B
//!   0x20  user code (64)    (= 0x10 + 16)  sysret CS, RPL 3 -> 0x23
//!   0x28  TSS (two slots)
//! ```
//!
//! Note user *data* comes before user *code*. That looks backwards until you
//! write out the `sysret` arithmetic; getting it the intuitive way round is a
//! classic way to land in a #GP the first time a process returns to ring 3.
//!
//! ## Why this is not a `lazy_static`
//!
//! `TSS.RSP0` — the stack the CPU switches to when an interrupt arrives in
//! ring 3 — has to change on every context switch, because it has to name the
//! *incoming* thread's kernel stack. A `lazy_static` gives out `&TSS` and no way
//! to mutate it, which is why [`set_kernel_stack`] used to be
//! `unimplemented!()`. The table and the TSS are now plain statics initialised
//! imperatively, in a defined order.
//!
//! ## The I/O permission bitmap
//!
//! A driver in ring 3 has to execute `in` and `out`. There are three ways to let
//! it, and only one of them is worth having:
//!
//! * **A port-I/O system call.** Safe, and unusably slow: a 512-byte ATA sector
//!   is 256 16-bit port reads, so a one-sector read becomes 256 round trips
//!   through the syscall stub.
//! * **IOPL = 3 in the thread's RFLAGS.** One bit, and it grants *every* port —
//!   the PIC at 0x20, the PIT at 0x40, the CMOS at 0x70, COM1 at 0x3F8. A disk
//!   driver that can mask the timer interrupt is not isolated from the kernel in
//!   any sense that matters.
//! * **The TSS I/O permission bitmap**, which is what this does. With IOPL = 0,
//!   the CPU consults one bit per port, in the TSS, on every `in`/`out`. A
//!   granted port costs nothing at run time; a denied one raises #GP.
//!
//! The bitmap is per-*task* in the hardware's sense, which since we do not use
//! hardware task switching means there is one of it. So it is rewritten on
//! context switch from the incoming thread's grant — see [`set_io_grant`] — the
//! same way `RSP0` is, and for the same reason.
//!
//! `TaskStateSegment` from the `x86_64` crate has an `iomap_base` field but no
//! room after it, and `Descriptor::tss_segment` hard-codes the limit to
//! `size_of::<TaskStateSegment>() - 1`. A bitmap that starts past the segment
//! limit means "deny everything" — which is a perfectly good default and exactly
//! what the stock descriptor gives you, but it is not a bitmap. Hence
//! [`TssWithIoBitmap`] and a descriptor built by hand.

use core::ptr::{addr_of, addr_of_mut};

use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

use crate::serial_println;

/// IST slot for the double-fault handler.
///
/// A double fault caused by a bad kernel stack cannot push its exception frame
/// onto that same stack, so it needs a known-good one.
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

const DOUBLE_FAULT_STACK_SIZE: usize = 4096 * 5;
const BOOT_KERNEL_STACK_SIZE: usize = 4096 * 5;

static mut DOUBLE_FAULT_STACK: [u8; DOUBLE_FAULT_STACK_SIZE] = [0; DOUBLE_FAULT_STACK_SIZE];

/// RSP0 before any thread has claimed it. Only used between `gdt::init` and the
/// first context switch, during which nothing runs in ring 3.
static mut BOOT_KERNEL_STACK: [u8; BOOT_KERNEL_STACK_SIZE] = [0; BOOT_KERNEL_STACK_SIZE];

/// Ports the bitmap covers. Everything a PC's legacy hardware lives at is below
/// 0x400: the PIC (0x20), the PIT (0x40), the keyboard controller (0x60), the
/// CMOS (0x70), the ATA channels (0x1F0, 0x170, 0x3F6, 0x376), VGA (0x3C0) and
/// the serial ports (0x3F8, 0x2F8).
///
/// A port whose bit lies past the segment limit is *denied*, so stopping at 1024
/// rather than 65536 fails closed. 8 KiB of always-ones would be the alternative.
pub const IO_PORT_LIMIT: u16 = 1024;
const IO_BITMAP_BYTES: usize = (IO_PORT_LIMIT as usize) / 8;

/// The TSS with the bitmap laid out immediately after it.
///
/// `packed(4)` matches `TaskStateSegment`'s own representation, so `bitmap`
/// really does start at offset 104 — which is the value written into
/// `iomap_base`, and the CPU will silently read the wrong bytes if the two
/// disagree.
#[repr(C, packed(4))]
struct TssWithIoBitmap {
    tss: TaskStateSegment,
    bitmap: [u8; IO_BITMAP_BYTES],
    /// The terminator byte.
    ///
    /// An `out dx, ax` at port 1023 asks the CPU about ports 1023 and 1024, and
    /// it reads the *pair* of bytes containing them — one byte past the bitmap.
    /// The manual requires that byte to be 0xFF and present within the limit;
    /// without it the CPU would read whatever follows in memory and could allow
    /// an access off the end of the table.
    terminator: u8,
}

static mut TSS_IO: TssWithIoBitmap = TssWithIoBitmap {
    tss: TaskStateSegment::new(),
    // All ones: deny. A thread gets ports only by being handed a grant.
    bitmap: [0xFF; IO_BITMAP_BYTES],
    terminator: 0xFF,
};

static mut GDT: GlobalDescriptorTable = GlobalDescriptorTable::new();

#[derive(Debug, Clone, Copy)]
pub struct Selectors {
    pub kernel_code: SegmentSelector,
    pub kernel_data: SegmentSelector,
    pub user_data: SegmentSelector,
    pub user_code: SegmentSelector,
    pub tss: SegmentSelector,
}

static mut SELECTORS: Option<Selectors> = None;

pub fn selectors() -> Selectors {
    unsafe { (*addr_of!(SELECTORS)).expect("gdt::init has not run") }
}

/// Build the TSS and GDT, load them, and reload every segment register.
pub fn init() {
    serial_println!("Setting up GDT and TSS...");

    unsafe {
        let tss = &mut (*addr_of_mut!(TSS_IO)).tss;

        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            let start = VirtAddr::from_ptr(addr_of!(DOUBLE_FAULT_STACK));
            start + DOUBLE_FAULT_STACK_SIZE
        };

        tss.privilege_stack_table[0] = {
            let start = VirtAddr::from_ptr(addr_of!(BOOT_KERNEL_STACK));
            start + BOOT_KERNEL_STACK_SIZE
        };

        // Offset of `bitmap` within the segment. `TaskStateSegment` is 104 bytes
        // and `packed(4)` adds no padding, so this is 104 — computed rather than
        // written out, because a hard-coded 104 that stops being true produces a
        // bitmap the CPU reads from the wrong place and no diagnostic at all.
        tss.iomap_base = core::mem::size_of::<TaskStateSegment>() as u16;

        let gdt = &mut *addr_of_mut!(GDT);

        // Order is load-bearing — see the module docs.
        let kernel_code = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data = gdt.add_entry(Descriptor::kernel_data_segment());
        let user_data = gdt.add_entry(Descriptor::user_data_segment());
        let user_code = gdt.add_entry(Descriptor::user_code_segment());
        let tss_sel = gdt.add_entry(tss_descriptor_with_bitmap());

        let selectors = Selectors {
            kernel_code,
            kernel_data,
            user_data,
            user_code,
            tss: tss_sel,
        };
        *addr_of_mut!(SELECTORS) = Some(selectors);

        gdt.load();

        CS::set_reg(kernel_code);
        DS::set_reg(kernel_data);
        ES::set_reg(kernel_data);
        FS::set_reg(kernel_data);
        GS::set_reg(kernel_data);
        SS::set_reg(kernel_data);

        load_tss(tss_sel);

        serial_println!(
            "  selectors: kcode 0x{:x}, kdata 0x{:x}, udata 0x{:x}, ucode 0x{:x}, tss 0x{:x}",
            kernel_code.0,
            kernel_data.0,
            user_data.0,
            user_code.0,
            tss_sel.0
        );
        serial_println!(
            "  TSS.RSP0 = 0x{:x} (boot stack; per-thread from the first switch)",
            tss.privilege_stack_table[0].as_u64()
        );
        serial_println!(
            "  I/O bitmap at TSS+{}, {} bytes, ports 0..{} all denied",
            tss.iomap_base,
            IO_BITMAP_BYTES,
            IO_PORT_LIMIT - 1
        );
    }
}

/// The TSS descriptor, with the limit stretched over the bitmap.
///
/// `Descriptor::tss_segment` cannot be used: it computes the limit from
/// `size_of::<TaskStateSegment>()`, which stops just before `iomap_base` points.
/// The failure mode is not a crash — a bitmap outside the limit reads as all
/// ones, so every port is denied and a driver simply never works.
fn tss_descriptor_with_bitmap() -> Descriptor {
    let ptr = addr_of!(TSS_IO) as u64;
    // Inclusive bound, hence the -1.
    let limit = (core::mem::size_of::<TssWithIoBitmap>() - 1) as u64;

    let low = (1u64 << 47)                        // present
        | (0b1001u64 << 40)                       // type: available 64-bit TSS
        | ((ptr & 0x00FF_FFFF) << 16)             // base 0..24
        | (((ptr >> 24) & 0xFF) << 56)            // base 24..32
        | (limit & 0xFFFF);                       // limit 0..16
    let high = ptr >> 32; // base 32..64

    Descriptor::SystemSegment(low, high)
}

/// Point RSP0 at `top`.
///
/// Called on every context switch with the incoming thread's kernel stack.
/// Without this, two threads in ring 3 would share one interrupt stack and
/// corrupt each other's exception frames the moment both were interrupted.
///
/// Note that the same stack serves a thread's syscalls and its ring-3
/// interrupts. That is safe because RSP0 is only consumed when the CPU switches
/// *from* ring 3: if the thread is already in the kernel, an interrupt stays on
/// the stack it is already using, and the syscall frame is not at risk.
pub fn set_kernel_stack(top: VirtAddr) {
    unsafe {
        (*addr_of_mut!(TSS_IO)).tss.privilege_stack_table[0] = top;
    }
}

/// Current RSP0, for diagnostics.
pub fn kernel_stack() -> VirtAddr {
    unsafe { (*addr_of!(TSS_IO)).tss.privilege_stack_table[0] }
}

/// Grant currently installed in the bitmap.
///
/// Cached so the common switch — between threads that hold no ports, which is
/// all of them but the driver — touches nothing. Rewriting 128 bytes on every
/// tick would work and would be silly.
static mut INSTALLED_IO_GRANT: u32 = 0;

/// Write `grant`'s ports into the bitmap, denying everything else.
///
/// Called from the scheduler with the incoming thread's grant, alongside
/// `set_kernel_stack`. The two failures are symmetrical: forget `set_kernel_stack`
/// and a thread lands on another's kernel stack; forget this and a thread
/// inherits another's hardware.
///
/// # Safety
/// Must be called with interrupts disabled — it mutates a table the CPU reads.
pub unsafe fn set_io_grant(grant: u32) {
    if INSTALLED_IO_GRANT == grant {
        return;
    }

    let tss_io = &mut *addr_of_mut!(TSS_IO);
    tss_io.bitmap.fill(0xFF);

    for (base, len) in crate::platform::devports::ports_for_grant(grant) {
        for port in base..base.saturating_add(len) {
            if port >= IO_PORT_LIMIT {
                continue;
            }
            tss_io.bitmap[(port / 8) as usize] &= !(1u8 << (port % 8));
        }
    }

    INSTALLED_IO_GRANT = grant;
}

/// Whether port `port` is currently permitted in ring 3. Diagnostics only.
pub fn io_port_allowed(port: u16) -> bool {
    if port >= IO_PORT_LIMIT {
        return false;
    }
    unsafe { (*addr_of!(TSS_IO)).bitmap[(port / 8) as usize] & (1u8 << (port % 8)) == 0 }
}

/// Top of the stack `init` installed as RSP0.
///
/// `task::init` adopts this as thread 0's kernel stack, so that the bootstrap
/// context has a real answer for `gs:0` rather than a zero.
pub fn boot_kernel_stack_top() -> VirtAddr {
    let start = VirtAddr::from_ptr(unsafe { addr_of!(BOOT_KERNEL_STACK) });
    (start + BOOT_KERNEL_STACK_SIZE).align_down(16u64)
}
