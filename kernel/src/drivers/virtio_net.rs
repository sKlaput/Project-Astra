// ---------------------------------------------------------------------------
// Astra OS — virtio-net driver (legacy PCI, polling, IPv4/UDP capable)
//
// Uses the same legacy virtio-over-PCI-I/O-BAR approach as virtio_blk.
// Queue 0 = RX (device → driver), Queue 1 = TX (driver → device).
//
// Hardware target: QEMU `-device virtio-net-pci`
// This is a polling driver (no interrupts). Frames up to 1514 bytes.
// ---------------------------------------------------------------------------

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering, fence};
use spin::Mutex;

// ── Port I/O helpers (identical to virtio_blk) ────────────────────────────────

unsafe fn in8(port: u16) -> u8 {
    let v: u8;
    unsafe { core::arch::asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack)); }
    v
}
unsafe fn in16(port: u16) -> u16 {
    let v: u16;
    unsafe { core::arch::asm!("in ax, dx", out("ax") v, in("dx") port, options(nomem, nostack)); }
    v
}
unsafe fn in32(port: u16) -> u32 {
    let v: u32;
    unsafe { core::arch::asm!("in eax, dx", out("eax") v, in("dx") port, options(nomem, nostack)); }
    v
}
unsafe fn out8(port: u16, v: u8) {
    unsafe { core::arch::asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack)); }
}
unsafe fn out16(port: u16, v: u16) {
    unsafe { core::arch::asm!("out dx, ax", in("dx") port, in("ax") v, options(nomem, nostack)); }
}
unsafe fn out32(port: u16, v: u32) {
    unsafe { core::arch::asm!("out dx, eax", in("dx") port, in("eax") v, options(nomem, nostack)); }
}

// ── PCI helpers ───────────────────────────────────────────────────────────────

const PCI_CFG_ADDR: u16 = 0xCF8;
const PCI_CFG_DATA: u16 = 0xCFC;

fn pci_addr(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    (1 << 31)
        | ((bus  as u32) << 16)
        | ((dev  as u32) << 11)
        | ((func as u32) <<  8)
        | ((offset & 0xFC) as u32)
}

unsafe fn pci_read32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    unsafe {
        out32(PCI_CFG_ADDR, pci_addr(bus, dev, func, offset));
        in32(PCI_CFG_DATA)
    }
}

fn find_virtio_net() -> Option<(u8, u8)> {
    const VIRTIO_VENDOR:  u16 = 0x1AF4;
    const VIRTIO_NET_DEV: u16 = 0x1000; // legacy network device
    for dev in 0u8..32 {
        let id = unsafe { pci_read32(0, dev, 0, 0x00) };
        if id == 0xFFFF_FFFF { continue; }
        let vendor = (id & 0xFFFF) as u16;
        let device = (id >> 16) as u16;
        if vendor == VIRTIO_VENDOR && device == VIRTIO_NET_DEV {
            return Some((0, dev));
        }
    }
    None
}

fn pci_io_base(bus: u8, dev: u8) -> Option<u16> {
    let bar0 = unsafe { pci_read32(bus, dev, 0, 0x10) };
    if bar0 & 1 == 0 { return None; }
    Some((bar0 & 0xFFFC) as u16)
}

// ── Virtio register offsets ───────────────────────────────────────────────────

const VIO_DEV_FEAT:   u16 = 0x00;
const VIO_DRV_FEAT:   u16 = 0x04;
const VIO_QUEUE_PFN:  u16 = 0x08;
const VIO_QUEUE_SIZE: u16 = 0x0C;
const VIO_QUEUE_SEL:  u16 = 0x0E;
const VIO_QUEUE_NTF:  u16 = 0x10;
const VIO_DEV_STATUS: u16 = 0x12;
const VIO_CFG:        u16 = 0x14;  // device-specific config: MAC[6] + status[2]

const STS_ACK:       u8 = 0x01;
const STS_DRIVER:    u8 = 0x02;
const STS_DRIVER_OK: u8 = 0x04;

// ── Virtio-net feature bits ───────────────────────────────────────────────────

const VIRTIO_NET_F_MAC:      u32 = 1 << 5;  // device has MAC address in config
const VIRTIO_NET_F_STATUS:   u32 = 1 << 16; // device has link status in config

// ── Virtio-net header (legacy, 12 bytes) ─────────────────────────────────────

