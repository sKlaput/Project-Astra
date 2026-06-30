// ---------------------------------------------------------------------------
// Astra OS — ICMP (Internet Control Message Protocol)
//
// RFC 792 — handles echo request/reply (ping).
//
// ICMP packet layout:
//   [0]      type   (8=echo request, 0=echo reply)
//   [1]      code   (0)
//   [2..3]   checksum
//   [4..5]   identifier
//   [6..7]   sequence number
//   [8..]    data
//
// To send a ping:
//   icmp::send_echo_request_to(dst_ip, dst_mac, id, seq)
//
// On RX, call icmp::handle_packet() with the ICMP payload.
// Poll icmp::poll_reply() to see if a reply arrived.
// ---------------------------------------------------------------------------

use crate::net::eth::ETH_HDR;
use crate::net::ipv4::{IPV4_HDR, PROTO_ICMP};
use crate::net::{arp, config, ipv4};
use core::sync::atomic::{AtomicU16, AtomicU32, Ordering};
use spin::Mutex;

pub const ICMP_ECHO_REQUEST: u8 = 8;
pub const ICMP_ECHO_REPLY: u8 = 0;
const ICMP_HDR: usize = 8;
const PING_DATA_LEN: usize = 32; // 32 bytes of payload data

// ── Reply tracking ────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct PingReply {
    pub from: [u8; 4],
    pub id: u16,
    pub seq: u16,
    pub rtt_ms: u32,
}

const REPLY_BUF: usize = 4;

struct ReplyQueue {
    entries: [PingReply; REPLY_BUF],
    head: usize,
    tail: usize,
    count: usize,
}

