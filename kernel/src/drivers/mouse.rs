// ---------------------------------------------------------------------------
// PS/2 Mouse Driver (polling-based)
//
// Initialises the PS/2 auxiliary device and enables default streaming mode.
// Mouse packets (3 bytes: status, dx, dy) are buffered in a small ring.
// The input layer calls `poll_aux_bytes()` to drain pending bytes from the
// PS/2 controller, then `read_mouse_packet()` to consume assembled packets.
// ---------------------------------------------------------------------------

use super::{Driver, DriverError};
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

// ── I/O ports ─────────────────────────────────────────────────────────────────

const PS2_DATA: u16 = 0x60;
const PS2_STATUS: u16 = 0x64;
const PS2_CMD: u16 = 0x64;

fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe { core::arch::asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack)); }
    value
}

fn outb(port: u16, value: u8) {
    unsafe { core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack)); }
}

fn wait_input_ready() {
    for _ in 0..100_000 {
        if inb(PS2_STATUS) & 0x02 == 0 { return; }
        core::hint::spin_loop();
    }
}

fn wait_output_ready() -> bool {
    for _ in 0..100_000 {
        if inb(PS2_STATUS) & 0x01 != 0 { return true; }
        core::hint::spin_loop();
    }
    false
}

fn ps2_cmd(cmd: u8) {
    wait_input_ready();
    outb(PS2_CMD, cmd);
}

fn ps2_write_data(data: u8) {
    wait_input_ready();
    outb(PS2_DATA, data);
}

fn ps2_read_data() -> u8 {
    if wait_output_ready() { inb(PS2_DATA) } else { 0 }
}

fn mouse_write(byte: u8) {
    ps2_cmd(0xD4);        // route next data byte to aux device
    ps2_write_data(byte);
    // Read and discard ACK (0xFA)
    let _ = ps2_read_data();
}

// ── Packet ring buffer ────────────────────────────────────────────────────────

#[derive(Copy, Clone)]
pub struct MousePacket {
    pub buttons: u8,
    pub dx: i32,
    pub dy: i32,
}

const PKT_BUF_SIZE: usize = 32;
static PKT_BUF: [AtomicU8; PKT_BUF_SIZE * 3] = {
    const Z: AtomicU8 = AtomicU8::new(0);
    [Z; PKT_BUF_SIZE * 3]
};
static PKT_HEAD: AtomicUsize = AtomicUsize::new(0);
static PKT_TAIL: AtomicUsize = AtomicUsize::new(0);

fn push_packet(status: u8, dx_raw: u8, dy_raw: u8) {
    let head = PKT_HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % PKT_BUF_SIZE;
    if next == PKT_TAIL.load(Ordering::Acquire) {
        // Full: drop oldest
        PKT_TAIL.store((PKT_TAIL.load(Ordering::Relaxed) + 1) % PKT_BUF_SIZE, Ordering::Release);
    }
    let base = head * 3;
    PKT_BUF[base].store(status, Ordering::Relaxed);
    PKT_BUF[base + 1].store(dx_raw, Ordering::Relaxed);
    PKT_BUF[base + 2].store(dy_raw, Ordering::Relaxed);
    PKT_HEAD.store(next, Ordering::Release);
}

/// Returns true if there is at least one assembled packet waiting.
pub fn has_pending_packets() -> bool {
    PKT_TAIL.load(Ordering::Relaxed) != PKT_HEAD.load(Ordering::Acquire)
}

/// Read one assembled mouse packet, or None if empty.
pub fn read_mouse_packet() -> Option<MousePacket> {
    let tail = PKT_TAIL.load(Ordering::Acquire);
    if tail == PKT_HEAD.load(Ordering::Acquire) {
        return None;
    }
    let base = tail * 3;
    let status = PKT_BUF[base].load(Ordering::Relaxed);
    let dx_raw = PKT_BUF[base + 1].load(Ordering::Relaxed);
    let dy_raw = PKT_BUF[base + 2].load(Ordering::Relaxed);
    PKT_TAIL.store((tail + 1) % PKT_BUF_SIZE, Ordering::Release);

    let buttons = status & 0x07;
    let mut dx = dx_raw as i32;
    let mut dy = dy_raw as i32;
    if status & 0x10 != 0 { dx -= 256; } // sign-extend X
    if status & 0x20 != 0 { dy -= 256; } // sign-extend Y
    dy = -dy; // PS/2 Y is inverted (up=positive), screen Y is down=positive

    Some(MousePacket { buttons, dx, dy })
}

// ── Polling (called from input layer) ─────────────────────────────────────────

static BYTE_IDX: AtomicU8 = AtomicU8::new(0);
static ACCUM: [AtomicU8; 3] = [AtomicU8::new(0), AtomicU8::new(0), AtomicU8::new(0)];

/// Drain all pending aux (mouse) bytes from the PS/2 controller into packets.
pub fn poll_aux_bytes() {
    for _ in 0..64 {
        let st = inb(PS2_STATUS);
        if st & 0x01 == 0 { break; }         // no data pending
        if st & 0x20 == 0 {
            // Keyboard byte — read it so the FIFO advances, then forward
            // to the keyboard ring buffer so keystrokes aren't lost.
            let kb_byte = inb(PS2_DATA);
            crate::drivers::keyboard::push_scancode_from_poll(kb_byte);
            continue;
        }
        let byte = inb(PS2_DATA);

        let idx = BYTE_IDX.load(Ordering::Relaxed);
        if idx == 0 {
            // First byte must have bit 3 set (always-1 bit in PS/2 status byte)
            if byte & 0x08 == 0 { continue; } // resync
        }
        ACCUM[idx as usize].store(byte, Ordering::Relaxed);
        if idx == 2 {
            push_packet(
                ACCUM[0].load(Ordering::Relaxed),
                ACCUM[1].load(Ordering::Relaxed),
                ACCUM[2].load(Ordering::Relaxed),
            );
            BYTE_IDX.store(0, Ordering::Relaxed);
        } else {
            BYTE_IDX.store(idx + 1, Ordering::Relaxed);
        }
    }
}

// ── Driver trait ──────────────────────────────────────────────────────────────

pub struct Ps2MouseDriver;

impl Driver for Ps2MouseDriver {
    fn name(&self) -> &'static str { "ps2-mouse" }
    fn category(&self) -> &'static str { "input" }

    fn init(&self) -> Result<(), DriverError> {
        // Enable auxiliary device
        ps2_cmd(0xA8);

        // Read controller config byte
        ps2_cmd(0x20);
        let mut cfg = ps2_read_data();
        cfg |= 0x02;  // enable IRQ12 (aux interrupt) — not used for polling but safe
        cfg |= 0x40;  // enable scancode set 2 → set 1 translation
        cfg &= !0x20; // clear aux disable bit
        ps2_cmd(0x60);
        ps2_write_data(cfg);

        // Reset mouse
        mouse_write(0xFF);
        // Discard BAT result and device ID
        let _ = ps2_read_data();
        let _ = ps2_read_data();

        // Set defaults and enable streaming
        mouse_write(0xF6); // set defaults
        mouse_write(0xF4); // enable data reporting

        // Unmask IRQ12 on the PIC so the CPU wakes from HLT on mouse events.
        // Without this, mouse data sits in the PS/2 FIFO until the next PIT tick (10ms).
        crate::arch::x86_64::interrupts::unmask_mouse_irq();

        crate::serial::write_line("drivers: ps2-mouse init OK");
        Ok(())
    }
}
// ---------------------------------------------------------------------------
