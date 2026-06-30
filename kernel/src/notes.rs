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
use crate::fs;
use crate::input::Key;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG: u32 = 0x08100A;
const HEADER_BG: u32 = 0x0C1A0E;
const HEADER_COL: u32 = 0xA8D8B0;
const BORDER_COL: u32 = 0x1A3020;
const TEXT_COL: u32 = 0xC8E8D0;
const CURSOR_COL: u32 = 0x40E860;
const LINE_NUM: u32 = 0x2A4830;
const STATUS_BG: u32 = 0x0C1A0E;
const STATUS_COL: u32 = 0x3A6040;
const STATUS_VAL: u32 = 0x60A070;
const DIRTY_COL: u32 = 0xE3B341;
const SAVED_COL: u32 = 0x40E860;

// ── Layout ────────────────────────────────────────────────────────────────────

const HEADER_H: usize = 22;
const STATUS_H: usize = 16;
const LNUM_W: usize = 28;
const PAD_X: usize = 8;
const ROW_H: usize = 13;
const CHAR_W: usize = 6;

// ── Limits ────────────────────────────────────────────────────────────────────

const BUF_CAP: usize = 16 * 1024; // 16 KiB
const MAX_LINES: usize = 1024;

/// Fallback dynamic VFS path used if FAT32 is not available.
const NOTES_DYN: &str = "/notes.txt";

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq)]
enum SaveState {
    Clean,
    Dirty,
    JustSaved,
}

// ── NotesApp ──────────────────────────────────────────────────────────────────

pub struct NotesApp {
    buf: [u8; BUF_CAP],
    buf_len: usize,
    cursor: usize,             // byte offset
    scroll: usize,             // first visible line index
    lines: [usize; MAX_LINES], // byte offsets of line starts
    line_count: usize,
    save_state: SaveState,
    flash_ticks: u8, // countdown for "Saved" flash
}

include!("notes/core.rs");
include!("notes/app_impl.rs");
include!("notes/formatting.rs");
