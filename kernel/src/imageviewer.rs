// ---------------------------------------------------------------------------
// Astra OS — Image Viewer app
//
// Loads and displays PPM P6 (binary RGB) image files from the VFS / FAT32.
//
// PPM P6 format:
//   P6\n
//   <width> <height>\n
//   255\n
//   <width * height * 3 bytes of raw RGB>
//
// Controls:
//   +/-       zoom in / out (1× – 8×)
//   Arrow keys  pan image when zoomed
//   R         reset zoom and pan
//   Escape    clear image (return to welcome screen)
//
// Opened from the File Manager by double-clicking a .ppm file (the desktop
// compositor routes AppAction::OpenFile for .ppm paths here instead of the
// text editor).
// ---------------------------------------------------------------------------

extern crate alloc;
use alloc::vec::Vec;

use crate::app::{App, AppAction};
use crate::framebuffer;
use crate::fs;
use crate::input::Key;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG: u32 = 0x060A0F;
const HEADER_BG: u32 = 0x0A1220;
const HEADER_COL: u32 = 0xD8EEFF;
const STATUS_BG: u32 = 0x0A1220;
const STATUS_COL: u32 = 0x4A7090;
const STATUS_VAL: u32 = 0x7AA8C8;
const BORDER_COL: u32 = 0x1A2F48;
const ERR_COL: u32 = 0xB04040;
const GRID_A: u32 = 0x0E1520; // checkerboard dark
const GRID_B: u32 = 0x111C28; // checkerboard light
const HELP_COL: u32 = 0x2A4060;
const HELP_KEY_COL: u32 = 0x4A7090;

// ── Layout ────────────────────────────────────────────────────────────────────

const HEADER_H: usize = 24;
const STATUS_H: usize = 18;
const PAD: usize = 4;

// ── Limits ────────────────────────────────────────────────────────────────────

/// Maximum source image dimensions (256×256 = 196 608 bytes pixel data).
const MAX_W: usize = 256;
const MAX_H: usize = 256;
/// File read buffer.  Must be ≥ MAX_W * MAX_H * 3 + header.
const FILE_BUF: usize = MAX_W * MAX_H * 3 + 64;

// ── Load state ────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq)]
enum ViewState {
    Empty,
    Loaded,
    ParseError,
    ReadError,
    TooBig,
    NotPpm,
}

// ── ImageViewerApp ────────────────────────────────────────────────────────────

pub struct ImageViewerApp {
    /// Raw file bytes read from VFS.
    buf: Vec<u8>,
    /// Number of bytes actually used in `buf`.
    buf_used: usize,
    /// Decoded image width (0 if not loaded).
    img_w: usize,
    /// Decoded image height.
    img_h: usize,
    /// Byte offset within `buf` where the raw RGB pixel data begins.
    px_start: usize,
    /// Current zoom level (pixels per source pixel).
    zoom: usize,
    /// Horizontal pan offset in screen pixels (may be negative).
    pan_x: i32,
    /// Vertical pan offset in screen pixels.
    pan_y: i32,
    /// Load/parse state.
    state: ViewState,
    /// Window title.
    title_buf: [u8; 80],
    title_len: usize,
    /// File path (for status bar display).
    path_buf: [u8; 128],
    path_len: usize,
}

include!("imageviewer/core.rs");
include!("imageviewer/render.rs");
include!("imageviewer/parser.rs");
include!("imageviewer/formatting.rs");
include!("imageviewer/app_impl.rs");
