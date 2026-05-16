// ---------------------------------------------------------------------------
// Astra OS — Log Viewer app
//
// Displays the kernel serial log ring buffer in real time.  Entries are
// captured from every `serial::write_line` / `serial::write_str` call across
// the kernel (boot messages, driver output, scheduler events, etc.)
//
// Controls:
//   Page Up / Page Down  (Tab / Shift) — scroll one page
//   Arrow Up / Down                    — scroll one line
//   Home / g                           — jump to top
//   End  / G                           — jump to bottom (newest)
//   F5 / R                             — force re-read ring buffer
//   Escape                             — no-op (keep open)
// ---------------------------------------------------------------------------

use crate::app::{App, AppAction};
use crate::framebuffer;
use crate::input::Key;
use crate::serial;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG:           u32 = 0x050A07;
const HEADER_BG:    u32 = 0x091408;
const HEADER_COL:   u32 = 0x80C888;
const BORDER_COL:   u32 = 0x183020;
const LINE_COL:     u32 = 0xA0C8A8;
const DIM_COL:      u32 = 0x2A5030;
const NUM_COL:      u32 = 0x2A6038;
const STATUS_BG:    u32 = 0x091408;
const STATUS_COL:   u32 = 0x3A6040;
const STATUS_VAL:   u32 = 0x60A870;
const SCROLL_BG:    u32 = 0x0A1810;
const SCROLL_FG:    u32 = 0x1E4828;
const WARN_COL:     u32 = 0xE3B341;
const ERR_COL:      u32 = 0xC05050;

// ── Layout ────────────────────────────────────────────────────────────────────

const HEADER_H:  usize = 22;
const STATUS_H:  usize = 16;
const LNUM_W:    usize = 36;
const PAD_X:     usize = 6;
const ROW_H:     usize = 12;
const CHAR_W:    usize = 6;
const SCROLL_W:  usize = 6;

// ── Buffer ────────────────────────────────────────────────────────────────────

/// Maximum number of log lines displayed.
const MAX_LINES: usize = 512;
/// Maximum line length kept in the view buffer.
const MAX_LINE_LEN: usize = 200;
/// Total byte storage for all lines (lines * avg_len).
const VIEW_BUF: usize = MAX_LINES * 80;

// ── LogViewerApp ──────────────────────────────────────────────────────────────

pub struct LogViewerApp {
    /// Flat byte store; lines are delimited by `\n`.
    text:       [u8; VIEW_BUF],
    text_len:   usize,
    /// Byte offset of each line start in `text`.
    lines:      [usize; MAX_LINES],
    line_count: usize,
    scroll:     usize,
    /// Ring buffer byte count captured at last refresh (detect new data).
    last_total: usize,
}

impl LogViewerApp {
    pub fn new() -> Self {
        let mut app = LogViewerApp {
            text:       [0u8; VIEW_BUF],
            text_len:   0,
            lines:      [0usize; MAX_LINES],
            line_count: 0,
            scroll:     0,
            last_total: 0,
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        // Read entire ring buffer.
        let n = serial::log_read(0, &mut self.text);
        self.text_len = n;
        self.last_total = serial::log_total();

        // Index lines.
        self.line_count = 0;
        if n == 0 { self.lines[0] = 0; self.line_count = 1; return; }
        self.lines[0] = 0;
        self.line_count = 1;
        for i in 0..n {
            if self.text[i] == b'\n' && self.line_count < MAX_LINES {
                self.lines[self.line_count] = i + 1;
                self.line_count += 1;
            }
        }
    }

    fn scroll_to_bottom(&mut self, visible: usize) {
        if self.line_count > visible {
            self.scroll = self.line_count - visible;
        }
    }

    fn visible_rows(ch: usize) -> usize {
        ch.saturating_sub(HEADER_H + STATUS_H) / ROW_H
    }

    fn line_col(line_bytes: &[u8]) -> u32 {
        // Colour-code lines by prefix keywords.
        if line_bytes.starts_with(b"error") || line_bytes.starts_with(b"PANIC") 
            || line_bytes.starts_with(b"fault") { return ERR_COL; }
        if line_bytes.starts_with(b"warn") || line_bytes.starts_with(b"WARN") { return WARN_COL; }
        LINE_COL
    }
}

// ── App trait ─────────────────────────────────────────────────────────────────

impl App for LogViewerApp {
    fn title(&self) -> &str { "Log Viewer" }
    fn app_id(&self) -> &'static str { "logviewer" }
    fn preferred_size(&self) -> (usize, usize) { (760, 500) }
    fn allow_multiple_instances(&self) -> bool { false }
    fn refresh_interval_ms(&self) -> Option<u64> { Some(500) }  // auto-refresh every 0.5s

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        let visible = Self::visible_rows(ch);

        // Header
        framebuffer::fill_rect(cx, cy, cw, HEADER_H, HEADER_BG);
        framebuffer::fill_rect(cx, cy + HEADER_H - 1, cw, 1, BORDER_COL);
        framebuffer::draw_text_at(cx + PAD_X, cy + (HEADER_H - 8) / 2, "Kernel Log", HEADER_COL);
        let hint = "R=refresh  g/G=top/bot  arrows=scroll";
        let hx = cx + cw.saturating_sub(hint.len() * 6 + 8);
        framebuffer::draw_text_at(hx, cy + (HEADER_H - 8) / 2, hint, DIM_COL);

        // Text area
        let text_y = cy + HEADER_H;
        let text_h = ch.saturating_sub(HEADER_H + STATUS_H);
        framebuffer::fill_rect(cx, text_y, cw, text_h, BG);

