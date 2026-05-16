// ---------------------------------------------------------------------------
// Astra OS — System Monitor app
//
// Implements the App trait.  Samples live kernel stats and renders them into
// the client rect.  Declares a 250 ms refresh interval so the compositor
// automatically re-damages the client area without special-casing.
// ---------------------------------------------------------------------------

use crate::app::{App, AppAction};
use crate::framebuffer;
use crate::input::Key;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG:           u32 = 0x0A0E14;
const HEADING_COL:  u32 = 0x4FC3F7;
const LABEL_COL:    u32 = 0x546E7A;
const VALUE_COL:    u32 = 0xE8F4FD;
const ACCENT_COL:   u32 = 0xB0D4B8;
const SEPARATOR:    u32 = 0x1E3A5F;
const BAR_BG:       u32 = 0x1A2332;
const BAR_FILL:     u32 = 0x4FC3F7;

// ── Font metrics (scale 2) ────────────────────────────────────────────────────

const SCALE:  usize = 2;
const CHAR_W: usize = 6 * SCALE;   // 12
const CHAR_H: usize = 8 * SCALE;   // 16
const PAD_X:  usize = 14;
const PAD_Y:  usize = 10;

// ── App struct ────────────────────────────────────────────────────────────────

pub struct SysMonitorApp;

impl SysMonitorApp {
    pub fn new() -> Self { SysMonitorApp }
}

impl App for SysMonitorApp {
    fn title(&self) -> &str { "System Monitor" }
    fn preferred_size(&self) -> (usize, usize) { (900, 520) }
    fn app_id(&self) -> &'static str { "sysmonitor" }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        render_stats(cx, cy, cw, ch);
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        match key {
            Key::Escape => AppAction::Nothing,
            _ => AppAction::RedrawAll,
        }
    }

    fn refresh_interval_ms(&self) -> Option<u64> { Some(250) }
}

// ── Stats renderer ────────────────────────────────────────────────────────────

