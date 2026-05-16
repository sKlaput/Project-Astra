// ---------------------------------------------------------------------------
// Astra OS — Settings app
//
// Multi-tab layout:
//   [System]  — hardware/build info  (read-only)
//   [Display] — desktop background colour picker
//   [Input]   — keyboard/mouse info (placeholder)
//   [About]   — project info + runtime stats
//
// Tab bar on the left; content on the right.
// Arrow keys navigate within a tab; Tab switches tabs.
// ---------------------------------------------------------------------------

use crate::app::{App, AppAction};
use crate::framebuffer;
use crate::input::Key;
use core::sync::atomic::Ordering as AO;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG:          u32 = 0x0A0E14;
const SIDEBAR_BG:  u32 = 0x080C10;
const SEP:         u32 = 0x1E3A5F;
const HEADING:     u32 = 0x4FC3F7;
const LABEL:       u32 = 0x546E7A;
const VALUE:       u32 = 0xE8F4FD;
const ACCENT:      u32 = 0xB0D4B8;
const TAB_SEL_BG:  u32 = 0x1A2E44;
const TAB_SEL_TXT: u32 = 0xFFFFFF;
const TAB_TXT:     u32 = 0x7090B0;
const HINT:        u32 = 0x2A4060;
const SWATCH_SEL:  u32 = 0xFFFFFF;

// ── Font metrics ──────────────────────────────────────────────────────────────

const SC:  usize = 2;
const CW:  usize = 6 * SC;   // 12
const CH:  usize = 8 * SC;   // 16

// ── Layout ────────────────────────────────────────────────────────────────────

const SIDEBAR_W: usize = 110;
const PAD:       usize = 12;
const TAB_H:     usize = 28;

// ── Tabs ──────────────────────────────────────────────────────────────────────

const NUM_TABS: usize = 4;
const TAB_LABELS: [&str; NUM_TABS] = ["System", "Display", "Input", "About"];

// ── System info ───────────────────────────────────────────────────────────────

const NUM_SYSINFO: usize = 5;
struct KV { k: &'static str, v: &'static str }
const SYSINFO: [KV; NUM_SYSINFO] = [
    KV { k: "Version",      v: "Astra OS v0.1" },
    KV { k: "Architecture", v: "x86_64"         },
    KV { k: "Bootloader",   v: "Limine (UEFI)"  },
    KV { k: "Resolution",   v: "1280x800 32bpp" },
    KV { k: "Timer",        v: "PIT @ 100 Hz"   },
];

// ── Display: background colour presets ────────────────────────────────────────

const NUM_THEMES: usize = 8;
const THEMES: [(u32, &str); NUM_THEMES] = [
    (0x0D1117, "Deep Space"  ),
    (0x071207, "Forest Night"),
    (0x0D0714, "Nebula"      ),
    (0x14070B, "Ember"       ),
    (0x060E18, "Ocean"       ),
    (0x141210, "Warm Slate"  ),
    (0x050508, "Midnight"    ),
    (0x0F1218, "Steel"       ),
];

// ── App struct ────────────────────────────────────────────────────────────────

pub struct SettingsApp {
    tab: usize,
    row: usize,
}

impl SettingsApp {
    pub fn new() -> Self { SettingsApp { tab: 0, row: 0 } }

    fn max_rows(&self) -> usize {
        match self.tab {
            0 => NUM_SYSINFO,
            1 => NUM_THEMES,
            2 => 2,
            _ => 0,
        }
    }
}

impl App for SettingsApp {
    fn title(&self) -> &str { "Settings" }
    fn preferred_size(&self) -> (usize, usize) { (760, 520) }
    fn app_id(&self) -> &'static str { "settings" }
    fn allow_multiple_instances(&self) -> bool { false }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        framebuffer::fill_rect(cx, cy, cw, ch, BG);

        // ── Sidebar ───────────────────────────────────────────────────────
        framebuffer::fill_rect(cx, cy, SIDEBAR_W, ch, SIDEBAR_BG);
        framebuffer::fill_rect(cx + SIDEBAR_W, cy, 1, ch, SEP);

