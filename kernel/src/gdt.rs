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

static mut TSS: TaskStateSegment = TaskStateSegment::new();
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
        let tss = &mut *addr_of_mut!(TSS);

        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            let start = VirtAddr::from_ptr(addr_of!(DOUBLE_FAULT_STACK));
            start + DOUBLE_FAULT_STACK_SIZE
        };

        tss.privilege_stack_table[0] = {
            let start = VirtAddr::from_ptr(addr_of!(BOOT_KERNEL_STACK));
            start + BOOT_KERNEL_STACK_SIZE
        };

        let gdt = &mut *addr_of_mut!(GDT);

        // Order is load-bearing — see the module docs.
        let kernel_code = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data = gdt.add_entry(Descriptor::kernel_data_segment());
        let user_data = gdt.add_entry(Descriptor::user_data_segment());
        let user_code = gdt.add_entry(Descriptor::user_code_segment());
        let tss_sel = gdt.add_entry(Descriptor::tss_segment(&*addr_of!(TSS)));

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
    }
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
        (*addr_of_mut!(TSS)).privilege_stack_table[0] = top;
    }
}

/// Current RSP0, for diagnostics.
pub fn kernel_stack() -> VirtAddr {
    unsafe { (*addr_of!(TSS)).privilege_stack_table[0] }
}

/// Top of the stack `init` installed as RSP0.
///
/// `task::init` adopts this as thread 0's kernel stack, so that the bootstrap
/// context has a real answer for `gs:0` rather than a zero.
pub fn boot_kernel_stack_top() -> VirtAddr {
    let start = VirtAddr::from_ptr(unsafe { addr_of!(BOOT_KERNEL_STACK) });
    (start + BOOT_KERNEL_STACK_SIZE).align_down(16u64)
}
