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
use crate::input::Key;
use crate::fs;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG:           u32 = 0x060A0F;
const HEADER_BG:    u32 = 0x0A1220;
const HEADER_COL:   u32 = 0xD8EEFF;
const STATUS_BG:    u32 = 0x0A1220;
const STATUS_COL:   u32 = 0x4A7090;
const STATUS_VAL:   u32 = 0x7AA8C8;
const BORDER_COL:   u32 = 0x1A2F48;
const ERR_COL:      u32 = 0xB04040;
const CANVAS_BG:    u32 = 0x0C1018;
const GRID_A:       u32 = 0x0E1520;  // checkerboard dark
const GRID_B:       u32 = 0x111C28;  // checkerboard light
const HELP_COL:     u32 = 0x2A4060;
const HELP_KEY_COL: u32 = 0x4A7090;

// ── Layout ────────────────────────────────────────────────────────────────────

const HEADER_H: usize = 24;
const STATUS_H: usize = 18;
const PAD:      usize = 4;

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
    buf:      Vec<u8>,
    /// Number of bytes actually used in `buf`.
    buf_used: usize,
    /// Decoded image width (0 if not loaded).
    img_w:    usize,
    /// Decoded image height.
    img_h:    usize,
    /// Byte offset within `buf` where the raw RGB pixel data begins.
    px_start: usize,
    /// Current zoom level (pixels per source pixel).
    zoom:     usize,
    /// Horizontal pan offset in screen pixels (may be negative).
    pan_x:    i32,
    /// Vertical pan offset in screen pixels.
    pan_y:    i32,
    /// Load/parse state.
    state:    ViewState,
    /// Window title.
    title_buf: [u8; 80],
    title_len: usize,
    /// File path (for status bar display).
    path_buf:  [u8; 128],
    path_len:  usize,
}

impl ImageViewerApp {
    pub fn new() -> Self {
        ImageViewerApp {
            buf:      Vec::new(),
            buf_used: 0,
            img_w:    0,
            img_h:    0,
            px_start: 0,
            zoom:     2,
            pan_x:    0,
            pan_y:    0,
            state:    ViewState::Empty,
            title_buf: [0u8; 80],
            title_len: 0,
            path_buf:  [0u8; 128],
            path_len:  0,
        }
    }

    pub fn open(path: &str) -> Self {
        let mut app = Self::new();
        app.load(path);
        app
    }

    // ── File loading ──────────────────────────────────────────────────────────

    fn load(&mut self, path: &str) {
        // Store path for display.
        let pb = path.as_bytes();
        let pn = pb.len().min(self.path_buf.len());
        self.path_buf[..pn].copy_from_slice(&pb[..pn]);
        self.path_len = pn;

        // Build window title from last path component.
        self.build_title(path);

        // Allocate / resize the file buffer.
        self.buf.resize(FILE_BUF, 0u8);

        // Read file.
        let mut handle = match fs::open(path) {
            Ok(h)  => h,
            Err(_) => { self.state = ViewState::ReadError; return; }
        };
        let n = match fs::read(&mut handle, &mut self.buf) {
            Ok(n)  => n,
            Err(_) => { self.state = ViewState::ReadError; return; }
        };
        self.buf_used = n;

        // Parse PPM P6 header.
        match parse_ppm_p6(&self.buf[..n]) {
            Some((w, h, px)) => {
                if w > MAX_W || h > MAX_H {
                    self.state = ViewState::TooBig;
                    return;
                }
                self.img_w    = w;
                self.img_h    = h;
                self.px_start = px;
                self.state    = ViewState::Loaded;
                self.zoom     = 2;
                self.pan_x    = 0;
                self.pan_y    = 0;
            }
            None => {
                // Check if it might be text (not a PPM at all).
                if n < 2 || self.buf[0] != b'P' || self.buf[1] != b'6' {
                    self.state = ViewState::NotPpm;
                } else {
                    self.state = ViewState::ParseError;
                }
            }
        }
    }