        for i in 0..NUM_TABS {
            let ty = cy + i * TAB_H + 4;
            let (bg, tc) = if i == self.tab { (TAB_SEL_BG, TAB_SEL_TXT) } else { (SIDEBAR_BG, TAB_TXT) };
            framebuffer::fill_rect(cx, ty, SIDEBAR_W, TAB_H - 2, bg);
            framebuffer::draw_text_at(cx + 10, ty + 7, TAB_LABELS[i], tc);
        }

        // Hint at bottom of sidebar
        let hint_y = cy + ch.saturating_sub(20);
        framebuffer::draw_text_at(cx + 4, hint_y, "Tab=next tab", HINT);

        // ── Content ───────────────────────────────────────────────────────
        let cx2 = cx + SIDEBAR_W + 1;
        let cw2 = cw.saturating_sub(SIDEBAR_W + 1);
        match self.tab {
            0 => render_system(cx2, cy, cw2, ch, self.row),
            1 => render_display(cx2, cy, cw2, ch, self.row),
            2 => render_input(cx2, cy, cw2, ch, self.row),
            3 => render_about(cx2, cy, cw2, ch),
            _ => {}
        }
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        match key {
            Key::Tab => {
                self.tab = (self.tab + 1) % NUM_TABS;
                self.row = 0;
                AppAction::RedrawAll
            }
            Key::ArrowUp => {
                let m = self.max_rows();
                if m > 0 { self.row = if self.row == 0 { m - 1 } else { self.row - 1 }; }
                AppAction::RedrawAll
            }
            Key::ArrowDown => {
                let m = self.max_rows();
                if m > 0 { self.row = (self.row + 1) % m; }
                AppAction::RedrawAll
            }
            Key::Char(b'\r') | Key::Char(b' ') => {
                if self.tab == 1 && self.row < NUM_THEMES {
                    crate::desktop::DESKTOP_BG_COLOR.store(THEMES[self.row].0, AO::Relaxed);
                }
                AppAction::RedrawAll
            }
            Key::Escape => AppAction::Nothing,
            _ => AppAction::Nothing,
        }
    }
}

// ── Tab renderers ─────────────────────────────────────────────────────────────

fn render_system(cx: usize, cy: usize, cw: usize, _ch: usize, sel: usize) {
    let x = cx + PAD;
    let mut y = cy + PAD;
    framebuffer::draw_text_scaled(x, y, "System Information", HEADING, SC);
    y += CH + 4;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    let vx = x + 16 * CW;
    for (i, item) in SYSINFO.iter().enumerate() {
        if i == sel {
            framebuffer::fill_rect(cx + 2, y - 2, cw.saturating_sub(4), CH + 4, TAB_SEL_BG);
            framebuffer::draw_text_scaled(x, y, item.k, TAB_SEL_TXT, SC);
            framebuffer::draw_text_scaled(vx, y, item.v, TAB_SEL_TXT, SC);
        } else {
            framebuffer::draw_text_scaled(x, y, item.k, LABEL, SC);
            framebuffer::draw_text_scaled(vx, y, item.v, VALUE, SC);
        }
        y += CH + 4;
    }
    y += 8;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    framebuffer::draw_text_scaled(x, y, "Runtime", HEADING, SC);
    y += CH + 4;
    let ms   = crate::arch::x86_64::interrupts::uptime_ms();
    let secs = ms / 1000;
    let mins = secs / 60;
    let hrs  = mins / 60;
    let mut buf = [0u8; 32];
    let len = fmt_uptime(&mut buf, hrs, mins % 60, secs % 60);
    let s = unsafe { core::str::from_utf8_unchecked(&buf[..len]) };
    framebuffer::draw_text_scaled(x, y, "Uptime", LABEL, SC);
    framebuffer::draw_text_scaled(vx, y, s, VALUE, SC);
    y += CH + 4;
    let heap = crate::memory::heap::get_telemetry();
    let used_kb  = (heap.used_bytes / 1024) as u64;
    let total_kb = ((heap.mapped_pages * 4096) / 1024) as u64;
    let mut buf2 = [0u8; 32];
    let len2 = fmt_kb_of_kb(&mut buf2, used_kb, total_kb);
    let s2 = unsafe { core::str::from_utf8_unchecked(&buf2[..len2]) };
    framebuffer::draw_text_scaled(x, y, "Heap", LABEL, SC);
    framebuffer::draw_text_scaled(vx, y, s2, VALUE, SC);
    let _ = y;
}

