// ---------------------------------------------------------------------------
// Astra OS — Terminal window client
//
// Pure content renderer + input handler.  The desktop compositor owns the
// window chrome, cursor, and presentation.  This module only:
//   - Manages terminal state (history + input buffer)
//   - Renders its content into a given client rect (called by compositor)
//   - Handles keyboard events routed from the compositor
// ---------------------------------------------------------------------------

use crate::framebuffer;
use crate::input::Key;
use spin::Mutex;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG: u32 = 0x0A0E14;
const TEXT_NORM: u32 = 0xB0D4B8;
const PROMPT_COL: u32 = 0x4FC3F7;
const INPUT_COL: u32 = 0xE8F4FD;
const CURSOR_COL: u32 = 0x4FC3F7;
const ERR_COL: u32 = 0xFF6B6B;
const SEPARATOR: u32 = 0x1E3A5F;

// ── Font metrics (scale 2) ────────────────────────────────────────────────────

const SCALE: usize = 2;
const CHAR_W: usize = 6 * SCALE; // 12
const CHAR_H: usize = 8 * SCALE; // 16 (7px glyph * 2 + 2px line gap)
const PAD_X: usize = 10;

const PROMPT: &str = "astra$ ";

// ── Storage ───────────────────────────────────────────────────────────────────

const HIST_ROWS: usize = 40;
const LINE_BUF: usize = 82;
const CMD_HIST: usize = 16; // command history ring size
const PATH_BUF: usize = 128; // max cwd path length

#[derive(Clone, Copy)]
struct Line {
    data: [u8; LINE_BUF],
    len: usize,
    col: u32,
}

impl Line {
    const fn empty() -> Self {
        Line {
            data: [0u8; LINE_BUF],
            len: 0,
            col: TEXT_NORM,
        }
    }
}

struct TermState {
    hist: [Line; HIST_ROWS],
    hist_cnt: usize,
    input: [u8; LINE_BUF],
    input_len: usize,
    cursor_pos: usize, // byte offset within input where next char is inserted
    inited: bool,
    // command history
    cmd_hist: [[u8; LINE_BUF]; CMD_HIST],
    cmd_hlen: [usize; CMD_HIST],
    cmd_hcount: usize, // total commands entered
    cmd_hpos: usize,   // browse position (0 = latest)
    // scroll: 0 = pinned to bottom (most recent), N = scrolled back N rows
    scroll_off: usize,
    // current working directory (FAT32 cluster + display path)
    cwd_cluster: u32, // 0 = use FAT32 root when mounted
    cwd_path: [u8; PATH_BUF],
    cwd_plen: usize,
}

impl TermState {
    const fn new() -> Self {
        TermState {
            hist: [Line::empty(); HIST_ROWS],
            hist_cnt: 0,
            input: [0u8; LINE_BUF],
            input_len: 0,
            cursor_pos: 0,
            inited: false,
            cmd_hist: [[0u8; LINE_BUF]; CMD_HIST],
            cmd_hlen: [0usize; CMD_HIST],
            cmd_hcount: 0,
            cmd_hpos: 0,
            scroll_off: 0,
            cwd_cluster: 0,
            cwd_path: [0u8; PATH_BUF],
            cwd_plen: 0,
        }
    }

    fn push_str(&mut self, s: &str, col: u32) {
        self.push_bytes(s.as_bytes(), col);
    }

    fn push_bytes(&mut self, b: &[u8], col: u32) {
        if self.hist_cnt == HIST_ROWS {
            for i in 0..HIST_ROWS - 1 {
                self.hist[i] = self.hist[i + 1];
            }
            self.hist_cnt -= 1;
        }
        let ln = &mut self.hist[self.hist_cnt];
        let n = b.len().min(LINE_BUF);
        ln.data[..n].copy_from_slice(&b[..n]);
        ln.len = n;
        ln.col = col;
        self.hist_cnt += 1;
        // Auto-scroll to bottom when new content arrives
        self.scroll_off = 0;
    }

    /// Push a command into the ring history and reset browse position.
    fn push_cmd_hist(&mut self, cmd: &[u8], len: usize) {
        if len == 0 {
            return;
        }
        let slot = self.cmd_hcount % CMD_HIST;
        let n = len.min(LINE_BUF);
        self.cmd_hist[slot][..n].copy_from_slice(&cmd[..n]);
        self.cmd_hlen[slot] = n;
        self.cmd_hcount += 1;
        self.cmd_hpos = 0;
    }