/// Prepended to every TX and RX frame.
/// Without VIRTIO_NET_F_MRG_RXBUF (legacy, non-negotiated), the header is
/// 10 bytes — the `num_buffers` field is absent.
#[repr(C)]
struct VirtioNetHdr {
    flags:      u8,
    gso_type:   u8,
    hdr_len:    u16,
    gso_size:   u16,
    csum_start: u16,
    csum_offset:u16,
}

impl VirtioNetHdr {
    const fn tx_default() -> Self {
        VirtioNetHdr { flags: 0, gso_type: 0, hdr_len: 0, gso_size: 0,
                       csum_start: 0, csum_offset: 0 }
    }
    const SIZE: usize = 10;
}

// ── VirtQueue constants ───────────────────────────────────────────────────────
//
// We use QUEUE_SIZE = 16 (small, minimal memory).  Each queue needs:
//   Descriptor table: 16 * 16 = 256 bytes
//   Available ring:   4 + 2*16 = 36 bytes
//   Used ring:        next 4096-aligned page → offset 4096
//   Used ring size:   4 + 4 + 16*8 = 136 bytes
// Total per queue: 4096 + 136 → round up to 4096 + 4096 = 8192 bytes (2 pages)
//
// We allocate 8 KiB per queue (2 pages each), plus frame buffers.

// Hardware queue size — MUST match what QEMU reports (256 for virtio-net).
// This drives the ring layout: AVAIL_OFF = QUEUE_SIZE*16, USED_OFF = next page.
const QUEUE_SIZE: usize = 256;

// How many RX/TX slots we actually pre-fill / cycle through.
// Smaller than QUEUE_SIZE to keep buffer memory manageable.
const MAX_RX_SLOTS: usize = 16;
const MAX_TX_SLOTS: usize = 16;

const DESC_OFF:  usize = 0;                  // descriptor table at offset 0
const AVAIL_OFF: usize = QUEUE_SIZE * 16;    // 4096 for QUEUE_SIZE=256
const USED_OFF:  usize = AVAIL_OFF + 4096;   // 8192 (next page boundary)

const VRING_F_NEXT:  u16 = 0x01;
const VRING_F_WRITE: u16 = 0x02;  // device-writable (RX descriptors)

// ── DMA memory (static, page-aligned) ─────────────────────────────────────────
//
// RX_QUEUE_MEM:  12 KiB  — RX virtqueue (desc table @ 0, avail @ 4096, used @ 8192)
// TX_QUEUE_MEM:  12 KiB  — TX virtqueue (same layout)
// RX_FRAME_MEM:  32 KiB  — MAX_RX_SLOTS receive frame slots, each 2048 bytes
//                           (12 B virtio header + up to 1514 B frame)
// TX_HDR_MEM:     4 KiB  — TX virtio-net headers (MAX_TX_SLOTS * 12 B)
// TX_FRAME_MEM:  32 KiB  — TX frame payload staging (MAX_TX_SLOTS * 2048 B)

struct DmaBuf<const N: usize>(UnsafeCell<[u8; N]>);
unsafe impl<const N: usize> Sync for DmaBuf<N> {}

#[repr(C, align(4096))]
struct Aligned4K<const N: usize>(DmaBuf<N>);

// 12 KiB per queue: desc (4096) + avail (4096) + used (4096)
static RX_QUEUE_MEM: Aligned4K<12288> = Aligned4K(DmaBuf(UnsafeCell::new([0u8; 12288])));
static TX_QUEUE_MEM: Aligned4K<12288> = Aligned4K(DmaBuf(UnsafeCell::new([0u8; 12288])));

// Each RX slot: 12-byte header + 1514-byte frame, rounded up to 1526, padded to 2048
const RX_SLOT_SIZE: usize = 2048;
static RX_FRAME_MEM: Aligned4K<{QUEUE_SIZE * 2048}> =
    Aligned4K(DmaBuf(UnsafeCell::new([0u8; QUEUE_SIZE * 2048])));

// TX header memory: 16 slots × 12 bytes, but rounded to one page
static TX_HDR_MEM: Aligned4K<4096> = Aligned4K(DmaBuf(UnsafeCell::new([0u8; 4096])));