fn render_display(cx: usize, cy: usize, cw: usize, _ch: usize, sel: usize) {
    let x = cx + PAD;
    let mut y = cy + PAD;
    framebuffer::draw_text_scaled(x, y, "Desktop Background", HEADING, SC);
    y += CH + 4;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    framebuffer::draw_text_at(x, y, "Arrow keys to browse presets, Enter/Space to apply.", LABEL);
    y += 14;

    // Current colour preview
    let cur_bg = crate::desktop::DESKTOP_BG_COLOR.load(AO::Relaxed);
    framebuffer::draw_text_at(x, y, "Current:", LABEL);
    framebuffer::fill_rect(x + 52, y - 1, 32, 11, 0x3A5878);
    framebuffer::fill_rect(x + 53, y,     30, 10, cur_bg);
    y += 20;

    // 2×4 swatch grid
    const SW: usize = 40;
    const SG: usize = 12;
    const COLS: usize = 4;

    for (i, (col, name)) in THEMES.iter().enumerate() {
        let is_sel    = i == sel;
        let is_active = *col == cur_bg;
        let sx = x + (i % COLS) * (SW + SG);
        let sy = y + (i / COLS) * (SW + 22);
        // Border
        let border_col = if is_sel { SWATCH_SEL } else if is_active { ACCENT } else { 0x2A3A4A };
        framebuffer::fill_rect(sx.saturating_sub(2), sy.saturating_sub(2), SW + 4, SW + 4, border_col);
        framebuffer::fill_rect(sx, sy, SW, SW, *col);
        // Tiny inner border for very dark swatches
        framebuffer::fill_rect(sx, sy, SW, 1, 0x1A2A3A);
        framebuffer::fill_rect(sx, sy, 1, SW, 0x1A2A3A);
        let tc = if is_sel { VALUE } else { LABEL };
        framebuffer::draw_text_at(sx, sy + SW + 3, name, tc);
    }
}

fn render_input(cx: usize, cy: usize, cw: usize, _ch: usize, sel: usize) {
    let x = cx + PAD;
    let mut y = cy + PAD;
    framebuffer::draw_text_scaled(x, y, "Input Devices", HEADING, SC);
    y += CH + 4;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    const ITEMS: [(&str, &str); 2] = [
        ("Keyboard", "PS/2 (IRQ1)"),
        ("Mouse",    "PS/2 Aux (IRQ12)"),
    ];
    let vx = x + 14 * CW;
    for (i, (k, v)) in ITEMS.iter().enumerate() {
        if i == sel {
            framebuffer::fill_rect(cx + 2, y - 2, cw.saturating_sub(4), CH + 4, TAB_SEL_BG);
            framebuffer::draw_text_scaled(x, y, k, TAB_SEL_TXT, SC);
            framebuffer::draw_text_scaled(vx, y, v, TAB_SEL_TXT, SC);
        } else {
            framebuffer::draw_text_scaled(x, y, k, LABEL, SC);
            framebuffer::draw_text_scaled(vx, y, v, VALUE, SC);
        }
        y += CH + 4;
    }
    y += 10;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    framebuffer::draw_text_at(x, y, "Mouse sensitivity and key repeat coming soon.", HINT);
    let _ = y;
}

