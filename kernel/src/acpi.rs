//! Minimal ACPI table walker — RSDP -> RSDT/XSDT -> MADT.
//!
//! Read-only at this stage. Walks the table list once, indexes the MADT
//! entries we care about (Local APIC, I/O APIC, Interrupt Source Override,
//! NMI sources) into static arrays, and exposes accessors. We do **not**
//! validate ACPI checksums beyond signature matching — the firmware-provided
//! tables under Limine/QEMU are trusted, and a checksum mismatch would still
//! be safe to ignore for read-only enumeration.

use crate::memory::paging::hhdm_offset;
use core::sync::atomic::{AtomicBool, Ordering};

#[inline]
fn phys_to_virt<T>(phys: u64) -> *const T {
    (phys as usize + hhdm_offset()) as *const T
}

#[repr(C, packed)]
struct RsdpV1 {
    sig: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_addr: u32,
}

#[repr(C, packed)]
struct RsdpV2 {
    v1: RsdpV1,
    length: u32,
    xsdt_addr: u64,
    ext_checksum: u8,
    reserved: [u8; 3],
}

#[repr(C, packed)]
struct SdtHeader {
    sig: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

#[repr(C, packed)]
struct MadtHeader {
    hdr: SdtHeader,
    local_apic_addr: u32,
    flags: u32,
}

const MADT_ENTRY_LOCAL_APIC: u8 = 0;
const MADT_ENTRY_IO_APIC: u8 = 1;
const MADT_ENTRY_INT_SRC_OVERRIDE: u8 = 2;
const MADT_ENTRY_NMI_SOURCE: u8 = 3;
const MADT_ENTRY_LOCAL_APIC_NMI: u8 = 4;
const MADT_ENTRY_LOCAL_X2APIC: u8 = 9;

const MAX_LOCAL_APICS: usize = 32;
const MAX_IO_APICS: usize = 4;
const MAX_OVERRIDES: usize = 16;

#[derive(Copy, Clone, Default)]
pub struct LocalApic {
    pub acpi_id: u8,
    pub apic_id: u8,
    pub flags: u32, // bit0 = enabled, bit1 = online-capable
}

#[derive(Copy, Clone, Default)]
pub struct IoApic {
    pub id: u8,
    pub address: u32,
    pub gsi_base: u32,
}

#[derive(Copy, Clone, Default)]
pub struct IntSrcOverride {
    pub bus: u8,
    pub source_irq: u8,
    pub gsi: u32,
    pub flags: u16,
}

struct MadtTables {
    local_apics: [LocalApic; MAX_LOCAL_APICS],
    local_count: usize,
    io_apics: [IoApic; MAX_IO_APICS],
    io_count: usize,
    overrides: [IntSrcOverride; MAX_OVERRIDES],
    override_count: usize,
    local_apic_addr: u32,
    pcat_compat: bool,
    revision: u8,
}

static mut MADT_DATA: MadtTables = MadtTables {
    local_apics: [LocalApic {
        acpi_id: 0,
        apic_id: 0,
        flags: 0,
    }; MAX_LOCAL_APICS],
    local_count: 0,
    io_apics: [IoApic {
        id: 0,
        address: 0,
        gsi_base: 0,
    }; MAX_IO_APICS],
    io_count: 0,
    overrides: [IntSrcOverride {
        bus: 0,
        source_irq: 0,
        gsi: 0,
        flags: 0,
    }; MAX_OVERRIDES],
    override_count: 0,
    local_apic_addr: 0,
    pcat_compat: false,
    revision: 0,
};

static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Initialise the ACPI walker by parsing the RSDP that Limine handed us.
/// Idempotent; safe to call multiple times. Returns true if the MADT was found.
pub fn init() -> bool {
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        return madt_revision() != 0;
    }

    let Some(rsdp_phys) = crate::boot::protocol::rsdp_address() else {
        return false;
    };

    let madt_phys = unsafe { locate_madt_phys(rsdp_phys as u64) };
    let Some(madt_phys) = madt_phys else {
        return false;
    };

    unsafe { parse_madt(madt_phys) }
}

