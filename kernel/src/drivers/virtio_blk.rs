// ---------------------------------------------------------------------------
// Astra OS — virtio-blk driver (legacy PCI, read-only, Step 1)
//
// Detects the virtio-blk device in QEMU via PCI config space, initialises
// the virtio legacy interface, and exposes a single `read_sector()` call.
//
// Protocol: virtio legacy over PCI I/O BAR (no MSI, no MMIO, polling only).
// Hardware target: QEMU `-device virtio-blk-pci,drive=…`
// ---------------------------------------------------------------------------

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering, fence};
use spin::Mutex;

// ── Port I/O helpers ──────────────────────────────────────────────────────────

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

// ── PCI config space ──────────────────────────────────────────────────────────

const PCI_CFG_ADDR: u16 = 0xCF8;
const PCI_CFG_DATA: u16 = 0xCFC;

/// Build a PCI CONFIG_ADDRESS value.
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

/// Scan PCI bus 0 for a virtio-blk device.
/// Returns (bus, device) if found.
fn find_virtio_blk() -> Option<(u8, u8)> {
    const VIRTIO_VENDOR:  u16 = 0x1AF4;
    const VIRTIO_BLK_DEV: u16 = 0x1001; // legacy block device
    for dev in 0u8..32 {
        let id = unsafe { pci_read32(0, dev, 0, 0x00) };
        if id == 0xFFFF_FFFF { continue; }
        let vendor = (id & 0xFFFF) as u16;
        let device = (id >> 16) as u16;
        if vendor == VIRTIO_VENDOR && device == VIRTIO_BLK_DEV {
            return Some((0, dev));
        }
    }
    None
}

/// Read PCI BAR0 and extract the I/O port base (bit 0 == 1 → I/O space).
fn pci_io_base(bus: u8, dev: u8) -> Option<u16> {
    let bar0 = unsafe { pci_read32(bus, dev, 0, 0x10) };
    if bar0 & 1 == 0 { return None; } // not I/O space
    Some((bar0 & 0xFFFC) as u16)
}

const VIO_DEV_FEAT:   u16 = 0x00; // device features (R, 32-bit)
const VIO_DRV_FEAT:   u16 = 0x04; // driver features (W, 32-bit)
const VIO_QUEUE_PFN:  u16 = 0x08; // virtqueue page frame number (R/W, 32-bit)
const VIO_QUEUE_SIZE: u16 = 0x0C; // virtqueue size (R, 16-bit)
const VIO_QUEUE_SEL:  u16 = 0x0E; // virtqueue select (W, 16-bit)
const VIO_QUEUE_NTF:  u16 = 0x10; // virtqueue notify (W, 16-bit)
const VIO_DEV_STATUS: u16 = 0x12; // device status (R/W, 8-bit)
const VIO_ISR:        u16 = 0x13; // ISR status (R, 8-bit)
const VIO_CFG:        u16 = 0x14; // device-specific config (block: u64 capacity)

const STS_ACK:       u8 = 0x01;
const STS_DRIVER:    u8 = 0x02;
const STS_DRIVER_OK: u8 = 0x04;

// ── Virtqueue constants ───────────────────────────────────────────────────────
//
// QEMU reports QueueSize = 256 for virtio-blk-pci.  Legacy virtio requires
// the driver to use the exact size the device reports; the buffer layout
// (avail ring offset, used ring offset) is derived from that size.
//
// For QueueSize N:
//   Descriptor table offset: 0            (16 * N bytes)
//   Available ring offset:   16 * N       (4 + 2*N bytes)
//   Used ring offset:        align_up(16*N + 4 + 2*N, 4096)
//
// For N = 256:
//   Descriptor table: 0       (4096 bytes)
//   Available ring:   4096    (4 + 512 = 516 bytes)
//   Used ring:        8192    (4 + 4 + 256*8 = 2056 bytes)
//   Total QUEUE_MEM:  8192 + 2056 = 10248 → round to 12288 (3 pages)

const QUEUE_SIZE:    usize = 256;
const VRING_F_NEXT:  u16 = 0x01;
const VRING_F_WRITE: u16 = 0x02;

// ── DMA memory (static, page-aligned, in kernel BSS) ─────────────────────────
//
// Layout of QUEUE_MEM (12 KiB = 3 pages):
//   Page 0 (offset    0): descriptor table — 256 × 16 = 4096 B
//   Page 1 (offset 4096): available ring   — 4 + 512 = 516 B, rest padding
//   Page 2 (offset 8192): used ring        — 4 + 4 + 256×8 = 2056 B
//
// Layout of REQ_MEM (4 KiB):
//   Offset    0: BlkReq header (16 bytes)
//   Offset  512: data buffer  (512 bytes)
//   Offset 1024: status byte  (1 byte)

