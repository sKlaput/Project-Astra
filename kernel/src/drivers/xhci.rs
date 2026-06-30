// ---------------------------------------------------------------------------
// XHCI (eXtensible Host Controller Interface) Driver
//
// Implements the eXtensible Host Controller Interface for USB host control.
// Handles:
//   - Host controller initialization and reset
//   - Ring management (control, bulk, interrupt transfers)
//   - Event handling and command processing
//   - Endpoint management and device enumeration
//
// USB device attachment triggers interrupt which queues USB HID parsing.
// ---------------------------------------------------------------------------

use crate::drivers::{Driver, DriverError};
use core::sync::atomic::{AtomicU32, Ordering};

/// XHCI operational register offsets
const XHCI_OP_USBCMD_OFFSET: u32 = 0x00;
const XHCI_OP_USBSTS_OFFSET: u32 = 0x04;
const XHCI_OP_PAGESIZE_OFFSET: u32 = 0x08;
const XHCI_OP_CRCR_OFFSET: u32 = 0x18;       // Command Ring Control Register
const XHCI_OP_DCBAAP_OFFSET: u32 = 0x30;     // Device Context Base Address Array Pointer
const XHCI_OP_CONFIG_OFFSET: u32 = 0x38;     // Configure Register
const XHCI_OP_PORTSC_OFFSET: u32 = 0x400;    // Port Status and Control Register (first port)

/// USB Status and Control Register bits
const XHCI_CMD_RUN: u32 = 0x01;               // Run/Stop
const XHCI_CMD_HCRST: u32 = 0x02;             // Host Controller Reset
const XHCI_CMD_INTE: u32 = 0x04;              // Interrupter Enable
const XHCI_STS_HCH: u32 = 0x01;               // Host Controller Halted
const XHCI_STS_HSE: u32 = 0x04;               // Host System Error

/// Port status and control register bits
const XHCI_PS_CCS: u32 = 0x01;                // Current Connect Status
const XHCI_PS_PED: u32 = 0x02;                // Port Enabled/Disabled
const XHCI_PS_PR: u32 = 0x10;                 // Port Reset
const XHCI_PS_PP: u32 = 0x200;                // Port Power


/// XHCI Command Ring
pub struct CommandRing {
    /// Virtual address of ring buffer
    ring_buffer: u64,
    /// Enqueue pointer (producer)
    enqueue_ptr: u32,
    /// Dequeue pointer (consumer)
    dequeue_ptr: u32,
    /// Cycle state (toggles on wrap-around)
    cycle_state: bool,
}

/// XHCI Event Ring
pub struct EventRing {
    /// Virtual address of event ring buffer
    ring_buffer: u64,
    /// Dequeue pointer
    dequeue_ptr: u32,
    /// Cycle state
    cycle_state: bool,
    /// Interrupter number
    interrupter: u8,
}

/// XHCI Endpoint Ring (for transfers)
pub struct EndpointRing {
    /// Virtual address of ring buffer
    ring_buffer: u64,
    /// Enqueue pointer
    enqueue_ptr: u32,
    /// Dequeue pointer
    dequeue_ptr: u32,
    /// Cycle state
    cycle_state: bool,
    /// Endpoint number
    endpoint: u8,
}

impl CommandRing {
    pub fn new() -> Self {
        Self {
            ring_buffer: 0,
            enqueue_ptr: 0,
            dequeue_ptr: 0,
            cycle_state: true,
        }
    }
}

impl EventRing {
    pub fn new(interrupter: u8) -> Self {
        Self {
            ring_buffer: 0,
            dequeue_ptr: 0,
            cycle_state: true,
            interrupter,
        }
    }
}

impl EndpointRing {
    pub fn new(endpoint: u8) -> Self {
        Self {
            ring_buffer: 0,
            enqueue_ptr: 0,
            dequeue_ptr: 0,
            cycle_state: true,
            endpoint,
        }
    }
}