fn render_about(cx: usize, cy: usize, cw: usize, _ch: usize) {
    let x = cx + PAD;
    let mut y = cy + PAD;
    framebuffer::draw_text_scaled(x, y, "About Astra OS", HEADING, SC);
    y += CH + 4;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    const LINES: &[(&str, u32)] = &[
        ("Astra OS is a from-scratch Rust-first desktop", 0xE8F4FD),
        ("operating system prototype focused on control,", 0xE8F4FD),
        ("privacy, simplicity, and eventually gaming-", 0xE8F4FD),
        ("capable personal computing.", 0xE8F4FD),
        ("", 0),
        ("Written entirely in Rust (no_std, bare-metal).", 0xB0D4B8),
        ("x86_64 / Limine UEFI boot.", 0xB0D4B8),
        ("Virtio-blk + FAT32 persistent storage.", 0xB0D4B8),
        ("Virtio-net Ethernet driver.", 0xB0D4B8),
        ("Ring-3 ELF user processes via SYSCALL.", 0xB0D4B8),
    ];
    for (line, col) in LINES {
        if !line.is_empty() {
            framebuffer::draw_text_at(x, y, line, *col);
        }
        y += 13;
    }
    y += 4;
    framebuffer::fill_rect(cx, y, cw, 1, SEP);
    y += 10;
    framebuffer::draw_text_scaled(x, y, "Runtime", HEADING, SC);
    y += CH + 4;
    let ms   = crate::arch::x86_64::interrupts::uptime_ms();
    let secs = ms / 1000;
    let mins = secs / 60;
    let hrs  = mins / 60;
    let mut buf = [0u8; 32];
    let len = fmt_uptime(&mut buf, hrs, mins % 60, secs % 60);
    let s = unsafe { core::str::from_utf8_unchecked(&buf[..len]) };
    framebuffer::draw_text_at(x, y, "Uptime:", LABEL);
    framebuffer::draw_text_at(x + 7 * 6 + 4, y, s, VALUE);
    y += 14;
    let heap = crate::memory::heap::get_telemetry();
    let used_kb  = (heap.used_bytes / 1024) as u64;
    let total_kb = ((heap.mapped_pages * 4096) / 1024) as u64;
    let mut buf2 = [0u8; 32];
    let len2 = fmt_kb_of_kb(&mut buf2, used_kb, total_kb);
    let s2 = unsafe { core::str::from_utf8_unchecked(&buf2[..len2]) };
    framebuffer::draw_text_at(x, y, "Heap:", LABEL);
    framebuffer::draw_text_at(x + 7 * 6 + 4, y, s2, VALUE);
    let _ = y;
}

// ── Number formatting ─────────────────────────────────────────────────────────

fn fmt_u64(buf: &mut [u8], mut n: u64) -> usize {
    if buf.is_empty() { return 0; }
    if n == 0 { buf[0] = b'0'; return 1; }
    let mut tmp = [0u8; 20];
    let mut pos = tmp.len();
    while n > 0 { pos -= 1; tmp[pos] = b'0' + (n % 10) as u8; n /= 10; }
    let len = (tmp.len() - pos).min(buf.len());
    buf[..len].copy_from_slice(&tmp[pos..pos + len]);
    len
}

fn fmt_padded2(buf: &mut [u8], n: u64) -> usize {
    if buf.len() < 2 { return 0; }
    buf[0] = b'0' + ((n / 10) % 10) as u8;
    buf[1] = b'0' + (n % 10) as u8;
    2
}

fn fmt_uptime(buf: &mut [u8], hrs: u64, mins: u64, secs: u64) -> usize {
    let mut pos = 0;
    pos += fmt_u64(&mut buf[pos..], hrs);
    if pos < buf.len() { buf[pos] = b'h'; pos += 1; }
    if pos < buf.len() { buf[pos] = b' '; pos += 1; }
    pos += fmt_padded2(&mut buf[pos..], mins);
    if pos < buf.len() { buf[pos] = b'm'; pos += 1; }
    if pos < buf.len() { buf[pos] = b' '; pos += 1; }
    pos += fmt_padded2(&mut buf[pos..], secs);
    if pos < buf.len() { buf[pos] = b's'; pos += 1; }
    pos
}

fn fmt_kb_of_kb(buf: &mut [u8], used: u64, total: u64) -> usize {
    let mut pos = 0;
    pos += fmt_u64(&mut buf[pos..], used);
    if pos + 4 <= buf.len() { buf[pos] = b' '; buf[pos+1] = b'K'; buf[pos+2] = b'B'; buf[pos+3] = b' '; pos += 4; }
    if pos + 2 <= buf.len() { buf[pos] = b'/'; buf[pos+1] = b' '; pos += 2; }
    pos += fmt_u64(&mut buf[pos..], total);
    if pos + 3 <= buf.len() { buf[pos] = b' '; buf[pos+1] = b'K'; buf[pos+2] = b'B'; pos += 3; }
    pos
}