// ---------------------------------------------------------------------------
// USB Protocol Implementation
//
// Implements USB enumeration protocol:
//   1. Get device descriptor
//   2. Assign device address
//   3. Configure device
//   4. Get HID report descriptor
//   5. Setup interrupt transfers
// ---------------------------------------------------------------------------

/// USB Request Types (bmRequestType)
pub const USB_REQUEST_TYPE_DIRECTION: u8 = 0x80;  // 1 = device-to-host
pub const USB_REQUEST_TYPE_TYPE: u8 = 0x60;       // 0 = standard, 1 = class, 2 = vendor
pub const USB_REQUEST_TYPE_RECIPIENT: u8 = 0x1F;  // 0 = device, 1 = interface, 2 = endpoint

/// USB Standard Requests
pub const USB_REQUEST_GET_DESCRIPTOR: u8 = 0x06;
pub const USB_REQUEST_SET_ADDRESS: u8 = 0x05;
pub const USB_REQUEST_SET_CONFIGURATION: u8 = 0x09;
pub const USB_REQUEST_GET_INTERFACE: u8 = 0x0A;
pub const USB_REQUEST_SET_INTERFACE: u8 = 0x0B;

/// USB Descriptor Types
pub const USB_DESCRIPTOR_TYPE_DEVICE: u8 = 0x01;
pub const USB_DESCRIPTOR_TYPE_CONFIGURATION: u8 = 0x02;
pub const USB_DESCRIPTOR_TYPE_STRING: u8 = 0x03;
pub const USB_DESCRIPTOR_TYPE_INTERFACE: u8 = 0x04;
pub const USB_DESCRIPTOR_TYPE_ENDPOINT: u8 = 0x05;
pub const USB_DESCRIPTOR_TYPE_HID: u8 = 0x21;
pub const USB_DESCRIPTOR_TYPE_HID_REPORT: u8 = 0x22;

/// USB Control Transfer Structure
#[repr(C)]
pub struct UsbSetupPacket {
    pub bmRequestType: u8,
    pub bRequest: u8,
    pub wValue: u16,          // [15:8] = high byte, [7:0] = low byte
    pub wIndex: u16,
    pub wLength: u16,         // Length of data phase
}

impl UsbSetupPacket {
    /// Create a GET_DESCRIPTOR request
    pub fn get_descriptor(descriptor_type: u8, descriptor_index: u8, language_id: u16, length: u16) -> Self {
        Self {
            bmRequestType: USB_REQUEST_TYPE_DIRECTION,  // Device-to-host, standard, device
            bRequest: USB_REQUEST_GET_DESCRIPTOR,
            wValue: ((descriptor_type as u16) << 8) | (descriptor_index as u16),
            wIndex: language_id,
            wLength: length,
        }
    }

    /// Create a SET_ADDRESS request
    pub fn set_address(address: u8) -> Self {
        Self {
            bmRequestType: 0x00,  // Host-to-device, standard, device
            bRequest: USB_REQUEST_SET_ADDRESS,
            wValue: address as u16,
            wIndex: 0,
            wLength: 0,
        }
    }

    /// Create a SET_CONFIGURATION request
    pub fn set_configuration(config_value: u8) -> Self {
        Self {
            bmRequestType: 0x00,  // Host-to-device, standard, device
            bRequest: USB_REQUEST_SET_CONFIGURATION,
            wValue: config_value as u16,
            wIndex: 0,
            wLength: 0,
        }
    }
}

/// USB Device Enumeration State Machine
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnumerationState {
    Idle,
    GettingDeviceDescriptor,
    SettingAddress,
    GettingConfigDescriptor,
    SettingConfiguration,
    GettingHidDescriptor,
    SettingIdle,
    Complete,
    Failed,
}

/// Result of enumeration operation
#[derive(Debug, Clone, Copy)]
pub struct EnumerationResult {
    pub device_address: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub hid_type: u8,  // 1=keyboard, 2=mouse
}

/// Enumeration context for a single device
pub struct EnumerationContext {
    pub state: EnumerationState,
    pub device_address: u8,
    pub port: u8,
    pub attempt: u8,
}

impl EnumerationContext {
    pub fn new(port: u8) -> Self {
        Self {
            state: EnumerationState::Idle,
            device_address: 0,
            port,
            attempt: 0,
        }
    }

    pub fn reset(&mut self) {
        self.state = EnumerationState::Idle;
        self.device_address = 0;
        self.attempt = 0;
    }
}

/// Enumerate a single USB device on the given port
pub fn enumerate_device(port: u8) -> Option<EnumerationResult> {
    // This is a placeholder for full enumeration
    // In production, would:
    // 1. Reset device
    // 2. Get device descriptor
    // 3. Assign address
    // 4. Get configuration descriptor
    // 5. Set configuration
    // 6. Get HID report descriptor
    // 7. Setup interrupt transfer
    
    crate::serial::write_str("usb: Attempting to enumerate device on port ");
    crate::serial::write_u32(port as u32);
    crate::serial::write_line("");

    // For now, return None (enumeration not yet implemented)
    // Full implementation would return actual device info
    None
}

/// Get the address to submit setup packet to endpoint 0 (control endpoint)
pub fn get_control_endpoint_address() -> u8 {
    0x00  // Endpoint 0, control endpoint
}

/// Check if enumeration is complete
pub fn is_enumeration_complete(result: Option<&EnumerationResult>) -> bool {
    result.is_some()
}