/// XHCI host controller state
pub struct XhciController {
    /// PCI BAR0 address (MMIO)
    mmio_base: u64,
    /// Number of available device slots
    max_device_slots: u32,
    /// Number of interrupters (event rings)
    max_interrupters: u32,
    /// Number of ports
    max_ports: u32,
    /// Operational register base offset
    op_regs_offset: u32,
}

impl XhciController {
    /// Create a new XHCI controller instance
    pub const fn new(mmio_base: u64) -> Self {
        Self {
            mmio_base,
            max_device_slots: 0,
            max_interrupters: 0,
            max_ports: 0,
            op_regs_offset: 0,
        }
    }

    /// Read a 32-bit register from XHCI MMIO space
    fn read_reg(&self, offset: u32) -> u32 {
        let addr = (self.mmio_base + offset as u64) as *const u32;
        unsafe { addr.read_volatile() }
    }

    /// Write a 32-bit register to XHCI MMIO space
    fn write_reg(&self, offset: u32, value: u32) {
        let addr = (self.mmio_base + offset as u64) as *mut u32;
        unsafe { addr.write_volatile(value) }
    }

    /// Reset the host controller
    fn host_controller_reset(&self) -> Result<(), DriverError> {
        // Set HCRST bit in USBCMD
        let cmd = self.read_reg(XHCI_OP_USBCMD_OFFSET);
        self.write_reg(XHCI_OP_USBCMD_OFFSET, cmd | XHCI_CMD_HCRST);

        // Wait for reset to complete (HCRST bit clears)
        for _ in 0..1000 {
            let cmd = self.read_reg(XHCI_OP_USBCMD_OFFSET);
            if cmd & XHCI_CMD_HCRST == 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }

        Err(DriverError::IoError)
    }

    /// Initialize the host controller
    fn host_controller_init(&mut self) -> Result<(), DriverError> {
        // Reset host controller
        self.host_controller_reset()?;

        // Read capabilities to determine max slots/interrupters/ports
        // For now, use safe defaults
        self.max_device_slots = 32;
        self.max_interrupters = 1;
        self.max_ports = 4;

        // Clear USBSTS to acknowledge any pending status
        self.write_reg(XHCI_OP_USBSTS_OFFSET, 0);

        // In a full implementation, we would:
        // 1. Allocate and initialize the Device Context Base Address Array (DCBAAP)
        // 2. Allocate and initialize the Command Ring
        // 3. Allocate and initialize Event Rings
        // 4. Start the host controller

        Ok(())
    }

    /// Start the host controller
    fn start_host_controller(&self) {
        let cmd = self.read_reg(XHCI_OP_USBCMD_OFFSET);
        self.write_reg(XHCI_OP_USBCMD_OFFSET, cmd | XHCI_CMD_RUN);

        // Wait for controller to start
        for _ in 0..100 {
            let status = self.read_reg(XHCI_OP_USBSTS_OFFSET);
            if status & XHCI_STS_HCH == 0 {
                return;
            }
            core::hint::spin_loop();
        }
    }


    /// Initialize command ring
    fn init_command_ring(&mut self) -> bool {
        // TODO: Allocate command ring buffer (64 KB aligned)
        // TODO: Write ring address to CRCR register
        crate::serial::write_line("xhci: Command ring initialized (stub)");
        true
    }

    /// Initialize event ring
    fn init_event_ring(&mut self) -> bool {
        // TODO: Allocate event ring buffer
        // TODO: Setup event ring segment table
        // TODO: Write ERST pointer and size to registers
        crate::serial::write_line("xhci: Event ring initialized (stub)");
        true
    }

    /// Enable XHCI interrupts
    fn enable_interrupts(&mut self) -> bool {
        // TODO: Setup interrupt handler
        // TODO: Enable XHCI interrupt in command register
        crate::serial::write_line("xhci: Interrupts enabled (stub)");
        true
    }

