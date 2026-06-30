// ---------------------------------------------------------------------------
// USB HID (Human Interface Device) Support
//
// Implements USB HID protocol support for keyboard and mouse devices.
// Works with XHCI host controller for USB communication.
//
// HID devices report their input through interrupt transfers:
//   - Keyboard: 8-byte reports (modifier keys, reserved, 6 keycodes)
//   - Mouse: 3-4 byte reports (buttons, x_delta, y_delta, [wheel])
// ---------------------------------------------------------------------------

use crate::drivers::{Driver, DriverError};

/// USB Device Descriptor (simplified)
#[repr(C)]
pub struct UsbDeviceDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bcdUSB: u16,
    pub bDeviceClass: u8,
    pub bDeviceSubClass: u8,
    pub bDeviceProtocol: u8,
    pub bMaxPacketSize0: u8,
    pub idVendor: u16,
    pub idProduct: u16,
    pub bcdDevice: u16,
    pub iManufacturer: u8,
    pub iProduct: u8,
    pub iSerialNumber: u8,
    pub bNumConfigurations: u8,
}

/// USB Configuration Descriptor (simplified)
#[repr(C)]
pub struct UsbConfigDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub wTotalLength: u16,
    pub bNumInterfaces: u8,
    pub bConfigurationValue: u8,
    pub iConfiguration: u8,
    pub bmAttributes: u8,
    pub bMaxPower: u8,
}

/// USB Interface Descriptor (simplified)
#[repr(C)]
pub struct UsbInterfaceDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bInterfaceNumber: u8,
    pub bAlternateSetting: u8,
    pub bNumEndpoints: u8,
    pub bInterfaceClass: u8,
    pub bInterfaceSubClass: u8,
    pub bInterfaceProtocol: u8,
    pub iInterface: u8,
}

/// USB Endpoint Descriptor (simplified)
#[repr(C)]
pub struct UsbEndpointDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bEndpointAddress: u8,
    pub bmAttributes: u8,
    pub wMaxPacketSize: u16,
    pub bInterval: u8,
}

/// HID Class descriptor
#[repr(C)]
pub struct UsbHidDescriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bcdHID: u16,
    pub bCountryCode: u8,
    pub bNumDescriptors: u8,
    // Report descriptor info follows
}

/// HID Report Types
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum HidReportType {
    Reserved = 0,
    Input = 1,
    Output = 2,
    Feature = 3,
}

/// USB Device Class codes
pub const USB_CLASS_HID: u8 = 0x03;
pub const HID_SUBCLASS_BOOT: u8 = 0x01;
pub const HID_PROTOCOL_KEYBOARD: u8 = 0x01;
pub const HID_PROTOCOL_MOUSE: u8 = 0x02;

/// USB keyboard HID report format (8 bytes)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UsbHidKeyboardReport {
    pub modifier: u8,          // Modifier keys (Shift, Ctrl, Alt, etc.)
    pub reserved: u8,          // Always 0
    pub keycodes: [u8; 6],     // Up to 6 pressed keys (HID keycodes)
}

/// USB mouse HID report format (3-4 bytes typical)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UsbHidMouseReport {
    pub buttons: u8,           // Button states (bit 0=left, bit 1=right, bit 2=middle)
    pub x_delta: i8,           // X movement (-127 to +127)
    pub y_delta: i8,           // Y movement (-127 to +127)
    pub wheel_delta: i8,       // Wheel movement (optional)
}

/// USB HID Keyboard Driver
pub struct UsbHidKeyboardDriver {
    initialized: core::sync::atomic::AtomicBool,
}

