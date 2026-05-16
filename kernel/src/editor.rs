// ---------------------------------------------------------------------------
// Astra OS — Editor app  (polished v2, read-only viewer)
//
// Features:
//   - Line-numbered text view with scrollbar
//   - Home / End jump to top / bottom
//   - g/G vi-style top/bottom
//   - Page scroll using Tab / Shift (Backspace key)
//   - Long-line clipping with visual ">" indicator
//   - Intentional empty-file and error states
//   - Status bar: "Line X / Y  (XX%)"
//   - Multiple instances allowed (each file gets its own window)
// ---------------------------------------------------------------------------

use crate::app::{App, AppAction};
use crate::framebuffer;
use crate::fs;
use crate::input::Key;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG:         u32 = 0x060A0F;
const HEADER_BG:  u32 = 0x0A1220;
const HEADER_COL: u32 = 0xD8EEFF;
const GUTTER_BG:  u32 = 0x080E18;
const LNUM_COL:   u32 = 0x2E4C68;
const LNUM_CUR:   u32 = 0x4A7296;
const LINE_COL:   u32 = 0xC0D8EC;
const CLIP_COL:   u32 = 0x4A8AAA;   // ">" clipped-line indicator
const TILDE_COL:  u32 = 0x1E3448;
const BORDER_COL: u32 = 0x142030;
const STATUS_BG:  u32 = 0x0A1220;
const STATUS_COL: u32 = 0x3A6080;
const STATUS_VAL: u32 = 0x5A8AAA;
const ERR_COL:    u32 = 0xB04040;
const ERR_BG:     u32 = 0x1A0A0A;
const EMPTY_COL:  u32 = 0x2A4058;
const SCROLL_BG:  u32 = 0x0A1018;
const SCROLL_FG:  u32 = 0x224060;
const CURSOR_BLOCK: u32 = 0x2A5FAA;  // block cursor background
const SAVE_OK_COL:  u32 = 0x4CAF78;  // "Saved" flash indicator
const DIRTY_COL:    u32 = 0xE3B341;  // modified indicator
const EDIT_BADGE_T: u32 = 0xC8E8FF;  // [EDIT] badge / cursor-line text
const PROMPT_BG:    u32 = 0x0E1E32;  // close-confirm overlay background
const PROMPT_BORDER: u32 = 0xE3B341; // amber border matching dirty indicator
const PROMPT_COL:   u32 = 0xD8EEFF; // prompt text
const PROMPT_KEY:   u32 = 0xE3B341; // key highlight in prompt

// ── Layout ────────────────────────────────────────────────────────────────────

const PAD_X:      usize = 10;
const ROW_H:      usize = 14;
const HEADER_H:   usize = 24;
const STATUS_H:   usize = 18;
const LNUM_W:     usize = 34;   // gutter for 4-digit number + space
const CHAR_W:     usize = 6;
const SCROLL_W:   usize = 6;

// ── Buffer limits ─────────────────────────────────────────────────────────────

const BUF_SIZE:  usize = 8192;
const MAX_LINES: usize = 512;

// ── Error kind ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoadState { Ok, Empty, NotFound, ReadError }

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorMode { View, Edit }

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClosePrompt { Hidden, Visible }

// ── EditorApp ──────────────────────────────────────────────────────────────────

pub struct EditorApp {
    buf:         [u8; BUF_SIZE],
    buf_len:     usize,
    lines:       [usize; MAX_LINES],
    line_count:  usize,
    scroll:      usize,
    title_buf:   [u8; 80],
    title_len:   usize,
    path_buf:    [u8; 128],
    path_len:    usize,
    state:       LoadState,
    // Edit mode
    mode:        EditorMode,
    cursor_line: usize,
    cursor_col:  usize,
    dirty:       bool,
    writable:    bool,
    saved_flash: bool,  // briefly show "Saved" after Ctrl+S
    close_prompt: ClosePrompt,
}

