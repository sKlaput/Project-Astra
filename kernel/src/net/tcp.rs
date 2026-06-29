// ---------------------------------------------------------------------------
// Astra OS — TCP client (minimal, single-connection)
//
// Implements just enough TCP to open a connection, send data, and receive
// the response.  Suitable for HTTP/1.0 GET over QEMU slirp.
//
// Supported state machine (client-only subset):
//   CLOSED → SYN_SENT → ESTABLISHED → (FIN_WAIT_1 → FIN_WAIT_2 →) CLOSED
//
// One static connection at a time (sufficient for a terminal HTTP client).
// Minimal retransmit for SYN/data/FIN with timeout-based retries.
//
// TCP segment header (20 bytes, no options):
//   [0..1]  src port
//   [2..3]  dst port
//   [4..7]  sequence number
//   [8..11] acknowledgement number
//   [12]    data offset (5 << 4 = 0x50) + reserved
//   [13]    flags: URG ACK PSH RST SYN FIN
//   [14..15] window size
//   [16..17] checksum (pseudo-header)
//   [18..19] urgent pointer
// ---------------------------------------------------------------------------

use crate::arch::x86_64::interrupts::uptime_ms;
use crate::net::eth::ETH_HDR;
use crate::net::ipv4::{IPV4_HDR, PROTO_TCP};
use crate::net::{arp, config, ipv4};
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

pub const TCP_HDR: usize = 20;

// ── Flags ─────────────────────────────────────────────────────────────────────
const FIN: u8 = 0x01;
const SYN: u8 = 0x02;
const RST: u8 = 0x04;
const PSH: u8 = 0x08;
const ACK: u8 = 0x10;

// ── Sequence number helpers (handle wrapping per RFC 793) ─────────────────────
/// True if `a` is strictly ahead of `b` in TCP sequence space.
#[inline]
fn seq_gt(a: u32, b: u32) -> bool {
    a != b && a.wrapping_sub(b) < 0x8000_0000u32
}
/// True if `a` >= `b` in TCP sequence space.
#[inline]
fn seq_ge(a: u32, b: u32) -> bool {
    a.wrapping_sub(b) < 0x8000_0000u32
}
const SYN_RETRY_MS: u64 = 300;
const DATA_ACK_TIMEOUT_MS: u64 = 3000;
const FIN_ACK_TIMEOUT_MS: u64 = 500;
const DATA_RETRIES: usize = 3;
const FIN_RETRIES: usize = 3;

// ── State ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcpState {
    Closed,
    SynSent,
    Established,
    CloseWait, // remote sent FIN, we haven't closed yet
    FinWait1,  // we sent FIN
    TimeWait,  // both sides closed
}

// ── RX buffer ─────────────────────────────────────────────────────────────────
pub const RX_CAP: usize = 8192;

pub struct TcpConn {
    pub state: TcpState,
    src_ip: [u8; 4],
    pub dst_ip: [u8; 4],
    dst_mac: [u8; 6],
    src_port: u16,
    pub dst_port: u16,
    // Send sequence state
    snd_seq: u32, // next byte to send
    snd_una: u32, // highest acknowledged sequence (next expected by peer)
    // Receive sequence state
    rcv_seq: u32, // next expected byte from remote
    // Receive buffer
    pub rx_buf: [u8; RX_CAP],
    pub rx_len: usize,
    pub rx_fin: bool, // remote sent FIN
}

impl TcpConn {
    const fn new() -> Self {
        TcpConn {
            state: TcpState::Closed,
            src_ip: [0; 4],
            dst_ip: [0; 4],
            dst_mac: [0; 6],
            src_port: 0,
            dst_port: 0,
            snd_seq: 0,
            snd_una: 0,
            rcv_seq: 0,
            rx_buf: [0; RX_CAP],
            rx_len: 0,
            rx_fin: false,
        }
    }
}

static CONN: Mutex<TcpConn> = Mutex::new(TcpConn::new());
static NEXT_PORT: AtomicU32 = AtomicU32::new(49200);

// ── Checksum ──────────────────────────────────────────────────────────────────

