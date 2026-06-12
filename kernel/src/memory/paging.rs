use crate::memory::frame_allocator::allocate_frame;
use crate::memory::frame_allocator::Frame;
use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Higher-half direct map offset: virtual address X maps to physical address (X - HHDM_OFFSET)
const DEFAULT_HHDM_OFFSET: usize = 0xffff_8000_0000_0000;
static HHDM_OFFSET_VALUE: AtomicUsize = AtomicUsize::new(DEFAULT_HHDM_OFFSET);

/// Lower-half canonical user virtual space limit (exclusive).
///
/// Valid user-space addresses satisfy: `addr < USER_SPACE_LIMIT`.
pub const USER_SPACE_LIMIT: usize = 0x0000_8000_0000_0000;
/// Upper-half canonical kernel virtual base (inclusive).
pub const KERNEL_SPACE_BASE: usize = 0xffff_8000_0000_0000;

#[inline]
pub fn is_user_virt(addr: usize) -> bool {
    addr < USER_SPACE_LIMIT
}

#[inline]
pub fn is_kernel_virt(addr: usize) -> bool {
    addr >= KERNEL_SPACE_BASE
}

/// Returns true if `[start, start+len)` lies fully in user space.
#[inline]
pub fn is_user_range(start: usize, len: usize) -> bool {
    if !is_user_virt(start) {
        return false;
    }
    match start.checked_add(len) {
        Some(end) => end <= USER_SPACE_LIMIT,
        None => false,
    }
}

pub fn set_hhdm_offset(offset: usize) {
    HHDM_OFFSET_VALUE.store(offset, Ordering::Relaxed);
}

pub fn hhdm_offset() -> usize {
    HHDM_OFFSET_VALUE.load(Ordering::Relaxed)
}

/// Size of a page table (4 KiB = 512 entries × 8 bytes each)
pub const PAGE_SIZE: usize = 4096;

/// Number of entries in a page table
const PAGE_TABLE_ENTRIES: usize = 512;

/// Page table entry flags
pub struct PageTableFlags(u64);

impl Copy for PageTableFlags {}

impl Clone for PageTableFlags {
    fn clone(&self) -> Self {
        PageTableFlags(self.0)
    }
}

impl PageTableFlags {
    pub const PRESENT: u64 = 1 << 0;
    pub const WRITABLE: u64 = 1 << 1;
    pub const USER_ACCESSIBLE: u64 = 1 << 2;
    pub const WRITE_THROUGH: u64 = 1 << 3;
    pub const CACHE_DISABLED: u64 = 1 << 4;
    pub const ACCESSED: u64 = 1 << 5;
    pub const DIRTY: u64 = 1 << 6;
    pub const HUGE_PAGE: u64 = 1 << 7;
    pub const GLOBAL: u64 = 1 << 8;
    pub const EXECUTE_DISABLE: u64 = 1 << 63;

    pub fn empty() -> Self {
        PageTableFlags(0)
    }

    pub fn new(flags: u64) -> Self {
        PageTableFlags(flags)
    }

    pub fn with(mut self, flag: u64) -> Self {
        self.0 |= flag;
        self
    }

    pub fn bits(&self) -> u64 {
        self.0
    }
}

/// A single page table (512 entries, each 8 bytes)
#[repr(align(4096))]
pub struct PageTable {
    entries: [u64; PAGE_TABLE_ENTRIES],
}

impl PageTable {
    /// Create a new, empty page table
    pub const fn new() -> Self {
        PageTable {
            entries: [0; PAGE_TABLE_ENTRIES],
        }
    }

    /// Get an entry from the page table
    pub fn entry(&self, index: usize) -> u64 {
        if index < PAGE_TABLE_ENTRIES {
            self.entries[index]
        } else {
            0
        }
    }

    /// Set an entry in the page table
    pub fn set_entry(&mut self, index: usize, value: u64) {
        if index < PAGE_TABLE_ENTRIES {
            self.entries[index] = value;
        }
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        for i in 0..PAGE_TABLE_ENTRIES {
            self.entries[i] = 0;
        }
    }
}

/// Physical memory paging manager
pub struct PageTableManager {
    pml4: &'static mut PageTable,
}