        // Line number gutter
        framebuffer::fill_rect(cx, text_y, LNUM_W, text_h, HEADER_BG);
        framebuffer::fill_rect(cx + LNUM_W - 1, text_y, 1, text_h, BORDER_COL);

        // Scrollbar track
        let sb_x = cx + cw - SCROLL_W;
        framebuffer::fill_rect(sb_x, text_y, SCROLL_W, text_h, SCROLL_BG);
        if self.line_count > visible && visible > 0 {
            let thumb_h = (text_h * visible / self.line_count).max(4);
            let thumb_y = text_y + (text_h - thumb_h) * self.scroll / (self.line_count - visible).max(1);
            framebuffer::fill_rect(sb_x, thumb_y, SCROLL_W, thumb_h, SCROLL_FG);
        }

        let text_w = cw.saturating_sub(LNUM_W + PAD_X + SCROLL_W + 2);
        let max_chars = text_w / CHAR_W;

        for row in 0..visible {
            let line_idx = self.scroll + row;
            if line_idx >= self.line_count { break; }

            let ry = text_y + row * ROW_H;

            // Line number
            let mut nbuf = [0u8; 8];
            let nlen = fmt_usize(&mut nbuf, line_idx + 1);
            let nstr = core::str::from_utf8(&nbuf[..nlen]).unwrap_or("");
            framebuffer::draw_text_at(cx + 2, ry + 2, nstr, NUM_COL);

            // Line text
            let start = self.lines[line_idx];
            let end = if line_idx + 1 < self.line_count {
                self.lines[line_idx + 1].saturating_sub(1)
            } else {
                self.text_len
            };
            let raw = &self.text[start..end];
            // Strip \r if present
            let raw = if raw.last() == Some(&b'\r') { &raw[..raw.len()-1] } else { raw };
            let display_len = raw.len().min(max_chars);
            let col = Self::line_col(raw);
            if let Ok(s) = core::str::from_utf8(&raw[..display_len]) {
                framebuffer::draw_text_at(cx + LNUM_W + PAD_X, ry + 2, s, col);
            }
        }

        // Status bar
        let sy = cy + ch - STATUS_H;
        framebuffer::fill_rect(cx, sy, cw, STATUS_H, STATUS_BG);
        framebuffer::fill_rect(cx, sy, cw, 1, BORDER_COL);
        let mut sbuf = [0u8; 80];
        let mut si = 0usize;
        write_label(&mut sbuf, &mut si, b"Lines: ");
        write_usize_s(&mut sbuf, &mut si, self.line_count);
        write_label(&mut sbuf, &mut si, b"  Bytes: ");
        write_usize_s(&mut sbuf, &mut si, self.text_len);
        write_label(&mut sbuf, &mut si, b"  Scroll: ");
        write_usize_s(&mut sbuf, &mut si, self.scroll + 1);
        write_label(&mut sbuf, &mut si, b"/");
        write_usize_s(&mut sbuf, &mut si, self.line_count);
        let stat = core::str::from_utf8(&sbuf[..si]).unwrap_or("");
        framebuffer::draw_text_at(cx + PAD_X, sy + (STATUS_H - 8) / 2, stat, STATUS_VAL);
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        let visible = Self::visible_rows(500);
        let prev_scroll = self.scroll;

        match key {
            Key::ArrowUp => {
                if self.scroll > 0 { self.scroll -= 1; }
            }
            Key::ArrowDown => {
                if self.scroll + 1 < self.line_count { self.scroll += 1; }
            }
            Key::Tab => {
                // Page down
                self.scroll = (self.scroll + visible).min(
                    self.line_count.saturating_sub(1)
                );
            }
            Key::Backspace => {
                // Page up
                self.scroll = self.scroll.saturating_sub(visible);
            }
            Key::Char(b'g') => {
                self.scroll = 0;
            }
            Key::Char(b'G') | Key::Char(b'\x05') => { // G or Ctrl+E
                self.scroll_to_bottom(visible);
            }
            Key::Char(b'r') | Key::Char(b'R') | Key::Char(b'\x0F') => { // r/R/Ctrl+O
                let was_at_bottom = self.scroll + visible >= self.line_count;
                self.refresh();
                if was_at_bottom { self.scroll_to_bottom(visible); }
            }
            _ => return AppAction::Nothing,
        }

        if self.scroll != prev_scroll { AppAction::RedrawAll } else { AppAction::Nothing }
    }
}

// ── Formatting helpers ────────────────────────────────────────────────────────

fn fmt_usize(buf: &mut [u8; 8], mut n: usize) -> usize {
    if n == 0 { buf[0] = b'0'; return 1; }
    let mut tmp = [0u8; 8];
    let mut ti = 0usize;
    while n > 0 { tmp[ti] = b'0' + (n % 10) as u8; ti += 1; n /= 10; }
    for i in 0..ti { buf[i] = tmp[ti - 1 - i]; }
    ti
}

fn write_label(buf: &mut [u8; 80], pos: &mut usize, label: &[u8]) {
    for &b in label { if *pos < buf.len() { buf[*pos] = b; *pos += 1; } }
}

fn write_usize_s(buf: &mut [u8; 80], pos: &mut usize, mut n: usize) {
    if n == 0 {
        if *pos < buf.len() { buf[*pos] = b'0'; *pos += 1; }
        return;
    }
    let start = *pos;
    let mut tmp = [0u8; 20];
    let mut ti = 0usize;
    while n > 0 { tmp[ti] = b'0' + (n % 10) as u8; ti += 1; n /= 10; }
    let end = start + ti;
    if end > buf.len() { return; }
    for i in 0..ti { buf[start + i] = tmp[ti - 1 - i]; }
    *pos = end;
}