impl EditorApp {
    pub fn open(path: &str) -> Self {
        let writable = crate::fs::is_writable(path);
        let mut app = EditorApp {
            buf:        [0u8; BUF_SIZE],
            buf_len:    0,
            lines:      [0usize; MAX_LINES],
            line_count: 0,
            scroll:     0,
            title_buf:  [0u8; 80],
            title_len:  0,
            path_buf:   [0u8; 128],
            path_len:   0,
            state:      LoadState::Ok,
            mode:       if writable { EditorMode::Edit } else { EditorMode::View },
            cursor_line: 0,
            cursor_col:  0,
            dirty:      false,
            writable,
            saved_flash: false,
            close_prompt: ClosePrompt::Hidden,
        };
        app.build_title(path);
        app.load(path);
        app
    }

    fn build_title(&mut self, path: &str) {
        // Store full path for saving
        let pb = path.as_bytes();
        let pn = pb.len().min(self.path_buf.len());
        self.path_buf[..pn].copy_from_slice(&pb[..pn]);
        self.path_len = pn;

        // Title shows the full path so the window bar reads e.g. "Editor - /etc/motd".
        let prefix = b"Editor - ";
        let mut i = 0usize;
        for b in prefix.iter() {
            if i < self.title_buf.len() { self.title_buf[i] = *b; i += 1; }
        }
        for b in path.bytes() {
            if i < self.title_buf.len() { self.title_buf[i] = b; i += 1; }
        }
        self.title_len = i;
    }

    fn path_str(&self) -> &str {
        core::str::from_utf8(&self.path_buf[..self.path_len]).unwrap_or("")
    }

    /// Number of content characters on line `n` (excluding newline).
    fn line_len(&self, n: usize) -> usize {
        self.line_slice(n).len()
    }

    /// Byte offset in buf for the cursor position.
    fn cursor_offset(&self) -> usize {
        if self.line_count == 0 { return 0; }
        let line = self.cursor_line.min(self.line_count.saturating_sub(1));
        self.lines[line] + self.cursor_col
    }

    /// Insert one byte at the current cursor position.
    fn insert_byte(&mut self, b: u8) {
        if self.buf_len >= BUF_SIZE { return; }
        let pos = self.cursor_offset();
        // Shift bytes right
        let mut i = self.buf_len;
        while i > pos { self.buf[i] = self.buf[i - 1]; i -= 1; }
        self.buf[pos] = b;
        self.buf_len += 1;
        self.index_lines();
        if b == b'\n' {
            self.cursor_line += 1;
            self.cursor_col = 0;
        } else {
            self.cursor_col += 1;
        }
        self.dirty = true;
    }

    /// Delete the byte before the cursor (Backspace).
    fn delete_before_cursor(&mut self) {
        if self.line_count == 0 && self.buf_len == 0 { return; }
        if self.cursor_col > 0 {
            let pos = self.cursor_offset() - 1;
            let mut i = pos;
            while i + 1 < self.buf_len { self.buf[i] = self.buf[i + 1]; i += 1; }
            self.buf_len -= 1;
            self.cursor_col -= 1;
            self.index_lines();
            self.dirty = true;
        } else if self.cursor_line > 0 {
            // Delete the \n at the end of the previous line
            // Previous line's content length before merge
            let prev_len = self.line_len(self.cursor_line - 1);
            let newline_pos = self.lines[self.cursor_line] - 1;
            let mut i = newline_pos;
            while i + 1 < self.buf_len { self.buf[i] = self.buf[i + 1]; i += 1; }
            self.buf_len -= 1;
            self.cursor_line -= 1;
            self.cursor_col = prev_len;
            self.index_lines();
            self.dirty = true;
        }
    }

    /// Delete the byte at the cursor (Delete key).
    fn delete_at_cursor(&mut self) {
        if self.line_count == 0 { return; }
        let pos = self.cursor_offset();
        if pos >= self.buf_len { return; }
        let mut i = pos;
        while i + 1 < self.buf_len { self.buf[i] = self.buf[i + 1]; i += 1; }
        self.buf_len -= 1;
        // Clamp cursor col to new line length
        self.index_lines();
        let ll = self.line_len(self.cursor_line);
        if self.cursor_col > ll { self.cursor_col = ll; }
        self.dirty = true;
    }