struct DmaBuf<const N: usize>(UnsafeCell<[u8; N]>);
// SAFETY: all mutable access is serialised by BLK_LOCK (spin::Mutex).
unsafe impl<const N: usize> Sync for DmaBuf<N> {}

#[repr(C, align(4096))]
struct Aligned4K<const N: usize>(DmaBuf<N>);

static QUEUE_MEM: Aligned4K<12288> = Aligned4K(DmaBuf(UnsafeCell::new([0u8; 12288])));
static REQ_MEM:   Aligned4K<4096>  = Aligned4K(DmaBuf(UnsafeCell::new([0u8; 4096])));

/// Convert a virtual address within the kernel binary to its physical address.
/// Uses Limine's kernel address response: phys = virt - virt_base + phys_base.
fn kernel_virt_to_phys(virt: usize) -> usize {
    let (phys_base, virt_base) = crate::boot::protocol::kernel_address()
        .unwrap_or((0, 0));
    virt - virt_base + phys_base
}

// ── Global driver state ───────────────────────────────────────────────────────

static INITIALIZED:  AtomicBool = AtomicBool::new(false);
static IO_BASE:      AtomicU16  = AtomicU16::new(0);
static CAPACITY:     AtomicU64  = AtomicU64::new(0); // sectors
static AVAIL_IDX:    AtomicU16  = AtomicU16::new(0); // next avail ring slot
static LAST_USED:    AtomicU16  = AtomicU16::new(0); // last processed used idx

/// Mutex serialises all request operations.
static BLK_LOCK: Mutex<()> = Mutex::new(());

// ── Virtqueue field accessors (using raw pointer arithmetic) ──────────────────
//
// All offsets are based on the legacy virtio spec:
//   Descriptor table: offset 0,   16 * QUEUE_SIZE bytes
//   Available ring:   offset 128, 4 + 2*QUEUE_SIZE bytes (2-byte alignment)
//   Used ring:        offset 4096 (next page boundary)

const DESC_OFF:   usize = 0;
const AVAIL_OFF:  usize = QUEUE_SIZE * 16;        // = 4096 for N=256
const USED_OFF:   usize = AVAIL_OFF + 4096;       // next page boundary = 8192

/// Write a single virtqueue descriptor at index `i`.
unsafe fn write_desc(base_virt: usize, i: usize, addr: u64, len: u32, flags: u16, next: u16) {
    unsafe {
        let p = (base_virt + DESC_OFF + i * 16) as *mut u64;
        p.write_volatile(addr);                           // addr
        ((base_virt + DESC_OFF + i * 16 + 8) as *mut u32).write_volatile(len);   // len
        ((base_virt + DESC_OFF + i * 16 + 12) as *mut u16).write_volatile(flags);// flags
        ((base_virt + DESC_OFF + i * 16 + 14) as *mut u16).write_volatile(next); // next
    }
}

/// Write available ring entry and advance idx.
unsafe fn write_avail(base_virt: usize, ring_idx: u16, desc_head: u16) {
    unsafe {
        let slot = (ring_idx as usize) % QUEUE_SIZE;
        // avail.ring[slot] at AVAIL_OFF + 4 + slot*2
        let ring_ptr = (base_virt + AVAIL_OFF + 4 + slot * 2) as *mut u16;
        ring_ptr.write_volatile(desc_head);
        fence(Ordering::Release);
        // avail.idx at AVAIL_OFF + 2
        let idx_ptr = (base_virt + AVAIL_OFF + 2) as *mut u16;
        idx_ptr.write_volatile(ring_idx.wrapping_add(1));
        fence(Ordering::Release);
    }
}