    fn build_title(&mut self, path: &str) {
        let name = path.rfind('/').map_or(path, |i| &path[i+1..]);
        let prefix = b"Viewer - ";
        let mut i = 0usize;
        for &b in prefix { if i < self.title_buf.len() { self.title_buf[i] = b; i += 1; } }
        for &b in name.as_bytes() { if i < self.title_buf.len() { self.title_buf[i] = b; i += 1; } }
        self.title_len = i;
    }

    // ── Rendering helpers ─────────────────────────────────────────────────────

    /// Draw a checkerboard background (indicates transparency / empty canvas).
    fn draw_checkerboard(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        let tile = 12usize;
        let mut y = 0usize;
        while y < ch {
            let mut x = 0usize;
            while x < cw {
                let col = if (x / tile + y / tile) % 2 == 0 { GRID_A } else { GRID_B };
                let pw = (cw - x).min(tile);
                let ph = (ch - y).min(tile);
                framebuffer::fill_rect(cx + x, cy + y, pw, ph, col);
                x += tile;
            }
            y += tile;
        }
    }

    fn draw_image(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        let buf = &self.buf[self.px_start..self.px_start + self.img_w * self.img_h * 3];
        let iw = self.img_w;
        let ih = self.img_h;
        let z  = self.zoom;

        // Image origin on screen (canvas-relative, may be negative).
        let origin_x = (cw as i32 / 2 - (iw * z) as i32 / 2) + self.pan_x;
        let origin_y = (ch as i32 / 2 - (ih * z) as i32 / 2) + self.pan_y;

        for py in 0..ih {
            for px in 0..iw {
                let sx = origin_x + (px * z) as i32;
                let sy = origin_y + (py * z) as i32;

                // Cull fully off-canvas pixels.
                if sx + z as i32 <= 0 || sx >= cw as i32 { continue; }
                if sy + z as i32 <= 0 || sy >= ch as i32 { continue; }

                let off = (py * iw + px) * 3;
                let r = buf[off]     as u32;
                let g = buf[off + 1] as u32;
                let b = buf[off + 2] as u32;
                let color = (r << 16) | (g << 8) | b;

                // Clip the fill_rect to canvas bounds.
                let fx  = sx.max(0) as usize;
                let fy  = sy.max(0) as usize;
                let fx2 = (sx + z as i32).min(cw as i32) as usize;
                let fy2 = (sy + z as i32).min(ch as i32) as usize;
                if fx2 > fx && fy2 > fy {
                    framebuffer::fill_rect(cx + fx, cy + fy, fx2 - fx, fy2 - fy, color);
                }
            }
        }
    }

    fn draw_header(&self, cx: usize, cy: usize, cw: usize) {
        framebuffer::fill_rect(cx, cy, cw, HEADER_H, HEADER_BG);
        framebuffer::fill_rect(cx, cy + HEADER_H - 1, cw, 1, BORDER_COL);

        let title = if self.title_len > 0 {
            core::str::from_utf8(&self.title_buf[..self.title_len]).unwrap_or("Image Viewer")
        } else { "Image Viewer" };
        framebuffer::draw_text_at(cx + PAD, cy + (HEADER_H - 8) / 2, title, HEADER_COL);
    }

