use core::fmt::{self, Write};
use spin::Mutex;

const COM1_DATA: u16 = 0x3f8;
const COM1_INTERRUPT_ENABLE: u16 = COM1_DATA + 1;
const COM1_FIFO_CONTROL: u16 = COM1_DATA + 2;
const COM1_LINE_CONTROL: u16 = COM1_DATA + 3;
const COM1_MODEM_CONTROL: u16 = COM1_DATA + 4;
const COM1_LINE_STATUS: u16 = COM1_DATA + 5;

static SERIAL: Mutex<SerialPort> = Mutex::new(SerialPort::new(COM1_DATA));

pub fn init() {
    SERIAL.lock().init();
}

pub fn write_line(message: &str) {
    LOG_RING.lock().push_str(message);
    LOG_RING.lock().push(b'\n');
    let mut serial = SERIAL.lock();
    let _ = writeln!(serial, "{message}");
}

pub fn write_str(message: &str) {
    LOG_RING.lock().push_str(message);
    let mut serial = SERIAL.lock();
    let _ = write!(serial, "{message}");
}

pub fn write_u32(value: u32) {
    write_u64(value as u64);
}

pub fn write_u64(value: u64) {
    let mut serial = SERIAL.lock();
    let _ = write!(serial, "{value}");
}

/// Write a u64 as hex without using core::fmt (avoids potential triple-fault).
pub fn write_hex64(value: u64) {
    let mut serial = SERIAL.lock();
    let hex = b"0123456789abcdef";
    for i in (0..16).rev() {
        let nibble = ((value >> (i * 4)) & 0xF) as usize;
        serial.write_byte(hex[nibble]);
    }
}

struct SerialPort {
    base: u16,
}

impl SerialPort {
    const fn new(base: u16) -> Self {
        Self { base }
    }

    fn init(&mut self) {
        // Safety: these port I/O writes configure the legacy COM1 device used for early debug output.
        unsafe {
            outb(COM1_INTERRUPT_ENABLE, 0x00);
            outb(COM1_LINE_CONTROL, 0x80);
            outb(self.base, 0x03);
            outb(COM1_INTERRUPT_ENABLE, 0x00);
            outb(COM1_LINE_CONTROL, 0x03);
            outb(COM1_FIFO_CONTROL, 0xc7);
            outb(COM1_MODEM_CONTROL, 0x0b);
        }
    }

    fn write_byte(&mut self, byte: u8) {
        while !self.transmit_empty() {
            core::hint::spin_loop();
        }

        // Safety: writing a byte to the configured COM1 data port is required for serial output.
        unsafe {
            outb(self.base, byte);
        }
    }

    fn transmit_empty(&self) -> bool {
        // Safety: reading the COM1 line status register is required to poll transmitter readiness.
        unsafe { inb(COM1_LINE_STATUS) & 0x20 != 0 }
    }
}

impl Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            match byte {
                b'\n' => {
                    self.write_byte(b'\r');
                    self.write_byte(b'\n');
                }
                _ => self.write_byte(byte),
            }
        }

        Ok(())
    }
}

unsafe fn outb(port: u16, value: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        core::arch::asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

// ── Kernel log ring buffer ────────────────────────────────────────────────────
//
// A fixed-size byte ring buffer that captures everything written through
// `write_line` / `write_str`.  The Log Viewer app reads from this buffer so
// the user can inspect boot messages and runtime output from the GUI.
//
// Capacity: 16 KiB.  Oldest bytes are silently discarded when full.

const LOG_CAP: usize = 16 * 1024;

struct LogRing {
    buf: [u8; LOG_CAP],
    write: usize, // next write position (monotonic, mod LOG_CAP)
    count: usize, // bytes stored (capped at LOG_CAP)
}

impl LogRing {
    const fn new() -> Self {
        LogRing {
            buf: [0u8; LOG_CAP],
            write: 0,
            count: 0,
        }
    }

    fn push(&mut self, b: u8) {
        self.buf[self.write % LOG_CAP] = b;
        self.write += 1;
        if self.count < LOG_CAP {
            self.count += 1;
        }
    }

    fn push_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.push(b);
        }
    }
}

static LOG_RING: spin::Mutex<LogRing> = spin::Mutex::new(LogRing::new());

/// Copy at most `buf.len()` bytes from the ring into `buf`, starting from
/// `offset` bytes from the oldest entry.  Returns the number of bytes copied.
/// `total_bytes()` gives the total captured byte count (for pagination).
pub fn log_read(offset: usize, buf: &mut [u8]) -> usize {
    let r = LOG_RING.lock();
    if r.count == 0 || offset >= r.count {
        return 0;
    }
    let available = r.count - offset;
    let n = available.min(buf.len());
    // oldest byte is at write - count (all mod LOG_CAP)
    let start = r.write.wrapping_sub(r.count).wrapping_add(offset) % LOG_CAP;
    for i in 0..n {
        buf[i] = r.buf[(start + i) % LOG_CAP];
    }
    n
}

/// Total bytes ever written to the ring (monotonic; older bytes may be gone).
pub fn log_total() -> usize {
    LOG_RING.lock().count
}
