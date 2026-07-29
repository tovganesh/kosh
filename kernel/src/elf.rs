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

/// A program that has been loaded and is ready to enter.
pub struct LoadedImage {
    pub entry: u64,
    pub segments: usize,
    pub bytes_mapped: usize,
}

/// Everything at or above this is kernel territory; a user segment claiming to
/// live here is rejected rather than mapped.
const USER_ADDRESS_LIMIT: u64 = 0x0000_8000_0000_0000;

/// Load a static ELF64 executable from a byte slice into the current address
/// space, mapped for ring 3.
///
/// # Safety
/// The caller must ensure no existing user mapping overlaps the segments this
/// image declares. Phase 6 runs one program at a time, so that holds trivially.
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
    })
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

    let start = ph.vaddr & !(PAGE_SIZE as u64 - 1);
    let end = (ph.vaddr + ph.memsz + PAGE_SIZE as u64 - 1) & !(PAGE_SIZE as u64 - 1);
    let pages = ((end - start) / PAGE_SIZE as u64) as usize;

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