// TX frame staging: 16 slots × 2048 bytes
const TX_SLOT_SIZE: usize = 2048;
static TX_FRAME_MEM: Aligned4K<{QUEUE_SIZE * 2048}> =
    Aligned4K(DmaBuf(UnsafeCell::new([0u8; QUEUE_SIZE * 2048])));

fn kernel_virt_to_phys(virt: usize) -> usize {
    let (phys_base, virt_base) = crate::boot::protocol::kernel_address()
        .unwrap_or((0, 0));
    virt - virt_base + phys_base
}

// ── Global driver state ───────────────────────────────────────────────────────

static INITIALIZED:  AtomicBool = AtomicBool::new(false);
static IO_BASE:      AtomicU16  = AtomicU16::new(0);

// Separate avail/used indices for each queue
static RX_AVAIL_IDX: AtomicU16 = AtomicU16::new(0);
static RX_LAST_USED: AtomicU16 = AtomicU16::new(0);
static TX_AVAIL_IDX: AtomicU16 = AtomicU16::new(0);
static TX_LAST_USED: AtomicU16 = AtomicU16::new(0);

static TX_FRAMES_SENT: AtomicU64 = AtomicU64::new(0);
static RX_FRAMES_RECV: AtomicU64 = AtomicU64::new(0);

static MAC_ADDR: Mutex<[u8; 6]> = Mutex::new([0u8; 6]);

/// Serialises TX/RX operations.
static NET_LOCK: Mutex<()> = Mutex::new(());

// ── Virtqueue helpers ─────────────────────────────────────────────────────────

unsafe fn write_desc(base_virt: usize, i: usize, addr: u64, len: u32, flags: u16, next: u16) {
    unsafe {
        let p = (base_virt + DESC_OFF + i * 16) as *mut u64;
        p.write_volatile(addr);
        ((base_virt + DESC_OFF + i * 16 + 8) as *mut u32).write_volatile(len);
        ((base_virt + DESC_OFF + i * 16 + 12) as *mut u16).write_volatile(flags);
        ((base_virt + DESC_OFF + i * 16 + 14) as *mut u16).write_volatile(next);
    }
}

unsafe fn write_avail(base_virt: usize, ring_idx: u16, desc_head: u16) {
    unsafe {
        let slot = (ring_idx as usize) % QUEUE_SIZE;
        let ring_ptr = (base_virt + AVAIL_OFF + 4 + slot * 2) as *mut u16;
        ring_ptr.write_volatile(desc_head);
        fence(Ordering::Release);
        let idx_ptr = (base_virt + AVAIL_OFF + 2) as *mut u16;
        idx_ptr.write_volatile(ring_idx.wrapping_add(1));
        fence(Ordering::Release);
    }
}

unsafe fn read_used_idx(base_virt: usize) -> u16 {
    unsafe {
        fence(Ordering::Acquire);
        let idx_ptr = (base_virt + USED_OFF + 2) as *const u16;
        idx_ptr.read_volatile()
    }
}

unsafe fn read_used_elem_id(base_virt: usize, slot: usize) -> u32 {
    // used ring element: 8 bytes at USED_OFF + 4 + slot*8;  [0..3] = id, [4..7] = len
    unsafe {
        let p = (base_virt + USED_OFF + 4 + slot * 8) as *const u32;
        p.read_volatile()
    }
}

unsafe fn read_used_elem_len(base_virt: usize, slot: usize) -> u32 {
    unsafe {
        let p = (base_virt + USED_OFF + 4 + slot * 8 + 4) as *const u32;
        p.read_volatile()
    }
}

// ── Init ──────────────────────────────────────────────────────────────────────