    /// Navigate history: delta = 1 means older, -1 means newer.
    /// Returns true if input was changed.
    fn history_navigate(&mut self, delta: isize) -> bool {
        let total = self.cmd_hcount.min(CMD_HIST);
        if total == 0 {
            return false;
        }
        let new_pos = (self.cmd_hpos as isize + delta).clamp(0, total as isize) as usize;
        if new_pos == self.cmd_hpos {
            return false;
        }
        self.cmd_hpos = new_pos;
        if new_pos == 0 {
            self.input_len = 0;
        } else {
            // pos=1 → most recent, pos=total → oldest
            let idx = (self.cmd_hcount.wrapping_sub(new_pos)) % CMD_HIST;
            let n = self.cmd_hlen[idx];
            self.input[..n].copy_from_slice(&self.cmd_hist[idx][..n]);
            self.input_len = n;
        }
        // always put cursor at end after history navigation
        self.cursor_pos = self.input_len;
        true
    }

    /// Return current directory display string (e.g. "/docs").
    fn cwd_str(&self) -> &str {
        if self.cwd_plen == 0 {
            "/"
        } else {
            core::str::from_utf8(&self.cwd_path[..self.cwd_plen]).unwrap_or("/")
        }
    }
}

static TERM: Mutex<TermState> = Mutex::new(TermState::new());

// ── Public interface (called by desktop compositor) ───────────────────────────

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TermAction {
    Nothing,
    RedrawAll,
    RedrawInput,
    Close,
}

/// Height of the input-line region at the bottom of client area.
/// Covers: separator (1px) + gap (3px) + prompt/input row (CHAR_H) + padding (10px).
pub const INPUT_REGION_H: usize = CHAR_H + 14;

/// One-time welcome banner.
pub fn init_if_needed() {
    let mut t = TERM.lock();
    if !t.inited {
        t.inited = true;
        t.push_str("Astra OS  Terminal  v0.1", TEXT_NORM);
        t.push_str("Type 'help' for commands.  ESC to close.", TEXT_NORM);
        t.push_str("", TEXT_NORM);
    }
}

/// Render terminal content into the given client area (backbuffer only).
pub fn render(cx: usize, cy: usize, cw: usize, ch: usize) {
    let t = TERM.lock();

    let inner_x = cx + PAD_X;
    let inner_w = cw.saturating_sub(PAD_X * 2);
    let max_cols = inner_w / CHAR_W;
    if max_cols == 0 {
        return;
    }

    // Input row at bottom of client area
    let input_y = cy + ch.saturating_sub(CHAR_H + 10);

    // History area
    let history_y = cy + 6;
    let avail_h = input_y.saturating_sub(history_y + 6);
    let vis_rows = avail_h / CHAR_H;

    // Clear history area background so stale lines don't bleed through on scroll
    framebuffer::fill_rect(cx, history_y, cw, avail_h, BG);

    // Clamp scroll so we can't scroll past the top of history
    let max_scroll = t.hist_cnt.saturating_sub(vis_rows);
    let scroll = t.scroll_off.min(max_scroll);
    let start_idx = t.hist_cnt.saturating_sub(vis_rows + scroll);
    let end_idx = t.hist_cnt.saturating_sub(scroll);

    // Show scroll indicator when not at the bottom
    if scroll > 0 {
        framebuffer::draw_text_scaled(
            inner_x,
            history_y,
            "^ PgUp/PgDn to scroll ^",
            0x444466,
            SCALE,
        );
    }

    for (row, idx) in (start_idx..end_idx).enumerate() {
        let ln = &t.hist[idx];
        if ln.len > 0 {
            let n = ln.len.min(max_cols);
            let s = unsafe { core::str::from_utf8_unchecked(&ln.data[..n]) };
            framebuffer::draw_text_scaled(inner_x, history_y + row * CHAR_H, s, ln.col, SCALE);
        }
    }

    // Separator line
    framebuffer::fill_rect(cx, input_y - 4, cw, 1, SEPARATOR);

    // Prompt
    let prompt_cols = PROMPT.len().min(max_cols);
    framebuffer::draw_text_scaled(inner_x, input_y, &PROMPT[..prompt_cols], PROMPT_COL, SCALE);

    // User input — scroll window keeps cursor visible
    let text_x = inner_x + prompt_cols * CHAR_W;
    let input_cols = max_cols.saturating_sub(prompt_cols);
    if t.input_len > 0 && input_cols > 0 {
        let scroll_start = if t.cursor_pos >= input_cols {
            t.cursor_pos + 1 - input_cols
        } else {
            0
        };
        let shown_end = (scroll_start + input_cols).min(t.input_len);
        if shown_end > scroll_start {
            let s = unsafe { core::str::from_utf8_unchecked(&t.input[scroll_start..shown_end]) };
            framebuffer::draw_text_scaled(text_x, input_y, s, INPUT_COL, SCALE);
        }
    }

    // Block cursor at cursor_pos
    let input_cols_c = max_cols.saturating_sub(prompt_cols);
    let scroll_c = if t.cursor_pos >= input_cols_c {
        t.cursor_pos + 1 - input_cols_c
    } else {
        0
    };
    let cur_x = text_x + (t.cursor_pos - scroll_c) * CHAR_W;
    framebuffer::fill_rect(cur_x, input_y, CHAR_W - 2, CHAR_H, CURSOR_COL);
}

