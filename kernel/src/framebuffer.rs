use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

const GLYPH_WIDTH: usize = 5;
const GLYPH_HEIGHT: usize = 7;
const H_SPACING: usize = 1;
const V_SPACING: usize = 1;

static WRITER: Mutex<FramebufferWriter> = Mutex::new(FramebufferWriter::new());

// ── Scissor rect ─────────────────────────────────────────────────────────────
// Stored on FramebufferWriter (inside WRITER mutex) so fill_rect / put_pixel
// read it at zero extra locking cost — WRITER is already held during all draws.
// The public set_scissor/clear_scissor just lock WRITER and set the fields.

/// Restrict all subsequent backbuffer draws to the given rectangle.
/// Call `clear_scissor()` when done.
pub fn set_scissor(x: usize, y: usize, w: usize, h: usize) {
    let mut wr = WRITER.lock();
    wr.sc_active = true;
    wr.sc_x0 = x;
    wr.sc_y0 = y;
    wr.sc_x1 = x + w;
    wr.sc_y1 = y + h;
}

/// Remove the scissor restriction.
pub fn clear_scissor() {
    WRITER.lock().sc_active = false;
}

// ── Public API ────────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn init_from_boot() -> bool {
    let Some(info) = crate::boot::protocol::framebuffer_info() else {
        return false;
    };
    let mut writer = WRITER.lock();
    writer.init(info)
}

/// Returns true if the framebuffer is available.
pub fn ensure_ready() -> bool {
    let mut writer = WRITER.lock();
    writer.ensure_initialized()
}

/// Returns (width, height) or None.
pub fn dimensions() -> Option<(usize, usize)> {
    let writer = WRITER.lock();
    if writer.enabled {
        Some((writer.width, writer.height))
    } else {
        None
    }
}

/// Boot-console text line (auto-presents each character).
pub fn write_line(message: &str) {
    let mut writer = WRITER.lock();
    writer.write_str(message);
    writer.newline();
}

/// Draw 1x-scale text to backbuffer. Caller must present.
pub fn draw_text_at(x: usize, y: usize, text: &str, color_rgb: u32) -> bool {
    let mut writer = WRITER.lock();
    if !writer.ensure_initialized() {
        return false;
    }
    // Fast scissor bail-out: if the text row is entirely above or below the
    // scissor rect, skip all glyph work (avoids thousands of put_pixel calls
    // that would each return immediately after the scissor check).
    if writer.sc_active
        && (y + GLYPH_HEIGHT <= writer.sc_y0 || y >= writer.sc_y1 || x >= writer.sc_x1)
    {
        return true;
    }
    writer.draw_text_at(x, y, text, color_rgb)
}

/// Draw scaled text to backbuffer. Returns x after last char. Caller must present.
pub fn draw_text_scaled(x: usize, y: usize, text: &str, color_rgb: u32, scale: usize) -> usize {
    let mut writer = WRITER.lock();
    if !writer.ensure_initialized() || scale == 0 {
        return x;
    }
    // Fast scissor bail-out: skip all glyph work if the row is outside the
    // scissor rect entirely.
    if writer.sc_active {
        let glyph_h = GLYPH_HEIGHT * scale;
        if y + glyph_h <= writer.sc_y0 || y >= writer.sc_y1 || x >= writer.sc_x1 {
            return x;
        }
    }
    writer.draw_text_scaled(x, y, text, color_rgb, scale)
}

/// Fill entire screen in backbuffer. Caller must present.
pub fn clear(color_rgb: u32) {
    let mut writer = WRITER.lock();
    if !writer.ensure_initialized() {
        return;
    }
    let color = writer.color_from_rgb24(color_rgb);
    writer.backbuffer.fill(color);
}