    /// Save buf to VFS.  Only works if writable.
    fn save(&mut self) -> bool {
        if !self.writable { return false; }
        let path = self.path_str();
        crate::serial::write_str("editor: save path=");
        crate::serial::write_line(path);
        let result = crate::fs::write_file(path, &self.buf[..self.buf_len]);
        match &result {
            Ok(n) => { crate::serial::write_str("editor: save OK bytes="); crate::serial::write_u64(*n as u64); crate::serial::write_line(""); }
            Err(_) => { crate::serial::write_line("editor: save FAILED"); }
        }
        matches!(result, Ok(_))
    }

    /// Scroll so the cursor line is visible.
    fn ensure_cursor_visible(&mut self, visible: usize) {
        if self.cursor_line < self.scroll {
            self.scroll = self.cursor_line;
        } else if visible > 0 && self.cursor_line >= self.scroll + visible {
            self.scroll = self.cursor_line.saturating_sub(visible - 1);
        }
    }

    fn load(&mut self, path: &str) {
        let mut handle = match fs::open(path) {
            Ok(h) => h,
            Err(_) => { self.state = LoadState::NotFound; return; }
        };
        match fs::read(&mut handle, &mut self.buf) {
            Ok(0) => { self.state = LoadState::Empty; return; }
            Ok(n) => { self.buf_len = n; }
            Err(_) => { self.state = LoadState::ReadError; return; }
        }
        self.index_lines();
    }

    fn index_lines(&mut self) {
        self.line_count = 0;
        if self.buf_len == 0 { return; }
        self.lines[0] = 0;
        self.line_count = 1;
        for i in 0..self.buf_len {
            if self.buf[i] == b'\n' {
                if i + 1 < self.buf_len && self.line_count < MAX_LINES {
                    self.lines[self.line_count] = i + 1;
                    self.line_count += 1;
                }
            }
        }
    }

    fn line_slice(&self, n: usize) -> &[u8] {
        if n >= self.line_count { return b""; }
        let start = self.lines[n];
        let end = if n + 1 < self.line_count {
            let e = self.lines[n + 1];
            if e > start && self.buf[e - 1] == b'\n' { e - 1 } else { e }
        } else {
            self.buf_len
        };
        &self.buf[start..end]
    }

    fn visible_lines(ch: usize) -> usize {
        ch.saturating_sub(HEADER_H + STATUS_H) / ROW_H
    }

    fn scroll_percent(&self) -> usize {
        if self.line_count <= 1 { return 100; }
        (self.scroll * 100) / (self.line_count - 1)
    }

    fn page_size(ch: usize) -> usize {
        Self::visible_lines(ch).saturating_sub(2).max(1)
    }
}

impl App for EditorApp {
    fn title(&self) -> &str {
        core::str::from_utf8(&self.title_buf[..self.title_len]).unwrap_or("Editor")
    }

    fn preferred_size(&self) -> (usize, usize) { (640, 480) }
    fn app_id(&self) -> &'static str { "editor" }

    fn allow_multiple_instances(&self) -> bool { true }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        framebuffer::fill_rect(cx, cy, cw, ch, BG);

        // ── Header bar ────────────────────────────────────────────────────
        framebuffer::fill_rect(cx, cy, cw, HEADER_H, HEADER_BG);
        framebuffer::fill_rect(cx, cy + HEADER_H - 1, cw, 1, BORDER_COL);

        // Modified indicator in title
        let title_x = cx + PAD_X;
        let title_y = cy + (HEADER_H - 8) / 2;
        if self.dirty {
            framebuffer::draw_text_at(title_x, title_y, "[*] ", DIRTY_COL);
            framebuffer::draw_text_at(title_x + 4 * CHAR_W, title_y, self.title(), HEADER_COL);
        } else {
            framebuffer::draw_text_at(title_x, title_y, self.title(), HEADER_COL);
        }