/// Render only the input-line region (separator + prompt + input + cursor).
/// The compositor should clear the input sub-rect before calling this.
pub fn render_input_line(cx: usize, cy: usize, cw: usize, ch: usize) {
    let t = TERM.lock();
    let inner_x = cx + PAD_X;
    let inner_w = cw.saturating_sub(PAD_X * 2);
    let max_cols = inner_w / CHAR_W;
    if max_cols == 0 {
        return;
    }

    let input_y = cy + ch.saturating_sub(CHAR_H + 10);

    // Separator line
    framebuffer::fill_rect(cx, input_y - 4, cw, 1, SEPARATOR);

    // Prompt
    let prompt_cols = PROMPT.len().min(max_cols);
    framebuffer::draw_text_scaled(inner_x, input_y, &PROMPT[..prompt_cols], PROMPT_COL, SCALE);

    // User input — scroll window keeps cursor visible
    let text_x = inner_x + prompt_cols * CHAR_W;
    let input_cols = max_cols.saturating_sub(prompt_cols);
    if t.input_len > 0 && input_cols > 0 {
        let scroll_start = if t.cursor_pos >= input_cols {
            t.cursor_pos + 1 - input_cols
        } else {
            0
        };
        let shown_end = (scroll_start + input_cols).min(t.input_len);
        if shown_end > scroll_start {
            let s = unsafe { core::str::from_utf8_unchecked(&t.input[scroll_start..shown_end]) };
            framebuffer::draw_text_scaled(text_x, input_y, s, INPUT_COL, SCALE);
        }
    }

    // Block cursor at cursor_pos
    let input_cols_c = max_cols.saturating_sub(prompt_cols);
    let scroll_c = if t.cursor_pos >= input_cols_c {
        t.cursor_pos + 1 - input_cols_c
    } else {
        0
    };
    let cur_x = text_x + (t.cursor_pos - scroll_c) * CHAR_W;
    framebuffer::fill_rect(cur_x, input_y, CHAR_W - 2, CHAR_H, CURSOR_COL);
}

/// Handle a keyboard event. Returns what the compositor should do.
pub fn handle_key(key: Key) -> TermAction {
    match key {
        Key::Escape => TermAction::Nothing,

        Key::Backspace => {
            let mut t = TERM.lock();
            if t.cursor_pos > 0 {
                let pos = t.cursor_pos - 1;
                let len = t.input_len;
                t.input.copy_within(pos + 1..len, pos);
                let new_len = len - 1;
                t.input[new_len] = 0;
                t.input_len = new_len;
                t.cursor_pos = pos;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::Delete => {
            let mut t = TERM.lock();
            let pos = t.cursor_pos;
            let len = t.input_len;
            if pos < len {
                t.input.copy_within(pos + 1..len, pos);
                let new_len = len - 1;
                t.input[new_len] = 0;
                t.input_len = new_len;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::ArrowLeft => {
            let mut t = TERM.lock();
            if t.cursor_pos > 0 {
                t.cursor_pos -= 1;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::ArrowRight => {
            let mut t = TERM.lock();
            if t.cursor_pos < t.input_len {
                t.cursor_pos += 1;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::Home => {
            let mut t = TERM.lock();
            if t.cursor_pos != 0 {
                t.cursor_pos = 0;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::End => {
            let mut t = TERM.lock();
            if t.cursor_pos != t.input_len {
                t.cursor_pos = t.input_len;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::ArrowUp => {
            let changed = TERM.lock().history_navigate(1);
            if changed {
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::ArrowDown => {
            let changed = TERM.lock().history_navigate(-1);
            if changed {
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::PageUp => {
            let mut t = TERM.lock();
            t.scroll_off = t.scroll_off.saturating_add(8);
            TermAction::RedrawAll
        }

        Key::PageDown => {
            let mut t = TERM.lock();
            t.scroll_off = t.scroll_off.saturating_sub(8);
            TermAction::RedrawAll
        }

        Key::Enter => {
            execute_input();
            TermAction::RedrawAll
        }

        Key::Char(c) => {
            let mut t = TERM.lock();
            if t.input_len < LINE_BUF - 1 {
                let pos = t.cursor_pos;
                let len = t.input_len;
                t.input.copy_within(pos..len, pos + 1);
                t.input[pos] = c;
                t.input_len += 1;
                t.cursor_pos += 1;
                t.cmd_hpos = 0;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        _ => TermAction::Nothing,
    }
}

include!("terminal/dispatch.rs");
include!("terminal/filesystem.rs");
include!("terminal/networking.rs");
include!("terminal/process.rs");
include!("terminal/path_helpers.rs");
include!("terminal/system.rs");
include!("terminal/app.rs");