/// Read the used ring idx (device increments this when a request completes).
unsafe fn read_used_idx(base_virt: usize) -> u16 {
    unsafe {
        fence(Ordering::Acquire);
        let idx_ptr = (base_virt + USED_OFF + 2) as *const u16;
        idx_ptr.read_volatile()
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Initialise the virtio-blk driver. Returns `true` on success.
pub fn init() -> bool {
    // --- 1. Find the PCI device ---
    let (bus, dev) = match find_virtio_blk() {
        Some(x) => x,
        None => {
            crate::serial::write_line("virtio-blk: device not found on PCI bus 0");
            return false;
        }
    };

    let io_base = unsafe {
        match pci_io_base(bus, dev) {
            Some(b) => b,
            None => {
                crate::serial::write_line("virtio-blk: BAR0 is not an I/O BAR");
                return false;
            }
        }
    };

    crate::serial::write_str("virtio-blk: found at PCI ");
    crate::serial::write_u64(bus as u64);
    crate::serial::write_str(":");
    crate::serial::write_u64(dev as u64);
    crate::serial::write_str("  io_base=0x");
    crate::serial::write_hex64(io_base as u64);
    crate::serial::write_line("");

    unsafe {
        // --- 2. Reset device ---
        out8(io_base + VIO_DEV_STATUS, 0);

        // --- 3. Acknowledge + Driver ---
        out8(io_base + VIO_DEV_STATUS, STS_ACK);
        out8(io_base + VIO_DEV_STATUS, STS_ACK | STS_DRIVER);

        // --- 4. Feature negotiation (we accept no optional features) ---
        let _dev_feat = in32(io_base + VIO_DEV_FEAT);
        out32(io_base + VIO_DRV_FEAT, 0);

        // --- 5. Set up virtqueue 0 ---
        out16(io_base + VIO_QUEUE_SEL, 0);
        let qsize = in16(io_base + VIO_QUEUE_SIZE) as usize;
        if qsize == 0 {
            crate::serial::write_line("virtio-blk: queue size is 0 — aborting");
            return false;
        }
        crate::serial::write_str("virtio-blk: queue size reported by device = ");
        crate::serial::write_u64(qsize as u64);
        crate::serial::write_line("");

        // Zero the queue memory before handing it to the device
        let q_virt = QUEUE_MEM.0.0.get() as *mut u8;
        core::ptr::write_bytes(q_virt, 0, 12288);
        let r_virt = REQ_MEM.0.0.get() as *mut u8;
        core::ptr::write_bytes(r_virt, 0, 4096);
        fence(Ordering::SeqCst);

        // Compute physical address of queue page
        let q_virt_addr = q_virt as usize;
        let q_phys = kernel_virt_to_phys(q_virt_addr);
        let q_pfn = (q_phys / 4096) as u32;

        crate::serial::write_str("virtio-blk: queue phys=0x");
        crate::serial::write_hex64(q_phys as u64);
        crate::serial::write_str(" pfn=");
        crate::serial::write_u64(q_pfn as u64);
        crate::serial::write_line("");

        out32(io_base + VIO_QUEUE_PFN, q_pfn);

        // --- 6. Driver OK ---
        out8(io_base + VIO_DEV_STATUS, STS_ACK | STS_DRIVER | STS_DRIVER_OK);

        // --- 7. Read disk capacity (config at offset VIO_CFG, u64 little-endian) ---
        let cap_lo = in32(io_base + VIO_CFG) as u64;
        let cap_hi = in32(io_base + VIO_CFG + 4) as u64;
        let capacity_sectors = cap_lo | (cap_hi << 32);

        crate::serial::write_str("virtio-blk: capacity = ");
        crate::serial::write_u64(capacity_sectors);
        crate::serial::write_str(" sectors (");
        crate::serial::write_u64(capacity_sectors / 2);
        crate::serial::write_line(" KiB)");

        CAPACITY.store(capacity_sectors, Ordering::Relaxed);
        IO_BASE.store(io_base, Ordering::Relaxed);
    }

    INITIALIZED.store(true, Ordering::Release);

    // --- 8. Self-test: read sector 0 ---
    let mut buf = [0u8; 512];
    match read_sector(0, &mut buf) {
        Ok(()) => {
            crate::serial::write_str("virtio-blk: sector 0 read OK  first bytes: ");
            for i in 0..8 {
                crate::serial::write_str("0x");
                crate::serial::write_hex64(buf[i] as u64);
                crate::serial::write_str(" ");
            }
            crate::serial::write_line("");
        }
        Err(e) => {
            crate::serial::write_str("virtio-blk: sector 0 read FAILED: ");
            crate::serial::write_line(e);
        }
    }

    true
}

/// Returns total number of 512-byte sectors on the disk. 0 if not initialised.
pub fn sector_count() -> u64 {
    CAPACITY.load(Ordering::Relaxed)
}

/// Read one 512-byte sector at `lba` into `buf`.
pub fn read_sector(lba: u64, buf: &mut [u8; 512]) -> Result<(), &'static str> {
    if !INITIALIZED.load(Ordering::Acquire) {
        return Err("not initialised");
    }
    if lba >= CAPACITY.load(Ordering::Relaxed) {
        return Err("lba out of range");
    }

    let _guard = BLK_LOCK.lock();

    let io_base  = IO_BASE.load(Ordering::Relaxed);
    let q_virt   = QUEUE_MEM.0.0.get() as usize;
    let req_virt = REQ_MEM.0.0.get() as usize;
    let req_phys = kernel_virt_to_phys(req_virt) as u64;

    unsafe {
        // --- Build BlkReq header at req_virt + 0 ---
        // type (u32) = 0 (read), reserved (u32) = 0, sector (u64) = lba
        let hdr = req_virt as *mut u32;
        hdr.write_volatile(0);           // type = VIRTIO_BLK_T_IN
        hdr.add(1).write_volatile(0);    // reserved
        (req_virt as *mut u64).add(1).write_volatile(lba); // sector at offset 8

        // Clear status byte at req_virt + 1024
        *((req_virt + 1024) as *mut u8) = 0xFF; // 0xFF = unset; device writes 0=ok
        fence(Ordering::Release);

        // --- Set up 3 descriptors ---
        // desc[0]: request header (device reads)
        write_desc(q_virt, 0, req_phys,        16,  VRING_F_NEXT,              1);
        // desc[1]: data buffer   (device writes 512 bytes)
        write_desc(q_virt, 1, req_phys + 512,  512, VRING_F_WRITE | VRING_F_NEXT, 2);
        // desc[2]: status byte   (device writes 1 byte)
        write_desc(q_virt, 2, req_phys + 1024, 1,   VRING_F_WRITE,             0);
        fence(Ordering::Release);

        // --- Submit to available ring ---
        let avail_idx = AVAIL_IDX.load(Ordering::Relaxed);
        write_avail(q_virt, avail_idx, 0); // chain starts at descriptor 0

        // --- Notify device: queue 0 has new entries ---
        out16(io_base + VIO_QUEUE_NTF, 0);

        // --- Poll used ring until device marks request done ---
        let target = avail_idx.wrapping_add(1);
        let deadline = crate::arch::x86_64::interrupts::uptime_ms() + 5000;
        loop {
            let used = read_used_idx(q_virt);
            if used == target { break; }
            if crate::arch::x86_64::interrupts::uptime_ms() > deadline {
                return Err("timeout waiting for device");
            }
            core::hint::spin_loop();
        }

        AVAIL_IDX.store(target, Ordering::Relaxed);

        // --- Check status byte ---
        let status = *((req_virt + 1024) as *const u8);
        if status != 0 {
            return Err("device returned error status");
        }

        // --- Copy data out of DMA buffer ---
        let src = (req_virt + 512) as *const u8;
        core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), 512);
    }

    Ok(())
}

