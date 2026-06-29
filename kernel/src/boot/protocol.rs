use limine::memory_map::EntryType;
use limine::request::{
    ExecutableAddressRequest, FramebufferRequest, HhdmRequest, MemoryMapRequest, MpRequest,
    RequestsEndMarker, RequestsStartMarker, RsdpRequest, StackSizeRequest,
};
use limine::BaseRevision;

const STACK_SIZE: u64 = 1024 * 1024;

#[used]
#[unsafe(link_section = ".requests")]
pub static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".requests")]
pub static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
pub static STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest::new().with_size(STACK_SIZE);

#[used]
#[unsafe(link_section = ".requests")]
pub static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
pub static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
pub static KERNEL_ADDRESS_REQUEST: ExecutableAddressRequest = ExecutableAddressRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
pub static RSDP_REQUEST: RsdpRequest = RsdpRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
pub static MP_REQUEST: MpRequest = MpRequest::new();

#[used]
#[unsafe(link_section = ".requests_start_marker")]
pub static REQUESTS_START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
pub static REQUESTS_END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

pub fn limine_revision_supported() -> bool {
    BASE_REVISION.is_supported()
}

/// Returns (physical_base, virtual_base) of the loaded kernel binary, if available.
pub fn kernel_address() -> Option<(usize, usize)> {
    let resp = KERNEL_ADDRESS_REQUEST.get_response()?;
    Some((resp.physical_base() as usize, resp.virtual_base() as usize))
}

/// Highest physical address (base + length) observed across all memory map entries.
pub fn max_physical_address() -> usize {
    let response = match MEMORY_MAP_REQUEST.get_response() {
        Some(r) => r,
        None => return 0,
    };
    // Include all region types so Limine response structures stored in
    // non-usable firmware/bootloader memory remain reachable after we switch
    // to kernel-owned page tables. Keep a hard cap to avoid overmapping.
    // 4 GiB is enough to include common PCI/MMIO framebuffer ranges on QEMU.
    const PHYS_CAP: usize = 4 * 1024 * 1024 * 1024;
    let mut max_addr = 0usize;
    for entry in response.entries() {
        let end = (entry.base as usize).saturating_add(entry.length as usize);
        if end > max_addr {
            max_addr = end;
        }
    }

    max_addr.min(PHYS_CAP)
}

pub struct MemoryMapSummary {
    pub region_count: usize,
    pub total_bytes: u64,
    pub usable_bytes: u64,
}

pub fn memory_map_summary() -> Option<MemoryMapSummary> {
    let response = MEMORY_MAP_REQUEST.get_response()?;
    let entries = response.entries();

    let mut total_bytes = 0u64;
    let mut usable_bytes = 0u64;

    for entry in entries {
        total_bytes = total_bytes.saturating_add(entry.length);
        if entry.entry_type == EntryType::USABLE {
            usable_bytes = usable_bytes.saturating_add(entry.length);
        }
    }

    Some(MemoryMapSummary {
        region_count: entries.len(),
        total_bytes,
        usable_bytes,
    })
}

/// Get usable memory regions (up to 128)
/// Returns a slice of (start_address, length) tuples
pub fn usable_memory_regions() -> &'static [(usize, usize)] {
    const MAX_REGIONS: usize = 128;
    static mut REGIONS: [(usize, usize); MAX_REGIONS] = [(0, 0); MAX_REGIONS];
    static mut REGION_COUNT: usize = 0;

    let response = match MEMORY_MAP_REQUEST.get_response() {
        Some(r) => r,
        None => return &[],
    };

    let entries = response.entries();
    let mut count = 0;

    for entry in entries {
        if entry.entry_type == EntryType::USABLE && count < MAX_REGIONS {
            unsafe {
                REGIONS[count] = (entry.base as usize, entry.length as usize);
                count += 1;
            }
        }
    }

    unsafe {
        REGION_COUNT = count;
        &REGIONS[..count]
    }
}

#[allow(dead_code)]
pub struct FramebufferInfo {
    pub addr: *mut u8,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub red_mask_shift: u8,
    pub red_mask_size: u8,
    pub green_mask_shift: u8,
    pub green_mask_size: u8,
    pub blue_mask_shift: u8,
    pub blue_mask_size: u8,
}

#[allow(dead_code)]
pub fn framebuffer_info() -> Option<FramebufferInfo> {
    let response = FRAMEBUFFER_REQUEST.get_response()?;
    let framebuffer = response.framebuffers().next()?;

    Some(FramebufferInfo {
        addr: framebuffer.addr(),
        width: framebuffer.width(),
        height: framebuffer.height(),
        pitch: framebuffer.pitch(),
        bpp: framebuffer.bpp(),
        red_mask_shift: framebuffer.red_mask_shift(),
        red_mask_size: framebuffer.red_mask_size(),
        green_mask_shift: framebuffer.green_mask_shift(),
        green_mask_size: framebuffer.green_mask_size(),
        blue_mask_shift: framebuffer.blue_mask_shift(),
        blue_mask_size: framebuffer.blue_mask_size(),
    })
}

pub fn hhdm_offset() -> Option<usize> {
    let response = HHDM_REQUEST.get_response()?;
    Some(response.offset() as usize)
}

/// Returns the physical (usually firmware-mapped) address of the ACPI RSDP
/// table reported by Limine, if available.
pub fn rsdp_address() -> Option<usize> {
    let response = RSDP_REQUEST.get_response()?;
    let addr = response.address();
    if addr == 0 {
        None
    } else {
        Some(addr)
    }
}

/// Information about a single CPU exposed by the Limine MP request.
#[derive(Copy, Clone)]
pub struct CpuEntry {
    pub acpi_id: u32,
    pub lapic_id: u32,
}

/// Summary of the multiprocessor topology reported by Limine.
pub struct MpSummary {
    pub bsp_lapic_id: u32,
    pub cpu_count: usize,
    pub x2apic: bool,
}

pub fn mp_summary() -> Option<MpSummary> {
    let response = MP_REQUEST.get_response()?;
    let cpus = response.cpus();
    Some(MpSummary {
        bsp_lapic_id: response.bsp_lapic_id(),
        cpu_count: cpus.len(),
        // x2APIC was not requested (RequestFlags::X2APIC unset), so always xAPIC mode.
        x2apic: false,
    })
}

/// Copies up to `out.len()` CPU entries into `out`, returning how many were written.
pub fn mp_cpus(out: &mut [CpuEntry]) -> usize {
    let response = match MP_REQUEST.get_response() {
        Some(r) => r,
        None => return 0,
    };
    let cpus = response.cpus();
    let n = cpus.len().min(out.len());
    for i in 0..n {
        out[i] = CpuEntry {
            acpi_id: cpus[i].id,
            lapic_id: cpus[i].lapic_id,
        };
    }
    n
}