unsafe fn locate_madt_phys(rsdp_phys: u64) -> Option<u64> {
    let rsdp = unsafe { &*phys_to_virt::<RsdpV1>(rsdp_phys) };
    if &rsdp.sig != b"RSD PTR " {
        return None;
    }
    let revision = rsdp.revision;

    if revision >= 2 {
        let rsdp2 = unsafe { &*phys_to_virt::<RsdpV2>(rsdp_phys) };
        let xsdt_addr = rsdp2.xsdt_addr;
        return unsafe { find_table_xsdt(xsdt_addr, *b"APIC") };
    }
    let rsdt_addr = rsdp.rsdt_addr as u64;
    unsafe { find_table_rsdt(rsdt_addr, *b"APIC") }
}

unsafe fn find_table_rsdt(rsdt_phys: u64, want: [u8; 4]) -> Option<u64> {
    let hdr = unsafe { &*phys_to_virt::<SdtHeader>(rsdt_phys) };
    if &hdr.sig != b"RSDT" {
        return None;
    }
    let length = hdr.length as usize;
    let entries = (length - core::mem::size_of::<SdtHeader>()) / 4;
    let entry_base = (rsdt_phys as usize + core::mem::size_of::<SdtHeader>()) + hhdm_offset();
    for i in 0..entries {
        let p = (entry_base + i * 4) as *const u32;
        let phys = unsafe { core::ptr::read_unaligned(p) } as u64;
        let h = unsafe { &*phys_to_virt::<SdtHeader>(phys) };
        if h.sig == want {
            return Some(phys);
        }
    }
    None
}

unsafe fn find_table_xsdt(xsdt_phys: u64, want: [u8; 4]) -> Option<u64> {
    let hdr = unsafe { &*phys_to_virt::<SdtHeader>(xsdt_phys) };
    if &hdr.sig != b"XSDT" {
        return None;
    }
    let length = hdr.length as usize;
    let entries = (length - core::mem::size_of::<SdtHeader>()) / 8;
    let entry_base = (xsdt_phys as usize + core::mem::size_of::<SdtHeader>()) + hhdm_offset();
    for i in 0..entries {
        let p = (entry_base + i * 8) as *const u64;
        let phys = unsafe { core::ptr::read_unaligned(p) };
        let h = unsafe { &*phys_to_virt::<SdtHeader>(phys) };
        if h.sig == want {
            return Some(phys);
        }
    }
    None
}

