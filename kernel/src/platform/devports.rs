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
        description: "primary IDE channel",
    },
    Device {
        name: "ata1",
        ranges: [Some((0x170, 8)), Some((0x376, 1))],
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