impl PageTableManager {
    /// Create a new page table manager with a pre-allocated PML4
    /// 
    /// # Safety
    /// The pml4_frame must be a valid, uniquely-owned frame that can be safely cast to a mutable reference
    pub unsafe fn new(pml4_frame: Frame) -> Self {
        let pml4_virt = pml4_frame.start_address() + hhdm_offset();
        let pml4 = unsafe { &mut *(pml4_virt as *mut PageTable) };
        pml4.clear();

        PageTableManager { pml4 }
    }

    /// Create a page table manager from the currently active CR3 root.
    ///
    /// # Safety
    /// Caller must ensure exclusive access to page table mutation while using
    /// this manager.
    pub unsafe fn from_current_cr3() -> Self {
        let mut cr3_phys: usize;
        unsafe {
            asm!("mov {}, cr3", out(reg) cr3_phys, options(nomem, nostack, preserves_flags));
        }
        let pml4_virt = (cr3_phys & 0x000f_ffff_ffff_f000) + hhdm_offset();
        let pml4 = unsafe { &mut *(pml4_virt as *mut PageTable) };
        PageTableManager { pml4 }
    }

    /// Get the physical address of the PML4
    pub fn pml4_address(&self) -> usize {
        (self.pml4 as *const _ as usize) - hhdm_offset()
    }

    /// Map a virtual address to a physical address
    /// 
    /// # Safety
    /// Caller must ensure no other code accesses overlapping address ranges
    pub unsafe fn map_page(
        &mut self,
        virt: usize,
        phys: usize,
        flags: PageTableFlags,
    ) -> Result<(), &'static str> {
        if flags.bits() & PageTableFlags::USER_ACCESSIBLE != 0 && !is_user_virt(virt) {
            return Err("user mapping outside user virtual range");
        }

        let pml4_index = (virt >> 39) & 0x1ff;
        let pdpt_index = (virt >> 30) & 0x1ff;
        let pdt_index = (virt >> 21) & 0x1ff;
        let pt_index = (virt >> 12) & 0x1ff;
        let parent_flags = if flags.bits() & PageTableFlags::USER_ACCESSIBLE != 0 {
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE
        } else {
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE
        };

        // PML4 entry
        let pml4_entry = self.pml4.entry(pml4_index);
        let pdpt: &mut PageTable = if pml4_entry & PageTableFlags::PRESENT == 0 {
            // Allocate a new PDPT
            let frame = allocate_frame().ok_or("No frames available for PDPT")?;
            let pdpt_phys = frame.start_address();
            let pdpt_virt = pdpt_phys + hhdm_offset();

            unsafe {
                let pdpt_ptr = pdpt_virt as *mut PageTable;
                (*pdpt_ptr).clear();

                self.pml4.set_entry(pml4_index, pdpt_phys as u64 | parent_flags);

                &mut *pdpt_ptr
            }
        } else {
            if pml4_entry & parent_flags != parent_flags {
                self.pml4.set_entry(pml4_index, pml4_entry | parent_flags);
            }
            let pdpt_phys = pml4_entry & 0x000f_ffff_ffff_f000;
            let pdpt_virt = pdpt_phys as usize + hhdm_offset();

            unsafe { &mut *(pdpt_virt as *mut PageTable) }
        };

        // PDPT entry
        let pdpt_entry = pdpt.entry(pdpt_index);
        let pdt: &mut PageTable = if pdpt_entry & PageTableFlags::PRESENT == 0 {
            // Allocate a new PDT
            let frame = allocate_frame().ok_or("No frames available for PDT")?;
            let pdt_phys = frame.start_address();
            let pdt_virt = pdt_phys + hhdm_offset();

            unsafe {
                let pdt_ptr = pdt_virt as *mut PageTable;
                (*pdt_ptr).clear();

                pdpt.set_entry(pdpt_index, pdt_phys as u64 | parent_flags);

                &mut *pdt_ptr
            }
        } else {
            if pdpt_entry & parent_flags != parent_flags {
                pdpt.set_entry(pdpt_index, pdpt_entry | parent_flags);
            }
            let pdt_phys = pdpt_entry & 0x000f_ffff_ffff_f000;
            let pdt_virt = pdt_phys as usize + hhdm_offset();

            unsafe { &mut *(pdt_virt as *mut PageTable) }
        };

