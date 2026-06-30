// ---------------------------------------------------------------------------
// USB Device Enumeration
//
// Implements the USB enumeration state machine:
//   1. Detect device connection
//   2. Reset device
//   3. Get device descriptor
//   4. Assign address
//   5. Get configuration descriptor
//   6. Set configuration
//   7. Get HID report descriptor
// ---------------------------------------------------------------------------

use super::usb_protocol;

/// Device enumeration states
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnumState {
    Disconnected,
    Connected,
    Resetting,
    GettingDevDescriptor,
    SettingAddress,
    GettingConfigDescriptor,
    SettingConfig,
    GettingHidDescriptor,
    Complete,
    Failed,
}

/// Enumeration context for a device
pub struct DeviceEnum {
    pub port: u8,
    pub state: EnumState,
    pub device_address: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub hid_type: u8,  // 1=keyboard, 2=mouse
    pub retry_count: u8,
}

impl DeviceEnum {
    pub fn new(port: u8) -> Self {
        Self {
            port,
            state: EnumState::Disconnected,
            device_address: 0,
            vendor_id: 0,
            product_id: 0,
            hid_type: 0,
            retry_count: 0,
        }
    }

    /// Process device enumeration state machine
    pub fn process_step(&mut self) -> bool {
        match self.state {
            EnumState::Disconnected => {
                // Check if device is connected (would read port status)
                self.state = EnumState::Connected;
                crate::serial::write_str("usb: Device detected on port ");
                crate::serial::write_u32(self.port as u32);
                crate::serial::write_line("");
                true
            }

            EnumState::Connected => {
                // Reset device
                self.state = EnumState::Resetting;
                crate::serial::write_line("usb: Resetting device...");
                true
            }

            EnumState::Resetting => {
                // Wait for reset to complete, then get device descriptor
                self.state = EnumState::GettingDevDescriptor;
                crate::serial::write_line("usb: Device reset complete, requesting descriptor");
                true
            }

            EnumState::GettingDevDescriptor => {
                // Get device descriptor (would execute control transfer)
                // For simulation: use test values
                self.vendor_id = 0x1234;
                self.product_id = 0x5678;
                self.state = EnumState::SettingAddress;
                
                crate::serial::write_str("usb: Got device descriptor - vendor 0x");
                crate::serial::write_u32(self.vendor_id as u32);
                crate::serial::write_str(", product 0x");
                crate::serial::write_u32(self.product_id as u32);
                crate::serial::write_line("");
                true
            }

            EnumState::SettingAddress => {
                // Assign address to device
                self.device_address = 1;  // Would be incremented for each device
                self.state = EnumState::GettingConfigDescriptor;
                
                crate::serial::write_str("usb: Device assigned address ");
                crate::serial::write_u32(self.device_address as u32);
                crate::serial::write_line("");
                true
            }

            EnumState::GettingConfigDescriptor => {
                // Get configuration descriptor
                self.state = EnumState::SettingConfig;
                crate::serial::write_line("usb: Got configuration descriptor");
                true
            }

            EnumState::SettingConfig => {
                // Set configuration
                self.state = EnumState::GettingHidDescriptor;
                crate::serial::write_line("usb: Configuration set");
                true
            }

            EnumState::GettingHidDescriptor => {
                // Get HID report descriptor (determine if keyboard or mouse)
                // For simulation: alternate between keyboard and mouse
                self.hid_type = if self.port % 2 == 0 { 1 } else { 2 };
                self.state = EnumState::Complete;
                
                let device_type = if self.hid_type == 1 { "Keyboard" } else { "Mouse" };
                crate::serial::write_str("usb: ");
                crate::serial::write_str(device_type);
                crate::serial::write_line(" HID device enumerated successfully!");
                true
            }

            EnumState::Complete => {
                crate::serial::write_str("usb: Enumeration complete for device on port ");
                crate::serial::write_u32(self.port as u32);
                crate::serial::write_line("");
                true
            }

            EnumState::Failed => {
                crate::serial::write_str("usb: Enumeration failed for port ");
                crate::serial::write_u32(self.port as u32);
                crate::serial::write_line("");
                false
            }
        }
    }

    /// Run enumeration to completion
    pub fn enumerate(&mut self) -> bool {
        // State machine: process steps until complete or failed
        while self.state != EnumState::Complete && self.state != EnumState::Failed {
            if !self.process_step() {
                self.state = EnumState::Failed;
                return false;
            }
        }

        self.state == EnumState::Complete
    }

    /// Check if enumeration is complete
    pub fn is_complete(&self) -> bool {
        self.state == EnumState::Complete
    }

    /// Get device type name
    pub fn device_type_name(&self) -> &'static str {
        match self.hid_type {
            1 => "Keyboard",
            2 => "Mouse",
            _ => "Unknown",
        }
    }
}

/// Enumerate all ports and devices
pub fn enumerate_all_devices(max_ports: u32) -> u32 {
    let mut devices_found = 0;

    for port in 0..max_ports {
        let mut dev_enum = DeviceEnum::new(port as u8);

        // Simulate device detection on even ports
        if port % 2 == 0 && devices_found < 4 {
            if dev_enum.enumerate() {
                devices_found += 1;

                crate::serial::write_str("usb: Registered ");
                crate::serial::write_str(dev_enum.device_type_name());
                crate::serial::write_str(" device (vendor 0x");
                crate::serial::write_u32(dev_enum.vendor_id as u32);
                crate::serial::write_str(", product 0x");
                crate::serial::write_u32(dev_enum.product_id as u32);
                crate::serial::write_line(")");
            }
        }
    }

    devices_found
}
