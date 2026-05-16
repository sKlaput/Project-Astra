// ---------------------------------------------------------------------------
// Astra OS — IP configuration
//
// Static IP for QEMU user-mode networking:
//   Guest:   10.0.2.15 / 255.255.255.0
//   Gateway: 10.0.2.2
//   DNS:     10.0.2.3
//
// QEMU's slirp assigns these by convention.
// ---------------------------------------------------------------------------

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

static CONFIGURED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
pub struct IpConfig {
    pub ip:      [u8; 4],
    pub mask:    [u8; 4],
    pub gateway: [u8; 4],
    pub dns:     [u8; 4],
}

impl IpConfig {
    const fn zero() -> Self {
        IpConfig {
            ip:      [0; 4],
            mask:    [0; 4],
            gateway: [0; 4],
            dns:     [0; 4],
        }
    }
}

static CONFIG: Mutex<IpConfig> = Mutex::new(IpConfig::zero());

/// Apply QEMU default static config (10.0.2.15/24, gw 10.0.2.2).
pub fn apply_qemu_defaults() {
    let mut cfg = CONFIG.lock();
    cfg.ip      = [10, 0, 2, 15];
    cfg.mask    = [255, 255, 255, 0];
    cfg.gateway = [10, 0, 2, 2];
    cfg.dns     = [10, 0, 2, 3];
    CONFIGURED.store(true, Ordering::Relaxed);
}

pub fn get() -> Option<IpConfig> {
    if CONFIGURED.load(Ordering::Relaxed) {
        Some(*CONFIG.lock())
    } else {
        None
    }
}

pub fn our_ip() -> Option<[u8; 4]> {
    get().map(|c| c.ip)
}

pub fn is_our_ip(ip: [u8; 4]) -> bool {
    our_ip().map_or(false, |ours| ours == ip)
}

pub fn gateway_ip() -> Option<[u8; 4]> {
    get().map(|c| c.gateway)
}

/// Returns true if `ip` is on our local subnet.
pub fn is_local(ip: [u8; 4]) -> bool {
    if let Some(cfg) = get() {
        for i in 0..4 {
            if ip[i] & cfg.mask[i] != cfg.ip[i] & cfg.mask[i] { return false; }
        }
        return true;
    }
    false
}
