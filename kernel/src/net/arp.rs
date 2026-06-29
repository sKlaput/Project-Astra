// ---------------------------------------------------------------------------
// Astra OS — ARP (Address Resolution Protocol)
//
// RFC 826 — IPv4 over Ethernet.
//
// ARP packet layout (28 bytes for IPv4/Ethernet):
//   [0..1]   Hardware type = 0x0001 (Ethernet)
//   [2..3]   Protocol type = 0x0800 (IPv4)
//   [4]      Hardware addr len = 6
//   [5]      Protocol addr len = 4
//   [6..7]   Operation: 1=request, 2=reply
//   [8..13]  Sender MAC
//   [14..17] Sender IP
//   [18..23] Target MAC
//   [24..27] Target IP
// ---------------------------------------------------------------------------

use crate::net::eth::{self, BROADCAST_MAC, ETH_ARP, ETH_HDR};
use spin::Mutex;

pub const ARP_PKT_LEN: usize = 28;
const ARP_OP_REQUEST: u16 = 1;
const ARP_OP_REPLY: u16 = 2;
const CACHE_TTL_MS: u64 = 30_000;

// ── ARP cache ─────────────────────────────────────────────────────────────────

const CACHE_CAP: usize = 16;

#[derive(Clone, Copy)]
struct ArpEntry {
    ip: [u8; 4],
    mac: [u8; 6],
    age: u64, // uptime_ms when inserted
}

impl ArpEntry {
    const fn empty() -> Self {
        ArpEntry {
            ip: [0; 4],
            mac: [0; 6],
            age: 0,
        }
    }
    fn is_valid(&self) -> bool {
        self.ip != [0, 0, 0, 0]
    }
}

struct ArpCache {
    entries: [ArpEntry; CACHE_CAP],
}

impl ArpCache {
    const fn new() -> Self {
        ArpCache {
            entries: [ArpEntry::empty(); CACHE_CAP],
        }
    }

    fn lookup_fresh(&mut self, ip: [u8; 4], now: u64) -> Option<[u8; 6]> {
        for e in self.entries.iter_mut() {
            if e.is_valid() && e.ip == ip {
                if now.wrapping_sub(e.age) > CACHE_TTL_MS {
                    *e = ArpEntry::empty();
                    return None;
                }
                return Some(e.mac);
            }
        }
        None
    }

    fn insert(&mut self, ip: [u8; 4], mac: [u8; 6]) {
        // Update existing
        for e in self.entries.iter_mut() {
            if e.ip == ip {
                e.mac = mac;
                e.age = crate::arch::x86_64::interrupts::uptime_ms();
                return;
            }
        }
        // Find empty slot
        for e in self.entries.iter_mut() {
            if !e.is_valid() {
                *e = ArpEntry {
                    ip,
                    mac,
                    age: crate::arch::x86_64::interrupts::uptime_ms(),
                };
                return;
            }
        }
        // Evict oldest
        let mut oldest = 0usize;
        let mut oldest_age = u64::MAX;
        for (i, e) in self.entries.iter().enumerate() {
            if e.age < oldest_age {
                oldest_age = e.age;
                oldest = i;
            }
        }
        self.entries[oldest] = ArpEntry {
            ip,
            mac,
            age: crate::arch::x86_64::interrupts::uptime_ms(),
        };
    }

    fn all(&self) -> &[ArpEntry; CACHE_CAP] {
        &self.entries
    }
}

static ARP_CACHE: Mutex<ArpCache> = Mutex::new(ArpCache::new());

// ── Public API ────────────────────────────────────────────────────────────────

pub fn cache_lookup(ip: [u8; 4]) -> Option<[u8; 6]> {
    let now = crate::arch::x86_64::interrupts::uptime_ms();
    ARP_CACHE.lock().lookup_fresh(ip, now)
}

pub fn cache_insert(ip: [u8; 4], mac: [u8; 6]) {
    ARP_CACHE.lock().insert(ip, mac);
}

/// Iterate the ARP cache, calling `f` for each valid entry.
pub fn cache_iter<F: FnMut([u8; 4], [u8; 6])>(mut f: F) {
    let now = crate::arch::x86_64::interrupts::uptime_ms();
    let cache = ARP_CACHE.lock();
    for e in cache.all() {
        if e.is_valid() && now.wrapping_sub(e.age) <= CACHE_TTL_MS {
            f(e.ip, e.mac);
        }
    }
}

