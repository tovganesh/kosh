//! Block devices.
//!
//! A filesystem should not know what kind of disk it is sitting on, and a disk
//! driver should not know what is stored on it. [`BlockDevice`] is the seam.
//!
//! `drivers/storage` already declared a `KoshDriver` trait for this, but its
//! `init` set a boolean, its `handle_request` returned an empty success, and the
//! whole file was 76 lines with no port I/O and no PCI. This is the first code
//! in the project that actually moves bytes off a disk.

pub mod ata;

use alloc::boxed::Box;
use spin::Mutex;

/// Bytes per block. 512 everywhere this kernel cares about.
pub const BLOCK_SIZE: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockError {
    /// No device present at that address.
    NoDevice,
    /// The drive reported an error for this request.
    DeviceError,
    /// The drive did not become ready in time.
    Timeout,
    /// Request runs past the end of the device.
    OutOfRange,
    /// Buffer length is not a whole number of blocks.
    BadBufferSize,
    /// The device is read-only.
    ReadOnly,
}

pub trait BlockDevice: Send + Sync {
    /// Human-readable identification, for `lsblk`-ish output.
    fn name(&self) -> &str;

    /// Total addressable blocks.
    fn block_count(&self) -> u64;

    /// Read `buf.len() / BLOCK_SIZE` blocks starting at `lba`.
    fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError>;

    /// Write blocks. Default implementation refuses, so a read-only device does
    /// not have to pretend.
    fn write_blocks(&self, _lba: u64, _buf: &[u8]) -> Result<(), BlockError> {
        Err(BlockError::ReadOnly)
    }

    /// Convenience: read exactly one block.
    fn read_block(&self, lba: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), BlockError> {
        self.read_blocks(lba, buf)
    }
}

/// The system's block device. One is enough for now; a real device list arrives
/// with PCI enumeration.
static DEVICE: Mutex<Option<Box<dyn BlockDevice>>> = Mutex::new(None);

pub fn register(device: Box<dyn BlockDevice>) {
    *DEVICE.lock() = Some(device);
}

pub fn is_present() -> bool {
    DEVICE.lock().is_some()
}

/// Run `f` against the registered device, if there is one.
pub fn with_device<T>(f: impl FnOnce(&dyn BlockDevice) -> T) -> Option<T> {
    let guard = DEVICE.lock();
    guard.as_ref().map(|d| f(d.as_ref()))
}

/// Read one block from the registered device.
pub fn read_block(lba: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), BlockError> {
    with_device(|d| d.read_block(lba, buf)).unwrap_or(Err(BlockError::NoDevice))
}

/// Read a run of blocks from the registered device.
pub fn read_blocks(lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
    with_device(|d| d.read_blocks(lba, buf)).unwrap_or(Err(BlockError::NoDevice))
}

/// Probe for disks and register the first one found.
pub fn init() {
    crate::serial_println!("Probing for block devices...");

    match ata::AtaDisk::probe(ata::Channel::Primary, ata::Drive::Master) {
        Ok(disk) => {
            crate::serial_println!(
                "  {}: {} blocks ({} MB), {}",
                disk.name(),
                disk.block_count(),
                disk.block_count() * BLOCK_SIZE as u64 / (1024 * 1024),
                disk.model()
            );
            register(Box::new(disk));
        }
        Err(e) => {
            crate::serial_println!("  no ATA disk on the primary channel ({:?})", e);
            crate::serial_println!("  (attach one with -drive file=disk.img,format=raw,if=ide,index=0,media=disk)");
        }
    }
}
