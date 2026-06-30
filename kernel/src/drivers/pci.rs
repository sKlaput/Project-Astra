// ---------------------------------------------------------------------------
// PCI Bus Enumeration
//
// Discovers and probes PCI devices on the system bus.
// Used to locate XHCI host controllers and network adapters.
// ---------------------------------------------------------------------------

use core::sync::atomic::{AtomicU32, Ordering};

/// PCI Configuration Space Register Offsets
pub const PCI_REG_VENDOR_ID: u16 = 0x00;      // Vendor ID (2 bytes)
pub const PCI_REG_DEVICE_ID: u16 = 0x02;      // Device ID (2 bytes)
pub const PCI_REG_COMMAND: u16 = 0x04;        // Command register (2 bytes)
pub const PCI_REG_STATUS: u16 = 0x06;         // Status register (2 bytes)
pub const PCI_REG_CLASS_CODE: u16 = 0x08;     // Class code (1 byte)
pub const PCI_REG_SUBCLASS: u16 = 0x09;       // Subclass (1 byte)
pub const PCI_REG_PROG_IF: u16 = 0x0A;        // Programming interface (1 byte)
pub const PCI_REG_BAR0: u16 = 0x10;           // Base Address Register 0 (4 bytes)
pub const PCI_REG_BAR1: u16 = 0x14;           // Base Address Register 1 (4 bytes)
pub const PCI_REG_INTERRUPT_LINE: u16 = 0x3C; // Interrupt line (1 byte)

/// PCI Command Register Bits
pub const PCI_CMD_IO_SPACE: u16 = 0x0001;     // Enable I/O space
pub const PCI_CMD_MEMORY_SPACE: u16 = 0x0002; // Enable memory space
pub const PCI_CMD_BUS_MASTER: u16 = 0x0004;   // Enable bus mastering

/// PCI Status Register Bits
pub const PCI_STS_CAP_LIST: u16 = 0x0010;     // Capabilities list available

/// PCI Base Class Codes
pub const PCI_CLASS_UNCLASSIFIED: u8 = 0x00;
pub const PCI_CLASS_MASS_STORAGE: u8 = 0x01;
pub const PCI_CLASS_NETWORK: u8 = 0x02;
pub const PCI_CLASS_DISPLAY: u8 = 0x03;
pub const PCI_CLASS_MULTIMEDIA: u8 = 0x04;
pub const PCI_CLASS_MEMORY: u8 = 0x05;
pub const PCI_CLASS_BRIDGE: u8 = 0x06;
pub const PCI_CLASS_COMMUNICATION: u8 = 0x07;
pub const PCI_CLASS_GENERIC: u8 = 0x08;
pub const PCI_CLASS_INPUT: u8 = 0x09;
pub const PCI_CLASS_DOCKING: u8 = 0x0A;
pub const PCI_CLASS_PROCESSOR: u8 = 0x0B;
pub const PCI_CLASS_SERIAL_BUS: u8 = 0x0C;    // USB controllers in this class
pub const PCI_CLASS_WIRELESS: u8 = 0x0D;
pub const PCI_CLASS_INTELLIGENT_IO: u8 = 0x0E;
pub const PCI_CLASS_SATELLITE: u8 = 0x0F;

/// PCI Serial Bus (0x0C) Subclass Codes
pub const PCI_SUBCLASS_SERIAL_FIREWIRE: u8 = 0x00;
pub const PCI_SUBCLASS_SERIAL_ACCESS: u8 = 0x01;
pub const PCI_SUBCLASS_SERIAL_SSA: u8 = 0x02;
pub const PCI_SUBCLASS_SERIAL_USB: u8 = 0x03;   // USB controllers
pub const PCI_SUBCLASS_SERIAL_FIBRE: u8 = 0x04;
pub const PCI_SUBCLASS_SERIAL_SMBUS: u8 = 0x05;
pub const PCI_SUBCLASS_SERIAL_INFINIBAND: u8 = 0x06;
pub const PCI_SUBCLASS_SERIAL_IPMI: u8 = 0x07;

/// USB Controller Programming Interface Codes (for Serial Bus, USB)
pub const PCI_PROG_IF_USB_UHCI: u8 = 0x00;    // UHCI controller
pub const PCI_PROG_IF_USB_OHCI: u8 = 0x10;    // OHCI controller
pub const PCI_PROG_IF_USB_EHCI: u8 = 0x20;    // EHCI controller
pub const PCI_PROG_IF_USB_XHCI: u8 = 0x30;    // XHCI controller