        // Mode badge on right side of header — only show for read-only
        let badge_y = cy + (HEADER_H - 10) / 2;
        if self.mode == EditorMode::Edit {
            // No badge needed — edit is the default state
        } else {
            let ro = "[read-only]";
            let ro_x = cx + cw.saturating_sub(PAD_X + ro.len() * CHAR_W);
            framebuffer::draw_text_at(ro_x, badge_y + 1, ro, STATUS_COL);
        }

        // ── Text area ─────────────────────────────────────────────────────
        let text_y  = cy + HEADER_H;
        let text_h  = ch.saturating_sub(HEADER_H + STATUS_H);
        let visible = text_h / ROW_H;

        // Gutter
        framebuffer::fill_rect(cx, text_y, PAD_X + LNUM_W, text_h, GUTTER_BG);
        framebuffer::fill_rect(cx + PAD_X + LNUM_W, text_y, 1, text_h, BORDER_COL);

        // Error / empty states
        match self.state {
            LoadState::NotFound => {
                framebuffer::fill_rect(cx, text_y, cw, text_h, ERR_BG);
                let ey = text_y + text_h / 3;
                framebuffer::draw_text_at(cx + PAD_X + LNUM_W + 8, ey,
                    "[!]  File not found", ERR_COL);
                framebuffer::draw_text_at(cx + PAD_X + LNUM_W + 8, ey + ROW_H + 4,
                    "     The VFS path does not exist.", STATUS_COL);
            }
            LoadState::ReadError => {
                framebuffer::fill_rect(cx, text_y, cw, text_h, ERR_BG);
                framebuffer::draw_text_at(cx + PAD_X + LNUM_W + 8, text_y + text_h / 3,
                    "[!]  Read error", ERR_COL);
            }
            LoadState::Empty | LoadState::Ok => {
                let content_x  = cx + PAD_X + LNUM_W + 2;
                let content_w  = cw.saturating_sub(PAD_X + LNUM_W + 2 + SCROLL_W);
                let max_chars  = content_w / CHAR_W;
                let in_edit    = self.mode == EditorMode::Edit;

                for i in 0..visible {
                    let li  = self.scroll + i;
                    let ry  = text_y + i * ROW_H;
                    let ty  = ry + (ROW_H - 8) / 2;

                    if li >= self.line_count {
                        framebuffer::draw_text_at(cx + PAD_X, ty, "~", TILDE_COL);
                        // Draw cursor at (cursor_line, 0) when past end of file
                        if in_edit && li == self.cursor_line {
                            framebuffer::fill_rect(content_x, ry, CHAR_W, ROW_H, CURSOR_BLOCK);
                        }
                        continue;
                    }

                    // Line number — highlight cursor line in edit mode
                    let lnum_col = if in_edit && li == self.cursor_line { LNUM_CUR }
                                   else if i == 0 { LNUM_CUR }
                                   else { LNUM_COL };
                    let mut lbuf = [b' '; 5];
                    fmt_lnum(&mut lbuf, li + 1);
                    let lstr = core::str::from_utf8(&lbuf).unwrap_or("    ");
                    framebuffer::draw_text_at(cx + PAD_X, ty, lstr, lnum_col);

                    // Cursor block — draw BEFORE text so text appears on top
                    if in_edit && li == self.cursor_line {
                        let cur_col_clamped = self.cursor_col.min(max_chars.saturating_sub(1));
                        let cursor_bx = content_x + cur_col_clamped * CHAR_W;
                        framebuffer::fill_rect(cursor_bx, ry, CHAR_W, ROW_H, CURSOR_BLOCK);
                    }

                    // Line content with clipping indicator
                    let slice   = self.line_slice(li);
                    let text    = core::str::from_utf8(slice).unwrap_or("<binary>");
                    let clipped = text.len() > max_chars;
                    let show    = if clipped { truncate_str(text, max_chars.saturating_sub(1)) }
                                  else { text };
                    let text_col = if in_edit && li == self.cursor_line { EDIT_BADGE_T } else { LINE_COL };
                    framebuffer::draw_text_at(content_x, ty, show, text_col);
                    if clipped {
                        let clip_x = content_x + max_chars.saturating_sub(1) * CHAR_W;
                        // Thin vertical wall to show where content was cut
                        framebuffer::fill_rect(clip_x.saturating_sub(1), ry, 1, ROW_H, 0x142840);
                        framebuffer::draw_text_at(clip_x, ty, ">", CLIP_COL);
                    }
                }

                // Scrollbar
                let sb_x = cx + cw.saturating_sub(SCROLL_W);
                framebuffer::fill_rect(sb_x, text_y, SCROLL_W, text_h, SCROLL_BG);
                if self.line_count > 0 && visible < self.line_count && text_h > 0 {
                    let thumb_h = ((visible * text_h) / self.line_count).max(6);
                    let thumb_y = if self.line_count > visible {
                        (self.scroll * (text_h - thumb_h)) / (self.line_count - visible)
                    } else { 0 };
                    framebuffer::fill_rect(sb_x + 1, text_y + thumb_y,
                                           SCROLL_W - 2, thumb_h, SCROLL_FG);
                }
                // Empty-file notice
                if self.state == LoadState::Empty {
                    framebuffer::draw_text_at(
                        content_x + 8, text_y + text_h / 3,
                        "(empty file)", EMPTY_COL);
                }
            }
        }