fn render_stats(cx: usize, cy: usize, cw: usize, ch: usize) {
    let inner_x = cx + PAD_X;
    let mut y = cy + PAD_Y;
    let section_gap = CHAR_H + 6;

    // ── Title ─────────────────────────────────────────────────────────────
    framebuffer::draw_text_scaled(inner_x, y, "System Monitor", HEADING_COL, SCALE);
    y += CHAR_H + 4;
    framebuffer::fill_rect(cx, y, cw, 1, SEPARATOR);
    y += 8;

    // ── Uptime ────────────────────────────────────────────────────────────
    let ms = crate::arch::x86_64::interrupts::uptime_ms();
    let secs = ms / 1000;
    let mins = secs / 60;
    let hrs = mins / 60;

    framebuffer::draw_text_scaled(inner_x, y, "Uptime", LABEL_COL, SCALE);
    let mut buf = [0u8; 48];
    let len = fmt_uptime(&mut buf, hrs, mins % 60, secs % 60, ms % 1000);
    let s = unsafe { core::str::from_utf8_unchecked(&buf[..len]) };
    framebuffer::draw_text_scaled(inner_x + 10 * CHAR_W, y, s, VALUE_COL, SCALE);
    y += section_gap;

    // ── Scheduler ─────────────────────────────────────────────────────────
    framebuffer::draw_text_scaled(inner_x, y, "Scheduler", HEADING_COL, SCALE);
    y += CHAR_H + 2;

    let sched_ticks = crate::scheduler::ticks();
    draw_stat_line(inner_x, y, "Ticks", sched_ticks, cw);
    y += CHAR_H;

    let stats = crate::scheduler::debug_stats_snapshot();
    draw_stat_line(inner_x, y, "Dispatches", stats.dispatches, cw);
    y += CHAR_H;
    draw_stat_line(inner_x, y, "Preemptions", stats.preempts, cw);
    y += CHAR_H;
    draw_stat_line(inner_x, y, "Sleeps", stats.sleeps, cw);
    y += CHAR_H;
    draw_stat_line(inner_x, y, "Wakes", stats.wakes, cw);
    y += CHAR_H;
    draw_stat_line(inner_x, y, "Exits", stats.exits, cw);
    y += CHAR_H;

    let runnable = crate::scheduler::runnable_count();
    draw_stat_line(inner_x, y, "Runnable", runnable as u64, cw);
    y += section_gap;

    // ── Heap ──────────────────────────────────────────────────────────────
    framebuffer::draw_text_scaled(inner_x, y, "Heap Memory", HEADING_COL, SCALE);
    y += CHAR_H + 2;

    let heap = crate::memory::heap::get_telemetry();
    draw_stat_line(inner_x, y, "Pages mapped", heap.mapped_pages as u64, cw);
    y += CHAR_H;

    let used_kb = heap.used_bytes / 1024;
    let total_kb = (heap.mapped_pages * 4096) / 1024;
    draw_stat_kb(inner_x, y, "Used", used_kb as u64, cw);
    y += CHAR_H;
    draw_stat_kb(inner_x, y, "Mapped", total_kb as u64, cw);
    y += CHAR_H + 4;

    // Usage bar
    if total_kb > 0 && y + 12 < cy + ch {
        let bar_w = cw.saturating_sub(PAD_X * 2);
        let fill_frac = (heap.used_bytes * 100) / (heap.mapped_pages * 4096).max(1);
        let fill_w = (bar_w * fill_frac) / 100;

        framebuffer::fill_rect(inner_x, y, bar_w, 10, BAR_BG);
        if fill_w > 0 {
            framebuffer::fill_rect(inner_x, y, fill_w, 10, BAR_FILL);
        }
        y += 14;

        // Percentage text
        let mut pct_buf = [0u8; 8];
        let pct_len = fmt_pct(&mut pct_buf, fill_frac as u64);
        let pct_s = unsafe { core::str::from_utf8_unchecked(&pct_buf[..pct_len]) };
        framebuffer::draw_text_scaled(inner_x, y, pct_s, ACCENT_COL, SCALE);
        y += section_gap;
    }

    // ── Interrupts ────────────────────────────────────────────────────────
    if y + CHAR_H * 3 < cy + ch {
        framebuffer::draw_text_scaled(inner_x, y, "Interrupts", HEADING_COL, SCALE);
        y += CHAR_H + 2;

        let hz = crate::arch::x86_64::interrupts::timer_hz();
        draw_stat_line(inner_x, y, "PIT Hz", hz as u64, cw);
        y += CHAR_H;

        let timer_ticks = crate::arch::x86_64::interrupts::timer_ticks();
        draw_stat_line(inner_x, y, "IRQ0 ticks", timer_ticks, cw);
    }

    // ── Right pane: Windows + Processes ──────────────────────────────────
    let rpane_x = cx + cw / 2;
    let rpane_w = cw / 2;
    framebuffer::fill_rect(rpane_x.saturating_sub(1), cy, 1, ch, SEPARATOR);

    let rx = rpane_x + PAD_X;
    let mut ry = cy + PAD_Y;

    // Open Windows
    framebuffer::draw_text_scaled(rx, ry, "Open Windows", HEADING_COL, SCALE);
    ry += CHAR_H + 4;
    framebuffer::fill_rect(rpane_x, ry, rpane_w, 1, SEPARATOR);
    ry += 8;

    {
        let tbl = crate::desktop::WIN_TABLE.lock();
        if tbl.count == 0 {
            framebuffer::draw_text_scaled(rx, ry, "(none)", LABEL_COL, SCALE);
            ry += CHAR_H;
        } else {
            for i in 0..tbl.count {
                if ry + CHAR_H > cy + ch / 2 { break; }
                let snap = &tbl.snaps[i];
                let title = unsafe { core::str::from_utf8_unchecked(&snap.title[..snap.title_len]) };
                let (tc, badge_col) = if snap.minimized { (LABEL_COL, 0x3A4A5A) } else { (VALUE_COL, ACCENT_COL) };
                // Colour badge
                framebuffer::fill_rect(rx, ry + 4, 6, 6, badge_col);
                framebuffer::draw_text_scaled(rx + 10, ry, title, tc, 1);
                ry += 12;
            }
        }
    }

    ry += 8;
    framebuffer::fill_rect(rpane_x, ry, rpane_w, 1, SEPARATOR);
    ry += 8;

    // User Processes
    framebuffer::draw_text_scaled(rx, ry, "User Processes", HEADING_COL, SCALE);
    ry += CHAR_H + 4;

    {
        let (entries, count) = crate::process::list_all();
        if count == 0 {
            framebuffer::draw_text_scaled(rx, ry, "(none)", LABEL_COL, SCALE);
        } else {
            // Header
            framebuffer::draw_text_at(rx, ry, "PID  STATE    NAME", LABEL_COL);
            ry += 12;
            for i in 0..count {
                if ry + 12 > cy + ch { break; }
                let e = &entries[i];
                let state_str: &str = match e.state {
                    crate::process::ProcessState::Running => "running",
                    crate::process::ProcessState::Exited  => "exited ",
                    crate::process::ProcessState::Empty   => "empty  ",
                };
                let col = if e.state == crate::process::ProcessState::Running { 0x66FF66 } else { 0x888888 };
                // Build "PID  STATE  name" line
                let mut buf = [0u8; 40];
                let mut p = 0usize;
                p += fmt_u64_sm(&mut buf[p..], e.pid);
                while p < 5 { buf[p] = b' '; p += 1; }
                let sb = state_str.as_bytes();
                let sl = sb.len().min(40 - p);
                buf[p..p + sl].copy_from_slice(&sb[..sl]); p += sl;
                while p < 14 { buf[p] = b' '; p += 1; }
                let nl = e.name_len.min(40 - p);
                buf[p..p + nl].copy_from_slice(&e.name[..nl]); p += nl;
                let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
                framebuffer::draw_text_at(rx, ry, s, col);
                ry += 11;
            }
        }
    }
}