    fn draw_status(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        let sy = cy + ch - STATUS_H;
        framebuffer::fill_rect(cx, sy, cw, STATUS_H, STATUS_BG);
        framebuffer::fill_rect(cx, sy, cw, 1, BORDER_COL);

        let path = core::str::from_utf8(&self.path_buf[..self.path_len]).unwrap_or("");
        if self.state == ViewState::Loaded {
            // "256×128  zoom:2×   /fat32/1a2b"
            let mut sbuf = [0u8; 128];
            let mut si = 0usize;
            // dimensions
            write_usize(&mut sbuf, &mut si, self.img_w);
            sbuf[si] = b'x'; si += 1;
            write_usize(&mut sbuf, &mut si, self.img_h);
            // zoom
            let zoom_label = b"  zoom:";
            for &b in zoom_label { if si < sbuf.len() { sbuf[si] = b; si += 1; } }
            write_usize(&mut sbuf, &mut si, self.zoom);
            sbuf[si] = b'x'; si += 1;
            // path
            let sep = b"   ";
            for &b in sep { if si < sbuf.len() { sbuf[si] = b; si += 1; } }
            for &b in path.as_bytes() { if si < sbuf.len() { sbuf[si] = b; si += 1; } }

            let stat_str = core::str::from_utf8(&sbuf[..si]).unwrap_or("");
            framebuffer::draw_text_at(cx + PAD, sy + (STATUS_H - 8) / 2, stat_str, STATUS_VAL);
        } else {
            let msg = match self.state {
                ViewState::ReadError  => "Error: could not read file",
                ViewState::ParseError => "Error: invalid PPM P6 data",
                ViewState::TooBig     => "Error: image too large (max 256×256)",
                ViewState::NotPpm     => "Error: not a PPM P6 file (.ppm with P6 header required)",
                _                     => "No image loaded",
            };
            let col = if self.state == ViewState::Empty { STATUS_COL } else { ERR_COL };
            framebuffer::draw_text_at(cx + PAD, sy + (STATUS_H - 8) / 2, msg, col);
        }
    }

    fn draw_welcome(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        let lines: &[&str] = &[
            "Image Viewer",
            "",
            "Open a .ppm (P6) file from the File Manager",
            "to view it here.",
            "",
            "+/-   zoom in / out",
            "Arrows  pan image",
            "R       reset view",
        ];
        let total_h = lines.len() * 16;
        let start_y = cy + ch.saturating_sub(total_h) / 2;
        for (i, line) in lines.iter().enumerate() {
            let col = if i == 0 { HEADER_COL } else if line.starts_with(|c: char| c.is_ascii_alphabetic() || c == '+' || c == '-') { HELP_KEY_COL } else { HELP_COL };
            let tx = cx + (cw.saturating_sub(line.len() * 6)) / 2;
            framebuffer::draw_text_at(tx, start_y + i * 16, line, col);
        }
    }

    // ── Zoom & pan ────────────────────────────────────────────────────────────

    fn zoom_in(&mut self) {
        if self.zoom < 8 { self.zoom += 1; }
    }

    fn zoom_out(&mut self) {
        if self.zoom > 1 { self.zoom -= 1; }
    }

    fn reset_view(&mut self) {
        self.zoom = 2;
        self.pan_x = 0;
        self.pan_y = 0;
    }
}

// ── PPM P6 parser ─────────────────────────────────────────────────────────────

/// Parse a PPM P6 header and return `Some((width, height, pixel_data_offset))`.
fn parse_ppm_p6(data: &[u8]) -> Option<(usize, usize, usize)> {
    let mut pos = 0usize;

    // Magic "P6"
    if data.len() < 3 || &data[..2] != b"P6" { return None; }
    pos += 2;

    // Skip whitespace after magic
    pos = skip_ws(data, pos);
    if pos >= data.len() { return None; }

    // Width
    let (w, p2) = parse_uint(data, pos)?;
    pos = skip_ws(data, p2);

    // Height
    let (h, p3) = parse_uint(data, pos)?;
    pos = skip_ws(data, p3);

    // Max value (must be 255)
    let (maxval, p4) = parse_uint(data, pos)?;
    if maxval != 255 { return None; }
    pos = p4;

    // Single whitespace after max value (required by spec)
    if pos >= data.len() { return None; }
    pos += 1;  // consume the single whitespace byte

    // Verify there's enough pixel data
    if w == 0 || h == 0 { return None; }
    let pixel_bytes = w * h * 3;
    if pos + pixel_bytes > data.len() { return None; }

    Some((w, h, pos))
}

fn skip_ws(data: &[u8], mut pos: usize) -> usize {
    while pos < data.len() {
        match data[pos] {
            b' ' | b'\t' | b'\r' | b'\n' => pos += 1,
            b'#' => {
                // Comment: skip to end of line
                while pos < data.len() && data[pos] != b'\n' { pos += 1; }
            }
            _ => break,
        }
    }
    pos
}

