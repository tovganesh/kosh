//! FAT32, read-only.
//!
//! ## Why FAT32 and not ext4
//!
//! `userspace/fs-service/src/ext4.rs` already claims to implement ext4, and it
//! is instructive about why this file exists instead: `read_block` fills the
//! buffer with zeroes, `write_block` returns success without writing, the
//! superblock is fabricated in code, `mtime` is the literal 1234567890, and
//! `read_dir` returns only `.` and `..`. It was never connected to a disk,
//! because there was no disk driver.
//!
//! FAT32 is a few hundred lines to read correctly, and — more usefully — the
//! image can be mounted on the development host, so every answer the kernel
//! gives can be checked against what is actually on the disk. ext4 is a lot of
//! surface area to get wrong silently.
//!
//! ## What is here
//!
//! Mount (BPB parsing and validation), cluster chain walking, directory
//! iteration including long filenames, path lookup, and file reads. No writing:
//! allocating clusters and updating both FAT copies is a separate problem, and
//! a read-only filesystem that is correct beats a read-write one that is not.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::block::{self, BlockError, BLOCK_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    Block(BlockError),
    /// Sector 0 does not describe a FAT32 filesystem.
    NotFat32,
    /// The BPB contains values that cannot be right.
    BadGeometry(&'static str),
    NotFound,
    NotADirectory,
    IsADirectory,
    /// The cluster chain is circular or points outside the volume.
    CorruptChain,
    NameTooLong,
}

impl From<BlockError> for FsError {
    fn from(e: BlockError) -> Self {
        FsError::Block(e)
    }
}

// Directory entry attribute bits.
const ATTR_READ_ONLY: u8 = 0x01;
const ATTR_HIDDEN: u8 = 0x02;
const ATTR_SYSTEM: u8 = 0x04;
const ATTR_VOLUME_ID: u8 = 0x08;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_ARCHIVE: u8 = 0x20;
/// A "file" with this exact attribute set is not a file at all — it is a
/// fragment of a long filename belonging to the entry that follows it.
const ATTR_LONG_NAME: u8 = ATTR_READ_ONLY | ATTR_HIDDEN | ATTR_SYSTEM | ATTR_VOLUME_ID;

/// End-of-chain marker. Anything at or above this ends the chain.
const CLUSTER_EOC: u32 = 0x0FFF_FFF8;
const CLUSTER_MASK: u32 = 0x0FFF_FFFF;

/// Guard against a corrupt FAT sending the chain walker into a loop.
const MAX_CHAIN_LENGTH: usize = 1 << 20;

/// A mounted FAT32 volume.
pub struct Fat32 {
    bytes_per_sector: u32,
    sectors_per_cluster: u32,
    reserved_sectors: u32,
    num_fats: u32,
    sectors_per_fat: u32,
    root_cluster: u32,
    total_sectors: u32,

    /// LBA of the first data sector — where cluster 2 begins.
    first_data_sector: u32,
    /// Highest valid cluster number.
    max_cluster: u32,

    label: String,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u32,
    pub first_cluster: u32,
    pub attributes: u8,
}

impl DirEntry {
    pub fn is_read_only(&self) -> bool {
        self.attributes & ATTR_READ_ONLY != 0
    }
}