// ── Drawing helpers ───────────────────────────────────────────────────────────

fn draw_stat_line(x: usize, y: usize, label: &str, value: u64, _cw: usize) {
    framebuffer::draw_text_scaled(x, y, label, LABEL_COL, SCALE);
    let val_x = x + 16 * CHAR_W;
    let mut buf = [0u8; 20];
    let len = fmt_u64(&mut buf, value);
    let s = unsafe { core::str::from_utf8_unchecked(&buf[..len]) };
    framebuffer::draw_text_scaled(val_x, y, s, VALUE_COL, SCALE);
}

fn draw_stat_kb(x: usize, y: usize, label: &str, kb: u64, _cw: usize) {
    framebuffer::draw_text_scaled(x, y, label, LABEL_COL, SCALE);
    let val_x = x + 16 * CHAR_W;
    let mut buf = [0u8; 24];
    let len = fmt_u64(&mut buf[..20], kb);
    buf[len] = b' ';
    buf[len + 1] = b'K';
    buf[len + 2] = b'B';
    let s = unsafe { core::str::from_utf8_unchecked(&buf[..len + 3]) };
    framebuffer::draw_text_scaled(val_x, y, s, VALUE_COL, SCALE);
}

// ── Number formatting (no alloc) ──────────────────────────────────────────────

fn fmt_u64(buf: &mut [u8], mut n: u64) -> usize {
    if buf.is_empty() { return 0; }
    if n == 0 { buf[0] = b'0'; return 1; }
    let mut tmp = [0u8; 20];
    let mut pos = tmp.len();
    while n > 0 {
        pos -= 1;
        tmp[pos] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let len = (tmp.len() - pos).min(buf.len());
    buf[..len].copy_from_slice(&tmp[pos..pos + len]);
    len
}

fn fmt_uptime(buf: &mut [u8], hrs: u64, mins: u64, secs: u64, ms: u64) -> usize {
    let mut pos = 0;
    pos += fmt_u64(&mut buf[pos..], hrs);
    buf[pos] = b'h'; pos += 1;
    buf[pos] = b' '; pos += 1;
    pos += fmt_padded2(&mut buf[pos..], mins);
    buf[pos] = b'm'; pos += 1;
    buf[pos] = b' '; pos += 1;
    pos += fmt_padded2(&mut buf[pos..], secs);
    buf[pos] = b'.'; pos += 1;
    pos += fmt_padded3(&mut buf[pos..], ms);
    buf[pos] = b's'; pos += 1;
    pos
}

fn fmt_padded2(buf: &mut [u8], n: u64) -> usize {
    if buf.len() < 2 { return 0; }
    buf[0] = b'0' + ((n / 10) % 10) as u8;
    buf[1] = b'0' + (n % 10) as u8;
    2
}

fn fmt_padded3(buf: &mut [u8], n: u64) -> usize {
    if buf.len() < 3 { return 0; }
    buf[0] = b'0' + ((n / 100) % 10) as u8;
    buf[1] = b'0' + ((n / 10) % 10) as u8;
    buf[2] = b'0' + (n % 10) as u8;
    3
}

fn fmt_pct(buf: &mut [u8], pct: u64) -> usize {
    let mut pos = fmt_u64(buf, pct);
    if pos + 1 < buf.len() {
        buf[pos] = b'%';
        pos += 1;
    }
    pos
}

fn fmt_u64_sm(buf: &mut [u8], mut n: u64) -> usize {
    if buf.is_empty() { return 0; }
    if n == 0 { buf[0] = b'0'; return 1; }
    let mut tmp = [0u8; 20];
    let mut pos = tmp.len();
    while n > 0 { pos -= 1; tmp[pos] = b'0' + (n % 10) as u8; n /= 10; }
    let len = (tmp.len() - pos).min(buf.len());
    buf[..len].copy_from_slice(&tmp[pos..pos + len]);
    len
}
