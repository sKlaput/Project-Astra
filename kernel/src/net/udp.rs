// ---------------------------------------------------------------------------
// Astra OS — UDP
//
// UDP datagram header (8 bytes, RFC 768):
//   [0..1] source port
//   [2..3] destination port
//   [4..5] length (header + data)
//   [6..7] checksum (optional for IPv4; we send 0 = disabled)
//
// Outgoing: build_and_send() wraps a payload in UDP + IPv4 + Ethernet.
// Incoming: handle_packet() is called by the IPv4 layer; routes to registered
//           port callbacks via a small static table.
// ---------------------------------------------------------------------------

use core::sync::atomic::{AtomicU16, Ordering};
use spin::Mutex;
use crate::net::{config, arp, eth, ipv4};
use crate::net::eth::ETH_HDR;
use crate::net::ipv4::{IPV4_HDR, PROTO_UDP};

pub const UDP_HDR: usize = 8;

// ── Receive queue ─────────────────────────────────────────────────────────────

const RX_BUF_CAPACITY: usize = 128;
const RX_SLOT_COUNT:   usize = 4;

#[derive(Clone, Copy)]
pub struct UdpPacket {
    pub src_ip:   [u8; 4],
    pub src_port: u16,
    pub dst_port: u16,
    pub len:      usize,
    pub data:     [u8; RX_BUF_CAPACITY],
}

impl UdpPacket {
    const fn empty() -> Self {
        UdpPacket {
            src_ip:   [0; 4],
            src_port: 0,
            dst_port: 0,
            len:      0,
            data:     [0; RX_BUF_CAPACITY],
        }
    }
}

struct RxQueue {
    slots: [UdpPacket; RX_SLOT_COUNT],
    head:  usize,
    tail:  usize,
    count: usize,
}

impl RxQueue {
    const fn new() -> Self {
        RxQueue {
            slots: [UdpPacket::empty(); RX_SLOT_COUNT],
            head:  0, tail: 0, count: 0,
        }
    }

    fn push(&mut self, pkt: UdpPacket) {
        if self.count < RX_SLOT_COUNT {
            self.slots[self.tail] = pkt;
            self.tail = (self.tail + 1) % RX_SLOT_COUNT;
            self.count += 1;
        }
    }

    fn pop_for_port(&mut self, port: u16) -> Option<UdpPacket> {
        // Linear scan (small queue, rarely called)
        for _ in 0..self.count {
            let pkt = self.slots[self.head];
            self.head = (self.head + 1) % RX_SLOT_COUNT;
            self.count -= 1;
            if pkt.dst_port == port {
                return Some(pkt);
            }
            // Put it back at tail if it's not for us
            self.push(pkt);
        }
        None
    }
}

static RX_QUEUE: Mutex<RxQueue> = Mutex::new(RxQueue::new());

// Ephemeral source port counter
static NEXT_SRC_PORT: AtomicU16 = AtomicU16::new(49152);

// ── RX ────────────────────────────────────────────────────────────────────────

pub fn handle_packet(src_ip: [u8; 4], payload: &[u8]) {
    if payload.len() < UDP_HDR { return; }
    let src_port = u16::from_be_bytes([payload[0], payload[1]]);
    let dst_port = u16::from_be_bytes([payload[2], payload[3]]);
    let udp_len  = u16::from_be_bytes([payload[4], payload[5]]) as usize;
    let data_len = udp_len.saturating_sub(UDP_HDR).min(payload.len() - UDP_HDR).min(RX_BUF_CAPACITY);

    let mut pkt = UdpPacket::empty();
    pkt.src_ip   = src_ip;
    pkt.src_port = src_port;
    pkt.dst_port = dst_port;
    pkt.len      = data_len;
    pkt.data[..data_len].copy_from_slice(&payload[UDP_HDR..UDP_HDR + data_len]);
    RX_QUEUE.lock().push(pkt);
}

/// Drain one received UDP datagram for `dst_port`. Returns None if none available.
pub fn recv(dst_port: u16) -> Option<UdpPacket> {
    RX_QUEUE.lock().pop_for_port(dst_port)
}

// ── TX ────────────────────────────────────────────────────────────────────────

/// Send a UDP datagram. `dst_mac` must be pre-resolved via ARP.
/// Returns true if the frame was submitted to the driver.
pub fn send(dst_ip: [u8; 4], dst_mac: [u8; 6],
            src_port: u16, dst_port: u16,
            payload: &[u8]) -> bool {
    let Some(cfg) = config::get() else { return false; };
    if payload.len() + UDP_HDR + IPV4_HDR + ETH_HDR > 1514 { return false; }

    let udp_len = UDP_HDR + payload.len();
    let total   = ETH_HDR + IPV4_HDR + udp_len;
    // Stack-allocate a max-size frame
    const MAX: usize = ETH_HDR + IPV4_HDR + UDP_HDR + 512;
    if total > MAX { return false; }

    let mut frame = [0u8; MAX];

    // UDP header
    let udp = &mut frame[ETH_HDR + IPV4_HDR..ETH_HDR + IPV4_HDR + udp_len];
    udp[0] = (src_port >> 8) as u8; udp[1] = (src_port & 0xFF) as u8;
    udp[2] = (dst_port >> 8) as u8; udp[3] = (dst_port & 0xFF) as u8;
    udp[4] = (udp_len as u16 >> 8) as u8;
    udp[5] = (udp_len as u16 & 0xFF) as u8;
    udp[6] = 0; udp[7] = 0;  // checksum = 0 (disabled for IPv4)
    udp[UDP_HDR..udp_len].copy_from_slice(payload);

    // IPv4 header
    let mut ip_hdr = [0u8; IPV4_HDR];
    static IP_ID: AtomicU16 = AtomicU16::new(0x4000);
    let ip_id = IP_ID.fetch_add(1, Ordering::Relaxed);
    ipv4::build_header(&mut ip_hdr, ip_id, 64, PROTO_UDP, cfg.ip, dst_ip, udp_len);
    frame[ETH_HDR..ETH_HDR + IPV4_HDR].copy_from_slice(&ip_hdr);

    // Ethernet header
    let our_mac = crate::drivers::virtio_net::mac_addr();
    frame[0..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&our_mac);
    frame[12] = 0x08; frame[13] = 0x00;

    crate::drivers::virtio_net::send_frame(&frame[..total])
}

/// Allocate a fresh ephemeral source port.
pub fn alloc_port() -> u16 {
    NEXT_SRC_PORT.fetch_add(1, Ordering::Relaxed)
}