/// Initialise the virtio-net driver.  Returns `true` on success.
pub fn init() -> bool {
    let (bus, dev) = match find_virtio_net() {
        Some(x) => x,
        None => {
            crate::serial::write_line("virtio-net: device not found on PCI bus 0");
            return false;
        }
    };

    let io_base = unsafe {
        match pci_io_base(bus, dev) {
            Some(b) => b,
            None => {
                crate::serial::write_line("virtio-net: BAR0 is not an I/O BAR");
                return false;
            }
        }
    };

    crate::serial::write_str("virtio-net: found at PCI ");
    crate::serial::write_u64(bus as u64);
    crate::serial::write_str(":");
    crate::serial::write_u64(dev as u64);
    crate::serial::write_str("  io_base=0x");
    crate::serial::write_hex64(io_base as u64);
    crate::serial::write_line("");

    unsafe {
        // 1. Reset
        out8(io_base + VIO_DEV_STATUS, 0);
        // 2. Acknowledge
        out8(io_base + VIO_DEV_STATUS, STS_ACK);
        out8(io_base + VIO_DEV_STATUS, STS_ACK | STS_DRIVER);

        // 3. Feature negotiation — accept MAC and STATUS bits only
        let dev_feat = in32(io_base + VIO_DEV_FEAT);
        let drv_feat = dev_feat & (VIRTIO_NET_F_MAC | VIRTIO_NET_F_STATUS);
        out32(io_base + VIO_DRV_FEAT, drv_feat);

        // 4. Read MAC from device config (offset VIO_CFG, 6 bytes)
        if drv_feat & VIRTIO_NET_F_MAC != 0 {
            let mut mac = MAC_ADDR.lock();
            for i in 0..6usize {
                mac[i] = in8(io_base + VIO_CFG + i as u16);
            }
            crate::serial::write_str("virtio-net: MAC ");
            for i in 0..6 {
                if i > 0 { crate::serial::write_str(":"); }
                crate::serial::write_hex64(mac[i] as u64);
            }
            crate::serial::write_line("");
        }

        // 5. Register RX queue (queue 0) — PFN only, NO avail population yet
        out16(io_base + VIO_QUEUE_SEL, 0);
        let rx_qsize = in16(io_base + VIO_QUEUE_SIZE) as usize;
        if rx_qsize == 0 {
            crate::serial::write_line("virtio-net: RX queue size is 0");
            return false;
        }
        crate::serial::write_str("virtio-net: RX qsize=");
        crate::serial::write_u64(rx_qsize as u64);
        crate::serial::write_line("");

        let rx_base_virt = RX_QUEUE_MEM.0.0.get() as usize;
        core::ptr::write_bytes(rx_base_virt as *mut u8, 0, 12288);
        fence(Ordering::SeqCst);

        let rx_phys = kernel_virt_to_phys(rx_base_virt);
        crate::serial::write_str("virtio-net: RX queue virt=0x");
        crate::serial::write_hex64(rx_base_virt as u64);
        crate::serial::write_str(" phys=0x");
        crate::serial::write_hex64(rx_phys as u64);
        crate::serial::write_str(" pfn=");
        crate::serial::write_u64((rx_phys / 4096) as u64);
        crate::serial::write_line("");
        out32(io_base + VIO_QUEUE_PFN, (rx_phys / 4096) as u32);

        // 6. Register TX queue (queue 1) — PFN only
        out16(io_base + VIO_QUEUE_SEL, 1);
        let tx_qsize = in16(io_base + VIO_QUEUE_SIZE) as usize;
        if tx_qsize == 0 {
            crate::serial::write_line("virtio-net: TX queue size is 0");
            return false;
        }

        let tx_base_virt = TX_QUEUE_MEM.0.0.get() as usize;
        core::ptr::write_bytes(tx_base_virt as *mut u8, 0, 12288);
        fence(Ordering::SeqCst);

        let tx_phys = kernel_virt_to_phys(tx_base_virt);
        out32(io_base + VIO_QUEUE_PFN, (tx_phys / 4096) as u32);

        // 7. Driver OK — must be set BEFORE populating avail rings.
        //    The virtio spec says the device ignores queue kicks until DRIVER_OK.
        out8(io_base + VIO_DEV_STATUS, STS_ACK | STS_DRIVER | STS_DRIVER_OK);
        fence(Ordering::SeqCst);

        IO_BASE.store(io_base, Ordering::Relaxed);

        // 8. NOW pre-populate RX avail ring (after DRIVER_OK so the kick is seen).
        //    Each slot: one device-writable descriptor = virtio-net header + Ethernet frame.
        let frame_base_virt = RX_FRAME_MEM.0.0.get() as usize;
        crate::serial::write_str("virtio-net: RX frame buf[0] phys=0x");
        crate::serial::write_hex64(kernel_virt_to_phys(frame_base_virt) as u64);
        crate::serial::write_line("");
        for i in 0..MAX_RX_SLOTS {
            let slot_virt = frame_base_virt + i * RX_SLOT_SIZE;
            let slot_phys = kernel_virt_to_phys(slot_virt) as u64;
            write_desc(rx_base_virt, i, slot_phys, RX_SLOT_SIZE as u32, VRING_F_WRITE, 0);
            let ai = RX_AVAIL_IDX.load(Ordering::Relaxed);
            write_avail(rx_base_virt, ai, i as u16);
            RX_AVAIL_IDX.store(ai.wrapping_add(1), Ordering::Relaxed);
        }
        crate::serial::write_str("virtio-net: RX avail.idx after init=");
        crate::serial::write_u64(RX_AVAIL_IDX.load(Ordering::Relaxed) as u64);
        crate::serial::write_line("");
        // Kick queue 0 so device processes our newly-posted RX buffers
        out16(io_base + VIO_QUEUE_NTF, 0);
    }

    INITIALIZED.store(true, Ordering::Release);
    crate::serial::write_line("virtio-net: init OK");
    true
}