/// PCI Device Info
#[derive(Debug, Clone, Copy)]
pub struct PciDeviceInfo {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub bar0: u32,
    pub bar1: u32,
    pub interrupt_line: u8,
}

impl PciDeviceInfo {
    /// Check if this device is an XHCI controller
    pub fn is_xhci_controller(&self) -> bool {
        self.class == PCI_CLASS_SERIAL_BUS
            && self.subclass == PCI_SUBCLASS_SERIAL_USB
            && self.prog_if == PCI_PROG_IF_USB_XHCI
    }

    /// Get the MMIO base address from BAR0 (assumes 32-bit BAR)
    pub fn mmio_base_address(&self) -> Option<u64> {
        if self.bar0 == 0 {
            return None;
        }
        // Mask off the low bits (memory type, prefetchable)
        let base = (self.bar0 & 0xFFFFFFF0) as u64;
        if base != 0 {
            Some(base)
        } else {
            None
        }
    }
}

/// Read a 32-bit value from PCI configuration space using CF8/CFC I/O ports
fn pci_config_read32(bus: u8, slot: u8, function: u8, offset: u16) -> u32 {
    let address = 0x80000000u32
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);

    unsafe {
        core::arch::asm!("out eax, dx", in("eax") address, in("dx") 0xCF8u16, options(nomem, nostack));
        let mut result: u32;
        core::arch::asm!("in eax, dx", out("eax") result, in("dx") 0xCFCu16, options(nomem, nostack));
        result
    }
}

/// Read a 16-bit value from PCI configuration space
fn pci_config_read16(bus: u8, slot: u8, function: u8, offset: u16) -> u16 {
    let value = pci_config_read32(bus, slot, function, offset);
    ((value >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

/// Read an 8-bit value from PCI configuration space
fn pci_config_read8(bus: u8, slot: u8, function: u8, offset: u16) -> u8 {
    let value = pci_config_read32(bus, slot, function, offset);
    ((value >> ((offset & 3) * 8)) & 0xFF) as u8
}

/// Probe a single PCI device
fn probe_pci_device(bus: u8, slot: u8, function: u8) -> Option<PciDeviceInfo> {
    let vendor_id = pci_config_read16(bus, slot, function, PCI_REG_VENDOR_ID);

    // 0xFFFF means device not present
    if vendor_id == 0xFFFF {
        return None;
    }

    let device_id = pci_config_read16(bus, slot, function, PCI_REG_DEVICE_ID);
    let class = pci_config_read8(bus, slot, function, PCI_REG_CLASS_CODE);
    let subclass = pci_config_read8(bus, slot, function, PCI_REG_SUBCLASS);
    let prog_if = pci_config_read8(bus, slot, function, PCI_REG_PROG_IF);
    let bar0 = pci_config_read32(bus, slot, function, PCI_REG_BAR0);
    let bar1 = pci_config_read32(bus, slot, function, PCI_REG_BAR1);
    let interrupt_line = pci_config_read8(bus, slot, function, PCI_REG_INTERRUPT_LINE);

    Some(PciDeviceInfo {
        bus,
        slot,
        function,
        vendor_id,
        device_id,
        class,
        subclass,
        prog_if,
        bar0,
        bar1,
        interrupt_line,
    })
}

/// Enumerate all PCI devices on the bus and call callback for each
pub fn enumerate_pci<F>(mut callback: F) -> u32
where
    F: FnMut(&PciDeviceInfo),
{
    let mut count = 0;

    // Scan bus 0 (primary bus), all slots and functions
    for slot in 0..32 {
        for function in 0..8 {
            if let Some(device) = probe_pci_device(0, slot, function) {
                callback(&device);
                count += 1;
            }
        }
    }

    count
}

/// Find the first XHCI controller on the PCI bus
pub fn find_xhci_controller() -> Option<PciDeviceInfo> {
    let mut found: Option<PciDeviceInfo> = None;

    enumerate_pci(|device| {
        if device.is_xhci_controller() && found.is_none() {
            found = Some(*device);
        }
    });

    found
}
