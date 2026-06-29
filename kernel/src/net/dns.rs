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

use crate::arch::x86_64::interrupts::uptime_ms;
use crate::net::{arp, config, udp};

const DNS_PORT: u16 = 53;
const DNS_SERVER: [u8; 4] = [10, 0, 2, 3]; // QEMU slirp DNS
const DNS_RETRIES: usize = 2;

const MAX_DNS_PAYLOAD: usize = 512;

// ── Query builder ─────────────────────────────────────────────────────────────

/// Encode a DNS name (e.g. "google.com") into wire format in `out`.
/// Returns the number of bytes written, or 0 on error.
fn encode_name(name: &str, out: &mut [u8]) -> usize {
    let mut pos = 0usize;
    for label in name.split('.') {
        let lb = label.as_bytes();
        if lb.is_empty() || lb.len() > 63 {
            return 0;
        }
        if pos + 1 + lb.len() >= out.len() {
            return 0;
        }
        out[pos] = lb.len() as u8;
        pos += 1;
        out[pos..pos + lb.len()].copy_from_slice(lb);
        pos += lb.len();
    }
    if pos >= out.len() {
        return 0;
    }
    out[pos] = 0; // root label
    pos + 1
}

fn build_query(name: &str, txid: u16, buf: &mut [u8; MAX_DNS_PAYLOAD]) -> usize {
    // Header
    buf[0] = (txid >> 8) as u8;
    buf[1] = (txid & 0xFF) as u8;
    buf[2] = 0x01;
    buf[3] = 0x00; // flags: RD
    buf[4] = 0x00;
    buf[5] = 0x01; // qdcount = 1
    buf[6] = 0x00;
    buf[7] = 0x00; // ancount = 0
    buf[8] = 0x00;
    buf[9] = 0x00; // nscount = 0
    buf[10] = 0x00;
    buf[11] = 0x00; // arcount = 0

    let mut pos = 12usize;
    let nlen = encode_name(name, &mut buf[pos..]);
    if nlen == 0 {
        return 0;
    }
    pos += nlen;

    // Type A (0x0001) and Class IN (0x0001)
    if pos + 4 > MAX_DNS_PAYLOAD {
        return 0;
    }
    buf[pos] = 0x00;
    buf[pos + 1] = 0x01; // QTYPE = A
    buf[pos + 2] = 0x00;
    buf[pos + 3] = 0x01; // QCLASS = IN
    pos + 4
}

// ── Response parser ───────────────────────────────────────────────────────────

/// Skip a DNS name at `data[off..]`. Returns new offset past the name.
fn skip_name(data: &[u8], mut off: usize) -> Option<usize> {
    loop {
        if off >= data.len() {
            return None;
        }
        let len = data[off] as usize;
        if len == 0 {
            return Some(off + 1);
        }
        if len & 0xC0 == 0xC0 {
            // Pointer (2 bytes)
            return Some(off + 2);
        }
        off += 1 + len;
    }
}

