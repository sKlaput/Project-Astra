// ---------------------------------------------------------------------------
// Astra OS — Ethernet frame layer
//
// Ethernet II frame:
//   [0..5]   destination MAC
//   [6..11]  source MAC
//   [12..13] EtherType (big-endian)
//   [14..]   payload
// ---------------------------------------------------------------------------

/// EtherType constants (big-endian wire values)
pub const ETH_ARP: u16 = 0x0806;
pub const ETH_IPV4: u16 = 0x0800;

pub const ETH_HDR: usize = 14;
pub const BROADCAST_MAC: [u8; 6] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];

// ── Parsed Ethernet frame header ──────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct EthHeader {
    pub etype: u16, // host byte order
}

impl EthHeader {
    /// Parse from the first 14 bytes of a frame.
    pub fn parse(frame: &[u8]) -> Option<Self> {
        if frame.len() < ETH_HDR {
            return None;
        }
        Some(EthHeader {
            etype: u16::from_be_bytes([frame[12], frame[13]]),
        })
    }
}

// ── Ethernet frame builder ────────────────────────────────────────────────────

/// Write an Ethernet header into `buf[0..14]`.
/// `buf` must be at least `ETH_HDR + payload.len()` bytes.
/// Returns total frame length written.
pub fn build_frame(
    buf: &mut [u8],
    dst: [u8; 6],
    src: [u8; 6],
    etype: u16,
    payload: &[u8],
) -> usize {
    let total = ETH_HDR + payload.len();
    if buf.len() < total {
        return 0;
    }
    buf[0..6].copy_from_slice(&dst);
    buf[6..12].copy_from_slice(&src);
    buf[12] = (etype >> 8) as u8;
    buf[13] = (etype & 0xFF) as u8;
    buf[ETH_HDR..total].copy_from_slice(payload);
    total
}