impl Fat32 {
    /// Read and validate the BPB at `lba`.
    pub fn mount(lba: u64) -> Result<Self, FsError> {
        let mut sector = [0u8; BLOCK_SIZE];
        block::read_block(lba, &mut sector)?;

        let u16_at = |o: usize| u16::from_le_bytes([sector[o], sector[o + 1]]) as u32;
        let u32_at = |o: usize| {
            u32::from_le_bytes([sector[o], sector[o + 1], sector[o + 2], sector[o + 3]])
        };

        // Signature first: it is the cheapest way to reject something that is
        // not a filesystem at all.
        if u16::from_le_bytes([sector[510], sector[511]]) != 0xAA55 {
            return Err(FsError::NotFat32);
        }

        let bytes_per_sector = u16_at(11);
        let sectors_per_cluster = sector[13] as u32;
        let reserved_sectors = u16_at(14);
        let num_fats = sector[16] as u32;
        let root_entry_count = u16_at(17);
        let total_sectors_16 = u16_at(19);
        let sectors_per_fat_16 = u16_at(22);
        let total_sectors_32 = u32_at(32);
        let sectors_per_fat_32 = u32_at(36);
        let root_cluster = u32_at(44);

        // FAT32 is identified by these two being zero and their 32-bit
        // counterparts being non-zero — not by the string in the filesystem type
        // field, which is informational and routinely wrong.
        if root_entry_count != 0 || sectors_per_fat_16 != 0 {
            return Err(FsError::NotFat32);
        }

        let total_sectors = if total_sectors_16 != 0 {
            total_sectors_16
        } else {
            total_sectors_32
        };
        let sectors_per_fat = sectors_per_fat_32;

        // Validate the geometry before doing arithmetic with it. A corrupt or
        // hostile BPB otherwise turns into out-of-range reads.
        if bytes_per_sector as usize != BLOCK_SIZE {
            return Err(FsError::BadGeometry("bytes per sector is not 512"));
        }
        if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
            return Err(FsError::BadGeometry("sectors per cluster is not a power of two"));
        }
        if num_fats == 0 || num_fats > 2 {
            return Err(FsError::BadGeometry("implausible FAT count"));
        }
        if sectors_per_fat == 0 || reserved_sectors == 0 || total_sectors == 0 {
            return Err(FsError::BadGeometry("zero-sized region"));
        }
        if root_cluster < 2 {
            return Err(FsError::BadGeometry("root cluster below 2"));
        }

        let first_data_sector = reserved_sectors + num_fats * sectors_per_fat;
        if first_data_sector >= total_sectors {
            return Err(FsError::BadGeometry("no data region"));
        }

        let data_sectors = total_sectors - first_data_sector;
        let cluster_count = data_sectors / sectors_per_cluster;
        let max_cluster = cluster_count + 1; // clusters are numbered from 2

        if root_cluster > max_cluster {
            return Err(FsError::BadGeometry("root cluster past the volume"));
        }

        // Bytes 71..82 hold the volume label, space-padded.
        let mut label = String::new();
        for &b in &sector[71..82] {
            if b != 0 {
                label.push(b as char);
            }
        }
        let label = String::from(label.trim_end());

        Ok(Self {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            num_fats,
            sectors_per_fat,
            root_cluster,
            total_sectors,
            first_data_sector,
            max_cluster,
            label,
        })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn cluster_size(&self) -> u32 {
        self.sectors_per_cluster * self.bytes_per_sector
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_sectors as u64 * self.bytes_per_sector as u64
    }

    pub fn describe(&self) -> String {
        use core::fmt::Write;
        let mut s = String::new();
        let _ = write!(
            s,
            "FAT32 '{}', {} MB, {} B/cluster, {} FAT(s) of {} sectors, root at cluster {}",
            self.label,
            self.total_bytes() / (1024 * 1024),
            self.cluster_size(),
            self.num_fats,
            self.sectors_per_fat,
            self.root_cluster
        );
        s
    }

    fn cluster_to_lba(&self, cluster: u32) -> u64 {
        (self.first_data_sector + (cluster - 2) * self.sectors_per_cluster) as u64
    }

    /// The next cluster in the chain, or `None` at the end.
    fn next_cluster(&self, cluster: u32) -> Result<Option<u32>, FsError> {
        if cluster < 2 || cluster > self.max_cluster {
            return Err(FsError::CorruptChain);
        }

        // Each FAT32 entry is 4 bytes, so 128 fit in a 512-byte sector.
        let byte_offset = cluster as u64 * 4;
        let sector = self.reserved_sectors as u64 + byte_offset / BLOCK_SIZE as u64;
        let offset = (byte_offset % BLOCK_SIZE as u64) as usize;

        let mut buf = [0u8; BLOCK_SIZE];
        block::read_block(sector, &mut buf)?;

        let raw = u32::from_le_bytes([
            buf[offset],
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
        ]);
        // The top four bits are reserved and must be ignored, not trusted.
        let entry = raw & CLUSTER_MASK;

        if entry >= CLUSTER_EOC {
            Ok(None)
        } else if entry < 2 || entry > self.max_cluster {
            // A free (0) or out-of-range entry inside a chain means the FAT is
            // damaged. Saying so beats reading whatever sector that implies.
            Err(FsError::CorruptChain)
        } else {
            Ok(Some(entry))
        }
    }

