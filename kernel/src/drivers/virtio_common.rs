//! Common virtio driver utilities
//!
//! Shared code for virtio device drivers:
//! - Port I/O helpers
//! - PCI bus scanning and configuration
//! - Common virtio protocol constants

// ── Port I/O Helpers ──────────────────────────────────────────────────────────

/// Read 8-bit value from I/O port
pub unsafe fn in8(port: u16) -> u8 {
    let v: u8;
    unsafe {
        core::arch::asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack));
    }
    v
}

/// Read 16-bit value from I/O port
pub unsafe fn in16(port: u16) -> u16 {
    let v: u16;
    unsafe {
        core::arch::asm!("in ax, dx", out("ax") v, in("dx") port, options(nomem, nostack));
    }
    v
}

/// Read 32-bit value from I/O port
pub unsafe fn in32(port: u16) -> u32 {
    let v: u32;
    unsafe {
        core::arch::asm!("in eax, dx", out("eax") v, in("dx") port, options(nomem, nostack));
    }
    v
}

/// Write 8-bit value to I/O port
pub unsafe fn out8(port: u16, v: u8) {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack));
    }
}

/// Write 16-bit value to I/O port
pub unsafe fn out16(port: u16, v: u16) {
    unsafe {
        core::arch::asm!("out dx, ax", in("dx") port, in("ax") v, options(nomem, nostack));
    }
}

/// Write 32-bit value to I/O port
pub unsafe fn out32(port: u16, v: u32) {
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") port, in("eax") v, options(nomem, nostack));
    }
}

// ── PCI Configuration Space ───────────────────────────────────────────────────

const PCI_CFG_ADDR: u16 = 0xCF8;
const PCI_CFG_DATA: u16 = 0xCFC;

/// Build a PCI CONFIG_ADDRESS value for the given bus/device/function/offset
pub fn pci_addr(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    (1 << 31)
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((offset & 0xFC) as u32)
}

/// Read a 32-bit value from PCI configuration space
pub unsafe fn pci_read32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    unsafe {
        out32(PCI_CFG_ADDR, pci_addr(bus, dev, func, offset));
        in32(PCI_CFG_DATA)
    }
}

/// Read the I/O base address (BAR0) from a PCI device
/// Returns None if BAR0 is not I/O space (bit 0 == 0)
pub fn pci_io_base(bus: u8, dev: u8) -> Option<u16> {
    let bar0 = unsafe { pci_read32(bus, dev, 0, 0x10) };
    if bar0 & 1 == 0 {
        return None;
    }
    Some((bar0 & 0xFFFC) as u16)
}

// ── Virtio Protocol Constants ─────────────────────────────────────────────────

pub const VIRTIO_VENDOR: u16 = 0x1AF4;

pub const VIRTIO_BLK_DEV: u16 = 0x1001;
pub const VIRTIO_NET_DEV: u16 = 0x1000;

// ── Virtio Register Offsets ───────────────────────────────────────────────────

pub const VIO_DEV_FEAT: u16 = 0x00;
pub const VIO_DRV_FEAT: u16 = 0x04;
pub const VIO_QUEUE_PFN: u16 = 0x08;
pub const VIO_QUEUE_SIZE: u16 = 0x0C;
pub const VIO_QUEUE_SEL: u16 = 0x0E;
pub const VIO_QUEUE_NTF: u16 = 0x10;
pub const VIO_DEV_STATUS: u16 = 0x12;
pub const VIO_CFG: u16 = 0x14;

// ── Virtio Status Codes ───────────────────────────────────────────────────────

pub const STS_ACK: u8 = 0x01;
pub const STS_DRIVER: u8 = 0x02;
pub const STS_DRIVER_OK: u8 = 0x04;

// ── Helper Functions ──────────────────────────────────────────────────────────

/// Scan PCI bus 0 for a specific virtio device
pub fn find_virtio_device(device_id: u16) -> Option<(u8, u8)> {
    for dev in 0u8..32 {
        let id = unsafe { pci_read32(0, dev, 0, 0x00) };
        if id == 0xFFFF_FFFF {
            continue;
        }
        let vendor = (id & 0xFFFF) as u16;
        let device = (id >> 16) as u16;
        if vendor == VIRTIO_VENDOR && device == device_id {
            return Some((0, dev));
        }
    }
    None
}
