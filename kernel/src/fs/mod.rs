//! The filesystem layer.
//!
//! One mounted volume, reached through a global. That is a deliberate limit
//! rather than a design: a mount table belongs with a VFS, and
//! `userspace/fs-service/src/vfs.rs` already has one — but it has never had a
//! real filesystem underneath it, so wiring the two together is a separate
//! piece of work from making FAT32 correct.
//!
//! Paths here are absolute and `/`-separated. Resolution of `.` and `..` and of
//! relative paths happens above this layer, in the console.

pub mod fat32;

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use fat32::{DirEntry, Fat32, FsError};

static MOUNTED: Mutex<Option<Fat32>> = Mutex::new(None);

/// Try to mount a filesystem from the registered block device.
pub fn init() {
    use crate::serial_println;

    if !crate::block::is_present() {
        serial_println!("Filesystem: no block device, nothing to mount");
        return;
    }

    serial_println!("Mounting filesystem...");

    // The image is a bare filesystem, not a partitioned disk, so the BPB is at
    // LBA 0. A partition table would mean reading the MBR first and mounting at
    // the partition's start sector.
    match Fat32::mount(0) {
        Ok(fs) => {
            serial_println!("  {}", fs.describe());
            *MOUNTED.lock() = Some(fs);
            selftest();
        }
        Err(e) => {
            serial_println!("  no FAT32 filesystem at LBA 0: {:?}", e);
        }
    }
}

pub fn is_mounted() -> bool {
    MOUNTED.lock().is_some()
}

/// Run `f` against the mounted filesystem.
fn with_fs<T>(f: impl FnOnce(&Fat32) -> Result<T, FsError>) -> Result<T, FsError> {
    let guard = MOUNTED.lock();
    match guard.as_ref() {
        Some(fs) => f(fs),
        None => Err(FsError::NotFound),
    }
}

/// Describe the mounted volume.
pub fn describe() -> Option<String> {
    MOUNTED.lock().as_ref().map(|fs| fs.describe())
}

pub fn label() -> Option<String> {
    MOUNTED.lock().as_ref().map(|fs| String::from(fs.label()))
}

/// List a directory by absolute path.
pub fn read_dir(path: &str) -> Result<Vec<DirEntry>, FsError> {
    with_fs(|fs| {
        let entry = fs.lookup(path)?;
        if !entry.is_dir {
            return Err(FsError::NotADirectory);
        }
        fs.read_dir(entry.first_cluster)
    })
}

/// Look up a path.
pub fn lookup(path: &str) -> Result<DirEntry, FsError> {
    with_fs(|fs| fs.lookup(path))
}

/// Read from an already-resolved entry at an offset. This is what file
/// descriptors use, so a sequential read does not re-read the whole prefix.
pub fn read_at(entry: &DirEntry, offset: u32, buf: &mut [u8]) -> Result<usize, FsError> {
    with_fs(|fs| fs.read_at(entry, offset, buf))
}

/// Read a file by absolute path, up to `limit` bytes.
pub fn read_file(path: &str, limit: usize) -> Result<Vec<u8>, FsError> {
    with_fs(|fs| {
        let entry = fs.lookup(path)?;
        fs.read_file(&entry, limit)
    })
}