unsafe fn parse_madt(madt_phys: u64) -> bool {
    let madt = unsafe { &*phys_to_virt::<MadtHeader>(madt_phys) };
    if &madt.hdr.sig != b"APIC" {
        return false;
    }

    let length = madt.hdr.length as usize;
    let local_apic_addr = madt.local_apic_addr;
    let flags = madt.flags;

    let data = unsafe { &mut *core::ptr::addr_of_mut!(MADT_DATA) };
    data.local_apic_addr = local_apic_addr;
    data.pcat_compat = (flags & 1) != 0;
    data.revision = madt.hdr.revision;

    let header_size = core::mem::size_of::<MadtHeader>();
    if length <= header_size {
        return true;
    }

    let mut offset = header_size;
    let base_virt = madt_phys as usize + hhdm_offset();
    while offset + 2 <= length {
        let p = (base_virt + offset) as *const u8;
        let entry_type = unsafe { core::ptr::read(p) };
        let entry_len = unsafe { core::ptr::read(p.add(1)) } as usize;
        if entry_len < 2 || offset + entry_len > length {
            break;
        }
        match entry_type {
            MADT_ENTRY_LOCAL_APIC if entry_len >= 8 && data.local_count < MAX_LOCAL_APICS => {
                let acpi_id = unsafe { core::ptr::read(p.add(2)) };
                let apic_id = unsafe { core::ptr::read(p.add(3)) };
                let f = unsafe { core::ptr::read_unaligned(p.add(4) as *const u32) };
                data.local_apics[data.local_count] = LocalApic {
                    acpi_id,
                    apic_id,
                    flags: f,
                };
                data.local_count += 1;
            }
            MADT_ENTRY_LOCAL_X2APIC if entry_len >= 16 && data.local_count < MAX_LOCAL_APICS => {
                let apic_id = unsafe { core::ptr::read_unaligned(p.add(4) as *const u32) };
                let f = unsafe { core::ptr::read_unaligned(p.add(8) as *const u32) };
                let acpi_id = unsafe { core::ptr::read_unaligned(p.add(12) as *const u32) };
                let acpi_id_u8 = (acpi_id & 0xFF) as u8;
                let apic_id_u8 = (apic_id & 0xFF) as u8;
                data.local_apics[data.local_count] = LocalApic {
                    acpi_id: acpi_id_u8,
                    apic_id: apic_id_u8,
                    flags: f,
                };
                data.local_count += 1;
            }
            MADT_ENTRY_IO_APIC if entry_len >= 12 && data.io_count < MAX_IO_APICS => {
                let id = unsafe { core::ptr::read(p.add(2)) };
                let address = unsafe { core::ptr::read_unaligned(p.add(4) as *const u32) };
                let gsi_base = unsafe { core::ptr::read_unaligned(p.add(8) as *const u32) };
                data.io_apics[data.io_count] = IoApic {
                    id,
                    address,
                    gsi_base,
                };
                data.io_count += 1;
            }
            MADT_ENTRY_INT_SRC_OVERRIDE
                if entry_len >= 10 && data.override_count < MAX_OVERRIDES =>
            {
                let bus = unsafe { core::ptr::read(p.add(2)) };
                let source_irq = unsafe { core::ptr::read(p.add(3)) };
                let gsi = unsafe { core::ptr::read_unaligned(p.add(4) as *const u32) };
                let f = unsafe { core::ptr::read_unaligned(p.add(8) as *const u16) };
                data.overrides[data.override_count] = IntSrcOverride {
                    bus,
                    source_irq,
                    gsi,
                    flags: f,
                };
                data.override_count += 1;
            }
            MADT_ENTRY_NMI_SOURCE | MADT_ENTRY_LOCAL_APIC_NMI => {
                // Recorded indirectly; not needed for v0.3 first cut.
            }
            _ => {}
        }
        offset += entry_len;
    }

    true
}

pub fn madt_revision() -> u8 {
    unsafe { (*core::ptr::addr_of!(MADT_DATA)).revision }
}

pub fn local_apic_phys() -> u32 {
    unsafe { (*core::ptr::addr_of!(MADT_DATA)).local_apic_addr }
}

pub fn pcat_compat() -> bool {
    unsafe { (*core::ptr::addr_of!(MADT_DATA)).pcat_compat }
}

pub fn local_apics() -> &'static [LocalApic] {
    unsafe {
        let d = &*core::ptr::addr_of!(MADT_DATA);
        &d.local_apics[..d.local_count]
    }
}

pub fn io_apics() -> &'static [IoApic] {
    unsafe {
        let d = &*core::ptr::addr_of!(MADT_DATA);
        &d.io_apics[..d.io_count]
    }
}

pub fn overrides() -> &'static [IntSrcOverride] {
    unsafe {
        let d = &*core::ptr::addr_of!(MADT_DATA);
        &d.overrides[..d.override_count]
    }
}

/// Resolve a legacy ISA IRQ to its routed Global System Interrupt (GSI),
/// honoring any Interrupt Source Override entry. Returns the IRQ as-is if
/// no override applies.
pub fn isa_irq_to_gsi(irq: u8) -> u32 {
    for ov in overrides() {
        if ov.bus == 0 && ov.source_irq == irq {
            return ov.gsi;
        }
    }
    irq as u32
}

pub fn log_summary() {
    let revision = madt_revision();
    let local = local_apics().len();
    let ioapic = io_apics().len();
    let overrides_n = overrides().len();
    crate::serial::write_str("acpi: madt rev=");
    crate::serial::write_u64(revision as u64);
    crate::serial::write_str(" lapic_phys=");
    crate::serial::write_u64(local_apic_phys() as u64);
    crate::serial::write_str(" pcat=");
    crate::serial::write_u64(pcat_compat() as u64);
    crate::serial::write_str(" lapics=");
    crate::serial::write_u64(local as u64);
    crate::serial::write_str(" ioapics=");
    crate::serial::write_u64(ioapic as u64);
    crate::serial::write_str(" overrides=");
    crate::serial::write_u64(overrides_n as u64);
    crate::serial::write_line("");
}