        // PDT entry
        let pdt_entry = pdt.entry(pdt_index);
        let pt: &mut PageTable = if pdt_entry & PageTableFlags::PRESENT == 0 {
            // Allocate a new PT
            let frame = allocate_frame().ok_or("No frames available for PT")?;
            let pt_phys = frame.start_address();
            let pt_virt = pt_phys + hhdm_offset();

            unsafe {
                let pt_ptr = pt_virt as *mut PageTable;
                (*pt_ptr).clear();

                pdt.set_entry(pdt_index, pt_phys as u64 | parent_flags);

                &mut *pt_ptr
            }
        } else {
            if pdt_entry & parent_flags != parent_flags {
                pdt.set_entry(pdt_index, pdt_entry | parent_flags);
            }
            let pt_phys = pdt_entry & 0x000f_ffff_ffff_f000;
            let pt_virt = pt_phys as usize + hhdm_offset();

            unsafe { &mut *(pt_virt as *mut PageTable) }
        };

        // PT entry
        pt.set_entry(pt_index, (phys as u64) | flags.bits());

        Ok(())
    }

    /// Enable paging with this page table
    /// 
    /// # Safety
    /// Must have valid page tables set up before calling
    pub unsafe fn enable_paging(&self) {
        let pml4_phys = self.pml4_address();

        // Set CR3 to PML4 address
        unsafe {
            asm!("mov cr3, {}", in(reg) pml4_phys);
        }

        // Enable PAE (Physical Address Extension) in CR4
        unsafe {
            asm!(
                "mov {tmp}, cr4",
                "or {tmp}, {pae_bit}",
                "mov cr4, {tmp}",
                tmp = out(reg) _,
                pae_bit = in(reg) 1usize << 5
            );
        }

        // Enable paging by setting PG bit in CR0
        unsafe {
            asm!(
                "mov {tmp}, cr0",
                "or {tmp}, {pg_bit}",
                "mov cr0, {tmp}",
                tmp = out(reg) _,
                pg_bit = in(reg) 1usize << 31
            );
        }
    }

    /// Map a 2 MiB huge page: virt -> phys (both must be 2 MiB-aligned).
    ///
    /// # Safety
    /// Same invariants as `map_page`.
    pub unsafe fn map_2mb_page(
        &mut self,
        virt: usize,
        phys: usize,
        flags: PageTableFlags,
    ) -> Result<(), &'static str> {
        let pml4_index = (virt >> 39) & 0x1ff;
        let pdpt_index = (virt >> 30) & 0x1ff;
        let pdt_index  = (virt >> 21) & 0x1ff;

        // PML4 entry → PDPT
        let pml4_entry = self.pml4.entry(pml4_index);
        let pdpt: &mut PageTable = if pml4_entry & PageTableFlags::PRESENT == 0 {
            let frame = allocate_frame().ok_or("No frames available for PDPT (2MB)")?;
            let pdpt_phys = frame.start_address();
            let pdpt_virt = pdpt_phys + hhdm_offset();
            unsafe {
                let p = pdpt_virt as *mut PageTable;
                (*p).clear();
                self.pml4.set_entry(
                    pml4_index,
                    pdpt_phys as u64 | PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                );
                &mut *p
            }
        } else {
            let pdpt_phys = (pml4_entry & 0x000f_ffff_ffff_f000) as usize;
            unsafe { &mut *((pdpt_phys + hhdm_offset()) as *mut PageTable) }
        };

        // PDPT entry → PDT
        let pdpt_entry = pdpt.entry(pdpt_index);
        let pdt: &mut PageTable = if pdpt_entry & PageTableFlags::PRESENT == 0 {
            let frame = allocate_frame().ok_or("No frames available for PDT (2MB)")?;
            let pdt_phys = frame.start_address();
            let pdt_virt = pdt_phys + hhdm_offset();
            unsafe {
                let p = pdt_virt as *mut PageTable;
                (*p).clear();
                pdpt.set_entry(
                    pdpt_index,
                    pdt_phys as u64 | PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                );
                &mut *p
            }
        } else {
            let pdt_phys = (pdpt_entry & 0x000f_ffff_ffff_f000) as usize;
            unsafe { &mut *((pdt_phys + hhdm_offset()) as *mut PageTable) }
        };

        // PDT entry: leaf huge-page mapping (no PT allocated)
        let huge_flags = PageTableFlags::new(flags.bits() | PageTableFlags::HUGE_PAGE);
        pdt.set_entry(pdt_index, (phys as u64) | huge_flags.bits());

        Ok(())
    }

    /// Check if paging is currently enabled
    pub fn is_enabled() -> bool {
        let mut cr0: usize;
        unsafe {
            asm!("mov {}, cr0", out(reg) cr0);
        }
        (cr0 & (1 << 31)) != 0
    }
}

