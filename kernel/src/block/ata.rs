//! ATA PIO driver for the legacy IDE interface.
//!
//! Polled PIO rather than DMA, and the fixed legacy port addresses rather than
//! PCI enumeration. Both are deliberate: it is the smallest thing that really
//! reads a disk, it needs no PCI, no interrupt plumbing and no bus-master
//! buffers, and every emulator and every PC chipset since 1990 supports it.
//!
//! It is also slow — one 16-bit port read per two bytes, with the CPU spinning
//! on a status register. That is fine for reading a filesystem at boot and
//! wrong for anything performance-sensitive, which is what DMA and a real
//! virtio-blk driver are for later.
//!
//! LBA28 addressing, so 128 GiB is the ceiling. LBA48 is a different command
//! and an extra round of register writes; nothing here needs it yet.

use x86_64::instructions::port::{Port, PortReadOnly, PortWriteOnly};

use super::{BlockDevice, BlockError, BLOCK_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Primary,
    Secondary,
}

impl Channel {
    /// Base of the command register block.
    const fn io_base(self) -> u16 {
        match self {
            Channel::Primary => 0x1F0,
            Channel::Secondary => 0x170,
        }
    }

    /// Base of the control register block, which holds the alternate status
    /// register. Reading *that* does not acknowledge an interrupt, which is why
    /// it is the one to use for delays.
    const fn control_base(self) -> u16 {
        match self {
            Channel::Primary => 0x3F6,
            Channel::Secondary => 0x376,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drive {
    Master,
    Slave,
}

impl Drive {
    const fn select_bit(self) -> u8 {
        match self {
            Drive::Master => 0x00,
            Drive::Slave => 0x10,
        }
    }
}

// Status register bits.
const STATUS_ERR: u8 = 0x01;
const STATUS_DRQ: u8 = 0x08;
const STATUS_DF: u8 = 0x20;
const STATUS_DRDY: u8 = 0x40;
const STATUS_BSY: u8 = 0x80;

const CMD_READ_SECTORS: u8 = 0x20;
const CMD_WRITE_SECTORS: u8 = 0x30;
const CMD_FLUSH_CACHE: u8 = 0xE7;
const CMD_IDENTIFY: u8 = 0xEC;

/// Spin limit while waiting on the drive. Generous: a real spinning disk can
/// take tens of milliseconds to answer, and this loop is only entered at boot
/// and during filesystem reads.
const POLL_LIMIT: u32 = 10_000_000;

pub struct AtaDisk {
    channel: Channel,
    drive: Drive,
    blocks: u64,
    model: [u8; 41],
    name: [u8; 8],
}

impl Channel {
    /// The name this channel goes by in `platform::devports`.
    const fn device_name(self) -> &'static str {
        match self {
            Channel::Primary => "ata0",
            Channel::Secondary => "ata1",
        }
    }
}

impl AtaDisk {
    /// Refuse if a ring-3 driver is currently driving this channel.
    ///
    /// The two drivers share one set of registers and neither can see the
    /// other's half-issued command: this one writes the LBA registers, the
    /// user-space one writes its own, and the command that follows reads
    /// whichever sector the last writer named. It is not a race that fails
    /// loudly — it silently returns the wrong sector's bytes.
    ///
    /// So the claim is checked on every entry point rather than once at probe
    /// time. A driver can be started and can exit while the system runs, and
    /// `fat32` holds an `AtaDisk` across all of it.
    fn check_not_claimed(channel: Channel) -> Result<(), BlockError> {
        if crate::platform::devports::is_claimed(channel.device_name()) {
            Err(BlockError::ClaimedByUserspace)
        } else {
            Ok(())
        }
    }

    /// Look for a drive and read its parameters with IDENTIFY.
    pub fn probe(channel: Channel, drive: Drive) -> Result<Self, BlockError> {
        Self::check_not_claimed(channel)?;
        let io = channel.io_base();

        unsafe {
            // A floating bus reads back 0xFF: nothing is driving the lines, so
            // there is no controller here at all. Checking this first avoids
            // spinning POLL_LIMIT times on a channel that does not exist.
            let status: u8 = PortReadOnly::<u8>::new(io + 7).read();
            if status == 0xFF {
                return Err(BlockError::NoDevice);
            }

            // Select the drive, and zero the LBA registers — a drive that is
            // present will leave them zero, while an ATAPI device signs them.
            PortWriteOnly::<u8>::new(io + 6).write(0xA0 | drive.select_bit());
            PortWriteOnly::<u8>::new(io + 2).write(0);
            PortWriteOnly::<u8>::new(io + 3).write(0);
            PortWriteOnly::<u8>::new(io + 4).write(0);
            PortWriteOnly::<u8>::new(io + 5).write(0);

            PortWriteOnly::<u8>::new(io + 7).write(CMD_IDENTIFY);

            // Status 0 after IDENTIFY means no drive.
            let status: u8 = PortReadOnly::<u8>::new(io + 7).read();
            if status == 0 {
                return Err(BlockError::NoDevice);
            }

            Self::wait_not_busy(channel)?;

            // Non-zero LBA mid/high here means an ATAPI or SATA device replying
            // to a command it does not implement. Reading it as a disk would
            // produce garbage.
            let lba_mid: u8 = PortReadOnly::<u8>::new(io + 4).read();
            let lba_high: u8 = PortReadOnly::<u8>::new(io + 5).read();
            if lba_mid != 0 || lba_high != 0 {
                return Err(BlockError::NoDevice);
            }

            Self::wait_ready_for_data(channel)?;

            // IDENTIFY returns 256 little-endian words.
            let mut data = [0u16; 256];
            let mut port: Port<u16> = Port::new(io);
            for word in data.iter_mut() {
                *word = port.read();
            }

            // Words 60..61 hold the LBA28 sector count.
            let blocks = (data[60] as u64) | ((data[61] as u64) << 16);
            if blocks == 0 {
                return Err(BlockError::NoDevice);
            }

            // Words 27..46 hold the model string, byte-swapped within each word.
            let mut model = [0u8; 41];
            for i in 0..20 {
                let w = data[27 + i];
                model[i * 2] = (w >> 8) as u8;
                model[i * 2 + 1] = (w & 0xFF) as u8;
            }
            // Trim the trailing padding the standard mandates.
            let mut end = 40;
            while end > 0 && (model[end - 1] == b' ' || model[end - 1] == 0) {
                end -= 1;
            }
            model[end] = 0;

            let name = match (channel, drive) {
                (Channel::Primary, Drive::Master) => *b"hda\0\0\0\0\0",
                (Channel::Primary, Drive::Slave) => *b"hdb\0\0\0\0\0",
                (Channel::Secondary, Drive::Master) => *b"hdc\0\0\0\0\0",
                (Channel::Secondary, Drive::Slave) => *b"hdd\0\0\0\0\0",
            };

            Ok(Self {
                channel,
                drive,
                blocks,
                model,
                name,
            })
        }
    }

    pub fn model(&self) -> &str {
        let end = self.model.iter().position(|&b| b == 0).unwrap_or(40);
        core::str::from_utf8(&self.model[..end]).unwrap_or("<unreadable>")
    }

    /// Read the alternate status register four times: ~400 ns, which is what the
    /// specification requires between selecting a drive and trusting its status.
    ///
    /// The *alternate* status register specifically — reading the ordinary one
    /// acknowledges a pending interrupt, which is not what a delay should do.
    fn delay_400ns(channel: Channel) {
        let mut port: PortReadOnly<u8> = PortReadOnly::new(channel.control_base());
        for _ in 0..4 {
            unsafe {
                let _ = port.read();
            }
        }
    }

    fn status(channel: Channel) -> u8 {
        unsafe {
            let mut port: PortReadOnly<u8> = PortReadOnly::new(channel.io_base() + 7);
            port.read()
        }
    }

    fn wait_not_busy(channel: Channel) -> Result<(), BlockError> {
        for _ in 0..POLL_LIMIT {
            let status = Self::status(channel);
            if status & STATUS_BSY == 0 {
                return Ok(());
            }
        }
        Err(BlockError::Timeout)
    }

    /// Wait until the drive has data for us, distinguishing "not yet" from
    /// "failed". Returning Timeout for a drive that has actually set ERR would
    /// send whoever is debugging in the wrong direction.
    fn wait_ready_for_data(channel: Channel) -> Result<(), BlockError> {
        for _ in 0..POLL_LIMIT {
            let status = Self::status(channel);

            if status & (STATUS_ERR | STATUS_DF) != 0 {
                return Err(BlockError::DeviceError);
            }
            if status & STATUS_BSY == 0 && status & STATUS_DRQ != 0 {
                return Ok(());
            }
        }
        Err(BlockError::Timeout)
    }

    fn wait_ready(channel: Channel) -> Result<(), BlockError> {
        for _ in 0..POLL_LIMIT {
            let status = Self::status(channel);

            if status & (STATUS_ERR | STATUS_DF) != 0 {
                return Err(BlockError::DeviceError);
            }
            if status & STATUS_BSY == 0 && status & STATUS_DRDY != 0 {
                return Ok(());
            }
        }
        Err(BlockError::Timeout)
    }

    /// Program the LBA registers and issue `command` for `count` sectors.
    fn issue(&self, lba: u64, count: u8, command: u8) -> Result<(), BlockError> {
        let io = self.channel.io_base();

        Self::wait_not_busy(self.channel)?;

        unsafe {
            // 0xE0 selects LBA mode; the low nibble carries LBA bits 24..27.
            PortWriteOnly::<u8>::new(io + 6)
                .write(0xE0 | self.drive.select_bit() | (((lba >> 24) & 0x0F) as u8));
            PortWriteOnly::<u8>::new(io + 1).write(0); // features
            PortWriteOnly::<u8>::new(io + 2).write(count);
            PortWriteOnly::<u8>::new(io + 3).write((lba & 0xFF) as u8);
            PortWriteOnly::<u8>::new(io + 4).write(((lba >> 8) & 0xFF) as u8);
            PortWriteOnly::<u8>::new(io + 5).write(((lba >> 16) & 0xFF) as u8);

            Self::delay_400ns(self.channel);
            PortWriteOnly::<u8>::new(io + 7).write(command);
        }

        Ok(())
    }
}

impl BlockDevice for AtaDisk {
    fn name(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(3);
        core::str::from_utf8(&self.name[..end]).unwrap_or("hd?")
    }

    fn block_count(&self) -> u64 {
        self.blocks
    }

    fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        Self::check_not_claimed(self.channel)?;
        if buf.len() % BLOCK_SIZE != 0 {
            return Err(BlockError::BadBufferSize);
        }
        let sectors = buf.len() / BLOCK_SIZE;
        if sectors == 0 {
            return Ok(());
        }
        if lba + sectors as u64 > self.blocks {
            return Err(BlockError::OutOfRange);
        }

        // One sector at a time. A multi-sector transfer is faster, but it makes
        // error recovery ambiguous — the drive can fail partway and there is no
        // clean way to say which sector — and correctness matters more here.
        for (i, chunk) in buf.chunks_mut(BLOCK_SIZE).enumerate() {
            self.issue(lba + i as u64, 1, CMD_READ_SECTORS)?;
            Self::wait_ready_for_data(self.channel)?;

            let mut port: Port<u16> = Port::new(self.channel.io_base());
            for pair in chunk.chunks_mut(2) {
                let word = unsafe { port.read() };
                pair[0] = (word & 0xFF) as u8;
                pair[1] = (word >> 8) as u8;
            }
        }

        Ok(())
    }

    fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<(), BlockError> {
        Self::check_not_claimed(self.channel)?;
        if buf.len() % BLOCK_SIZE != 0 {
            return Err(BlockError::BadBufferSize);
        }
        let sectors = buf.len() / BLOCK_SIZE;
        if sectors == 0 {
            return Ok(());
        }
        if lba + sectors as u64 > self.blocks {
            return Err(BlockError::OutOfRange);
        }

        for (i, chunk) in buf.chunks(BLOCK_SIZE).enumerate() {
            self.issue(lba + i as u64, 1, CMD_WRITE_SECTORS)?;
            Self::wait_ready_for_data(self.channel)?;

            let mut port: Port<u16> = Port::new(self.channel.io_base());
            for pair in chunk.chunks(2) {
                let word = (pair[0] as u16) | ((pair[1] as u16) << 8);
                unsafe { port.write(word) };
            }
        }

        // Without a cache flush the drive may acknowledge writes it has not
        // committed, and a reset loses them.
        self.issue(lba, 1, CMD_FLUSH_CACHE)?;
        Self::wait_ready(self.channel)?;

        Ok(())
    }
}
