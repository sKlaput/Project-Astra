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

const BG: u32 = 0x060A0F;
const HEADER_BG: u32 = 0x0A1220;
const HEADER_COL: u32 = 0xD8EEFF;
const GUTTER_BG: u32 = 0x080E18;
const LNUM_COL: u32 = 0x2E4C68;
const LNUM_CUR: u32 = 0x4A7296;
const LINE_COL: u32 = 0xC0D8EC;
const CLIP_COL: u32 = 0x4A8AAA; // ">" clipped-line indicator
const TILDE_COL: u32 = 0x1E3448;
const BORDER_COL: u32 = 0x142030;
const STATUS_BG: u32 = 0x0A1220;
const STATUS_COL: u32 = 0x3A6080;
const STATUS_VAL: u32 = 0x5A8AAA;
const ERR_COL: u32 = 0xB04040;
const ERR_BG: u32 = 0x1A0A0A;
const EMPTY_COL: u32 = 0x2A4058;
const SCROLL_BG: u32 = 0x0A1018;
const SCROLL_FG: u32 = 0x224060;
const CURSOR_BLOCK: u32 = 0x2A5FAA; // block cursor background
const SAVE_OK_COL: u32 = 0x4CAF78; // "Saved" flash indicator
const DIRTY_COL: u32 = 0xE3B341; // modified indicator
const EDIT_BADGE_T: u32 = 0xC8E8FF; // [EDIT] badge / cursor-line text
const PROMPT_BG: u32 = 0x0E1E32; // close-confirm overlay background
const PROMPT_BORDER: u32 = 0xE3B341; // amber border matching dirty indicator
const PROMPT_COL: u32 = 0xD8EEFF; // prompt text
const PROMPT_KEY: u32 = 0xE3B341; // key highlight in prompt

// ── Layout ────────────────────────────────────────────────────────────────────

const PAD_X: usize = 10;
const ROW_H: usize = 14;
const HEADER_H: usize = 24;
const STATUS_H: usize = 18;
const LNUM_W: usize = 34; // gutter for 4-digit number + space
const CHAR_W: usize = 6;
const SCROLL_W: usize = 6;

// ── Buffer limits ─────────────────────────────────────────────────────────────

const BUF_SIZE: usize = 8192;
const MAX_LINES: usize = 512;

// ── Error kind ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoadState {
    Ok,
    Empty,
    NotFound,
    ReadError,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorMode {
    View,
    Edit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClosePrompt {
    Hidden,
    Visible,
}

// ── EditorApp ──────────────────────────────────────────────────────────────────

pub struct EditorApp {
    buf: [u8; BUF_SIZE],
    buf_len: usize,
    lines: [usize; MAX_LINES],
    line_count: usize,
    scroll: usize,
    title_buf: [u8; 80],
    title_len: usize,
    path_buf: [u8; 128],
    path_len: usize,
    state: LoadState,
    // Edit mode
    mode: EditorMode,
    cursor_line: usize,
    cursor_col: usize,
    dirty: bool,
    writable: bool,
    saved_flash: bool, // briefly show "Saved" after Ctrl+S
    close_prompt: ClosePrompt,
}

include!("editor/core.rs");
include!("editor/app_impl.rs");
include!("editor/helpers.rs");
