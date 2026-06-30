use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ── Network stack organized by OSI layers ──────────────────────────────────────

/// Layer 2 (Data Link) — Ethernet and network configuration
pub mod layer2;

/// Layer 3 (Network) — IPv4, ARP, and ICMP
pub mod layer3;

/// Layer 4 (Transport) — TCP and UDP
pub mod layer4;

/// Application Layer — DNS and HTTP
pub mod application;

// ── Re-export commonly used types from each layer for convenience ─────────────

pub use layer2::{config, eth};
pub use layer3::{arp, icmp, ipv4};
pub use layer4::{tcp, udp};
pub use application::{dns, http};

// ── Public init ───────────────────────────────────────────────────────────────

/// Initialise the network stack.  Call once after the virtio-net driver is up.
/// Applies a static QEMU IP configuration and logs the assigned address.
pub fn init() {
    layer2::config::apply_qemu_defaults();
    crate::serial::write_line("net: IP 10.0.2.15/24 gw 10.0.2.2");
}

/// Dispatch a raw Ethernet frame (with header) to the appropriate protocol handler.
/// Call this from the RX poll loop.
pub fn dispatch_frame(frame: &[u8]) {
    use layer2::eth::{EthHeader, ETH_ARP, ETH_HDR, ETH_IPV4};
    let Some(hdr) = EthHeader::parse(frame) else {
        return;
    };
    let payload = &frame[ETH_HDR..];
    match hdr.etype {
        ETH_ARP => layer3::arp::handle_packet(payload),
        ETH_IPV4 => handle_ipv4(payload),
        _ => {}
    }
}

fn handle_ipv4(ip_pkt: &[u8]) {
    use layer3::ipv4::{Ipv4Header, PROTO_ICMP, PROTO_TCP, PROTO_UDP};
    let Some(hdr) = Ipv4Header::parse(ip_pkt) else {
        return;
    };
    if !layer2::config::is_our_ip(hdr.dst) && hdr.dst != [255, 255, 255, 255] {
        return;
    }
    let payload = hdr.payload(ip_pkt);
    match hdr.protocol {
        PROTO_ICMP => layer3::icmp::handle_packet(hdr.src, payload),
        PROTO_UDP => layer4::udp::handle_packet(hdr.src, payload),
        PROTO_TCP => layer4::tcp::handle_segment(hdr.src, payload),
        _ => {}
    }
}

/// Poll the virtio-net driver for incoming frames and dispatch them.
/// Returns the number of frames processed.
pub fn poll_and_dispatch() -> usize {
    let mut n = 0usize;
    driver::poll_rx(|frame| {
        dispatch_frame(frame);
        n += 1;
    });
    n
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    Invalid,
    NotReady,
    Unsupported,
    BufferTooSmall,
    IoError,
}

pub mod driver {
    use super::{AtomicU64, NetError, Ordering};

    static DRIVER_REGISTERED: AtomicU64 = AtomicU64::new(0);
    static TX_FRAMES: AtomicU64 = AtomicU64::new(0);

    pub fn register_driver(name: &'static str) -> Result<(), NetError> {
        if name.is_empty() {
            return Err(NetError::Invalid);
        }
        DRIVER_REGISTERED.store(1, Ordering::Relaxed);
        Ok(())
    }

    /// Returns true if the underlying virtio-net NIC is initialised and has link.
    pub fn is_ready() -> bool {
        crate::drivers::virtio_net::is_ready()
    }

    pub fn submit_tx_frame(frame: &[u8]) -> Result<usize, NetError> {
        if frame.is_empty() {
            return Err(NetError::Invalid);
        }
        if !crate::drivers::virtio_net::is_ready() {
            return Err(NetError::NotReady);
        }
        if crate::drivers::virtio_net::send_frame(frame) {
            let n = frame.len();
            TX_FRAMES.fetch_add(1, Ordering::Relaxed);
            Ok(n)
        } else {
            Err(NetError::IoError)
        }
    }

