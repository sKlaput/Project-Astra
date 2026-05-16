// ---------------------------------------------------------------------------
// Astra OS — Notes app
//
// A simple scratchpad that auto-saves to FAT32 as "notes.txt" in the root.
// The first open creates the file if it does not exist; subsequent opens load
// the existing content.
//
// Controls:
//   Printable keys  — insert character at cursor
//   Backspace       — delete previous character
//   Delete (Ctrl+D) — delete character at cursor
//   Enter           — insert newline
//   Ctrl+S          — save to FAT32 now
//   Ctrl+A          — select all (jump to end of text)
//   Ctrl+L          — clear all text
//   Arrow keys      — move cursor
// ---------------------------------------------------------------------------

extern crate alloc;

use crate::app::{App, AppAction};
use crate::framebuffer;
use crate::input::Key;
use crate::fs;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG:          u32 = 0x08100A;
const HEADER_BG:   u32 = 0x0C1A0E;
const HEADER_COL:  u32 = 0xA8D8B0;
const BORDER_COL:  u32 = 0x1A3020;
const TEXT_COL:    u32 = 0xC8E8D0;
const CURSOR_COL:  u32 = 0x40E860;
const LINE_NUM:    u32 = 0x2A4830;
const STATUS_BG:   u32 = 0x0C1A0E;
const STATUS_COL:  u32 = 0x3A6040;
const STATUS_VAL:  u32 = 0x60A070;
const DIRTY_COL:   u32 = 0xE3B341;
const SAVED_COL:   u32 = 0x40E860;

// ── Layout ────────────────────────────────────────────────────────────────────

const HEADER_H:  usize = 22;
const STATUS_H:  usize = 16;
const LNUM_W:    usize = 28;
const PAD_X:     usize = 8;
const ROW_H:     usize = 13;
const CHAR_W:    usize = 6;

// ── Limits ────────────────────────────────────────────────────────────────────

const BUF_CAP:   usize = 16 * 1024;   // 16 KiB
const MAX_LINES: usize = 1024;

/// Fallback dynamic VFS path used if FAT32 is not available.
const NOTES_DYN:  &str = "/notes.txt";

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq)]
enum SaveState { Clean, Dirty, JustSaved }

// ── NotesApp ──────────────────────────────────────────────────────────────────

pub struct NotesApp {
    buf:          [u8; BUF_CAP],
    buf_len:      usize,
    cursor:       usize,          // byte offset
    scroll:       usize,          // first visible line index
    lines:        [usize; MAX_LINES],  // byte offsets of line starts
    line_count:   usize,
    save_state:   SaveState,
    flash_ticks:  u8,             // countdown for "Saved" flash
    fat32_id:     Option<u16>,    // FAT32 node id once discovered
}

impl NotesApp {
    pub fn new() -> Self {
        let mut app = NotesApp {
            buf:         [0u8; BUF_CAP],
            buf_len:     0,
            cursor:      0,
            scroll:      0,
            lines:       [0usize; MAX_LINES],
            line_count:  1,
            save_state:  SaveState::Clean,
            flash_ticks: 0,
            fat32_id:    None,
        };
        app.lines[0] = 0;
        app.try_load();
        app
    }

    // ── Load / Save ───────────────────────────────────────────────────────────

    fn try_load(&mut self) {
        // Try FAT32 root directly.
        if crate::fat32::is_mounted() {
            let root = crate::fat32::root_cluster();
            if let Some(de) = crate::fat32::find_in_dir(root, b"notes.txt") {
                let n = crate::fat32::read_file(de.cluster, de.size, &mut self.buf);
                if n > 0 {
                    self.buf_len = n;
                    self.cursor = n;
                    self.reindex();
                    return;
                }
            }
        }
        // Try dynamic VFS.
        if let Ok(mut h) = fs::open(NOTES_DYN) {
            if let Ok(n) = fs::read(&mut h, &mut self.buf) {
                self.buf_len = n;
                self.cursor = n;
                self.reindex();
            }
        }
    }

    fn save(&mut self) {
        let data = &self.buf[..self.buf_len];
        // Try FAT32 root directly.
        if crate::fat32::is_mounted() {
            let root = crate::fat32::root_cluster();
            if crate::fat32::write_file(root, b"notes.txt", data) {
                self.save_state = SaveState::JustSaved;
                self.flash_ticks = 30;
                return;
            }
        }
        // Fallback: dynamic VFS.
        if fs::write_file(NOTES_DYN, data).is_ok() {
            self.save_state = SaveState::JustSaved;
            self.flash_ticks = 30;
        }
    }

    // ── Text editing ──────────────────────────────────────────────────────────

