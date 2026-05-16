// ---------------------------------------------------------------------------
// Astra OS — Real-Time Clock driver (CMOS/RTC)
//
// Reads the x86_64 CMOS real-time clock via I/O ports 0x70 (address) and
// 0x71 (data).  Values are BCD-encoded by default; this module decodes them
// to binary.  The UIP (Update-In-Progress) flag is polled to ensure a
// consistent read.
// ---------------------------------------------------------------------------

const ADDR_PORT: u16 = 0x70;
const DATA_PORT: u16 = 0x71;

const REG_SECONDS:  u8 = 0x00;
const REG_MINUTES:  u8 = 0x02;
const REG_HOURS:    u8 = 0x04;
const REG_DAY:      u8 = 0x07;
const REG_MONTH:    u8 = 0x08;
const REG_YEAR:     u8 = 0x09;
const REG_STATUS_A: u8 = 0x0A;
const REG_STATUS_B: u8 = 0x0B;
const STATUS_A_UIP: u8 = 0x80;  // Update-In-Progress bit
const STATUS_B_24H: u8 = 0x02;  // 24-hour format bit
const STATUS_B_BIN: u8 = 0x04;  // binary (non-BCD) mode bit

/// Date/time snapshot from the CMOS RTC.
#[derive(Copy, Clone, Debug, Default)]
pub struct DateTime {
    pub year:   u16,
    pub month:  u8,
    pub day:    u8,
    pub hour:   u8,
    pub minute: u8,
    pub second: u8,
}

// ── Port helpers ──────────────────────────────────────────────────────────────

unsafe fn in8(port: u16) -> u8 {
    let v: u8;
    unsafe { core::arch::asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack)); }
    v
}

unsafe fn out8(port: u16, v: u8) {
    unsafe { core::arch::asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack)); }
}

/// Read one CMOS register.  Masks the NMI-disable bit (bit 7) in the address.
unsafe fn cmos_read(reg: u8) -> u8 {
    unsafe {
        out8(ADDR_PORT, reg & 0x7F);
        // Small delay: read a dummy port to allow the CMOS to settle.
        let _ = in8(0x80);
        in8(DATA_PORT)
    }
}

fn bcd_to_bin(v: u8) -> u8 {
    (v >> 4) * 10 + (v & 0x0F)
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Read the current date and time from the CMOS RTC.
///
/// Waits for the Update-In-Progress (UIP) flag to clear, then reads all
/// registers.  Reads twice and retries if values differ (caught an update mid-
/// read).  Handles both BCD (default) and binary modes, and both 12-hour and
/// 24-hour formats.
pub fn read_datetime() -> DateTime {
    // Wait for UIP to clear (typical latency: <1 µs, max 248 µs).
    for _ in 0..10_000u32 {
        let a = unsafe { cmos_read(REG_STATUS_A) };
        if a & STATUS_A_UIP == 0 { break; }
    }

    // Read Status B to discover encoding.
    let sb = unsafe { cmos_read(REG_STATUS_B) };
    let is_binary = sb & STATUS_B_BIN != 0;
    let is_24h    = sb & STATUS_B_24H != 0;

    // Read all time/date registers twice for consistency.
    let decode = |v: u8| if is_binary { v } else { bcd_to_bin(v) };

    let (mut s1, mut m1, mut h1, mut d1, mut mo1, mut y1);
    let (mut s2, mut m2, mut h2, mut d2, mut mo2, mut y2);
    loop {
        s1  = decode(unsafe { cmos_read(REG_SECONDS) });
        m1  = decode(unsafe { cmos_read(REG_MINUTES) });
        h1  = decode(unsafe { cmos_read(REG_HOURS)   });
        d1  = decode(unsafe { cmos_read(REG_DAY)     });
        mo1 = decode(unsafe { cmos_read(REG_MONTH)   });
        y1  = decode(unsafe { cmos_read(REG_YEAR)    });

        s2  = decode(unsafe { cmos_read(REG_SECONDS) });
        m2  = decode(unsafe { cmos_read(REG_MINUTES) });
        h2  = decode(unsafe { cmos_read(REG_HOURS)   });
        d2  = decode(unsafe { cmos_read(REG_DAY)     });
        mo2 = decode(unsafe { cmos_read(REG_MONTH)   });
        y2  = decode(unsafe { cmos_read(REG_YEAR)    });

        if s1==s2 && m1==m2 && h1==h2 && d1==d2 && mo1==mo2 && y1==y2 { break; }
    }

    // Convert 12h → 24h if needed.
    if !is_24h && h1 & 0x80 != 0 {
        h1 = ((h1 & 0x7F) + 12) % 24;
    }

    // QEMU reports year as 2-digit offset from century (e.g. 24 for 2024).
    let full_year: u16 = if y1 < 70 { 2000 + y1 as u16 } else { 1900 + y1 as u16 };

    DateTime {
        year:   full_year,
        month:  mo1,
        day:    d1,
        hour:   h1,
        minute: m1,
        second: s1,
    }
}

/// Convenience: returns `(hour, minute, second)` only.
pub fn read_time() -> (u8, u8, u8) {
    let dt = read_datetime();
    (dt.hour, dt.minute, dt.second)
}