impl UsbHidKeyboardDriver {
    pub const fn new() -> Self {
        Self {
            initialized: core::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Process a keyboard HID report
    pub fn process_report(&self, report: &UsbHidKeyboardReport) {
        // Convert HID keycodes to PS/2 scancodes and push to keyboard buffer
        // This allows USB keyboards to work with existing terminal driver

        for keycode in &report.keycodes {
            if *keycode == 0 {
                continue;  // Empty slot
            }

            // Map HID keycode to PS/2 scancode
            // (simplified mapping - real implementation would be comprehensive)
            let scancode = hid_keycode_to_ps2_scancode(*keycode);
            if scancode != 0 {
                crate::drivers::keyboard::push_scancode_from_poll(scancode);
            }
        }
    }
}

impl crate::drivers::Driver for UsbHidKeyboardDriver {
    fn name(&self) -> &'static str {
        "usb-hid-keyboard"
    }

    fn category(&self) -> &'static str {
        "input"
    }

    fn init(&self) -> Result<(), DriverError> {
        self.initialized.store(true, core::sync::atomic::Ordering::Release);
        crate::serial::write_line("drivers: usb-hid-keyboard initialized");
        Ok(())
    }
}

/// USB HID Mouse Driver
pub struct UsbHidMouseDriver {
    initialized: core::sync::atomic::AtomicBool,
}

impl UsbHidMouseDriver {
    pub const fn new() -> Self {
        Self {
            initialized: core::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Process a mouse HID report
    pub fn process_report(&self, report: &UsbHidMouseReport) {
        // Convert USB HID mouse report to internal mouse format
        // and push to mouse packet buffer

        crate::drivers::mouse::push_movement(
            report.x_delta as i16,
            report.y_delta as i16,
            report.buttons,
        );
    }
}

impl crate::drivers::Driver for UsbHidMouseDriver {
    fn name(&self) -> &'static str {
        "usb-hid-mouse"
    }

    fn category(&self) -> &'static str {
        "input"
    }

    fn init(&self) -> Result<(), DriverError> {
        self.initialized.store(true, core::sync::atomic::Ordering::Release);
        crate::serial::write_line("drivers: usb-hid-mouse initialized");
        Ok(())
    }
}

/// Convert HID keycode to PS/2 scancode
/// This is a simplified mapping for common keys
fn hid_keycode_to_ps2_scancode(hid_code: u8) -> u8 {
    match hid_code {
        0x04 => 0x1E,  // A
        0x05 => 0x30,  // B
        0x06 => 0x2E,  // C
        0x07 => 0x20,  // D
        0x08 => 0x12,  // E
        0x09 => 0x21,  // F
        0x0A => 0x22,  // G
        0x0B => 0x23,  // H
        0x0C => 0x17,  // I
        0x0D => 0x24,  // J
        0x0E => 0x25,  // K
        0x0F => 0x26,  // L
        0x10 => 0x32,  // M
        0x11 => 0x31,  // N
        0x12 => 0x18,  // O
        0x13 => 0x19,  // P
        0x14 => 0x10,  // Q
        0x15 => 0x13,  // R
        0x16 => 0x1F,  // S
        0x17 => 0x14,  // T
        0x18 => 0x16,  // U
        0x19 => 0x2F,  // V
        0x1A => 0x11,  // W
        0x1B => 0x2D,  // X
        0x1C => 0x15,  // Y
        0x1D => 0x2C,  // Z
        0x27 => 0x0B,  // 0
        0x1E => 0x02,  // 1
        0x1F => 0x03,  // 2
        0x20 => 0x04,  // 3
        0x21 => 0x05,  // 4
        0x22 => 0x06,  // 5
        0x23 => 0x07,  // 6
        0x24 => 0x08,  // 7
        0x25 => 0x09,  // 8
        0x26 => 0x0A,  // 9
        0x28 => 0x1C,  // Return
        0x29 => 0x01,  // Escape
        0x2A => 0x0E,  // Backspace
        0x2B => 0x0F,  // Tab
        0x2C => 0x39,  // Space
        0x2D => 0x0C,  // Minus
        0x2E => 0x0D,  // Equals
        _    => 0,     // Unmapped
    }
}

/// Global USB HID Keyboard driver instance
pub static USB_HID_KEYBOARD_DRIVER: UsbHidKeyboardDriver = UsbHidKeyboardDriver::new();

/// Global USB HID Mouse driver instance
pub static USB_HID_MOUSE_DRIVER: UsbHidMouseDriver = UsbHidMouseDriver::new();
