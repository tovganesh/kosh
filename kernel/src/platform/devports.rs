//! Which ports a named device is.
//!
//! The `SYS_REQUEST_DEVICE` system call takes a *device name*, not a port range.
//! That is the whole point: a driver that could ask for arbitrary ports could
//! ask for 0x20 and mask the timer, or 0x64 and reset the CPU through the
//! keyboard controller. Naming a device means the kernel decides what that
//! device consists of, and the answer lives here, in one table, where it can be
//! read.
//!
//! The table is deliberately short. Every entry is a promise that handing those
//! ports to an unprivileged process is survivable, and that is a claim worth
//! making one device at a time.

use spin::Mutex;

use crate::serial_println;

/// A contiguous run of ports: `(base, count)`.
pub type PortRange = (u16, u16);

/// Ports that make up a device, and who may currently drive it.
pub struct Device {
    pub name: &'static str,
    /// At most two runs — a command block and a control block, which is the
    /// shape of every legacy device that is split at all.
    pub ranges: [Option<PortRange>; 2],
    /// The PIC line this device raises, if it has one.
    ///
    /// Part of the device rather than a separate grant, for the same reason the
    /// ports are: a driver names a device, and the kernel decides what that
    /// means. A driver that could name an arbitrary IRQ could wait on the timer
    /// line and watch the scheduler.
    pub irq: Option<u8>,
    pub description: &'static str,
}

/// The devices a ring-3 driver may be given.
///
/// Notably absent: the PIC (0x20/0xA0), the PIT (0x40), the keyboard controller
/// (0x60/0x64), the CMOS/NMI gate (0x70) and COM1 (0x3F8). The first three can
/// stop the scheduler, the fourth can mask NMIs, and the last is where every
/// diagnostic in this system goes. None of them are things a disk driver needs,
/// so none of them are in the table, so no capability can name them.
pub static DEVICES: &[Device] = &[
    Device {
        name: "ata0",
        // 0x1F0..0x1F7 is the command block; 0x3F6 is the device control and
        // alternate status register, which is a *separate* range on the ISA bus
        // and is why this table has room for two.
        ranges: [Some((0x1F0, 8)), Some((0x3F6, 1))],
        irq: Some(14),
        description: "primary IDE channel",
    },
    Device {
        name: "ata1",
        ranges: [Some((0x170, 8)), Some((0x376, 1))],
        // Masked at the PIC, so waiting on it would block until the deadline
        // every time. Named anyway, because the table is the description of the
        // device and not of what the kernel currently bothers to deliver.
        irq: Some(15),
        description: "secondary IDE channel",
    },
];

/// Index of a device by name.
pub fn index_of(name: &str) -> Option<usize> {
    DEVICES.iter().position(|d| d.name == name)
}

/// A grant is a bitmask of [`DEVICES`] indices. `0` means no ports at all, which
/// is what every thread starts with.
pub const NO_GRANT: u32 = 0;

pub fn grant_bit(index: usize) -> u32 {
    1u32 << index
}

/// Every port range a grant covers, for the bitmap writer.
pub fn ports_for_grant(grant: u32) -> impl Iterator<Item = PortRange> {
    DEVICES
        .iter()
        .enumerate()
        .filter(move |(i, _)| grant & grant_bit(*i) != 0)
        .flat_map(|(_, d)| d.ranges.iter().flatten().copied())
}

/// Whether `grant` includes a device that raises `irq`.
///
/// This is the whole of the IRQ permission check: a thread may wait on a line
/// only if it holds the device that raises it. There is no separate "IRQ
/// capability", because an interrupt is part of a device and granting them
/// separately would mean the two could disagree.
pub fn grant_covers_irq(grant: u32, irq: u8) -> bool {
    DEVICES
        .iter()
        .enumerate()
        .any(|(i, d)| grant & grant_bit(i) != 0 && d.irq == Some(irq))
}

/// Human-readable form of a grant, for the log.
pub fn describe_grant(grant: u32) -> &'static str {
    if grant == NO_GRANT {
        return "none";
    }
    // One name, because nothing yet holds two devices; when something does this
    // wants to be a formatter, not a lookup.
    DEVICES
        .iter()
        .enumerate()
        .find(|(i, _)| grant & grant_bit(*i) != 0)
        .map(|(_, d)| d.name)
        .unwrap_or("?")
}

// ---------------------------------------------------------------------------
// Claims
// ---------------------------------------------------------------------------

/// Which thread, if any, currently drives each device.
///
/// This exists because the kernel still contains an ATA driver of its own, at
/// `block/ata.rs`, which `fs/fat32.rs` reads through. Two drivers polling the
/// same status register interleave: one writes the LBA registers, the other
/// issues its command, and both then read the wrong sector — intermittently, and
/// only when both happen to be active.
///
/// So a device has one driver at a time. While a ring-3 process holds `ata0`,
/// the kernel's own block layer refuses that channel; when the driver exits, the
/// claim is released and the kernel can read the disk again. That is a temporary
/// arrangement and it is visible in the log rather than assumed: the alternative
/// to two drivers coexisting badly is one of them going away, which is what the
/// next phase does to the in-kernel one.
static CLAIMS: Mutex<[Option<usize>; 4]> = Mutex::new([None; 4]);

/// Give `thread` exclusive use of device `index`.
pub fn claim(index: usize, thread: usize) -> Result<(), &'static str> {
    let mut claims = CLAIMS.lock();
    let Some(slot) = claims.get_mut(index) else {
        return Err("no such device");
    };
    match *slot {
        Some(owner) if owner != thread => Err("device already claimed"),
        _ => {
            *slot = Some(thread);
            Ok(())
        }
    }
}

/// Drop every claim held by `thread`. Called when a thread exits, so a driver
/// that crashes does not take its disk with it.
pub fn release_all(thread: usize) {
    let mut claims = CLAIMS.lock();
    for (i, slot) in claims.iter_mut().enumerate() {
        if *slot == Some(thread) {
            *slot = None;
            serial_println!(
                "  released device '{}' held by thread {}",
                DEVICES.get(i).map(|d| d.name).unwrap_or("?"),
                thread
            );
        }
    }
}

/// Whether a named device is currently driven from ring 3.
///
/// The kernel's own drivers consult this before touching hardware.
pub fn is_claimed(name: &str) -> bool {
    match index_of(name) {
        Some(i) => CLAIMS.lock().get(i).copied().flatten().is_some(),
        None => false,
    }
}
