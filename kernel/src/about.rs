// ---------------------------------------------------------------------------
// Astra OS — About Astra app
//
// A visually polished splash-style information panel showing the OS identity,
// build metadata, and credits.  Read-only display; no editing.
// ---------------------------------------------------------------------------

use crate::app::{App, AppAction};
use crate::arch::x86_64::interrupts::uptime_ms;
use crate::framebuffer;
use crate::input::Key;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG: u32 = 0x060A10;
const LOGO_DARK: u32 = 0x0A1420;
const LOGO_ACCENT: u32 = 0x1E5090;
const LOGO_BRIGHT: u32 = 0x4A90D8;
const LOGO_STAR: u32 = 0x80C0FF;
const TITLE_COL: u32 = 0xD8EEFF;
const VER_COL: u32 = 0x4A90D8;
const TAGLINE_COL: u32 = 0x6090A8;
const SECT_COL: u32 = 0x2E5888;
const LABEL_COL: u32 = 0x4A7090;
const VALUE_COL: u32 = 0xC8E0F0;
const BORDER_COL: u32 = 0x1E3A5F;
const SEP_COL: u32 = 0x162840;
const LINK_COL: u32 = 0x3A80C0;
const FOOTER_COL: u32 = 0x243848;

// ── Layout ────────────────────────────────────────────────────────────────────

const PAD_X: usize = 24;
const PAD_Y: usize = 20;
const ROW_H: usize = 18;

// ── Info rows ─────────────────────────────────────────────────────────────────

struct Row {
    label: &'static str,
    value: &'static str,
}

const INFO_ROWS: &[Row] = &[
    Row {
        label: "Version",
        value: "Astra OS v0.3.0-dev",
    },
    Row {
        label: "Architecture",
        value: "x86_64 bare-metal",
    },
    Row {
        label: "Language",
        value: "Rust (nightly, no_std)",
    },
    Row {
        label: "Bootloader",
        value: "Limine  (UEFI)",
    },
    Row {
        label: "Storage",
        value: "FAT32 via virtio-blk",
    },
    Row {
        label: "Network",
        value: "virtio-net (PCI legacy)",
    },
    Row {
        label: "Display",
        value: "Linear framebuffer 32-bpp",
    },
    Row {
        label: "Timer",
        value: "PIT @ 100 Hz",
    },
    Row {
        label: "Input",
        value: "PS/2 keyboard + serial mouse",
    },
    Row {
        label: "Heap",
        value: "Custom no_std allocator",
    },
    Row {
        label: "License",
        value: "Proprietary — all rights reserved",
    },
];

// ── AboutApp ──────────────────────────────────────────────────────────────────

pub struct AboutApp {
    scroll: usize,
}

impl AboutApp {
    pub fn new() -> Self {
        AboutApp { scroll: 0 }
    }
}

// ── Rendering helpers ─────────────────────────────────────────────────────────

fn draw_logo(cx: usize, cy: usize) {
    // A stylised "A" star-glyph logo: two angled arms + crossbar + central star.
    // All coordinates relative to (cx, cy).

    // Background panel
    framebuffer::fill_rect(cx, cy, 80, 60, LOGO_DARK);

    // Left arm
    for i in 0..24usize {
        framebuffer::fill_rect(cx + 4 + i / 2, cy + 36 - i, 4, 4, LOGO_ACCENT);
    }
    // Right arm
    for i in 0..24usize {
        framebuffer::fill_rect(cx + 50 - i / 2, cy + 36 - i, 4, 4, LOGO_ACCENT);
    }
    // Crossbar
    framebuffer::fill_rect(cx + 18, cy + 22, 28, 5, LOGO_BRIGHT);
    // Central star (bright point at apex)
    framebuffer::fill_rect(cx + 35, cy + 6, 8, 8, LOGO_BRIGHT);
    framebuffer::fill_rect(cx + 37, cy + 4, 4, 12, LOGO_STAR); // vertical ray
    framebuffer::fill_rect(cx + 33, cy + 8, 12, 4, LOGO_STAR); // horizontal ray
}

fn draw_separator(cx: usize, y: usize, cw: usize) {
    framebuffer::fill_rect(cx + PAD_X, y, cw.saturating_sub(PAD_X * 2), 1, SEP_COL);
}

// ── App trait ─────────────────────────────────────────────────────────────────

