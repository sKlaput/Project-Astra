// ---------------------------------------------------------------------------
// Minimal static ELF64 loader.
//
// Accepts an ET_EXEC ELF64 little-endian binary targeting x86_64.
// Maps each PT_LOAD segment page-by-page into the current page tables as
// user-accessible pages via map_page_current(), writing file data through
// the HHDM.
//
// Only handles a small fixed number of PT_LOAD segments (no heap needed).
// ---------------------------------------------------------------------------

/// Maximum PT_LOAD segments processed without heap allocation.
const MAX_LOAD_SEGS: usize = 4;

const PT_LOAD: u32 = 1;
const PF_W: u32 = 2;
const PF_X: u32 = 1;
const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 62;

include!("loader/embedded.rs");
include!("loader/elf.rs");