/// Return the current (last_consumed, hw_used_idx) pair for the RX queue.
pub fn debug_rx_state() -> (u16, u16) {
    if !INITIALIZED.load(Ordering::Acquire) { return (0, 0); }
    let rx_base_virt = RX_QUEUE_MEM.0.0.get() as usize;
    let last = RX_LAST_USED.load(Ordering::Relaxed);
    let hw   = unsafe { read_used_idx(rx_base_virt) };
    (last, hw)
}

/// Return the current (last_consumed, hw_used_idx) pair for the TX queue.
/// If hw_tx > 0 after a send, QEMU IS processing our TX queue.
pub fn debug_tx_state() -> (u16, u16) {
    if !INITIALIZED.load(Ordering::Acquire) { return (0, 0); }
    let tx_base_virt = TX_QUEUE_MEM.0.0.get() as usize;
    let last = TX_LAST_USED.load(Ordering::Relaxed);
    let hw   = unsafe { read_used_idx(tx_base_virt) };
    (last, hw)
}

// ── TX ─────────────────────────────────────────────────────────────────────────

/// Send a raw Ethernet frame. `data` must include the Ethernet header (14 B) and payload.
/// Returns `true` on success. Frame must be ≤ 1514 bytes.
pub fn send_frame(data: &[u8]) -> bool {
    if !INITIALIZED.load(Ordering::Acquire) { return false; }
    if data.is_empty() || data.len() > 1514 { return false; }

    let _lock = NET_LOCK.lock();
    let io_base = IO_BASE.load(Ordering::Relaxed);

    let tx_base_virt = TX_QUEUE_MEM.0.0.get() as usize;
    let hdr_base_virt = TX_HDR_MEM.0.0.get() as usize;
    let frame_base_virt = TX_FRAME_MEM.0.0.get() as usize;

    // Pick a TX descriptor slot (round-robin through MAX_TX_SLOTS slots)
    let avail = TX_AVAIL_IDX.load(Ordering::Relaxed);
    let last  = TX_LAST_USED.load(Ordering::Relaxed);
    let in_flight = avail.wrapping_sub(last) as usize;
    if in_flight >= MAX_TX_SLOTS {
        // Drain used TX descriptors
        let used = unsafe { read_used_idx(tx_base_virt) };
        TX_LAST_USED.store(used, Ordering::Relaxed);
    }

    let slot = (avail as usize) % MAX_TX_SLOTS;
    // Two chained descriptors per slot: [slot*2]=header, [slot*2+1]=frame data
    let desc0 = slot * 2;
    let desc1 = slot * 2 + 1;
    // desc1 max = (MAX_TX_SLOTS-1)*2+1 = 31, well within QUEUE_SIZE=256
    if desc1 >= QUEUE_SIZE {
        return false;
    }

    unsafe {
        // Write virtio-net header into TX_HDR_MEM
        let hdr_slot_virt = hdr_base_virt + slot * VirtioNetHdr::SIZE;
        let hdr_ptr = hdr_slot_virt as *mut VirtioNetHdr;
        hdr_ptr.write_volatile(VirtioNetHdr::tx_default());

        // Copy frame data into TX_FRAME_MEM
        let frame_slot_virt = frame_base_virt + slot * TX_SLOT_SIZE;
        core::ptr::copy_nonoverlapping(data.as_ptr(), frame_slot_virt as *mut u8, data.len());

        let hdr_phys   = kernel_virt_to_phys(hdr_slot_virt) as u64;
        let frame_phys = kernel_virt_to_phys(frame_slot_virt) as u64;

        // desc0: header (chained to desc1, device-read)
        write_desc(tx_base_virt, desc0, hdr_phys, VirtioNetHdr::SIZE as u32,
                   VRING_F_NEXT, desc1 as u16);
        // desc1: frame data (end of chain, device-read)
        write_desc(tx_base_virt, desc1, frame_phys, data.len() as u32, 0, 0);

        // Post to avail ring and notify device (queue 1)
        write_avail(tx_base_virt, avail, desc0 as u16);
        TX_AVAIL_IDX.store(avail.wrapping_add(1), Ordering::Relaxed);
        out16(io_base + VIO_QUEUE_NTF, 1);
    }

    TX_FRAMES_SENT.fetch_add(1, Ordering::Relaxed);
    true
}