impl App for AboutApp {
    fn title(&self) -> &str {
        "About Astra OS"
    }
    fn app_id(&self) -> &'static str {
        "about"
    }
    fn preferred_size(&self) -> (usize, usize) {
        (560, 480)
    }
    fn allow_multiple_instances(&self) -> bool {
        false
    }
    fn refresh_interval_ms(&self) -> Option<u64> {
        Some(1000)
    } // update uptime

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        framebuffer::fill_rect(cx, cy, cw, ch, BG);
        // Top border
        framebuffer::fill_rect(cx, cy, cw, 2, BORDER_COL);

        let mut y = cy + PAD_Y;

        // ── Logo + title block ────────────────────────────────────────────
        draw_logo(cx + PAD_X, y);

        let title_x = cx + PAD_X + 90;
        framebuffer::draw_text_scaled(title_x, y + 4, "ASTRA OS", TITLE_COL, 2);
        framebuffer::draw_text_at(title_x, y + 24, "v0.3.0-dev", VER_COL);
        framebuffer::draw_text_at(
            title_x,
            y + 38,
            "A from-scratch Rust desktop OS",
            TAGLINE_COL,
        );
        framebuffer::draw_text_at(
            title_x,
            y + 50,
            "focused on control and simplicity.",
            TAGLINE_COL,
        );

        y += 70;
        draw_separator(cx, y, cw);
        y += 10;

        // ── System info table ─────────────────────────────────────────────
        framebuffer::draw_text_at(cx + PAD_X, y, "System Information", SECT_COL);
        y += ROW_H + 2;

        let label_w = 100usize;
        let value_x = cx + PAD_X + label_w + 8;

        for row in INFO_ROWS {
            if y + ROW_H > cy + ch - 40 {
                break;
            }
            framebuffer::draw_text_at(cx + PAD_X, y, row.label, LABEL_COL);
            framebuffer::draw_text_at(value_x, y, row.value, VALUE_COL);
            y += ROW_H;
        }

        // ── Live uptime ───────────────────────────────────────────────────
        {
            let ms = uptime_ms();
            let s = ms / 1000;
            let m = s / 60;
            let h = m / 60;
            let mut buf = [0u8; 32];
            let len = fmt_uptime(&mut buf, h, m % 60, s % 60);
            let up = core::str::from_utf8(&buf[..len]).unwrap_or("");
            framebuffer::draw_text_at(cx + PAD_X, y, "Uptime", LABEL_COL);
            framebuffer::draw_text_at(value_x, y, up, VALUE_COL);
            y += ROW_H;
        }

        y += 6;
        draw_separator(cx, y, cw);
        y += 10;

        // ── Build / tech tagline ──────────────────────────────────────────
        framebuffer::draw_text_at(
            cx + PAD_X,
            y,
            "Built with Rust nightly  *  no libc  *  no_std  *  bare-metal",
            FOOTER_COL,
        );
        y += ROW_H;
        framebuffer::draw_text_at(
            cx + PAD_X,
            y,
            "github.com/astra-os  (coming soon)",
            LINK_COL,
        );
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        match key {
            Key::ArrowUp => {
                if self.scroll > 0 {
                    self.scroll -= 1;
                    AppAction::RedrawAll
                } else {
                    AppAction::Nothing
                }
            }
            Key::ArrowDown => {
                self.scroll += 1;
                AppAction::RedrawAll
            }
            _ => AppAction::Nothing,
        }
    }
}

// ── Formatting ────────────────────────────────────────────────────────────────

fn fmt_uptime(buf: &mut [u8; 32], h: u64, m: u64, s: u64) -> usize {
    let mut i = 0usize;
    fn pu(buf: &mut [u8; 32], i: &mut usize, n: u64) {
        if n >= 10 {
            buf[*i] = b'0' + (n / 10) as u8;
            *i += 1;
        }
        buf[*i] = b'0' + (n % 10) as u8;
        *i += 1;
    }
    pu(buf, &mut i, h);
    buf[i] = b'h';
    i += 1;
    buf[i] = b' ';
    i += 1;
    if m < 10 {
        buf[i] = b'0';
        i += 1;
    }
    pu(buf, &mut i, m);
    buf[i] = b'm';
    i += 1;
    buf[i] = b' ';
    i += 1;
    if s < 10 {
        buf[i] = b'0';
        i += 1;
    }
    pu(buf, &mut i, s);
    buf[i] = b's';
    i += 1;
    i
}
