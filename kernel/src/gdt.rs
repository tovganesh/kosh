//! GDT and TSS.
//!
//! Split out of `boot.rs` in Phase 5, because the descriptor *order* stops
//! being arbitrary the moment `syscall`/`sysret` exist.
//!
//! ## Why the order is forced
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
//! Before Phase 5 this table had three entries — kernel code, kernel data,
//! TSS — so the `cs: 0x1B / ss: 0x23` values in `process/context.rs` pointed at
//! descriptors that did not exist.

use lazy_static::lazy_static;
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
const KERNEL_INTERRUPT_STACK_SIZE: usize = 4096 * 5;

static mut DOUBLE_FAULT_STACK: [u8; DOUBLE_FAULT_STACK_SIZE] = [0; DOUBLE_FAULT_STACK_SIZE];

/// The stack the CPU switches to when an interrupt arrives while running in
/// ring 3. Without a valid RSP0, the first timer tick after entering user mode
/// is a triple fault.
static mut KERNEL_INTERRUPT_STACK: [u8; KERNEL_INTERRUPT_STACK_SIZE] =
    [0; KERNEL_INTERRUPT_STACK_SIZE];

lazy_static! {
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();

        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            let start = VirtAddr::from_ptr(&raw const DOUBLE_FAULT_STACK);
            start + DOUBLE_FAULT_STACK_SIZE
        };

        // RSP0: ring 3 -> ring 0 stack switch on interrupt.
        tss.privilege_stack_table[0] = {
            let start = VirtAddr::from_ptr(&raw const KERNEL_INTERRUPT_STACK);
            start + KERNEL_INTERRUPT_STACK_SIZE
        };

        tss
    };
}

pub struct Selectors {
    pub kernel_code: SegmentSelector,
    pub kernel_data: SegmentSelector,
    pub user_data: SegmentSelector,
    pub user_code: SegmentSelector,
    pub tss: SegmentSelector,
}

lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();

        // Order is load-bearing — see the module docs.
        let kernel_code = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data = gdt.add_entry(Descriptor::kernel_data_segment());
        let user_data = gdt.add_entry(Descriptor::user_data_segment());
        let user_code = gdt.add_entry(Descriptor::user_code_segment());
        let tss = gdt.add_entry(Descriptor::tss_segment(&TSS));

        (
            gdt,
            Selectors {
                kernel_code,
                kernel_data,
                user_data,
                user_code,
                tss,
            },
        )
    };
}

pub fn selectors() -> &'static Selectors {
    &GDT.1
}

/// Load the GDT, reload every segment register, and install the TSS.
pub fn init() {
    serial_println!("Setting up GDT and TSS...");

    GDT.0.load();

    let sel = selectors();

    unsafe {
        CS::set_reg(sel.kernel_code);
        DS::set_reg(sel.kernel_data);
        ES::set_reg(sel.kernel_data);
        FS::set_reg(sel.kernel_data);
        GS::set_reg(sel.kernel_data);
        SS::set_reg(sel.kernel_data);

        load_tss(sel.tss);
    }

    serial_println!(
        "  selectors: kcode 0x{:x}, kdata 0x{:x}, udata 0x{:x}, ucode 0x{:x}, tss 0x{:x}",
        sel.kernel_code.0,
        sel.kernel_data.0,
        sel.user_data.0,
        sel.user_code.0,
        sel.tss.0
    );
    serial_println!(
        "  TSS.RSP0 = 0x{:x} (ring 3 -> ring 0 interrupt stack)",
        TSS.privilege_stack_table[0].as_u64()
    );
}

/// Point RSP0 at a specific kernel stack.
///
/// Phase 5 runs exactly one ring-3 thread, so the static stack above is enough.
/// Once several threads can be in user mode, this must be called on every
/// context switch with the incoming thread's kernel stack — otherwise two
/// threads share one interrupt stack and corrupt each other's frames.
#[allow(dead_code)]
pub fn set_kernel_stack(_top: VirtAddr) {
    // Deliberately not implemented yet rather than silently doing nothing:
    // the TSS is behind a `lazy_static` and needs interior mutability to
    // update. Phase 6 turns the TSS into a `static mut` per CPU.
    unimplemented!("per-thread RSP0 arrives with multiple user threads");
}