fn parse_uint(data: &[u8], mut pos: usize) -> Option<(usize, usize)> {
    if pos >= data.len() || !data[pos].is_ascii_digit() { return None; }
    let mut n = 0usize;
    while pos < data.len() && data[pos].is_ascii_digit() {
        n = n * 10 + (data[pos] - b'0') as usize;
        pos += 1;
    }
    Some((n, pos))
}

// ── Formatting helper ─────────────────────────────────────────────────────────

fn write_usize(buf: &mut [u8], pos: &mut usize, mut n: usize) {
    let start = *pos;
    if n == 0 {
        if *pos < buf.len() { buf[*pos] = b'0'; *pos += 1; }
        return;
    }
    let mut tmp = [0u8; 20];
    let mut ti = 0usize;
    while n > 0 { tmp[ti] = b'0' + (n % 10) as u8; ti += 1; n /= 10; }
    let end = *pos + ti;
    if end > buf.len() { return; }
    for i in 0..ti { buf[start + i] = tmp[ti - 1 - i]; }
    *pos = end;
}

// ── App trait ─────────────────────────────────────────────────────────────────

impl App for ImageViewerApp {
    fn title(&self) -> &str {
        if self.title_len > 0 {
            core::str::from_utf8(&self.title_buf[..self.title_len]).unwrap_or("Image Viewer")
        } else {
            "Image Viewer"
        }
    }

    fn app_id(&self) -> &'static str { "imageviewer" }

    fn preferred_size(&self) -> (usize, usize) { (640, 480) }

    fn allow_multiple_instances(&self) -> bool { true }

    fn refresh_interval_ms(&self) -> Option<u64> { None }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        // Header bar
        self.draw_header(cx, cy, cw);

        // Canvas area (below header, above status)
        let canvas_y = cy + HEADER_H;
        let canvas_h = ch.saturating_sub(HEADER_H + STATUS_H);

        if self.state == ViewState::Loaded {
            self.draw_checkerboard(cx, canvas_y, cw, canvas_h);
            self.draw_image(cx, canvas_y, cw, canvas_h);
        } else {
            framebuffer::fill_rect(cx, canvas_y, cw, canvas_h, BG);
            self.draw_welcome(cx, canvas_y, cw, canvas_h);
        }

        // Status bar
        self.draw_status(cx, cy, cw, ch);
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        match key {
            Key::Char(b'+') | Key::Char(b'=') => {
                if self.state == ViewState::Loaded { self.zoom_in(); AppAction::RedrawAll }
                else { AppAction::Nothing }
            }
            Key::Char(b'-') | Key::Char(b'_') => {
                if self.state == ViewState::Loaded { self.zoom_out(); AppAction::RedrawAll }
                else { AppAction::Nothing }
            }
            Key::Char(b'r') | Key::Char(b'R') => {
                if self.state == ViewState::Loaded { self.reset_view(); AppAction::RedrawAll }
                else { AppAction::Nothing }
            }
            Key::ArrowLeft => {
                if self.state == ViewState::Loaded {
                    self.pan_x -= 16; AppAction::RedrawAll
                } else { AppAction::Nothing }
            }
            Key::ArrowRight => {
                if self.state == ViewState::Loaded {
                    self.pan_x += 16; AppAction::RedrawAll
                } else { AppAction::Nothing }
            }
            Key::ArrowUp => {
                if self.state == ViewState::Loaded {
                    self.pan_y -= 16; AppAction::RedrawAll
                } else { AppAction::Nothing }
            }
            Key::ArrowDown => {
                if self.state == ViewState::Loaded {
                    self.pan_y += 16; AppAction::RedrawAll
                } else { AppAction::Nothing }
            }
            Key::Escape => {
                self.state = ViewState::Empty;
                self.title_len = 0;
                self.path_len = 0;
                AppAction::RedrawAll
            }
            _ => AppAction::Nothing,
        }
    }
}