    /// Every cluster in a chain, starting at `first`.
    fn chain(&self, first: u32) -> Result<Vec<u32>, FsError> {
        let mut clusters = Vec::new();
        let mut current = first;

        loop {
            clusters.push(current);
            if clusters.len() > MAX_CHAIN_LENGTH {
                return Err(FsError::CorruptChain);
            }
            match self.next_cluster(current)? {
                Some(next) => current = next,
                None => break,
            }
        }

        Ok(clusters)
    }

    /// Read a whole cluster.
    fn read_cluster(&self, cluster: u32, buf: &mut [u8]) -> Result<(), FsError> {
        let size = self.cluster_size() as usize;
        if buf.len() < size {
            return Err(FsError::BadGeometry("cluster buffer too small"));
        }
        block::read_blocks(self.cluster_to_lba(cluster), &mut buf[..size])?;
        Ok(())
    }

    /// List a directory, given its first cluster.
    pub fn read_dir(&self, first_cluster: u32) -> Result<Vec<DirEntry>, FsError> {
        let mut entries = Vec::new();
        let mut long_name = LongName::new();
        let cluster_size = self.cluster_size() as usize;
        let mut buf = vec![0u8; cluster_size];

        for cluster in self.chain(first_cluster)? {
            self.read_cluster(cluster, &mut buf)?;

            for raw in buf.chunks_exact(32) {
                match classify(raw) {
                    Slot::End => return Ok(entries),
                    Slot::Free => {
                        long_name.reset();
                    }
                    Slot::LongName => long_name.absorb(raw),
                    Slot::Short => {
                        // A long name only applies to the entry immediately
                        // after it, so take it and clear it either way.
                        let name = long_name.take().unwrap_or_else(|| short_name(raw));
                        if let Some(entry) = short_entry(raw, name) {
                            entries.push(entry);
                        }
                    }
                }
            }
        }

        Ok(entries)
    }

    /// The root directory.
    pub fn read_root(&self) -> Result<Vec<DirEntry>, FsError> {
        self.read_dir(self.root_cluster)
    }

    /// Resolve an absolute path to its directory entry.
    ///
    /// `/` resolves to a synthetic entry for the root, which has no directory
    /// entry of its own on disk.
    pub fn lookup(&self, path: &str) -> Result<DirEntry, FsError> {
        let mut current = DirEntry {
            name: String::from("/"),
            is_dir: true,
            size: 0,
            first_cluster: self.root_cluster,
            attributes: ATTR_DIRECTORY,
        };

        for component in path.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }

            if !current.is_dir {
                return Err(FsError::NotADirectory);
            }

            let entries = self.read_dir(current.first_cluster)?;
            let found = entries
                .into_iter()
                // FAT is case-insensitive, and users type lowercase.
                .find(|e| e.name.eq_ignore_ascii_case(component))
                .ok_or(FsError::NotFound)?;

            current = found;
        }

        Ok(current)
    }

    /// Read up to `limit` bytes of a file.
    pub fn read_file(&self, entry: &DirEntry, limit: usize) -> Result<Vec<u8>, FsError> {
        if entry.is_dir {
            return Err(FsError::IsADirectory);
        }

        let want = core::cmp::min(entry.size as usize, limit);
        let mut out = Vec::with_capacity(want);

        // A zero-length file has no cluster allocated at all, so the chain walk
        // must not be attempted.
        if want == 0 || entry.first_cluster == 0 {
            return Ok(out);
        }

        let cluster_size = self.cluster_size() as usize;
        let mut buf = vec![0u8; cluster_size];

        for cluster in self.chain(entry.first_cluster)? {
            if out.len() >= want {
                break;
            }
            self.read_cluster(cluster, &mut buf)?;
            let take = core::cmp::min(cluster_size, want - out.len());
            out.extend_from_slice(&buf[..take]);
        }

        Ok(out)
    }
}