    /// Poll for received frames; delivers each to `callback`.
    pub fn poll_rx<F: FnMut(&[u8])>(callback: F) -> usize {
        crate::drivers::virtio_net::poll_rx(callback)
    }

    pub fn mac_addr() -> [u8; 6] {
        crate::drivers::virtio_net::mac_addr()
    }

    pub fn stats() -> (bool, bool, u64, u64) {
        let ready = crate::drivers::virtio_net::is_ready();
        let link = crate::drivers::virtio_net::link_up();
        let (tx, rx) = crate::drivers::virtio_net::stats();
        (ready, link, tx, rx)
    }

    pub fn debug_rx_state() -> (u16, u16) {
        crate::drivers::virtio_net::debug_rx_state()
    }

    pub fn debug_tx_state() -> (u16, u16) {
        crate::drivers::virtio_net::debug_tx_state()
    }
}

pub mod stack {
    use super::{AtomicU64, NetError, Ordering};

    static RX_FRAMES: AtomicU64 = AtomicU64::new(0);
    static INGEST_OK: AtomicU64 = AtomicU64::new(0);

    pub fn ingest_frame(frame: &[u8]) -> Result<(), NetError> {
        if frame.len() < 2 {
            return Err(NetError::Invalid);
        }
        RX_FRAMES.fetch_add(1, Ordering::Relaxed);
        INGEST_OK.store(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn process_tick(_budget_frames: usize) -> usize {
        0
    }

    pub fn route_packet(proto: u8) -> bool {
        proto == 0x11 || proto == 0x06
    }

    pub fn emit_frame(payload: &[u8], out: &mut [u8]) -> Result<usize, NetError> {
        let needed = payload.len() + 1;
        if out.len() < needed {
            return Err(NetError::BufferTooSmall);
        }
        out[0] = 0x45;
        out[1..needed].copy_from_slice(payload);
        Ok(needed)
    }

    pub fn stats() -> (u64, bool) {
        (
            RX_FRAMES.load(Ordering::Relaxed),
            INGEST_OK.load(Ordering::Relaxed) == 1,
        )
    }
}

pub mod socket {
    use super::{AtomicU64, Mutex, NetError, Ordering};

    pub const AF_INET: u16 = 2;
    pub const SOCK_DGRAM: u16 = 2;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SocketHandle(pub u64);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum SocketState {
        Closed,
        Created,
        Bound,
        Connected,
    }

    #[derive(Clone, Copy)]
    struct SocketEntry {
        handle: u64,
        state: SocketState,
        local_addr: [u8; 4],
        local_port: u16,
        remote_addr: [u8; 4],
        remote_port: u16,
    }

    impl SocketEntry {
        const fn empty() -> Self {
            Self {
                handle: 0,
                state: SocketState::Closed,
                local_addr: [0, 0, 0, 0],
                local_port: 0,
                remote_addr: [0, 0, 0, 0],
                remote_port: 0,
            }
        }
    }

    const SOCKET_CAP: usize = 8;
    static SOCKETS: Mutex<[SocketEntry; SOCKET_CAP]> =
        Mutex::new([SocketEntry::empty(); SOCKET_CAP]);

    static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

    fn find_slot(slots: &[SocketEntry; SOCKET_CAP], handle: u64) -> Option<usize> {
        let mut i = 0;
        while i < SOCKET_CAP {
            if slots[i].handle == handle && slots[i].state != SocketState::Closed {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn alloc_slot(slots: &mut [SocketEntry; SOCKET_CAP], handle: u64) -> Result<usize, NetError> {
        let mut i = 0;
        while i < SOCKET_CAP {
            if slots[i].state == SocketState::Closed {
                slots[i] = SocketEntry {
                    handle,
                    state: SocketState::Created,
                    local_addr: [0, 0, 0, 0],
                    local_port: 0,
                    remote_addr: [0, 0, 0, 0],
                    remote_port: 0,
                };
                return Ok(i);
            }
            i += 1;
        }
        Err(NetError::NotReady)
    }

    pub fn create(domain: u16, ty: u16, protocol: u16) -> Result<SocketHandle, NetError> {
        if domain != AF_INET || ty != SOCK_DGRAM || protocol != 17 {
            return Err(NetError::Unsupported);
        }

        let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        let mut slots = SOCKETS.lock();
        alloc_slot(&mut slots, handle)?;
        Ok(SocketHandle(handle))
    }

    pub fn bind(sock: SocketHandle, addr: [u8; 4], port: u16) -> Result<(), NetError> {
        if addr == [0, 0, 0, 0] || port == 0 {
            return Err(NetError::Invalid);
        }

        let mut slots = SOCKETS.lock();
        let idx = find_slot(&slots, sock.0).ok_or(NetError::NotReady)?;
        if slots[idx].state != SocketState::Created && slots[idx].state != SocketState::Bound {
            return Err(NetError::NotReady);
        }
        slots[idx].local_addr = addr;
        slots[idx].local_port = port;
        slots[idx].state = SocketState::Bound;
        Ok(())
    }

    pub fn connect(sock: SocketHandle, addr: [u8; 4], port: u16) -> Result<(), NetError> {
        if addr == [0, 0, 0, 0] || port == 0 {
            return Err(NetError::Invalid);
        }

        let mut slots = SOCKETS.lock();
        let idx = find_slot(&slots, sock.0).ok_or(NetError::NotReady)?;
        if slots[idx].state != SocketState::Created && slots[idx].state != SocketState::Bound {
            return Err(NetError::NotReady);
        }
        slots[idx].remote_addr = addr;
        slots[idx].remote_port = port;
        slots[idx].state = SocketState::Connected;
        Ok(())
    }

    pub fn send(sock: SocketHandle, payload: &[u8]) -> Result<usize, NetError> {
        if payload.is_empty() {
            return Err(NetError::Invalid);
        }

        let slots = SOCKETS.lock();
        let idx = find_slot(&slots, sock.0).ok_or(NetError::NotReady)?;
        if slots[idx].state != SocketState::Connected {
            return Err(NetError::NotReady);
        }
        Ok(payload.len())
    }

    pub fn recv(sock: SocketHandle, out: &mut [u8]) -> Result<usize, NetError> {
        if out.is_empty() {
            return Err(NetError::BufferTooSmall);
        }

        let slots = SOCKETS.lock();
        let idx = find_slot(&slots, sock.0).ok_or(NetError::NotReady)?;
        if slots[idx].state != SocketState::Connected {
            return Err(NetError::NotReady);
        }
        out[0] = b'O';
        Ok(1)
    }

    pub fn close(sock: SocketHandle) -> Result<(), NetError> {
        let mut slots = SOCKETS.lock();
        let idx = find_slot(&slots, sock.0).ok_or(NetError::NotReady)?;
        slots[idx] = SocketEntry::empty();
        Ok(())
    }

    pub fn stats() -> (u64, u64, u64) {
        let slots = SOCKETS.lock();
        let mut open = 0u64;
        let mut connected = 0u64;
        let mut bound = 0u64;
        let mut i = 0;
        while i < SOCKET_CAP {
            match slots[i].state {
                SocketState::Closed => {}
                SocketState::Created => {
                    open += 1;
                }
                SocketState::Bound => {
                    open += 1;
                    bound += 1;
                }
                SocketState::Connected => {
                    open += 1;
                    connected += 1;
                }
            }
            i += 1;
        }
        (open, bound, connected)
    }
}

pub mod service {
    use super::Mutex;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FirewallDecision {
        Allow,
        Deny,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum DhcpState {
        Idle,
        Discovering,
        Bound,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FirewallMode {
        AllowAll,
        BlockUdp,
    }

    #[derive(Debug, Clone, Copy)]
    struct NetConfig {
        addr: [u8; 4],
        gateway: [u8; 4],
        dns: [u8; 4],
        lease_ticks: u64,
        state: DhcpState,
        firewall_mode: FirewallMode,
        fw_allow_ingress: u64,
        fw_deny_ingress: u64,
        fw_allow_egress: u64,
        fw_deny_egress: u64,
    }

    impl NetConfig {
        const fn empty() -> Self {
            Self {
                addr: [0, 0, 0, 0],
                gateway: [0, 0, 0, 0],
                dns: [0, 0, 0, 0],
                lease_ticks: 0,
                state: DhcpState::Idle,
                firewall_mode: FirewallMode::AllowAll,
                fw_allow_ingress: 0,
                fw_deny_ingress: 0,
                fw_allow_egress: 0,
                fw_deny_egress: 0,
            }
        }
    }

    static CONFIG: Mutex<NetConfig> = Mutex::new(NetConfig::empty());

    pub fn dns_resolve(name: &str) -> Option<[u8; 4]> {
        let cfg = CONFIG.lock();
        match name {
            "kernel.local" => {
                if cfg.addr == [0, 0, 0, 0] {
                    None
                } else {
                    Some(cfg.addr)
                }
            }
            "resolver.local" => {
                if cfg.dns == [0, 0, 0, 0] {
                    None
                } else {
                    Some(cfg.dns)
                }
            }
            _ => None,
        }
    }

    pub fn dhcp_start() -> bool {
        let mut cfg = CONFIG.lock();
        cfg.state = DhcpState::Discovering;
        cfg.lease_ticks = 0;
        true
    }

    pub fn dhcp_tick() -> bool {
        let mut cfg = CONFIG.lock();
        if cfg.state != DhcpState::Discovering {
            return false;
        }

        // Deterministic v0 lease for probe/evidence.
        cfg.addr = [10, 0, 2, 15];
        cfg.gateway = [10, 0, 2, 2];
        cfg.dns = [1, 1, 1, 1];
        cfg.lease_ticks = 300;
        cfg.state = DhcpState::Bound;
        true
    }

    pub fn dhcp_renew() -> bool {
        let mut cfg = CONFIG.lock();
        if cfg.state != DhcpState::Bound {
            return false;
        }
        cfg.lease_ticks = 300;
        true
    }

    pub fn network_config() -> ([u8; 4], [u8; 4], [u8; 4], u64, bool) {
        let cfg = CONFIG.lock();
        (
            cfg.addr,
            cfg.gateway,
            cfg.dns,
            cfg.lease_ticks,
            cfg.state == DhcpState::Bound,
        )
    }

    pub fn firewall_set_udp_block(enabled: bool) {
        let mut cfg = CONFIG.lock();
        cfg.firewall_mode = if enabled {
            FirewallMode::BlockUdp
        } else {
            FirewallMode::AllowAll
        };
    }

    pub fn firewall_stats() -> (u64, u64, u64, u64, bool) {
        let cfg = CONFIG.lock();
        (
            cfg.fw_allow_ingress,
            cfg.fw_deny_ingress,
            cfg.fw_allow_egress,
            cfg.fw_deny_egress,
            cfg.firewall_mode == FirewallMode::BlockUdp,
        )
    }

    pub fn firewall_decide(ingress: bool, proto: u8, _len: usize) -> FirewallDecision {
        let mut cfg = CONFIG.lock();
        let is_udp = proto == 0x11;
        let decision = match cfg.firewall_mode {
            FirewallMode::AllowAll => FirewallDecision::Allow,
            FirewallMode::BlockUdp => {
                if is_udp {
                    FirewallDecision::Deny
                } else {
                    FirewallDecision::Allow
                }
            }
        };

        match (ingress, decision) {
            (true, FirewallDecision::Allow) => cfg.fw_allow_ingress += 1,
            (true, FirewallDecision::Deny) => cfg.fw_deny_ingress += 1,
            (false, FirewallDecision::Allow) => cfg.fw_allow_egress += 1,
            (false, FirewallDecision::Deny) => cfg.fw_deny_egress += 1,
        }

        decision
    }
}