        // ── Status bar ────────────────────────────────────────────────────
        let stat_y = cy + ch.saturating_sub(STATUS_H);
        framebuffer::fill_rect(cx, stat_y, cw, STATUS_H, STATUS_BG);
        framebuffer::fill_rect(cx, stat_y, cw, 1, BORDER_COL);
        let ty = stat_y + (STATUS_H - 8) / 2;

        if self.mode == EditorMode::Edit {
            // Edit mode status bar layout:
            //  LEFT:   Ln X  Col Y  /  N lines
            //  CENTRE: shortened path
            //  RIGHT:  [modified] / [Saved!] / [read-only]  |  Ctrl+S=save

            // Left — cursor position
            let mut ibuf = [0u8; 64];
            let pos_str = fmt_cursor_pos(&mut ibuf, self.cursor_line + 1,
                                         self.cursor_col + 1, self.line_count);
            framebuffer::draw_text_at(cx + PAD_X, ty, pos_str, STATUS_VAL);
            let left_end_x = cx + PAD_X + pos_str.len() * CHAR_W;

            // Right — dirty/saved state + key hint
            let (state_str, state_col) = if self.saved_flash {
                ("Saved!  ", SAVE_OK_COL)
            } else if !self.writable {
                ("read-only  ", STATUS_COL)
            } else if self.dirty {
                ("modified  ", DIRTY_COL)
            } else {
                ("  ", STATUS_COL)
            };
            let hint = if self.writable { "Ctrl+S=save" } else { "" };
            let right_str_len = state_str.len() + hint.len();
            let right_x = cx + cw.saturating_sub(right_str_len * CHAR_W + PAD_X);
            framebuffer::draw_text_at(right_x, ty, state_str, state_col);
            framebuffer::draw_text_at(right_x + state_str.len() * CHAR_W, ty, hint, STATUS_COL);

            // Centre — shortened path
            let gap_chars = right_x.saturating_sub(left_end_x + CHAR_W * 2) / CHAR_W;
            if gap_chars >= 4 {
                let mut pbuf = [0u8; 64];
                let plen = fmt_path_short(
                    &mut pbuf, &self.path_buf[..self.path_len], gap_chars);
                if let Ok(pstr) = core::str::from_utf8(&pbuf[..plen]) {
                    // Centre the path in the available gap
                    let path_px = pstr.len() * CHAR_W;
                    let path_x = left_end_x + CHAR_W + (right_x.saturating_sub(left_end_x + CHAR_W).saturating_sub(path_px)) / 2;
                    framebuffer::draw_text_at(path_x, ty, pstr, STATUS_COL);
                }
            }
        } else {
            // View mode status: line info | path | key hints
            let mut ibuf = [0u8; 48];
            let (info_str, info_col): (&str, u32) = match self.state {
                LoadState::NotFound  => ("File not found", ERR_COL),
                LoadState::ReadError => ("Read error",     ERR_COL),
                LoadState::Empty     => ("0 lines",        STATUS_VAL),
                LoadState::Ok        => (fmt_line_info(&mut ibuf, self.scroll + 1,
                                             self.line_count, self.scroll_percent()),
                                         STATUS_VAL),
            };
            framebuffer::draw_text_at(cx + PAD_X, ty, info_str, info_col);

            let ok = self.state == LoadState::Ok || self.state == LoadState::Empty;
            let hints = if !ok {
                ""
            } else {
                "Up/Dn  PgUp/Dn  Home/End"
            };
            let hx = cx + cw.saturating_sub(hints.len() * CHAR_W + PAD_X);
            framebuffer::draw_text_at(hx, ty, hints, STATUS_COL);
            let _ = hx;

            // File path — centre of status bar, shortened to fit available space
            if ok {
                let info_end_x = cx + PAD_X + info_str.len() * CHAR_W + CHAR_W * 2;
                let budget = hx.saturating_sub(info_end_x + CHAR_W * 2) / CHAR_W;
                if budget >= 4 {
                    let mut pbuf = [0u8; 64];
                    let plen = fmt_path_short(
                        &mut pbuf, &self.path_buf[..self.path_len], budget);
                    if let Ok(pstr) = core::str::from_utf8(&pbuf[..plen]) {
                        framebuffer::draw_text_at(info_end_x + CHAR_W, ty, pstr, STATUS_COL);
                    }
                }
            }
        }   // end match self.mode