enum Slot {
    /// No entry here, and none after it.
    End,
    /// Deleted entry.
    Free,
    LongName,
    Short,
}

fn classify(raw: &[u8]) -> Slot {
    match raw[0] {
        0x00 => Slot::End,
        0xE5 => Slot::Free,
        _ if raw[11] == ATTR_LONG_NAME => Slot::LongName,
        _ => Slot::Short,
    }
}

/// Build a `DirEntry` from a short-name directory entry, or `None` if it is not
/// a real file or directory.
fn short_entry(raw: &[u8], name: String) -> Option<DirEntry> {
    let attributes = raw[11];

    // The volume label is stored as a pseudo-entry in the root directory. It is
    // metadata, not a file.
    if attributes & ATTR_VOLUME_ID != 0 {
        return None;
    }

    let first_cluster = ((u16::from_le_bytes([raw[20], raw[21]]) as u32) << 16)
        | (u16::from_le_bytes([raw[26], raw[27]]) as u32);
    let size = u32::from_le_bytes([raw[28], raw[29], raw[30], raw[31]]);

    Some(DirEntry {
        name,
        is_dir: attributes & ATTR_DIRECTORY != 0,
        // Directories report size 0 on disk regardless of their contents.
        size: if attributes & ATTR_DIRECTORY != 0 { 0 } else { size },
        first_cluster,
        attributes,
    })
}

/// Decode an 8.3 name into something printable.
fn short_name(raw: &[u8]) -> String {
    let mut name = String::new();

    for &b in &raw[0..8] {
        if b == b' ' {
            break;
        }
        name.push(b as char);
    }

    let has_ext = raw[8] != b' ';
    if has_ext {
        name.push('.');
        for &b in &raw[8..11] {
            if b == b' ' {
                break;
            }
            name.push(b as char);
        }
    }

    name
}

/// Accumulator for long filename entries.
///
/// They appear *before* the short entry they belong to, in reverse order, with
/// a sequence number in the first byte and the last one flagged 0x40. Each
/// carries 13 UTF-16 code units split awkwardly across three ranges of the
/// 32-byte slot.
struct LongName {
    /// Indexed by sequence number, so out-of-order entries still land right.
    parts: [[u16; 13]; 20],
    present: [bool; 20],
    highest: usize,
}

impl LongName {
    fn new() -> Self {
        Self {
            parts: [[0u16; 13]; 20],
            present: [false; 20],
            highest: 0,
        }
    }

    fn reset(&mut self) {
        self.present = [false; 20];
        self.highest = 0;
    }

    fn absorb(&mut self, raw: &[u8]) {
        let sequence = (raw[0] & 0x1F) as usize;
        if sequence == 0 || sequence > 20 {
            return;
        }
        let index = sequence - 1;

        let mut chars = [0u16; 13];
        let offsets: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
        for (i, &off) in offsets.iter().enumerate() {
            chars[i] = u16::from_le_bytes([raw[off], raw[off + 1]]);
        }

        self.parts[index] = chars;
        self.present[index] = true;
        if sequence > self.highest {
            self.highest = sequence;
        }
    }

    /// Assemble and clear. `None` if no long name was accumulated, or if the
    /// sequence has a hole in it — in which case the short name is more
    /// trustworthy than a name with a piece missing.
    fn take(&mut self) -> Option<String> {
        if self.highest == 0 {
            return None;
        }

        let complete = (0..self.highest).all(|i| self.present[i]);
        if !complete {
            self.reset();
            return None;
        }

        let mut units: Vec<u16> = Vec::new();
        for i in 0..self.highest {
            for &unit in self.parts[i].iter() {
                // 0x0000 terminates, 0xFFFF pads.
                if unit == 0x0000 || unit == 0xFFFF {
                    break;
                }
                units.push(unit);
            }
        }

        self.reset();

        let mut name = String::new();
        for result in char::decode_utf16(units.into_iter()) {
            name.push(result.unwrap_or('\u{FFFD}'));
        }

        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }
}