/// Resolve `target_ip` to a MAC address with ARP retries.
///
/// Attempts up to `retries` ARP requests and waits a slice of `timeout_ms`
/// between attempts while polling RX. Returns None on timeout.
pub fn resolve_with_retry(target_ip: [u8; 4], timeout_ms: u64, retries: usize) -> Option<[u8; 6]> {
    if let Some(mac) = cache_lookup(target_ip) {
        return Some(mac);
    }
    if retries == 0 {
        return None;
    }

    let attempts = retries.max(1);
    let slice_ms = (timeout_ms / attempts as u64).max(1);
    for _ in 0..attempts {
        send_request(target_ip);
        let deadline = crate::arch::x86_64::interrupts::uptime_ms().saturating_add(slice_ms);
        while crate::arch::x86_64::interrupts::uptime_ms() < deadline {
            crate::net::poll_and_dispatch();
            if let Some(mac) = cache_lookup(target_ip) {
                return Some(mac);
            }
            crate::arch::x86_64::halt::idle_once();
        }
    }
    cache_lookup(target_ip)
}

/// Handle an incoming ARP packet (Ethernet payload, 28 bytes).
/// Learns the sender's IP→MAC mapping and sends a reply if the packet
/// is a request for our IP.
pub fn handle_packet(payload: &[u8]) {
    if payload.len() < ARP_PKT_LEN {
        return;
    }

    // Validate hardware and protocol type
    if u16::from_be_bytes([payload[0], payload[1]]) != 0x0001 {
        return;
    } // Ethernet
    if u16::from_be_bytes([payload[2], payload[3]]) != 0x0800 {
        return;
    } // IPv4
    if payload[4] != 6 || payload[5] != 4 {
        return;
    }

    let op = u16::from_be_bytes([payload[6], payload[7]]);
    let sender_mac: [u8; 6] = payload[8..14].try_into().unwrap_or([0; 6]);
    let sender_ip: [u8; 4] = payload[14..18].try_into().unwrap_or([0; 4]);
    let target_ip: [u8; 4] = payload[24..28].try_into().unwrap_or([0; 4]);

    // Learn sender
    if sender_ip != [0, 0, 0, 0] {
        cache_insert(sender_ip, sender_mac);
    }

    // If it's a request for our IP, send a reply
    if op == ARP_OP_REQUEST && crate::net::config::is_our_ip(target_ip) {
        let our_mac = crate::drivers::virtio_net::mac_addr();
        let our_ip = crate::net::config::our_ip().unwrap_or([0; 4]);
        send_reply(our_mac, our_ip, sender_mac, sender_ip);
    }
}

/// Send an ARP reply.
fn send_reply(our_mac: [u8; 6], our_ip: [u8; 4], target_mac: [u8; 6], target_ip: [u8; 4]) {
    let mut buf = [0u8; ETH_HDR + ARP_PKT_LEN];
    let pkt = build_packet(ARP_OP_REPLY, our_mac, our_ip, target_mac, target_ip);
    let len = eth::build_frame(&mut buf, target_mac, our_mac, ETH_ARP, &pkt);
    if len > 0 {
        crate::drivers::virtio_net::send_frame(&buf[..len]);
    }
}

/// Send an ARP request for `target_ip`.
pub fn send_request(target_ip: [u8; 4]) {
    let our_mac = crate::drivers::virtio_net::mac_addr();
    let our_ip = crate::net::config::our_ip().unwrap_or([0; 4]);
    let mut buf = [0u8; ETH_HDR + ARP_PKT_LEN];
    let pkt = build_packet(ARP_OP_REQUEST, our_mac, our_ip, [0; 6], target_ip);
    let len = eth::build_frame(&mut buf, BROADCAST_MAC, our_mac, ETH_ARP, &pkt);
    if len > 0 {
        crate::drivers::virtio_net::send_frame(&buf[..len]);
    }
}

fn build_packet(
    op: u16,
    sender_mac: [u8; 6],
    sender_ip: [u8; 4],
    target_mac: [u8; 6],
    target_ip: [u8; 4],
) -> [u8; ARP_PKT_LEN] {
    let mut p = [0u8; ARP_PKT_LEN];
    p[0] = 0x00;
    p[1] = 0x01; // hw type = Ethernet
    p[2] = 0x08;
    p[3] = 0x00; // proto type = IPv4
    p[4] = 6;
    p[5] = 4; // hw/proto addr lengths
    p[6] = (op >> 8) as u8;
    p[7] = (op & 0xFF) as u8;
    p[8..14].copy_from_slice(&sender_mac);
    p[14..18].copy_from_slice(&sender_ip);
    p[18..24].copy_from_slice(&target_mac);
    p[24..28].copy_from_slice(&target_ip);
    p
}