/// Parse a DNS A-record response.  Returns the first IPv4 address or a DnsError.
fn parse_response(data: &[u8], txid: u16) -> Result<[u8; 4], DnsError> {
    if data.len() < 12 {
        return Err(DnsError::Timeout);
    }
    let resp_txid = u16::from_be_bytes([data[0], data[1]]);
    if resp_txid != txid {
        return Err(DnsError::Timeout);
    }
    let flags = u16::from_be_bytes([data[2], data[3]]);
    if flags & 0x8000 == 0 {
        return Err(DnsError::Timeout);
    } // not a response
    let rcode = (flags & 0x000F) as u8;
    if rcode != 0 {
        return Err(DnsError::RcodeError(rcode));
    }

    let qdcount = u16::from_be_bytes([data[4], data[5]]) as usize;
    let ancount = u16::from_be_bytes([data[6], data[7]]) as usize;

    let mut off = 12usize;
    // Skip questions
    for _ in 0..qdcount {
        off = skip_name(data, off).ok_or(DnsError::Timeout)?;
        off += 4; // type + class
        if off > data.len() {
            return Err(DnsError::Timeout);
        }
    }
    // Parse answers looking for A records
    for _ in 0..ancount {
        off = skip_name(data, off).ok_or(DnsError::Timeout)?;
        if off + 10 > data.len() {
            return Err(DnsError::Timeout);
        }
        let rtype = u16::from_be_bytes([data[off], data[off + 1]]);
        let rdlen = u16::from_be_bytes([data[off + 8], data[off + 9]]) as usize;
        off += 10;
        if off + rdlen > data.len() {
            return Err(DnsError::Timeout);
        }
        if rtype == 1 && rdlen == 4 {
            let ip: [u8; 4] = data[off..off + 4]
                .try_into()
                .map_err(|_| DnsError::Timeout)?;
            return Ok(ip);
        }
        off += rdlen;
    }
    Err(DnsError::NxDomain)
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Reason a DNS query failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsError {
    /// IP stack not configured.
    NotReady,
    /// Could not resolve gateway / DNS-server MAC via ARP.
    ArpFailed,
    /// UDP send to DNS server failed after ARP refresh.
    SendFailed,
    /// No response arrived within the timeout.
    Timeout,
    /// Name does not exist (RCODE=3 NXDOMAIN).
    NxDomain,
    /// DNS server returned an error (RCODE value in the u8).
    RcodeError(u8),
}

/// Resolve `name` to an IPv4 address, returning a detailed error on failure.
/// Blocks for up to `timeout_ms` milliseconds.
pub fn resolve(name: &str, timeout_ms: u64) -> Result<[u8; 4], DnsError> {
    // Need IP config
    config::get().ok_or(DnsError::NotReady)?;

    // Resolve DNS server MAC via ARP.
    // QEMU slirp: 10.0.2.3 (DNS) and 10.0.2.2 (gateway) share a MAC.
    // ARP for .3 often goes unanswered; resolve gateway MAC and use it for DNS.
    let gw: [u8; 4] = config::gateway_ip().ok_or(DnsError::NotReady)?;
    let mut dns_mac = arp::resolve_with_retry(gw, 1200, 3)
        .or_else(|| arp::resolve_with_retry(DNS_SERVER, 900, 2))
        .ok_or(DnsError::ArpFailed)?;

    static TXID: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0x1234);
    let src_port = udp::alloc_port();

    let attempts = DNS_RETRIES.max(1);
    let per_attempt_ms = (timeout_ms / attempts as u64).max(350);

    let mut last_err = DnsError::Timeout;

    for _ in 0..attempts {
        let txid = TXID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let mut buf = [0u8; MAX_DNS_PAYLOAD];
        let qlen = build_query(name, txid, &mut buf);
        if qlen == 0 {
            return Err(DnsError::NotReady);
        }

        // Submit query; refresh ARP once if the send fails.
        if !udp::send(DNS_SERVER, dns_mac, src_port, DNS_PORT, &buf[..qlen]) {
            dns_mac = arp::resolve_with_retry(gw, 1200, 3)
                .or_else(|| arp::resolve_with_retry(DNS_SERVER, 900, 2))
                .ok_or(DnsError::ArpFailed)?;
            if !udp::send(DNS_SERVER, dns_mac, src_port, DNS_PORT, &buf[..qlen]) {
                last_err = DnsError::SendFailed;
                continue;
            }
        }

        let deadline = uptime_ms().saturating_add(per_attempt_ms);
        while uptime_ms() < deadline {
            crate::net::poll_and_dispatch();
            if let Some(pkt) = udp::recv(src_port) {
                match parse_response(&pkt.data[..pkt.len], txid) {
                    Ok(ip) => return Ok(ip),
                    Err(DnsError::Timeout) => {} // wrong TXID or malformed, keep waiting
                    Err(e) => return Err(e),     // NXDOMAIN or RCODE error: definitive
                }
            }
            crate::arch::x86_64::halt::idle_once();
        }
    }

    Err(last_err)
}