/// Write one 512-byte sector at `lba` from `buf`.
pub fn write_sector(lba: u64, buf: &[u8; 512]) -> Result<(), &'static str> {
    if !INITIALIZED.load(Ordering::Acquire) {
        return Err("not initialised");
    }
    if lba >= CAPACITY.load(Ordering::Relaxed) {
        return Err("lba out of range");
    }

    let _guard = BLK_LOCK.lock();

    let io_base  = IO_BASE.load(Ordering::Relaxed);
    let q_virt   = QUEUE_MEM.0.0.get() as usize;
    let req_virt = REQ_MEM.0.0.get() as usize;
    let req_phys = kernel_virt_to_phys(req_virt) as u64;

    unsafe {
        // Build BlkReq header: type=1 (VIRTIO_BLK_T_OUT), reserved=0, sector=lba
        let hdr = req_virt as *mut u32;
        hdr.write_volatile(1);           // VIRTIO_BLK_T_OUT
        hdr.add(1).write_volatile(0);
        (req_virt as *mut u64).add(1).write_volatile(lba);

        // Copy caller's data into DMA buffer at offset 512
        let dst = (req_virt + 512) as *mut u8;
        core::ptr::copy_nonoverlapping(buf.as_ptr(), dst, 512);

        // Status byte
        *((req_virt + 1024) as *mut u8) = 0xFF;
        fence(Ordering::Release);

        // desc[0]: header — device reads
        write_desc(q_virt, 0, req_phys,        16,  VRING_F_NEXT,  1);
        // desc[1]: data   — device reads (no VRING_F_WRITE)
        write_desc(q_virt, 1, req_phys + 512,  512, VRING_F_NEXT,  2);
        // desc[2]: status — device writes 1 byte
        write_desc(q_virt, 2, req_phys + 1024, 1,   VRING_F_WRITE, 0);
        fence(Ordering::Release);

        let avail_idx = AVAIL_IDX.load(Ordering::Relaxed);
        write_avail(q_virt, avail_idx, 0);
        out16(io_base + VIO_QUEUE_NTF, 0);

        let target = avail_idx.wrapping_add(1);
        let deadline = crate::arch::x86_64::interrupts::uptime_ms() + 5000;
        loop {
            let used = read_used_idx(q_virt);
            if used == target { break; }
            if crate::arch::x86_64::interrupts::uptime_ms() > deadline {
                return Err("timeout waiting for device");
            }
            core::hint::spin_loop();
        }

        AVAIL_IDX.store(target, Ordering::Relaxed);

        let status = *((req_virt + 1024) as *const u8);
        if status != 0 {
            return Err("device returned error status");
        }
    }

    Ok(())
}
