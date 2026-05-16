// ---------------------------------------------------------------------------
// Astra OS — DNS (Domain Name System) client
//
// RFC 1035 — sends a single A-record (IPv4) query to QEMU's slirp DNS
// forwarder at 10.0.2.3:53 and parses the response.
//
// Query format:
//   Header (12 bytes): txid | flags=0x0100 | qdcount=1 | 0 | 0 | 0
//   Question: encoded name | type=A (0x0001) | class=IN (0x0001)
//
// Response parsing: skip header, re-skip question, read first A-record answer.
// ---------------------------------------------------------------------------

use crate::net::{config, arp, udp};
use crate::arch::x86_64::interrupts::uptime_ms;

const DNS_PORT: u16 = 53;
const DNS_SERVER: [u8; 4] = [10, 0, 2, 3]; // QEMU slirp DNS

const MAX_DNS_PAYLOAD: usize = 256;

// ── Query builder ─────────────────────────────────────────────────────────────

/// Encode a DNS name (e.g. "google.com") into wire format in `out`.
/// Returns the number of bytes written, or 0 on error.
fn encode_name(name: &str, out: &mut [u8]) -> usize {
    let mut pos = 0usize;
    for label in name.split('.') {
        let lb = label.as_bytes();
        if lb.is_empty() || lb.len() > 63 { return 0; }
        if pos + 1 + lb.len() >= out.len() { return 0; }
        out[pos] = lb.len() as u8;
        pos += 1;
        out[pos..pos + lb.len()].copy_from_slice(lb);
        pos += lb.len();
    }
    if pos >= out.len() { return 0; }
    out[pos] = 0;  // root label
    pos + 1
}

fn build_query(name: &str, txid: u16, buf: &mut [u8; MAX_DNS_PAYLOAD]) -> usize {
    // Header
    buf[0] = (txid >> 8) as u8;   buf[1] = (txid & 0xFF) as u8;
    buf[2] = 0x01;                  buf[3] = 0x00; // flags: RD
    buf[4] = 0x00;                  buf[5] = 0x01; // qdcount = 1
    buf[6] = 0x00;                  buf[7] = 0x00; // ancount = 0
    buf[8] = 0x00;                  buf[9] = 0x00; // nscount = 0
    buf[10] = 0x00;                 buf[11] = 0x00; // arcount = 0

    let mut pos = 12usize;
    let nlen = encode_name(name, &mut buf[pos..]);
    if nlen == 0 { return 0; }
    pos += nlen;

    // Type A (0x0001) and Class IN (0x0001)
    if pos + 4 > MAX_DNS_PAYLOAD { return 0; }
    buf[pos] = 0x00; buf[pos+1] = 0x01;  // QTYPE = A
    buf[pos+2] = 0x00; buf[pos+3] = 0x01; // QCLASS = IN
    pos + 4
}

// ── Response parser ───────────────────────────────────────────────────────────

/// Skip a DNS name at `data[off..]`. Returns new offset past the name.
fn skip_name(data: &[u8], mut off: usize) -> Option<usize> {
    loop {
        if off >= data.len() { return None; }
        let len = data[off] as usize;
        if len == 0 { return Some(off + 1); }
        if len & 0xC0 == 0xC0 {
            // Pointer (2 bytes)
            return Some(off + 2);
        }
        off += 1 + len;
    }
}

/// Parse a DNS A-record response.  Returns the first IPv4 address found, or None.
pub fn parse_response(data: &[u8], txid: u16) -> Option<[u8; 4]> {
    if data.len() < 12 { return None; }
    let resp_txid = u16::from_be_bytes([data[0], data[1]]);
    if resp_txid != txid { return None; }
    let flags = u16::from_be_bytes([data[2], data[3]]);
    if flags & 0x8000 == 0 { return None; } // not a response
    if flags & 0x000F != 0 { return None; } // RCODE != 0 (error)

    let qdcount = u16::from_be_bytes([data[4], data[5]]) as usize;
    let ancount = u16::from_be_bytes([data[6], data[7]]) as usize;

    let mut off = 12usize;
    // Skip questions
    for _ in 0..qdcount {
        off = skip_name(data, off)?;
        off += 4; // type + class
        if off > data.len() { return None; }
    }
    // Parse answers looking for A records
    for _ in 0..ancount {
        off = skip_name(data, off)?;
        if off + 10 > data.len() { return None; }
        let rtype  = u16::from_be_bytes([data[off], data[off+1]]);
        let rdlen  = u16::from_be_bytes([data[off+8], data[off+9]]) as usize;
        off += 10;
        if off + rdlen > data.len() { return None; }
        if rtype == 1 && rdlen == 4 {
            // A record
            let ip: [u8; 4] = data[off..off+4].try_into().ok()?;
            return Some(ip);
        }
        off += rdlen;
    }
    None
}

// ── Public API ────────────────────────────────────────────────────────────────

pub struct DnsResult {
    pub name: &'static str,  // only used internally
    pub ip:   [u8; 4],
}

/// Resolve `name` to an IPv4 address by querying QEMU's slirp DNS at 10.0.2.3.
/// Blocks for up to `timeout_ms` milliseconds.  Returns None on failure.
pub fn resolve(name: &str, timeout_ms: u64) -> Option<[u8; 4]> {
    // Need IP config
    config::get()?;

    // Resolve DNS server MAC via ARP (try up to 1s)
    // QEMU slirp: 10.0.2.3 (DNS) and 10.0.2.2 (gateway) are both served by the
    // same slirp process and share a MAC.  ARP for .3 often goes unanswered while
    // ARP for .2 always works.  Resolve the gateway MAC and use it for DNS too.
    let gw: [u8; 4] = config::gateway_ip()?;
    let dns_mac = {
        let m = arp::cache_lookup(gw)
            .or_else(|| arp::cache_lookup(DNS_SERVER));
        if let Some(mac) = m {
            mac
        } else {
            // Try gateway first, then DNS server directly
            arp::send_request(gw);
            let deadline = uptime_ms() + 1500;
            let mut found = None;
            while uptime_ms() < deadline {
                crate::net::poll_and_dispatch();
                if let Some(m) = arp::cache_lookup(gw)
                    .or_else(|| arp::cache_lookup(DNS_SERVER))
                {
                    found = Some(m);
                    break;
                }
                crate::arch::x86_64::halt::idle_once();
            }
            found?
        }
    };

    static TXID: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0x1234);
    let txid = TXID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let src_port = udp::alloc_port();

    let mut buf = [0u8; MAX_DNS_PAYLOAD];
    let qlen = build_query(name, txid, &mut buf);
    if qlen == 0 { return None; }

    // Send the query
    if !udp::send(DNS_SERVER, dns_mac, src_port, DNS_PORT, &buf[..qlen]) {
        return None;
    }

    // Wait for response
    let deadline = uptime_ms() + timeout_ms;
    while uptime_ms() < deadline {
        crate::net::poll_and_dispatch();
        if let Some(pkt) = udp::recv(src_port) {
            if let Some(ip) = parse_response(&pkt.data[..pkt.len], txid) {
                return Some(ip);
            }
        }
        crate::arch::x86_64::halt::idle_once();
    }
    None
}