// ── RX ─────────────────────────────────────────────────────────────────────────

/// Poll for received frames. Calls `callback(frame_data)` for each available frame.
/// `frame_data` is the raw Ethernet frame (virtio header stripped).
/// Returns the number of frames delivered.
///
/// NOTE: Must NOT hold NET_LOCK when called — the callback may call send_frame
/// (e.g. ARP reply) which acquires NET_LOCK.  RX uses a separate queue from TX
/// so no lock is needed here on a single-CPU system.
pub fn poll_rx<F: FnMut(&[u8])>(mut callback: F) -> usize {
    if !INITIALIZED.load(Ordering::Acquire) { return 0; }

    // Do NOT acquire NET_LOCK here.  The callback (dispatch_frame) may call
    // send_frame (e.g. to send an ARP reply), which takes NET_LOCK for TX.
    // RX and TX use fully separate queue memory so no mutual exclusion is needed.

    let rx_base_virt = RX_QUEUE_MEM.0.0.get() as usize;
    let frame_base_virt = RX_FRAME_MEM.0.0.get() as usize;
    let io_base = IO_BASE.load(Ordering::Relaxed);

    let mut count = 0usize;
    loop {
        let last = RX_LAST_USED.load(Ordering::Relaxed);
        let used = unsafe { read_used_idx(rx_base_virt) };
        if last == used { break; }

        let slot = (last as usize) % QUEUE_SIZE;
        let desc_id = unsafe { read_used_elem_id(rx_base_virt, slot) } as usize;
        let rx_len  = unsafe { read_used_elem_len(rx_base_virt, slot) } as usize;

        // The slot data = VirtioNetHdr (12 B) + Ethernet frame
        if rx_len > VirtioNetHdr::SIZE {
            let frame_slot_virt = frame_base_virt + desc_id * RX_SLOT_SIZE;
            let frame_ptr = (frame_slot_virt + VirtioNetHdr::SIZE) as *const u8;
            let frame_len = rx_len - VirtioNetHdr::SIZE;
            let frame = unsafe { core::slice::from_raw_parts(frame_ptr, frame_len) };
            callback(frame);
            RX_FRAMES_RECV.fetch_add(1, Ordering::Relaxed);
            count += 1;
        }

        // Recycle descriptor back to device
        let new_last = last.wrapping_add(1);
        RX_LAST_USED.store(new_last, Ordering::Relaxed);
        let ai = RX_AVAIL_IDX.load(Ordering::Relaxed);
        unsafe {
            write_avail(rx_base_virt, ai, desc_id as u16);
            RX_AVAIL_IDX.store(ai.wrapping_add(1), Ordering::Relaxed);
            out16(io_base + VIO_QUEUE_NTF, 0);
        }
    }
    count
}

// ── Public info API ────────────────────────────────────────────────────────────

/// Returns the device MAC address (or all-zeros if not initialised).
pub fn mac_addr() -> [u8; 6] {
    *MAC_ADDR.lock()
}

/// Returns whether the driver is initialised.
pub fn is_ready() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Returns (tx_frames_sent, rx_frames_received).
pub fn stats() -> (u64, u64) {
    (
        TX_FRAMES_SENT.load(Ordering::Relaxed),
        RX_FRAMES_RECV.load(Ordering::Relaxed),
    )
}

/// Check link status from device config register (bit 0 of status field at VIO_CFG+6).
pub fn link_up() -> bool {
    if !INITIALIZED.load(Ordering::Acquire) { return false; }
    let io_base = IO_BASE.load(Ordering::Relaxed);
    let status = unsafe { in16(io_base + VIO_CFG + 6) };
    status & 1 != 0
}
