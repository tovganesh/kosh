pub mod physical;
pub mod vmm;
pub mod heap;
pub mod swap;
pub mod swap_file;
pub mod swap_config;
pub mod swap_algorithm;

/// Page size constant (4KB on x86-64)
pub const PAGE_SIZE: usize = 4096;

/// Convert bytes to pages (rounded up)
#[allow(dead_code)]
pub const fn bytes_to_pages(bytes: usize) -> usize {
    (bytes + PAGE_SIZE - 1) / PAGE_SIZE
}

/// Convert pages to bytes
#[allow(dead_code)]
pub const fn pages_to_bytes(pages: usize) -> usize {
    pages * PAGE_SIZE
}

/// Align address up to page boundary
#[allow(dead_code)]
pub const fn align_up(addr: usize) -> usize {
    (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

/// Align address down to page boundary
pub const fn align_down(addr: usize) -> usize {
    addr & !(PAGE_SIZE - 1)
}

/// Check if address is page-aligned
#[allow(dead_code)]
pub const fn is_aligned(addr: usize) -> bool {
    addr & (PAGE_SIZE - 1) == 0
}