//! A minimal static ELF64 loader.
//!
//! Before this existed there was no way for the kernel to run a program it had
//! not been compiled with. The only mentions of ELF loading in the tree were
//! comments — `driver_loader.rs:21`, "in a real implementation, this would
//! handle ELF loading" — and `sys_exec` returned `NotSupported`.
//!
//! ## Scope
//!
//! Static, non-relocatable `ET_EXEC` binaries only: parse the program headers,
//! map each `PT_LOAD` segment at its `p_vaddr`, copy `p_filesz` bytes and zero
//! the rest. No dynamic linking, no relocations, no interpreter. That is
//! enough to run a `no_std` Rust binary linked at a fixed address, which is
//! what userspace looks like here for the foreseeable future.
//!
//! ## Where the bytes come from
//!
//! GRUB loads modules into physical memory and reports them through multiboot2
//! module tags. Two consequences the kernel has to respect:
//!
//! 1. Those frames must be marked used before the allocator starts handing out
//!    memory, or the module gets overwritten by the first allocation. See
//!    `physical.rs`.
//! 2. A module can land anywhere in RAM, including outside the low identity
//!    window, so it is read through the physmap rather than by physical
//!    address.

use x86_64::structures::paging::PageTableFlags;

use crate::memory::paging::{self, PHYSMAP_BASE};
use crate::memory::PAGE_SIZE;
use crate::serial_println;

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 0x3E;

const PT_LOAD: u32 = 1;

const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