/// Fill a rectangle in backbuffer. Caller must present.
pub fn fill_rect(x: usize, y: usize, w: usize, h: usize, color_rgb: u32) {
    let mut writer = WRITER.lock();
    if !writer.ensure_initialized() {
        return;
    }
    let color = writer.color_from_rgb24(color_rgb);
    let (x0, x1, y0, y1) = writer.scissor_clip(x, y, x + w, y + h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let width = writer.width;
    let buf_len = writer.backbuffer.len();
    for ry in y0..y1 {
        let row_start = ry * width + x0;
        let row_end = ry * width + x1;
        if row_end > buf_len {
            break;
        }
        writer.backbuffer[row_start..row_end].fill(color);
    }
}

/// Read a rectangle from the backbuffer into `dst` (row-major, w*h entries).
/// Returns the number of pixels actually read.
pub fn read_rect(x: usize, y: usize, w: usize, h: usize, dst: &mut [u32]) -> usize {
    let writer = WRITER.lock();
    if !writer.enabled || writer.backbuffer.is_empty() {
        return 0;
    }
    let x_end = (x + w).min(writer.width);
    let y_end = (y + h).min(writer.height);
    let row_px = x_end.saturating_sub(x);
    let width = writer.width;
    let buf_len = writer.backbuffer.len();
    let mut count = 0;
    for ry in y..y_end {
        let row_start = ry * width + x;
        let row_end = row_start + row_px;
        if row_end > buf_len {
            break;
        }
        let copy_n = row_px.min(dst.len().saturating_sub(count));
        if copy_n == 0 {
            break;
        }
        dst[count..count + copy_n]
            .copy_from_slice(&writer.backbuffer[row_start..row_start + copy_n]);
        count += copy_n;
    }
    count
}

/// Write a rectangle from `src` (row-major, w*h entries) into the backbuffer.
pub fn write_rect(x: usize, y: usize, w: usize, h: usize, src: &[u32]) {
    let mut writer = WRITER.lock();
    if !writer.enabled || writer.backbuffer.is_empty() {
        return;
    }
    let x_end = (x + w).min(writer.width);
    let y_end = (y + h).min(writer.height);
    let row_px = x_end.saturating_sub(x);
    let width = writer.width;
    let buf_len = writer.backbuffer.len();
    let mut src_off = 0usize;
    for ry in y..y_end {
        let row_start = ry * width + x;
        let row_end = row_start + row_px;
        if row_end > buf_len {
            break;
        }
        let copy_n = row_px.min(src.len().saturating_sub(src_off));
        if copy_n == 0 {
            break;
        }
        writer.backbuffer[row_start..row_start + copy_n]
            .copy_from_slice(&src[src_off..src_off + copy_n]);
        src_off += copy_n;
    }
}

/// Write a sub-region of `src` (row-major, total row width `src_w`) into the backbuffer.
/// Reads from (sub_x, sub_y) in src with size (w, h), writes to (dst_x, dst_y) in backbuffer.
pub fn write_rect_sub(
    dst_x: usize,
    dst_y: usize,
    w: usize,
    h: usize,
    src: &[u32],
    src_w: usize,
    sub_x: usize,
    sub_y: usize,
) {
    let mut writer = WRITER.lock();
    if !writer.enabled || writer.backbuffer.is_empty() {
        return;
    }
    let x_end = (dst_x + w).min(writer.width);
    let y_end = (dst_y + h).min(writer.height);
    let row_px = x_end.saturating_sub(dst_x);
    let width = writer.width;
    let buf_len = writer.backbuffer.len();
    for ry in dst_y..y_end {
        let src_row = sub_y + (ry - dst_y);
        let src_start = src_row * src_w + sub_x;
        let src_end = src_start + row_px;
        let dst_start = ry * width + dst_x;
        let dst_end = dst_start + row_px;
        if src_end > src.len() || dst_end > buf_len {
            break;
        }
        writer.backbuffer[dst_start..dst_end].copy_from_slice(&src[src_start..src_end]);
    }
}

/// Copy the entire backbuffer to the hardware framebuffer.
pub fn present_full() {
    let writer = WRITER.lock();
    if !writer.enabled || writer.backbuffer.is_empty() {
        return;
    }
    let (w, h) = (writer.width, writer.height);
    writer.present_region(0, 0, w, h);
}

/// Copy a rectangle from backbuffer to hardware.
pub fn present_rect(x: usize, y: usize, w: usize, h: usize) {
    let writer = WRITER.lock();
    if !writer.enabled || writer.backbuffer.is_empty() {
        return;
    }
    writer.present_region(x, y, w, h);
}

include!("framebuffer/writer.rs");
include!("framebuffer/glyphs.rs");