/// TCP checksum: internet checksum over pseudo-header + TCP segment.
fn tcp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], tcp_seg: &[u8]) -> u16 {
    let tcp_len = tcp_seg.len() as u16;
    // Pseudo-header: src(4) dst(4) zero(1) proto(1) tcp_len(2)
    let mut sum: u32 = 0;
    // Src IP
    sum += u16::from_be_bytes([src_ip[0], src_ip[1]]) as u32;
    sum += u16::from_be_bytes([src_ip[2], src_ip[3]]) as u32;
    // Dst IP
    sum += u16::from_be_bytes([dst_ip[0], dst_ip[1]]) as u32;
    sum += u16::from_be_bytes([dst_ip[2], dst_ip[3]]) as u32;
    // Zero + Protocol
    sum += PROTO_TCP as u32;
    // TCP length
    sum += tcp_len as u32;
    // TCP segment itself
    let mut i = 0usize;
    while i + 2 <= tcp_seg.len() {
        sum += u16::from_be_bytes([tcp_seg[i], tcp_seg[i + 1]]) as u32;
        i += 2;
    }
    if i < tcp_seg.len() {
        sum += (tcp_seg[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

// ── Segment builder ───────────────────────────────────────────────────────────

fn build_segment(conn: &TcpConn, flags: u8, payload: &[u8], out: &mut [u8]) -> usize {
    let tcp_len = TCP_HDR + payload.len();
    if out.len() < ETH_HDR + IPV4_HDR + tcp_len {
        return 0;
    }

    // TCP header
    let tcp = &mut out[ETH_HDR + IPV4_HDR..ETH_HDR + IPV4_HDR + tcp_len];
    tcp[0] = (conn.src_port >> 8) as u8;
    tcp[1] = (conn.src_port & 0xFF) as u8;
    tcp[2] = (conn.dst_port >> 8) as u8;
    tcp[3] = (conn.dst_port & 0xFF) as u8;
    let seq = conn.snd_seq;
    tcp[4] = (seq >> 24) as u8;
    tcp[5] = (seq >> 16) as u8;
    tcp[6] = (seq >> 8) as u8;
    tcp[7] = seq as u8;
    let ack = conn.rcv_seq;
    tcp[8] = (ack >> 24) as u8;
    tcp[9] = (ack >> 16) as u8;
    tcp[10] = (ack >> 8) as u8;
    tcp[11] = ack as u8;
    tcp[12] = 0x50; // data offset = 5 (20 bytes), reserved = 0
    tcp[13] = flags;
    tcp[14] = 0x20; // window size high byte (0x2000 = 8192, matching RX_CAP)
    tcp[15] = 0x00;
    tcp[16] = 0;
    tcp[17] = 0; // checksum placeholder
    tcp[18] = 0;
    tcp[19] = 0; // urgent pointer

    if !payload.is_empty() {
        tcp[TCP_HDR..tcp_len].copy_from_slice(payload);
    }

    // Compute checksum
    let csum = tcp_checksum(conn.src_ip, conn.dst_ip, tcp);
    tcp[16] = (csum >> 8) as u8;
    tcp[17] = (csum & 0xFF) as u8;

    // IPv4 header
    let mut ip_hdr = [0u8; IPV4_HDR];
    static IP_ID: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0x5000);
    let ip_id = IP_ID.fetch_add(1, Ordering::Relaxed);
    ipv4::build_header(
        &mut ip_hdr,
        ip_id,
        64,
        PROTO_TCP,
        conn.src_ip,
        conn.dst_ip,
        tcp_len,
    );
    out[ETH_HDR..ETH_HDR + IPV4_HDR].copy_from_slice(&ip_hdr);

    // Ethernet header
    let our_mac = crate::drivers::virtio_net::mac_addr();
    out[0..6].copy_from_slice(&conn.dst_mac);
    out[6..12].copy_from_slice(&our_mac);
    out[12] = 0x08;
    out[13] = 0x00;

    ETH_HDR + IPV4_HDR + tcp_len
}

fn send_segment(conn: &TcpConn, flags: u8, payload: &[u8]) -> bool {
    const MAX: usize = ETH_HDR + IPV4_HDR + TCP_HDR + 1460;
    let mut frame = [0u8; MAX];
    let len = build_segment(conn, flags, payload, &mut frame);
    if len == 0 {
        return false;
    }
    crate::drivers::virtio_net::send_frame(&frame[..len])
}

// ── Public connect API ────────────────────────────────────────────────────────

/// Open a TCP connection to `dst_ip:dst_port`.  Blocks until ESTABLISHED or timeout.
pub fn connect(dst_ip: [u8; 4], dst_port: u16, timeout_ms: u64) -> bool {
    let Some(cfg) = config::get() else {
        return false;
    };

    // Resolve destination MAC first (before touching CONN state).
    // For off-subnet IPs (or when direct ARP fails) use the gateway MAC —
    // slirp routes all external traffic through 10.0.2.2 anyway.
    let dst_mac = {
        let arp_target = if config::is_local(dst_ip) {
            dst_ip
        } else {
            match config::gateway_ip() {
                Some(gw) => gw,
                None => return false,
            }
        };
        match arp::resolve_with_retry(arp_target, 1500, 3) {
            Some(m) => m,
            None => return false,
        }
    };

    let src_port = (NEXT_PORT.fetch_add(1, Ordering::Relaxed) & 0xFFFF) as u16;

    // Per-connection ISN derived from uptime so back-to-back connections do not
    // reuse the same sequence space (which can confuse stateful NAT / the peer
    // while a previous connection is still in TIME_WAIT).
    let isn = 0x1000_0000u32
        .wrapping_add((uptime_ms() as u32).wrapping_mul(65_536))
        .wrapping_add((src_port as u32) << 8);

    // Full clean-slate reset immediately before setting up the new connection.
    // Done here (after ARP) so no polling occurs between the reset and SYN send.
    {
        let mut conn = CONN.lock();
        conn.state = TcpState::SynSent;
        conn.src_ip = cfg.ip;
        conn.dst_ip = dst_ip;
        conn.dst_mac = dst_mac;
        conn.src_port = src_port;
        conn.dst_port = dst_port;
        conn.snd_seq = isn;
        conn.snd_una = conn.snd_seq;
        conn.rcv_seq = 0;
        conn.rx_len = 0;
        conn.rx_fin = false;
    }

    // Send SYN
    {
        let conn = CONN.lock();
        send_segment(&conn, SYN, &[]);
    }

    // Wait for ESTABLISHED; retransmit SYN periodically until timeout.
    let deadline = uptime_ms().saturating_add(timeout_ms);
    let mut next_syn_retry = uptime_ms().saturating_add(SYN_RETRY_MS);
    while uptime_ms() < deadline {
        let now = uptime_ms();
        if now >= next_syn_retry {
            let conn = CONN.lock();
            if conn.state == TcpState::SynSent {
                send_segment(&conn, SYN, &[]);
            }
            next_syn_retry = now.saturating_add(SYN_RETRY_MS);
        }

        crate::net::poll_and_dispatch();
        let state = CONN.lock().state;
        if state == TcpState::Established {
            return true;
        }
        if state == TcpState::Closed {
            return false;
        }
        crate::arch::x86_64::halt::idle_once();
    }
    CONN.lock().state = TcpState::Closed;
    false
}

fn wait_for_ack(expected_ack: u32, timeout_ms: u64) -> bool {
    let deadline = uptime_ms().saturating_add(timeout_ms);
    while uptime_ms() < deadline {
        crate::net::poll_and_dispatch();
        let conn = CONN.lock();
        // Check the ACK first: the peer may bundle ACK + data + FIN (HTTP/1.0
        // "Connection: close"), driving us to CloseWait/Closed in the same poll
        // batch where our data was acknowledged.  A reached ACK is success even
        // if the connection is now closing.
        if seq_ge(conn.snd_una, expected_ack) {
            return true;
        }
        if conn.state == TcpState::Closed {
            return false;
        }
        drop(conn);
        crate::arch::x86_64::halt::idle_once();
    }
    false
}

/// Send data on the established connection.
pub fn send(data: &[u8]) -> bool {
    {
        let s = CONN.lock().state;
        if s != TcpState::Established && s != TcpState::CloseWait {
            return false;
        }
    }
    // Send in 1460-byte chunks and wait for ACK for each chunk.
    let mut off = 0usize;
    while off < data.len() {
        let chunk_end = (off + 1460).min(data.len());
        let chunk = &data[off..chunk_end];
        let mut acked = false;

        for _ in 0..DATA_RETRIES {
            let expected_ack = {
                let conn = CONN.lock();
                let s = conn.state;
                if s != TcpState::Established && s != TcpState::CloseWait {
                    return false;
                }
                if !send_segment(&conn, ACK | PSH, chunk) {
                    return false;
                }
                conn.snd_seq.wrapping_add(chunk.len() as u32)
            };

            if wait_for_ack(expected_ack, DATA_ACK_TIMEOUT_MS) {
                let mut conn = CONN.lock();
                conn.snd_seq = expected_ack;
                if !seq_ge(conn.snd_una, expected_ack) {
                    conn.snd_una = expected_ack;
                }
                acked = true;
                break;
            }
        }

        if !acked {
            return false;
        }
        off = chunk_end;
    }
    true
}

/// Read received data into `buf`. Returns bytes copied.
pub fn read(buf: &mut [u8]) -> usize {
    let mut conn = CONN.lock();
    let n = conn.rx_len.min(buf.len());
    buf[..n].copy_from_slice(&conn.rx_buf[..n]);
    // Use locals to avoid split-borrow: rx_buf mut + rx_len immut in one expr
    let old_len = conn.rx_len;
    conn.rx_buf.copy_within(n..old_len, 0);
    conn.rx_len = old_len - n;
    n
}

/// Returns true if the remote side has closed (FIN received) and RX buffer is empty.
pub fn is_closed_remote() -> bool {
    let conn = CONN.lock();
    conn.rx_fin && conn.rx_len == 0
}

/// Returns true if any data is available.
pub fn has_data() -> bool {
    CONN.lock().rx_len > 0
}

pub fn state() -> TcpState {
    CONN.lock().state
}

/// Close the connection by sending FIN.
pub fn close() {
    {
        let mut conn = CONN.lock();
        if conn.state != TcpState::Established && conn.state != TcpState::CloseWait {
            conn.state = TcpState::Closed;
            return;
        }
        conn.state = TcpState::FinWait1;
    }

    for _ in 0..FIN_RETRIES {
        let fin_ack_target = {
            let conn = CONN.lock();
            if !send_segment(&conn, FIN | ACK, &[]) {
                break;
            }
            conn.snd_seq.wrapping_add(1)
        };

        if wait_for_ack(fin_ack_target, FIN_ACK_TIMEOUT_MS) {
            let mut conn = CONN.lock();
            conn.snd_seq = fin_ack_target;
            if !seq_ge(conn.snd_una, fin_ack_target) {
                conn.snd_una = fin_ack_target;
            }
            conn.state = TcpState::Closed;
            conn.rx_fin = false;
            conn.rx_len = 0;
            return;
        }
    }

    let mut conn = CONN.lock();
    conn.state = TcpState::Closed;
    conn.rx_fin = false;
    conn.rx_len = 0;
}

// ── RX handler (called by net::dispatch_frame) ────────────────────────────────

/// Handle an incoming TCP segment.  `src_ip` is the IPv4 source.
/// `payload` is the TCP segment (starting at TCP header byte 0).
pub fn handle_segment(src_ip: [u8; 4], payload: &[u8]) {
    if payload.len() < TCP_HDR {
        return;
    }

    let src_port = u16::from_be_bytes([payload[0], payload[1]]);
    let dst_port = u16::from_be_bytes([payload[2], payload[3]]);
    let seq = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let ack_num = u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);
    let data_off = ((payload[12] >> 4) as usize) * 4;
    let flags = payload[13];
    let tcp_data = if data_off < payload.len() {
        &payload[data_off..]
    } else {
        &[]
    };

    let mut conn = CONN.lock();

    // Only accept segments for our active connection
    if src_ip != conn.dst_ip {
        return;
    }
    if src_port != conn.dst_port {
        return;
    }
    if dst_port != conn.src_port {
        return;
    }

    if flags & RST != 0 {
        conn.state = TcpState::Closed;
        return;
    }

    match conn.state {
        TcpState::SynSent => {
            // Expect SYN+ACK
            if flags & (SYN | ACK) == (SYN | ACK) {
                conn.rcv_seq = seq.wrapping_add(1); // next expected = remote ISN + 1
                conn.snd_seq = conn.snd_seq.wrapping_add(1); // SYN consumed one seq
                conn.snd_una = conn.snd_seq;
                let _ = ack_num;
                conn.state = TcpState::Established;
                // Send ACK (must drop lock first to avoid deadlock in send)
                let ack_seg_seq = conn.snd_seq;
                let ack_seg_rcv = conn.rcv_seq;
                let src_ip_c = conn.src_ip;
                let dst_ip_c = conn.dst_ip;
                let dst_mac_c = conn.dst_mac;
                let sp = conn.src_port;
                let dp = conn.dst_port;
                drop(conn);
                // Build and send ACK inline (can't call send_segment which needs conn)
                send_ack_raw(
                    src_ip_c,
                    dst_ip_c,
                    dst_mac_c,
                    sp,
                    dp,
                    ack_seg_seq,
                    ack_seg_rcv,
                );
            }
        }
        TcpState::Established | TcpState::CloseWait => {
            if flags & ACK != 0 && seq_gt(ack_num, conn.snd_una) {
                conn.snd_una = ack_num;
            }

            // Receive data
            if !tcp_data.is_empty() {
                let space = RX_CAP - conn.rx_len;
                let copy_len = tcp_data.len().min(space);
                if copy_len > 0 {
                    let dst_start = conn.rx_len;
                    conn.rx_buf[dst_start..dst_start + copy_len]
                        .copy_from_slice(&tcp_data[..copy_len]);
                    conn.rx_len += copy_len;
                    conn.rcv_seq = conn.rcv_seq.wrapping_add(copy_len as u32);
                }
            }

            if flags & FIN != 0 {
                conn.rcv_seq = conn.rcv_seq.wrapping_add(1);
                conn.rx_fin = true;
                if conn.state == TcpState::Established {
                    conn.state = TcpState::CloseWait;
                } else {
                    conn.state = TcpState::Closed;
                }
            }

            // Send ACK if we received data or FIN
            if !tcp_data.is_empty() || flags & FIN != 0 {
                let src_ip_c = conn.src_ip;
                let dst_ip_c = conn.dst_ip;
                let dst_mac_c = conn.dst_mac;
                let sp = conn.src_port;
                let dp = conn.dst_port;
                let snd_seq = conn.snd_seq;
                let rcv_seq = conn.rcv_seq;
                drop(conn);
                send_ack_raw(src_ip_c, dst_ip_c, dst_mac_c, sp, dp, snd_seq, rcv_seq);
                return;
            }
        }
        TcpState::FinWait1 => {
            if flags & ACK != 0 && seq_gt(ack_num, conn.snd_una) {
                conn.snd_una = ack_num;
            }
            if flags & ACK != 0 {
                conn.state = TcpState::TimeWait;
            }
        }
        _ => {}
    }
}

/// Send a bare ACK without needing a TcpConn reference (avoids lock re-entry).
fn send_ack_raw(
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    dst_mac: [u8; 6],
    src_port: u16,
    dst_port: u16,
    snd_seq: u32,
    rcv_seq: u32,
) {
    const FRAME_LEN: usize = ETH_HDR + IPV4_HDR + TCP_HDR;
    let mut frame = [0u8; FRAME_LEN];

    let tcp = &mut frame[ETH_HDR + IPV4_HDR..];
    tcp[0] = (src_port >> 8) as u8;
    tcp[1] = (src_port & 0xFF) as u8;
    tcp[2] = (dst_port >> 8) as u8;
    tcp[3] = (dst_port & 0xFF) as u8;
    tcp[4] = (snd_seq >> 24) as u8;
    tcp[5] = (snd_seq >> 16) as u8;
    tcp[6] = (snd_seq >> 8) as u8;
    tcp[7] = snd_seq as u8;
    tcp[8] = (rcv_seq >> 24) as u8;
    tcp[9] = (rcv_seq >> 16) as u8;
    tcp[10] = (rcv_seq >> 8) as u8;
    tcp[11] = rcv_seq as u8;
    tcp[12] = 0x50;
    tcp[13] = ACK;
    tcp[14] = 0x20;
    tcp[15] = 0x00; // window = 8192
    tcp[16] = 0;
    tcp[17] = 0;
    tcp[18] = 0;
    tcp[19] = 0;
    let csum = tcp_checksum(src_ip, dst_ip, &frame[ETH_HDR + IPV4_HDR..]);
    let tcp = &mut frame[ETH_HDR + IPV4_HDR..];
    tcp[16] = (csum >> 8) as u8;
    tcp[17] = (csum & 0xFF) as u8;

    let mut ip_hdr = [0u8; IPV4_HDR];
    static IP_ID: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0x6000);
    let ip_id = IP_ID.fetch_add(1, Ordering::Relaxed);
    ipv4::build_header(&mut ip_hdr, ip_id, 64, PROTO_TCP, src_ip, dst_ip, TCP_HDR);
    frame[ETH_HDR..ETH_HDR + IPV4_HDR].copy_from_slice(&ip_hdr);

    let our_mac = crate::drivers::virtio_net::mac_addr();
    frame[0..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&our_mac);
    frame[12] = 0x08;
    frame[13] = 0x00;

    crate::drivers::virtio_net::send_frame(&frame);
}