    fn insert_byte(&mut self, b: u8) {
        if self.buf_len >= BUF_CAP { return; }
        // Shift bytes right from cursor
        let pos = self.cursor;
        for i in (pos..self.buf_len).rev() {
            self.buf[i + 1] = self.buf[i];
        }
        self.buf[pos] = b;
        self.buf_len += 1;
        self.cursor += 1;
        self.reindex();
        self.save_state = SaveState::Dirty;
    }

    fn delete_back(&mut self) {
        if self.cursor == 0 { return; }
        let pos = self.cursor - 1;
        for i in pos..self.buf_len - 1 {
            self.buf[i] = self.buf[i + 1];
        }
        self.buf_len -= 1;
        self.cursor = pos;
        self.reindex();
        self.save_state = SaveState::Dirty;
    }

    fn delete_fwd(&mut self) {
        if self.cursor >= self.buf_len { return; }
        let pos = self.cursor;
        for i in pos..self.buf_len - 1 {
            self.buf[i] = self.buf[i + 1];
        }
        self.buf_len -= 1;
        self.reindex();
        self.save_state = SaveState::Dirty;
    }

    fn clear_all(&mut self) {
        self.buf_len = 0;
        self.cursor = 0;
        self.lines[0] = 0;
        self.line_count = 1;
        self.save_state = SaveState::Dirty;
    }

    fn reindex(&mut self) {
        self.line_count = 0;
        self.lines[0] = 0;
        self.line_count = 1;
        for i in 0..self.buf_len {
            if self.buf[i] == b'\n' && self.line_count < MAX_LINES {
                self.lines[self.line_count] = i + 1;
                self.line_count += 1;
            }
        }
    }

    fn cursor_line(&self) -> usize {
        let mut line = 0usize;
        for i in (0..self.line_count).rev() {
            if self.lines[i] <= self.cursor {
                line = i;
                break;
            }
        }
        line
    }

    fn cursor_col(&self) -> usize {
        let line = self.cursor_line();
        self.cursor - self.lines[line]
    }

    fn move_up(&mut self) {
        let line = self.cursor_line();
        if line == 0 { self.cursor = 0; return; }
        let col = self.cursor_col();
        let prev_start = self.lines[line - 1];
        let prev_end = if line - 1 + 1 < self.line_count { self.lines[line] - 1 } else { self.buf_len };
        let prev_len = prev_end - prev_start;
        self.cursor = prev_start + col.min(prev_len);
    }

    fn move_down(&mut self) {
        let line = self.cursor_line();
        if line + 1 >= self.line_count { self.cursor = self.buf_len; return; }
        let col = self.cursor_col();
        let next_start = self.lines[line + 1];
        let next_end = if line + 2 < self.line_count { self.lines[line + 2] - 1 } else { self.buf_len };
        let next_len = next_end - next_start;
        self.cursor = next_start + col.min(next_len);
    }

    fn move_left(&mut self) {
        if self.cursor > 0 { self.cursor -= 1; }
    }

    fn move_right(&mut self) {
        if self.cursor < self.buf_len { self.cursor += 1; }
    }

    fn ensure_cursor_visible(&mut self, visible_rows: usize) {
        let line = self.cursor_line();
        if line < self.scroll {
            self.scroll = line;
        } else if visible_rows > 0 && line >= self.scroll + visible_rows {
            self.scroll = line + 1 - visible_rows;
        }
    }

    fn visible_rows(ch: usize) -> usize {
        ch.saturating_sub(HEADER_H + STATUS_H) / ROW_H
    }
}

// ── App trait ─────────────────────────────────────────────────────────────────

impl App for NotesApp {
    fn title(&self) -> &str { "Notes" }
    fn app_id(&self) -> &'static str { "notes" }
    fn preferred_size(&self) -> (usize, usize) { (600, 440) }
    fn allow_multiple_instances(&self) -> bool { false }
    fn refresh_interval_ms(&self) -> Option<u64> { None }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        // Header
        framebuffer::fill_rect(cx, cy, cw, HEADER_H, HEADER_BG);
        framebuffer::fill_rect(cx, cy + HEADER_H - 1, cw, 1, BORDER_COL);
        let title = match self.save_state {
            SaveState::Dirty     => "Notes  [modified]",
            SaveState::JustSaved => "Notes  [saved]",
            SaveState::Clean     => "Notes",
        };
        let title_col = match self.save_state {
            SaveState::Dirty     => DIRTY_COL,
            SaveState::JustSaved => SAVED_COL,
            SaveState::Clean     => HEADER_COL,
        };
        framebuffer::draw_text_at(cx + PAD_X, cy + (HEADER_H - 8) / 2, title, title_col);
        // Help hint in header
        let hint = "Ctrl+S save  Ctrl+L clear";
        let hx = cx + cw.saturating_sub(hint.len() * 6 + 8);
        framebuffer::draw_text_at(hx, cy + (HEADER_H - 8) / 2, hint, STATUS_COL);

