pub mod frame_allocator;
pub mod paging;
pub mod heap;
pub mod user_frames;

pub fn init_from_boot() {
    if let Some(offset) = crate::boot::protocol::hhdm_offset() {
        paging::set_hhdm_offset(offset);
        crate::serial::write_str("paging: hhdm offset=0x");
        crate::serial::write_u64(offset as u64);
        crate::serial::write_line("");
    } else {
        crate::serial::write_line("paging: hhdm offset unavailable (using default)");
    }

    let Some(summary) = crate::boot::protocol::memory_map_summary() else {
        crate::serial::write_line("memory: map unavailable");
        return;
    };

    crate::serial::write_str("memory: regions=");
    crate::serial::write_u64(summary.region_count as u64);
    crate::serial::write_str(" total-bytes=");
    crate::serial::write_u64(summary.total_bytes);
    crate::serial::write_str(" usable-bytes=");
    crate::serial::write_u64(summary.usable_bytes);
    crate::serial::write_line("");

    // Initialize frame allocator with usable memory regions
    let regions = crate::boot::protocol::usable_memory_regions();
    frame_allocator::init_from_memory_map(regions);

    // Initialize paging and HHDM
    setup_paging_and_hhdm();

    crate::serial::write_line("heap: allocator ready (virtual bump)");
}

fn setup_paging_and_hhdm() {
    const TWO_MB: usize = 2 * 1024 * 1024;
    const KERNEL_MAP_SIZE: usize = 32 * 1024 * 1024; // 32 MiB covers code + BSS + stack

    let hhdm = paging::hhdm_offset();

    // Kernel physical / virtual base
    let (kernel_phys, kernel_virt) = match crate::boot::protocol::kernel_address() {
        Some(a) => a,
        None => {
            crate::serial::write_line("paging: FATAL no kernel address response");
            return;
        }
    };

    // Highest physical address across all memory-map entries
    let max_phys = crate::boot::protocol::max_physical_address();
    // Round up to next 2 MiB boundary
    let map_limit = (max_phys + TWO_MB - 1) & !(TWO_MB - 1);

    crate::serial::write_str("paging: kernel phys=");
    crate::serial::write_u64(kernel_phys as u64);
    crate::serial::write_str(" virt=");
    crate::serial::write_u64(kernel_virt as u64);
    crate::serial::write_line("");
    crate::serial::write_str("paging: map_limit=");
    crate::serial::write_u64(map_limit as u64);
    crate::serial::write_line("");

    // Allocate PML4 frame
    let pml4_frame = match frame_allocator::allocate_frame() {
        Some(f) => f,
        None => {
            crate::serial::write_line("paging: FATAL no frame for PML4");
            return;
        }
    };

    let mut mgr = unsafe { paging::PageTableManager::new(pml4_frame) };

    // ── HHDM mapping: all physical memory as 2 MiB huge pages ──────────────
    // Covers heap frames, stack (Limine places stack in HHDM-mapped region),
    // and page-table frames we just allocated.
    crate::serial::write_line("paging: building HHDM map (2 MiB huge pages)");
    let mut phys = 0usize;
    while phys < map_limit {
        let flags = paging::PageTableFlags::new(
            paging::PageTableFlags::PRESENT | paging::PageTableFlags::WRITABLE,
        );
        if let Err(e) = unsafe { mgr.map_2mb_page(hhdm + phys, phys, flags) } {
            crate::serial::write_str("paging: hhdm map err at phys=");
            crate::serial::write_u64(phys as u64);
            crate::serial::write_str(": ");
            crate::serial::write_line(e);
            return;
        }
        phys += TWO_MB;
    }

    // ── Kernel binary mapping: 4 KiB pages ─────────────────────────────────
    // Ensures the code executing at 0xffffffff80000000 is still reachable
    // after the CR3 switch.
    crate::serial::write_line("paging: building kernel binary map (4 KiB pages)");
    let mut offset = 0usize;
    while offset < KERNEL_MAP_SIZE {
        let flags = paging::PageTableFlags::new(
            paging::PageTableFlags::PRESENT | paging::PageTableFlags::WRITABLE,
        );
        if let Err(e) = unsafe { mgr.map_page(kernel_virt + offset, kernel_phys + offset, flags) }
        {
            crate::serial::write_str("paging: kernel map err at offset=");
            crate::serial::write_u64(offset as u64);
            crate::serial::write_str(": ");
            crate::serial::write_line(e);
            return;
        }
        offset += paging::PAGE_SIZE;
    }

    // ── Activate ────────────────────────────────────────────────────────────
    crate::serial::write_line("paging: [PAGING-PRE-SWITCH] activating new page tables");
    unsafe { mgr.enable_paging() };

    // If execution reaches here the CR3 switch succeeded.
    crate::serial::write_line("paging: [PAGING-OK] new page tables active");

    let heap_base = kernel_virt + KERNEL_MAP_SIZE;
    heap::init_virtual_heap(heap_base);
}