        // ── Close-confirm overlay (dirty + trying to close) ─────────────────
        if self.close_prompt == ClosePrompt::Visible {
            let ov_h = 36usize;
            let ov_w = (cw * 3) / 4;
            let ov_x = cx + (cw - ov_w) / 2;
            let ov_y = cy + (ch - ov_h) / 2;
            // Shadow
            framebuffer::fill_rect(ov_x + 3, ov_y + 3, ov_w, ov_h, 0x00000080 & 0x040608);
            // Border
            framebuffer::fill_rect(ov_x, ov_y, ov_w, ov_h, PROMPT_BORDER);
            // Fill
            framebuffer::fill_rect(ov_x + 1, ov_y + 1, ov_w - 2, ov_h - 2, PROMPT_BG);
            let ty = ov_y + (ov_h - 8) / 2;
            // Message line
            framebuffer::draw_text_at(ov_x + 12, ty - 5, "Unsaved changes", PROMPT_COL);
            // Key hints
            let hx = ov_x + 12;
            framebuffer::draw_text_at(hx, ty + 7, "S", PROMPT_KEY);
            framebuffer::draw_text_at(hx + CHAR_W, ty + 7, "=Save  ", PROMPT_COL);
            framebuffer::draw_text_at(hx + 8 * CHAR_W, ty + 7, "D", PROMPT_KEY);
            framebuffer::draw_text_at(hx + 9 * CHAR_W, ty + 7, "=Discard  ", PROMPT_COL);
            framebuffer::draw_text_at(hx + 19 * CHAR_W, ty + 7, "Esc", PROMPT_KEY);
            framebuffer::draw_text_at(hx + 22 * CHAR_W, ty + 7, "=Cancel", PROMPT_COL);
        }
    }

    fn request_close(&mut self) -> AppAction {
        if self.dirty {
            self.close_prompt = ClosePrompt::Visible;
            AppAction::RedrawAll
        } else {
            AppAction::Close
        }
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        use crate::input::Key as K;

        // ── Close-confirm prompt intercepts all keys ───────────────────────────
        if self.close_prompt == ClosePrompt::Visible {
            match key {
                K::Char(b's') | K::Char(b'S') => {
                    // Save then close
                    let _ = self.save();
                    self.dirty = false;
                    return AppAction::Close;
                }
                K::Char(b'd') | K::Char(b'D') => {
                    // Discard and close
                    return AppAction::Close;
                }
                K::Escape => {
                    // Cancel — return to editor
                    self.close_prompt = ClosePrompt::Hidden;
                    return AppAction::RedrawAll;
                }
                _ => return AppAction::Nothing,
            }
        }

        // Ctrl+S works in any mode (save)
        if key == K::Ctrl(b's') {
            if self.save() {
                self.dirty = false;
                self.saved_flash = true;
            }
            return AppAction::RedrawAll;
        }

        if self.state == LoadState::NotFound || self.state == LoadState::ReadError {
            return AppAction::Nothing;
        }

        let (_, ph) = self.preferred_size();
        let visible = Self::visible_lines(ph);
        let page    = Self::page_size(ph);

        if self.mode == EditorMode::Edit {
            self.saved_flash = false;
            match key {
                K::Escape => {
                    // Escape in edit mode: consume the key so the WM does NOT
                    // interpret Nothing-for-Escape as "please close this window".
                    // (A close attempt goes through request_close() which shows
                    // the unsaved-changes prompt when dirty.)
                    return AppAction::RedrawAll;
                }
                K::Backspace => { self.delete_before_cursor(); }
                K::Delete    => { self.delete_at_cursor(); }
                K::Enter => { self.insert_byte(b'\n'); }
                K::ArrowUp => {
                    if self.cursor_line > 0 {
                        self.cursor_line -= 1;
                        let ll = self.line_len(self.cursor_line);
                        if self.cursor_col > ll { self.cursor_col = ll; }
                    }
                }
                K::ArrowDown => {
                    let max_line = if self.line_count > 0 { self.line_count - 1 } else { 0 };
                    if self.cursor_line < max_line {
                        self.cursor_line += 1;
                        let ll = self.line_len(self.cursor_line);
                        if self.cursor_col > ll { self.cursor_col = ll; }
                    }
                }
                K::ArrowLeft => {
                    if self.cursor_col > 0 {
                        self.cursor_col -= 1;
                    } else if self.cursor_line > 0 {
                        self.cursor_line -= 1;
                        self.cursor_col = self.line_len(self.cursor_line);
                    }
                }
                K::ArrowRight => {
                    let ll = self.line_len(self.cursor_line);
                    if self.cursor_col < ll {
                        self.cursor_col += 1;
                    } else {
                        let max_line = if self.line_count > 0 { self.line_count - 1 } else { 0 };
                        if self.cursor_line < max_line {
                            self.cursor_line += 1;
                            self.cursor_col = 0;
                        }
                    }
                }
                K::Home => { self.cursor_col = 0; }
                K::End  => { self.cursor_col = self.line_len(self.cursor_line); }
                K::PageUp => {
                    self.cursor_line = self.cursor_line.saturating_sub(page);
                    let ll = self.line_len(self.cursor_line);
                    if self.cursor_col > ll { self.cursor_col = ll; }
                }
                K::PageDown => {
                    let max_line = if self.line_count > 0 { self.line_count - 1 } else { 0 };
                    self.cursor_line = (self.cursor_line + page).min(max_line);
                    let ll = self.line_len(self.cursor_line);
                    if self.cursor_col > ll { self.cursor_col = ll; }
                }
                K::Char(c) if c >= 0x20 && c < 0x7F => {
                    self.insert_byte(c);
                }
                K::Tab => {
                    // Insert 4 spaces
                    for _ in 0..4 { self.insert_byte(b' '); }
                }
                _ => return AppAction::Nothing,
            }
            self.ensure_cursor_visible(visible);
            return AppAction::RedrawAll;
        }

        // View mode (read-only files only)
        self.saved_flash = false;
        let max_scroll = self.line_count.saturating_sub(1);
        let old = self.scroll;
        match key {
            K::Escape => return self.request_close(),
            K::Char(b'i') | K::Char(b'I') => return AppAction::Nothing,
            K::ArrowUp   | K::Char(b'k') | K::Char(b'K') => {
                if self.scroll > 0 { self.scroll -= 1; }
            }
            K::ArrowDown | K::Char(b'j') | K::Char(b'J') => {
                if self.scroll < max_scroll { self.scroll += 1; }
            }
            K::Tab       | K::PageDown => { self.scroll = (self.scroll + page).min(max_scroll); }
            K::Backspace | K::PageUp   => { self.scroll = self.scroll.saturating_sub(page); }
            K::Char(b'g') | K::Home   => { self.scroll = 0; }
            K::Char(b'G') | K::End    => { self.scroll = max_scroll; }
            _ => return AppAction::Nothing,
        }
        let _ = visible;
        if self.scroll != old { AppAction::RedrawAll } else { AppAction::Nothing }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn fmt_cursor_pos(buf: &mut [u8; 64], line: usize, col: usize, total: usize) -> &str {
    let mut i = 0usize;
    macro_rules! w { ($s:expr) => { for b in $s { if i < buf.len() { buf[i] = *b; i += 1; } } } }
    w!(b"Ln ");
    i = write_uint64(buf, i, line as u64);
    w!(b"  Col ");
    i = write_uint64(buf, i, col as u64);
    w!(b"  /  ");
    i = write_uint64(buf, i, total as u64);
    w!(b" lines");
    core::str::from_utf8(&buf[..i]).unwrap_or("")
}

fn write_uint64(buf: &mut [u8; 64], mut i: usize, mut n: u64) -> usize {
    if n == 0 {
        if i < buf.len() { buf[i] = b'0'; i += 1; }
        return i;
    }
    let start = i;
    while n > 0 && i < buf.len() { buf[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
    buf[start..i].reverse();
    i
}

fn fmt_lnum(buf: &mut [u8; 5], n: usize) {
    buf[4] = b' ';
    let digits = [
        b'0' + ((n / 1000) % 10) as u8,
        b'0' + ((n / 100)  % 10) as u8,
        b'0' + ((n / 10)   % 10) as u8,
        b'0' + (n          % 10) as u8,
    ];
    let mut started = false;
    for i in 0..4 {
        if digits[i] != b'0' { started = true; }
        buf[i] = if started { digits[i] } else { b' ' };
    }
    if !started { buf[3] = b'0'; }
}

fn fmt_line_info(buf: &mut [u8; 48], cur: usize, total: usize, pct: usize) -> &str {
    let mut i = 0usize;
    macro_rules! w { ($s:expr) => { for b in $s { if i < buf.len() { buf[i] = *b; i += 1; } } } }
    w!(b"Ln ");
    i = write_uint(buf, i, cur);
    w!(b" / ");
    i = write_uint(buf, i, total);
    w!(b"   (");
    i = write_uint(buf, i, pct);
    w!(b"%)");
    core::str::from_utf8(&buf[..i]).unwrap_or("")
}

fn write_uint(buf: &mut [u8; 48], mut i: usize, mut n: usize) -> usize {
    if n == 0 {
        if i < buf.len() { buf[i] = b'0'; i += 1; }
        return i;
    }
    let start = i;
    while n > 0 && i < buf.len() { buf[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
    buf[start..i].reverse();
    i
}

fn truncate_str(s: &str, max: usize) -> &str {
    let b = s.as_bytes();
    if b.len() <= max { return s; }
    let mut end = max;
    while end > 0 && (b[end] & 0xC0) == 0x80 { end -= 1; }
    core::str::from_utf8(&b[..end]).unwrap_or("")
}

/// Writes a shortened path into `buf`.  If the path fits within `max_chars` it
/// is copied verbatim; otherwise `...` is prepended and the trailing portion of
/// the path is kept so the filename/tail stays readable.
fn fmt_path_short(buf: &mut [u8; 64], path: &[u8], max_chars: usize) -> usize {
    let max = max_chars.min(64);
    if path.len() <= max {
        let n = path.len().min(64);
        buf[..n].copy_from_slice(&path[..n]);
        n
    } else if max > 3 {
        buf[0] = b'.';
        buf[1] = b'.';
        buf[2] = b'.';
        let skip = path.len().saturating_sub(max - 3);
        let copy = (path.len() - skip).min(61);
        buf[3..3 + copy].copy_from_slice(&path[skip..skip + copy]);
        3 + copy
    } else {
        0
    }
}