        // Text area background
        let text_y = cy + HEADER_H;
        let text_h = ch.saturating_sub(HEADER_H + STATUS_H);
        framebuffer::fill_rect(cx, text_y, cw, text_h, BG);

        // Line number gutter
        framebuffer::fill_rect(cx, text_y, LNUM_W, text_h, HEADER_BG);
        framebuffer::fill_rect(cx + LNUM_W - 1, text_y, 1, text_h, BORDER_COL);

        let visible = Self::visible_rows(ch);
        let cur_line = self.cursor_line();

        for row in 0..visible {
            let line_idx = self.scroll + row;
            if line_idx >= self.line_count { break; }

            let ry = text_y + row * ROW_H;

            // Line number
            let mut lbuf = [0u8; 8];
            let llen = fmt_usize(&mut lbuf, line_idx + 1);
            let lstr = core::str::from_utf8(&lbuf[..llen]).unwrap_or("");
            let lnum_col = if line_idx == cur_line { HEADER_COL } else { LINE_NUM };
            framebuffer::draw_text_at(cx + 2, ry + 2, lstr, lnum_col);

            // Line text
            let start = self.lines[line_idx];
            let end = if line_idx + 1 < self.line_count {
                self.lines[line_idx + 1].saturating_sub(1)
            } else {
                self.buf_len
            };
            let max_chars = (cw.saturating_sub(LNUM_W + PAD_X)) / CHAR_W;
            let text_bytes = &self.buf[start..end];
            let display_len = text_bytes.len().min(max_chars);
            if let Ok(s) = core::str::from_utf8(&text_bytes[..display_len]) {
                framebuffer::draw_text_at(cx + LNUM_W + PAD_X, ry + 2, s, TEXT_COL);
            }

            // Cursor
            if line_idx == cur_line {
                let col = self.cursor - start;
                let cx2 = cx + LNUM_W + PAD_X + col * CHAR_W;
                framebuffer::fill_rect(cx2, ry + 1, 2, ROW_H - 2, CURSOR_COL);
            }
        }

        // Status bar
        let sy = cy + ch - STATUS_H;
        framebuffer::fill_rect(cx, sy, cw, STATUS_H, STATUS_BG);
        framebuffer::fill_rect(cx, sy, cw, 1, BORDER_COL);
        let mut sbuf = [0u8; 80];
        let mut si = 0usize;
        // "Ln N  Col N  N chars"
        write_label(&mut sbuf, &mut si, b"Ln ");
        write_usize_s(&mut sbuf, &mut si, cur_line + 1);
        write_label(&mut sbuf, &mut si, b"  Col ");
        write_usize_s(&mut sbuf, &mut si, self.cursor_col() + 1);
        write_label(&mut sbuf, &mut si, b"  ");
        write_usize_s(&mut sbuf, &mut si, self.buf_len);
        write_label(&mut sbuf, &mut si, b" chars");
        let stat = core::str::from_utf8(&sbuf[..si]).unwrap_or("");
        framebuffer::draw_text_at(cx + PAD_X, sy + (STATUS_H - 8) / 2, stat, STATUS_VAL);
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        let ch_before = self.buf_len;
        let cur_before = self.cursor;

        match key {
            Key::Char(b'\x13') => {  // Ctrl+S
                self.save();
                return AppAction::RedrawAll;
            }
            Key::Char(b'\x0C') => {  // Ctrl+L
                self.clear_all();
                self.scroll = 0;
                return AppAction::RedrawAll;
            }
            Key::Char(b'\x08') | Key::Backspace => {
                self.delete_back();
            }
            Key::Char(b'\x04') => {  // Ctrl+D
                self.delete_fwd();
            }
            Key::Char(b'\r') | Key::Char(b'\n') | Key::Enter => {
                self.insert_byte(b'\n');
            }
            Key::ArrowLeft  => self.move_left(),
            Key::ArrowRight => self.move_right(),
            Key::ArrowUp    => self.move_up(),
            Key::ArrowDown  => self.move_down(),
            Key::Char(b) if b >= 0x20 => {
                self.insert_byte(b);
            }
            _ => return AppAction::Nothing,
        }

        // Decrement flash counter
        if self.flash_ticks > 0 {
            self.flash_ticks -= 1;
            if self.flash_ticks == 0 {
                if self.save_state == SaveState::JustSaved {
                    self.save_state = SaveState::Clean;
                }
            }
        }

        let visible = Self::visible_rows(440); // use preferred height (440)
        self.ensure_cursor_visible(visible);

        if self.buf_len != ch_before || self.cursor != cur_before {
            AppAction::RedrawAll
        } else {
            AppAction::Nothing
        }
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