/// Virtual address components
pub struct VirtualAddress {
    pub pml4_index: usize,
    pub pdpt_index: usize,
    pub pdt_index: usize,
    pub pt_index: usize,
    pub offset: usize,
}

impl VirtualAddress {
    pub fn from_address(addr: usize) -> Self {
        VirtualAddress {
            pml4_index: (addr >> 39) & 0x1ff,
            pdpt_index: (addr >> 30) & 0x1ff,
            pdt_index: (addr >> 21) & 0x1ff,
            pt_index: (addr >> 12) & 0x1ff,
            offset: addr & 0xfff,
        }
    }
}

/// Convert physical address to virtual (via HHDM)
pub fn phys_to_virt(phys: usize) -> usize {
    phys + hhdm_offset()
}

/// Convert virtual address to physical (via HHDM)
pub fn virt_to_phys(virt: usize) -> usize {
    virt - hhdm_offset()
}

/// Map a single 4 KiB page in the currently active page tables.
///
/// # Safety
/// Caller must ensure the virtual address range does not overlap existing
/// mappings and that no concurrent page table mutation occurs.
pub unsafe fn map_page_current(
    virt: usize,
    phys: usize,
    flags: PageTableFlags,
) -> Result<(), &'static str> {
    let mut manager = unsafe { PageTableManager::from_current_cr3() };
    let result = unsafe { manager.map_page(virt, phys, flags) };
    if result.is_ok() {
        unsafe {
            asm!("invlpg [{addr}]", addr = in(reg) virt, options(nostack, preserves_flags));
        }
    }
    result
}

/// Look up the current leaf entry bits for a 4 KiB mapping in the active page tables.
///
/// Returns `None` when any level is absent or when the address resolves through
/// a huge-page entry instead of a normal 4 KiB PT leaf.
pub unsafe fn lookup_page_entry_current(virt: usize) -> Option<u64> {
    let manager = unsafe { PageTableManager::from_current_cr3() };

    let pml4_index = (virt >> 39) & 0x1ff;
    let pdpt_index = (virt >> 30) & 0x1ff;
    let pdt_index = (virt >> 21) & 0x1ff;
    let pt_index = (virt >> 12) & 0x1ff;

    let pml4_entry = manager.pml4.entry(pml4_index);
    if pml4_entry & PageTableFlags::PRESENT == 0 {
        return None;
    }

    let pdpt_phys = (pml4_entry & 0x000f_ffff_ffff_f000) as usize;
    let pdpt = unsafe { &*((pdpt_phys + hhdm_offset()) as *const PageTable) };
    let pdpt_entry = pdpt.entry(pdpt_index);
    if pdpt_entry & PageTableFlags::PRESENT == 0 || pdpt_entry & PageTableFlags::HUGE_PAGE != 0 {
        return None;
    }

    let pdt_phys = (pdpt_entry & 0x000f_ffff_ffff_f000) as usize;
    let pdt = unsafe { &*((pdt_phys + hhdm_offset()) as *const PageTable) };
    let pdt_entry = pdt.entry(pdt_index);
    if pdt_entry & PageTableFlags::PRESENT == 0 || pdt_entry & PageTableFlags::HUGE_PAGE != 0 {
        return None;
    }

    let pt_phys = (pdt_entry & 0x000f_ffff_ffff_f000) as usize;
    let pt = unsafe { &*((pt_phys + hhdm_offset()) as *const PageTable) };
    let pt_entry = pt.entry(pt_index);
    if pt_entry & PageTableFlags::PRESENT == 0 {
        None
    } else {
        Some(pt_entry)
    }
}
