
// ---------------------------------------------------------------------------
// Load error type
// ---------------------------------------------------------------------------
#[derive(Debug)]
pub enum LoadError {
    TooShort,
    InvalidMagic,
    UnsupportedClass,
    UnsupportedEndian,
    UnsupportedType,
    UnsupportedMachine,
    InvalidProgramHeader,
    UnsupportedSegmentLayout,
    SegmentOutOfBounds,
    AllocFailed,
    MapFailed,
}

// ---------------------------------------------------------------------------
// ELF loader
// ---------------------------------------------------------------------------

/// Load a static ELF64 binary, mapping its PT_LOAD segments into the active
/// page tables as user-accessible pages.
///
/// This loader intentionally supports only simple page-aligned ET_EXEC images.
/// Truncated segments or unsupported PT_LOAD layouts are rejected instead of
/// being repaired implicitly.
///
/// Returns the entry-point virtual address (`e_entry`) on success.
///
/// # Safety
/// The virtual address ranges claimed by PT_LOAD segments must not overlap
/// existing mappings, and no concurrent page-table mutation may occur.
pub fn load_elf(bytes: &[u8]) -> Result<u64, LoadError> {
    let pml4_phys = crate::memory::paging::current_cr3_phys();
    load_elf_into_pml4(bytes, pml4_phys)
}

/// Load an ELF image into the provided page-table root.
///
/// Returns the entry-point virtual address (`e_entry`) on success.
pub fn load_elf_into_pml4(bytes: &[u8], pml4_phys: usize) -> Result<u64, LoadError> {
    use crate::memory::frame_allocator::allocate_frame;
    use crate::memory::paging::{
        hhdm_offset, is_user_range, is_user_virt, map_page_in_pml4, PageTableFlags, PAGE_SIZE,
    };

    #[derive(Clone, Copy)]