/// Check the mount against facts the build script guarantees.
///
/// `scripts/run.sh` builds the test image with a known label and known files, so
/// this can assert against real expected values rather than just "no error was
/// returned". A filesystem that returns an empty root without complaining would
/// otherwise look like a pass.
fn selftest() {
    use crate::serial_println;

    serial_println!("Filesystem self-test:");

    let entries = match read_dir("/") {
        Ok(e) => e,
        Err(e) => {
            serial_println!("Filesystem: FAIL — cannot read the root directory: {:?}", e);
            return;
        }
    };

    serial_println!("  root has {} entries:", entries.len());
    for entry in entries.iter() {
        serial_println!(
            "    {:<14} {:>8} {}",
            entry.name,
            entry.size,
            if entry.is_dir { "<DIR>" } else { "" }
        );
    }

    let mut failures = 0;

    // A file whose contents the build script controls.
    match read_file("/HELLO.TXT", 4096) {
        Ok(bytes) => {
            let text = core::str::from_utf8(&bytes).unwrap_or("<not utf-8>");
            if text.trim() == "hello from the filesystem" {
                serial_println!("  PASS read /HELLO.TXT: '{}'", text.trim());
            } else {
                serial_println!("  FAIL /HELLO.TXT contains '{}'", text.trim());
                failures += 1;
            }
        }
        Err(e) => {
            serial_println!("  FAIL cannot read /HELLO.TXT: {:?}", e);
            failures += 1;
        }
    }

    // A file larger than one cluster, so the FAT chain has to be walked.
    match lookup("/BIG.TXT") {
        Ok(entry) => match read_file("/BIG.TXT", 1 << 20) {
            Ok(bytes) => {
                let lines = bytes.iter().filter(|&&b| b == b'\n').count();
                if bytes.len() == entry.size as usize && lines == 400 {
                    serial_println!(
                        "  PASS read /BIG.TXT: {} bytes, {} lines across the cluster chain",
                        bytes.len(),
                        lines
                    );
                } else {
                    serial_println!(
                        "  FAIL /BIG.TXT: {} bytes ({} expected), {} lines (400 expected)",
                        bytes.len(),
                        entry.size,
                        lines
                    );
                    failures += 1;
                }
            }
            Err(e) => {
                serial_println!("  FAIL cannot read /BIG.TXT: {:?}", e);
                failures += 1;
            }
        },
        Err(e) => {
            serial_println!("  FAIL cannot find /BIG.TXT: {:?}", e);
            failures += 1;
        }
    }

    // A subdirectory, which means the lookup recursed.
    match read_dir("/docs") {
        Ok(entries) => {
            // `.` and `..` are real entries in a FAT subdirectory.
            let named = entries.iter().filter(|e| e.name == "NOTES.TXT").count();
            if named == 1 {
                serial_println!("  PASS /docs contains NOTES.TXT ({} entries total)", entries.len());
            } else {
                serial_println!("  FAIL /docs does not contain NOTES.TXT");
                for e in entries.iter() {
                    serial_println!("       saw '{}'", e.name);
                }
                failures += 1;
            }
        }
        Err(e) => {
            serial_println!("  FAIL cannot read /docs: {:?}", e);
            failures += 1;
        }
    }

    // A name that does not fit 8.3, so it is stored as long-filename entries.
    // Every other name on the image is a valid short name, which means this is
    // the only thing exercising the LFN assembly.
    const LONG: &str = "A Long File Name.txt";
    match entries.iter().find(|e| e.name == LONG) {
        Some(_) => serial_println!("  PASS long filename read back as '{}'", LONG),
        None => {
            serial_println!("  FAIL no entry named '{}' in the root", LONG);
            serial_println!("       (long-filename assembly is wrong, or mcopy stored it differently)");
            failures += 1;
        }
    }

    // ...and it has to be openable by that name, not just listable.
    match read_file("/A Long File Name.txt", 4096) {
        Ok(bytes) if !bytes.is_empty() => {
            serial_println!("  PASS opened it by long name, {} bytes", bytes.len())
        }
        Ok(_) => {
            serial_println!("  FAIL long-name file opened but read empty");
            failures += 1;
        }
        Err(e) => {
            serial_println!("  FAIL cannot open by long name: {:?}", e);
            failures += 1;
        }
    }

    // A path that does not exist must fail, not return something plausible.
    match lookup("/NOPE.TXT") {
        Err(FsError::NotFound) => serial_println!("  PASS /NOPE.TXT reports NotFound"),
        Err(e) => {
            serial_println!("  FAIL /NOPE.TXT reported {:?}, expected NotFound", e);
            failures += 1;
        }
        Ok(_) => {
            serial_println!("  FAIL /NOPE.TXT resolved to something");
            failures += 1;
        }
    }

    if failures == 0 {
        serial_println!("Filesystem: PASS — all checks against the known test image");
    } else {
        serial_println!("Filesystem: FAIL — {} check(s) failed", failures);
    }
}
