// ---------------------------------------------------------------------------
// Astra OS — IPv4
//
// Minimal IPv4: parse headers, build headers, checksum.
//
// Header format (20 bytes, no options):
//   [0]      version(4) + IHL(4) = 0x45
//   [1]      DSCP/ECN = 0
//   [2..3]   total length (big-endian)
//   [4..5]   identification
//   [6..7]   flags + fragment offset = 0x40 0x00 (DF, no frag)
//   [8]      TTL
//   [9]      protocol (1=ICMP, 6=TCP, 17=UDP)
//   [10..11] header checksum
//   [12..15] source IP
//   [16..19] destination IP
// ---------------------------------------------------------------------------

pub const IPV4_HDR: usize = 20;

pub const PROTO_ICMP: u8 = 1;
pub const PROTO_TCP:  u8 = 6;
pub const PROTO_UDP:  u8 = 17;
#[derive(Clone, Copy, Debug)]
pub struct Ipv4Header {
    pub ihl:      u8,    // in bytes (always 20 for us)
    pub total_len: u16,
    pub id:       u16,
    pub ttl:      u8,
    pub protocol: u8,
    pub checksum: u16,
    pub src:      [u8; 4],
    pub dst:      [u8; 4],
}

impl Ipv4Header {
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < IPV4_HDR { return None; }
        let version_ihl = buf[0];
        if version_ihl >> 4 != 4 { return None; } // not IPv4
        let ihl_bytes = ((version_ihl & 0x0F) as usize) * 4;
        if ihl_bytes < IPV4_HDR || buf.len() < ihl_bytes { return None; }
        Some(Ipv4Header {
            ihl:       ihl_bytes as u8,
            total_len: u16::from_be_bytes([buf[2], buf[3]]),
            id:        u16::from_be_bytes([buf[4], buf[5]]),
            ttl:       buf[8],
            protocol:  buf[9],
            checksum:  u16::from_be_bytes([buf[10], buf[11]]),
            src:       buf[12..16].try_into().ok()?,
            dst:       buf[16..20].try_into().ok()?,
        })
    }

    /// Return the payload slice (after the IP header, trimmed to total_len).
    pub fn payload<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        let hdr = self.ihl as usize;
        let total = self.total_len as usize;
        let end = total.min(buf.len());
        if end <= hdr { &[] } else { &buf[hdr..end] }
    }
}

/// Build a 20-byte IPv4 header into `out[0..20]`.
/// `payload_len` is the length of the upper-layer payload (e.g. ICMP packet).
/// Returns the header bytes; caller must append payload.
pub fn build_header(out: &mut [u8; IPV4_HDR],
                    id: u16, ttl: u8, protocol: u8,
                    src: [u8; 4], dst: [u8; 4],
                    payload_len: usize) {
    let total = (IPV4_HDR + payload_len) as u16;
    out[0]  = 0x45;                          // version=4, IHL=5
    out[1]  = 0;                             // DSCP/ECN
    out[2]  = (total >> 8) as u8;
    out[3]  = (total & 0xFF) as u8;
    out[4]  = (id >> 8) as u8;
    out[5]  = (id & 0xFF) as u8;
    out[6]  = 0x40;                          // DF flag
    out[7]  = 0x00;                          // fragment offset = 0
    out[8]  = ttl;
    out[9]  = protocol;
    out[10] = 0; out[11] = 0;               // checksum placeholder
    out[12..16].copy_from_slice(&src);
    out[16..20].copy_from_slice(&dst);
    let csum = checksum(out);
    out[10] = (csum >> 8) as u8;
    out[11] = (csum & 0xFF) as u8;
}

/// Internet checksum (RFC 1071) over a slice.
pub fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 <= data.len() {
        sum += u16::from_be_bytes([data[i], data[i+1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}