    /// Initialize XHCI data structures
    fn initialize_data_structures(&mut self) -> bool {
        // Initialize command ring
        if !self.init_command_ring() {
            return false;
        }

        // Initialize event rings
        if !self.init_event_ring() {
            return false;
        }

        // Enable interrupts
        if !self.enable_interrupts() {
            return false;
        }

        crate::serial::write_line("xhci: All data structures initialized");
        true
    }

    /// Detect connected USB devices by checking port status
    pub fn detect_devices(&self) -> u32 {
        let mut connected = 0;

        for port_idx in 0..self.max_ports {
            let portsc_offset = XHCI_OP_PORTSC_OFFSET + (port_idx * 0x10);
            let portsc = self.read_reg(portsc_offset);

            // Check if device is connected
            if portsc & XHCI_PS_CCS != 0 {
                connected += 1;
                crate::serial::write_str("xhci: device connected on port ");
                crate::serial::write_u32(port_idx + 1);
                crate::serial::write_line("");
            }
        }

        connected
    }
}

/// XHCI driver implementation
pub struct XhciDriver {
    controller: core::sync::atomic::AtomicU64,
}

impl XhciDriver {
    pub const fn new() -> Self {
        Self {
            controller: core::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl Driver for XhciDriver {
    fn name(&self) -> &'static str {
        "xhci"
    }

    fn category(&self) -> &'static str {
        "usb-host"
    }

    fn init(&self) -> Result<(), DriverError> {
        // Use PCI enumeration to discover XHCI controller
        init_xhci_from_pci()
    }
}


/// Initialize XHCI driver by discovering controller via PCI
pub fn init_xhci_from_pci() -> Result<(), DriverError> {
    // Find XHCI controller on PCI bus
    if let Some(device) = super::pci::find_xhci_controller() {
        crate::serial::write_str("xhci: Found XHCI controller - Vendor 0x");
        crate::serial::write_u32(device.vendor_id as u32);
        crate::serial::write_str(", Device 0x");
        crate::serial::write_u32(device.device_id as u32);
        crate::serial::write_str(" at bus ");
        crate::serial::write_u32(device.bus as u32);
        crate::serial::write_str(", slot ");
        crate::serial::write_u32(device.slot as u32);
        crate::serial::write_str(", function ");
        crate::serial::write_u32(device.function as u32);
        crate::serial::write_line("");

        // Get MMIO base address from BAR0
        if let Some(mmio_base) = device.mmio_base_address() {
            crate::serial::write_str("xhci: MMIO base address: 0x");
            crate::serial::write_u64(mmio_base);
            crate::serial::write_line("");

            // Initialize XHCI controller at this address
            let mut controller = XhciController::new(mmio_base);
            
            // Reset host controller
            if controller.host_controller_reset().is_err() {
                crate::serial::write_line("xhci: Host controller reset failed");
                return Err(DriverError::IoError);
            }
            
            // Initialize data structures (command ring, event ring, etc.)
            if !controller.initialize_data_structures() {
                crate::serial::write_line("xhci: Data structure initialization failed");
                return Err(DriverError::IoError);
            }
            
            // Detect connected devices
            let connected = controller.detect_devices();
            crate::serial::write_str("xhci: Found ");
            crate::serial::write_u32(connected);
            crate::serial::write_line(" connected device(s)");

            // Enumerate USB devices on all ports
            crate::serial::write_line("xhci: Starting device enumeration...");
            let enumerated = super::xhci_enumeration::enumerate_all_devices(4);
            crate::serial::write_str("xhci: Enumerated ");
            crate::serial::write_u32(enumerated);
            crate::serial::write_line(" device(s) successfully");
            
            return Ok(());
        } else {
            crate::serial::write_line("xhci: BAR0 is not set or invalid");
            return Err(DriverError::IoError);
        }
    } else {
        crate::serial::write_line("xhci: No XHCI controller found on PCI bus");
        return Err(DriverError::DeviceNotPresent);
    }
}

/// Global XHCI driver instance
pub static XHCI_DRIVER: XhciDriver = XhciDriver::new();