#[derive(Debug)]
pub enum ElfError {
    TooSmall,
    BadMagic,
    Not64Bit,
    NotLittleEndian,
    NotExecutable,
    WrongArchitecture,
    NoLoadableSegments,
    /// More `PT_LOAD` segments than [`MAX_SEGMENTS`], so the range table this
    /// loader hands back could not describe the whole image — and an image that
    /// cannot be described cannot be unmapped.
    TooManySegments,
    SegmentOutOfRange,
    /// A segment wants to live where the kernel already is.
    SegmentInKernelSpace,
    MappingFailed(&'static str),
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Header {
    ident: [u8; 16],
    e_type: u16,
    machine: u16,
    version: u32,
    entry: u64,
    phoff: u64,
    shoff: u64,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64ProgramHeader {
    p_type: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    paddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

/// The most `PT_LOAD` segments this loader will map. A `no_std` Rust binary
/// linked with the scripts in `userspace/` produces three or four.
pub const MAX_SEGMENTS: usize = 8;

/// A page range this loader mapped, so it can be handed back later.
#[derive(Debug, Clone, Copy)]
pub struct MappedRange {
    pub start: u64,
    pub pages: usize,
}

/// A program that has been loaded and is ready to enter.
pub struct LoadedImage {
    pub entry: u64,
    pub segments: usize,
    pub bytes_mapped: usize,
    /// Exactly what was mapped, in the order it was mapped.
    ///
    /// Returned rather than discarded because a program that cannot be unmapped
    /// can only be run once: the second `spawn` hits `PageAlreadyMapped`. The
    /// caller records these and frees them when the program exits.
    pub ranges: [Option<MappedRange>; MAX_SEGMENTS],
}

impl LoadedImage {
    /// Lowest and highest virtual addresses this image occupies, for overlap
    /// checks against programs that are already resident.
    pub fn extent(&self) -> (u64, u64) {
        let mut lo = u64::MAX;
        let mut hi = 0;
        for range in self.ranges.iter().flatten() {
            lo = lo.min(range.start);
            hi = hi.max(range.start + (range.pages * PAGE_SIZE) as u64);
        }
        if lo == u64::MAX {
            (0, 0)
        } else {
            (lo, hi)
        }
    }
}

/// Everything at or above this is kernel territory; a user segment claiming to
/// live here is rejected rather than mapped.
const USER_ADDRESS_LIMIT: u64 = 0x0000_8000_0000_0000;

/// Load a static ELF64 executable from a byte slice into the current address
/// space, mapped for ring 3.
///
/// # Safety
/// The caller must ensure no existing user mapping overlaps the segments this
/// image declares. There is one address space, so that is a real obligation, not
/// a formality — `usermode::spawn_program` checks it against the table of
/// resident programs before calling here.
pub unsafe fn load(image: &[u8]) -> Result<LoadedImage, ElfError> {
    let header = parse_header(image)?;

    serial_println!(
        "  ELF: type {} machine 0x{:x}, entry 0x{:x}, {} program headers",
        header.e_type,
        header.machine,
        header.entry,
        header.phnum
    );

    let mut segments = 0usize;
    let mut bytes_mapped = 0usize;
    let mut ranges: [Option<MappedRange>; MAX_SEGMENTS] = [None; MAX_SEGMENTS];

    for i in 0..header.phnum as usize {
        let offset = header.phoff as usize + i * header.phentsize as usize;
        if offset + core::mem::size_of::<Elf64ProgramHeader>() > image.len() {
            return Err(ElfError::TooSmall);
        }

        let ph: Elf64ProgramHeader =
            core::ptr::read_unaligned(image.as_ptr().add(offset) as *const _);

        if ph.p_type != PT_LOAD || ph.memsz == 0 {
            continue;
        }

        if segments >= MAX_SEGMENTS {
            return Err(ElfError::TooManySegments);
        }

        // Record the range *before* mapping it, so a failure halfway through
        // still tells the caller what to tear down.
        ranges[segments] = Some(segment_range(&ph));

        load_segment(image, &ph)?;

        segments += 1;
        bytes_mapped += ph.memsz as usize;
    }

    if segments == 0 {
        return Err(ElfError::NoLoadableSegments);
    }

    Ok(LoadedImage {
        entry: header.entry,
        segments,
        bytes_mapped,
        ranges,
    })
}

/// Lowest and highest virtual address this image *would* occupy, without
/// mapping anything.
///
/// Needed because there is one address space: the only moment at which a
/// conflicting load can be refused cheaply is before the first page of it has
/// been mapped. Afterwards the choice is between a half-loaded program and an
/// unwind path.
pub fn extent_of(image: &[u8]) -> Result<(u64, u64), ElfError> {
    let header = parse_header(image)?;

    let mut lo = u64::MAX;
    let mut hi = 0u64;

    for i in 0..header.phnum as usize {
        let offset = header.phoff as usize + i * header.phentsize as usize;
        if offset + core::mem::size_of::<Elf64ProgramHeader>() > image.len() {
            return Err(ElfError::TooSmall);
        }
        let ph: Elf64ProgramHeader =
            unsafe { core::ptr::read_unaligned(image.as_ptr().add(offset) as *const _) };
        if ph.p_type != PT_LOAD || ph.memsz == 0 {
            continue;
        }
        let range = segment_range(&ph);
        lo = lo.min(range.start);
        hi = hi.max(range.start + (range.pages * PAGE_SIZE) as u64);
    }

    if lo == u64::MAX {
        return Err(ElfError::NoLoadableSegments);
    }

    Ok((lo, hi))
}

/// Page range a segment occupies, rounded out to page boundaries.
fn segment_range(ph: &Elf64ProgramHeader) -> MappedRange {
    let start = ph.vaddr & !(PAGE_SIZE as u64 - 1);
    let end = (ph.vaddr + ph.memsz + PAGE_SIZE as u64 - 1) & !(PAGE_SIZE as u64 - 1);
    MappedRange {
        start,
        pages: ((end - start) / PAGE_SIZE as u64) as usize,
    }
}

fn parse_header(image: &[u8]) -> Result<Elf64Header, ElfError> {
    if image.len() < core::mem::size_of::<Elf64Header>() {
        return Err(ElfError::TooSmall);
    }

    let header: Elf64Header = unsafe { core::ptr::read_unaligned(image.as_ptr() as *const _) };

    if header.ident[0..4] != ELF_MAGIC {
        return Err(ElfError::BadMagic);
    }
    if header.ident[4] != ELFCLASS64 {
        return Err(ElfError::Not64Bit);
    }
    if header.ident[5] != ELFDATA2LSB {
        return Err(ElfError::NotLittleEndian);
    }
    if header.e_type != ET_EXEC {
        // ET_DYN would need relocation processing, which this loader does not do.
        return Err(ElfError::NotExecutable);
    }
    if header.machine != EM_X86_64 {
        return Err(ElfError::WrongArchitecture);
    }

    Ok(header)
}

/// Map one `PT_LOAD` segment and populate it.
///
/// Pages are written through the physmap rather than through the mapping being
/// created. That keeps the final page permissions honest — a read-only or
/// non-writable segment never has to be temporarily writable just so the loader
/// can fill it in.
unsafe fn load_segment(image: &[u8], ph: &Elf64ProgramHeader) -> Result<(), ElfError> {
    if ph.vaddr >= USER_ADDRESS_LIMIT || ph.vaddr.saturating_add(ph.memsz) >= USER_ADDRESS_LIMIT {
        return Err(ElfError::SegmentInKernelSpace);
    }
    if (ph.offset + ph.filesz) as usize > image.len() {
        return Err(ElfError::SegmentOutOfRange);
    }

    let MappedRange { start, pages } = segment_range(ph);

    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if ph.flags & PF_W != 0 {
        flags |= PageTableFlags::WRITABLE;
    }
    if ph.flags & PF_X == 0 {
        flags |= PageTableFlags::NO_EXECUTE;
    }

    serial_println!(
        "  segment: vaddr 0x{:x} filesz {} memsz {} [{}{}{}] -> {} page(s)",
        ph.vaddr,
        ph.filesz,
        ph.memsz,
        if ph.flags & PF_R != 0 { "r" } else { "-" },
        if ph.flags & PF_W != 0 { "w" } else { "-" },
        if ph.flags & PF_X != 0 { "x" } else { "-" },
        pages
    );

    // Allocate and map the whole span with its final permissions.
    paging::map_user_pages(start, pages, flags).map_err(ElfError::MappingFailed)?;

    // Fill it in through the physmap. `map_user_pages` already zeroed every
    // frame, which is what makes the .bss tail correct without extra work: we
    // only have to write the `p_filesz` bytes that come from the file.
    for i in 0..ph.filesz as usize {
        let vaddr = ph.vaddr + i as u64;
        let phys = paging::translate(vaddr).ok_or(ElfError::MappingFailed("segment unmapped"))?;
        core::ptr::write_volatile(
            (PHYSMAP_BASE + phys) as *mut u8,
            image[ph.offset as usize + i],
        );
    }

    Ok(())
}

/// Report what a module looks like without loading it — used for diagnostics
/// when loading fails.
pub fn describe(image: &[u8]) {
    serial_println!("  module: {} bytes", image.len());
    if image.len() >= 4 {
        serial_println!(
            "  first 4 bytes: {:02x} {:02x} {:02x} {:02x}",
            image[0],
            image[1],
            image[2],
            image[3]
        );
    }
}