impl ReplyQueue {
    const fn new() -> Self {
        ReplyQueue {
            entries: [PingReply {
                from: [0; 4],
                id: 0,
                seq: 0,
                rtt_ms: 0,
            }; REPLY_BUF],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    fn push(&mut self, r: PingReply) {
        if self.count < REPLY_BUF {
            self.entries[self.tail] = r;
            self.tail = (self.tail + 1) % REPLY_BUF;
            self.count += 1;
        }
    }

    fn pop(&mut self) -> Option<PingReply> {
        if self.count == 0 {
            return None;
        }
        let r = self.entries[self.head];
        self.head = (self.head + 1) % REPLY_BUF;
        self.count -= 1;
        Some(r)
    }
}

static REPLY_QUEUE: Mutex<ReplyQueue> = Mutex::new(ReplyQueue::new());

// Monotone send-time for RTT measurement (stores uptime_ms of last echo request)
static LAST_SEND_MS: AtomicU32 = AtomicU32::new(0);
static LAST_SEND_ID: AtomicU16 = AtomicU16::new(0);
static LAST_SEND_SEQ: AtomicU16 = AtomicU16::new(0);

static PINGS_SENT: AtomicU32 = AtomicU32::new(0);
static PINGS_RECV: AtomicU32 = AtomicU32::new(0);

// ── TX ────────────────────────────────────────────────────────────────────────

/// Send an ICMP echo request to `dst_ip` with a pre-resolved `dst_mac`.
/// Caller must resolve ARP before calling this.
pub fn send_echo_request_to(dst_ip: [u8; 4], dst_mac: [u8; 6], id: u16, seq: u16) -> bool {
    let Some(cfg) = config::get() else {
        return false;
    };
    if !crate::net::driver::is_ready() {
        return false;
    }

    let mut icmp = [0u8; ICMP_HDR + PING_DATA_LEN];
    icmp[0] = ICMP_ECHO_REQUEST;
    icmp[1] = 0;
    icmp[4] = (id >> 8) as u8;
    icmp[5] = (id & 0xFF) as u8;
    icmp[6] = (seq >> 8) as u8;
    icmp[7] = (seq & 0xFF) as u8;
    for b in icmp[8..].iter_mut() {
        *b = 0xAB;
    }
    let csum = ipv4::checksum(&icmp);
    icmp[2] = (csum >> 8) as u8;
    icmp[3] = (csum & 0xFF) as u8;

    let mut ip_hdr = [0u8; IPV4_HDR];
    static IP_ID: AtomicU16 = AtomicU16::new(0x1337);
    let ip_id = IP_ID.fetch_add(1, Ordering::Relaxed);
    ipv4::build_header(
        &mut ip_hdr,
        ip_id,
        64,
        PROTO_ICMP,
        cfg.ip,
        dst_ip,
        icmp.len(),
    );

    let frame_len = ETH_HDR + IPV4_HDR + icmp.len();
    let mut frame = [0u8; ETH_HDR + IPV4_HDR + ICMP_HDR + PING_DATA_LEN];
    let our_mac = crate::drivers::virtio_net::mac_addr();
    frame[0..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&our_mac);
    frame[12] = 0x08;
    frame[13] = 0x00;
    frame[ETH_HDR..ETH_HDR + IPV4_HDR].copy_from_slice(&ip_hdr);
    frame[ETH_HDR + IPV4_HDR..frame_len].copy_from_slice(&icmp);

    LAST_SEND_MS.store(
        crate::arch::x86_64::interrupts::uptime_ms() as u32,
        Ordering::Relaxed,
    );
    LAST_SEND_ID.store(id, Ordering::Relaxed);
    LAST_SEND_SEQ.store(seq, Ordering::Relaxed);

    let ok = crate::drivers::virtio_net::send_frame(&frame[..frame_len]);
    if ok {
        PINGS_SENT.fetch_add(1, Ordering::Relaxed);
    }
    ok
}

// ── RX ────────────────────────────────────────────────────────────────────────

/// Handle an incoming ICMP packet.
/// `src_ip` is the IPv4 source of the containing packet.
/// `payload` is the ICMP payload (starting at type byte).
pub fn handle_packet(src_ip: [u8; 4], payload: &[u8]) {
    if payload.len() < ICMP_HDR {
        return;
    }
    let icmp_type = payload[0];
    let id = u16::from_be_bytes([payload[4], payload[5]]);
    let seq = u16::from_be_bytes([payload[6], payload[7]]);

    match icmp_type {
        ICMP_ECHO_REPLY => {
            let send_ms = LAST_SEND_MS.load(Ordering::Relaxed);
            let now_ms = crate::arch::x86_64::interrupts::uptime_ms() as u32;
            let rtt = now_ms.saturating_sub(send_ms);
            PINGS_RECV.fetch_add(1, Ordering::Relaxed);
            REPLY_QUEUE.lock().push(PingReply {
                from: src_ip,
                id,
                seq,
                rtt_ms: rtt,
            });
        }
        ICMP_ECHO_REQUEST => {
            // If we're the target, send a reply
            send_echo_reply(src_ip, id, seq, &payload[ICMP_HDR..]);
        }
        _ => {}
    }
}

fn send_echo_reply(dst_ip: [u8; 4], id: u16, seq: u16, data: &[u8]) {
    let Some(cfg) = config::get() else {
        return;
    };
    let dst_mac = match arp::cache_lookup(dst_ip) {
        Some(m) => m,
        None => return,
    };

    let data_len = data.len().min(PING_DATA_LEN);
    let icmp_total = ICMP_HDR + data_len;
    let frame_len = ETH_HDR + IPV4_HDR + icmp_total;
    let mut frame = [0u8; ETH_HDR + IPV4_HDR + ICMP_HDR + PING_DATA_LEN];

    // ICMP reply packet
    let icmp = &mut frame[ETH_HDR + IPV4_HDR..ETH_HDR + IPV4_HDR + icmp_total];
    icmp[0] = ICMP_ECHO_REPLY;
    icmp[1] = 0;
    icmp[2] = 0;
    icmp[3] = 0;
    icmp[4] = (id >> 8) as u8;
    icmp[5] = (id & 0xFF) as u8;
    icmp[6] = (seq >> 8) as u8;
    icmp[7] = (seq & 0xFF) as u8;
    icmp[8..8 + data_len].copy_from_slice(&data[..data_len]);
    let icmp_slice = &frame[ETH_HDR + IPV4_HDR..ETH_HDR + IPV4_HDR + icmp_total];
    let csum = ipv4::checksum(icmp_slice);
    frame[ETH_HDR + IPV4_HDR + 2] = (csum >> 8) as u8;
    frame[ETH_HDR + IPV4_HDR + 3] = (csum & 0xFF) as u8;

    // IPv4 header
    let mut ip_hdr = [0u8; IPV4_HDR];
    static IP_ID: AtomicU16 = AtomicU16::new(0x2000);
    let ip_id = IP_ID.fetch_add(1, Ordering::Relaxed);
    ipv4::build_header(
        &mut ip_hdr,
        ip_id,
        64,
        PROTO_ICMP,
        cfg.ip,
        dst_ip,
        icmp_total,
    );

    let our_mac = crate::drivers::virtio_net::mac_addr();
    frame[0..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&our_mac);
    frame[12] = 0x08;
    frame[13] = 0x00;
    frame[ETH_HDR..ETH_HDR + IPV4_HDR].copy_from_slice(&ip_hdr);

    crate::drivers::virtio_net::send_frame(&frame[..frame_len]);
}

// ── Poll API ──────────────────────────────────────────────────────────────────

/// Drain at most one reply from the queue. Returns None when queue is empty.
pub fn poll_reply() -> Option<PingReply> {
    REPLY_QUEUE.lock().pop()
}
