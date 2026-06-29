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

// ── Internal writer ───────────────────────────────────────────────────────────

struct FramebufferWriter {
    enabled: bool,
    addr: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    bpp: usize,
    red_mask_shift: u8,
    red_mask_size: u8,
    green_mask_shift: u8,
    green_mask_size: u8,
    blue_mask_shift: u8,
    blue_mask_size: u8,
    cursor_x: usize,
    cursor_y: usize,
    backbuffer: Vec<u32>,
    // Scissor clip rect — set per damage-rect in compose_damage, zero overhead.
    sc_active: bool,
    sc_x0: usize,
    sc_y0: usize,
    sc_x1: usize,
    sc_y1: usize,
}

unsafe impl Send for FramebufferWriter {}

impl FramebufferWriter {
    const fn new() -> Self {
        Self {
            enabled: false,
            addr: core::ptr::null_mut(),
            width: 0,
            height: 0,
            pitch: 0,
            bpp: 0,
            red_mask_shift: 0,
            red_mask_size: 0,
            green_mask_shift: 0,
            green_mask_size: 0,
            blue_mask_shift: 0,
            blue_mask_size: 0,
            cursor_x: 0,
            cursor_y: 0,
            backbuffer: Vec::new(),
            sc_active: false,
            sc_x0: 0,
            sc_y0: 0,
            sc_x1: 0,
            sc_y1: 0,
        }
    }

    #[allow(dead_code)]
    fn init(&mut self, info: crate::boot::protocol::FramebufferInfo) -> bool {
        if info.addr.is_null() || info.bpp != 32 {
            self.enabled = false;
            return false;
        }
        if info.width == 0 || info.height == 0 || info.pitch == 0 {
            self.enabled = false;
            return false;
        }
        if info.pitch < info.width * 4 {
            self.enabled = false;
            return false;
        }
        if info.red_mask_shift >= 32
            || info.green_mask_shift >= 32
            || info.blue_mask_shift >= 32
            || info.red_mask_size > 8
            || info.green_mask_size > 8
            || info.blue_mask_size > 8
        {
            self.enabled = false;
            return false;
        }

        self.addr = info.addr;
        self.width = info.width as usize;
        self.height = info.height as usize;
        self.pitch = info.pitch as usize;
        self.bpp = info.bpp as usize;
        self.red_mask_shift = info.red_mask_shift;
        self.red_mask_size = info.red_mask_size;
        self.green_mask_shift = info.green_mask_shift;
        self.green_mask_size = info.green_mask_size;
        self.blue_mask_shift = info.blue_mask_shift;
        self.blue_mask_size = info.blue_mask_size;
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.backbuffer = vec![0u32; self.width * self.height];
        self.enabled = true;
        true
    }

    fn ensure_initialized(&mut self) -> bool {
        if self.enabled {
            return true;
        }
        let Some(info) = crate::boot::protocol::framebuffer_info() else {
            return false;
        };
        self.init(info)
    }

    // ── Backbuffer pixel write ────────────────────────────────────────────

    #[inline(always)]
    fn scissor_clip(
        &self,
        x: usize,
        y: usize,
        x_end: usize,
        y_end: usize,
    ) -> (usize, usize, usize, usize) {
        if self.sc_active {
            (
                x.max(self.sc_x0),
                x_end.min(self.sc_x1).min(self.width),
                y.max(self.sc_y0),
                y_end.min(self.sc_y1).min(self.height),
            )
        } else {
            (x, x_end.min(self.width), y, y_end.min(self.height))
        }
    }

    fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        if self.sc_active
            && (x < self.sc_x0 || x >= self.sc_x1 || y < self.sc_y0 || y >= self.sc_y1)
        {
            return;
        }
        let idx = y * self.width + x;
        if idx < self.backbuffer.len() {
            self.backbuffer[idx] = color;
        }
    }

    // ── Hardware volatile write ───────────────────────────────────────────

    fn put_pixel_front(&self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let byte_offset = y * self.pitch + x * (self.bpp / 8);
        unsafe {
            let ptr = self.addr.add(byte_offset).cast::<u32>();
            ptr.write_volatile(color);
        }
    }

    // ── Present (backbuffer → hardware) ───────────────────────────────────

    fn present_region(&self, x: usize, y: usize, w: usize, h: usize) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        let row_px = x_end.saturating_sub(x);
        if row_px == 0 || y >= y_end {
            return;
        }

        // Backbuffer stores hardware-format pixels (color_from_rgb24 is applied
        // at fill_rect time), so we can copy each scanline directly as raw bytes
        // instead of writing pixel-by-pixel via put_pixel_front.
        // This avoids per-pixel bounds checks, pointer arithmetic, and volatile
        // write overhead — especially important for wide damage rects.
        let bytes_per_px = self.bpp / 8; // 4 for 32-bpp
        let row_bytes = row_px * bytes_per_px;

        for py in y..y_end {
            let src_idx = py * self.width + x;
            if src_idx + row_px > self.backbuffer.len() {
                break;
            }
            let dst_byte_offset = py * self.pitch + x * bytes_per_px;
            unsafe {
                let src = self.backbuffer.as_ptr().add(src_idx) as *const u8;
                let dst = self.addr.add(dst_byte_offset);
                core::ptr::copy_nonoverlapping(src, dst, row_bytes);
            }
        }
    }

    // ── Boot console (auto-presents) ──────────────────────────────────────

    fn write_str(&mut self, text: &str) {
        if !self.enabled {
            return;
        }
        for ch in text.chars() {
            if ch == '\n' {
                self.newline();
                continue;
            }
            self.write_char(ch);
        }
    }

    fn write_char(&mut self, ch: char) {
        if !self.enabled {
            return;
        }
        let char_step = GLYPH_WIDTH + H_SPACING;
        if self.cursor_x + char_step >= self.width {
            self.newline();
        }
        if self.cursor_y + GLYPH_HEIGHT + V_SPACING >= self.height {
            self.clear_screen();
            self.cursor_x = 0;
            self.cursor_y = 0;
        }
        let x = self.cursor_x;
        let y = self.cursor_y;
        self.draw_glyph(x, y, glyph_for(ch));
        // Boot console: present immediately so text is visible during boot
        self.present_region(x, y, GLYPH_WIDTH, GLYPH_HEIGHT);
        self.cursor_x += char_step;
    }

    fn newline(&mut self) {
        if !self.enabled {
            return;
        }
        self.cursor_x = 0;
        self.cursor_y += GLYPH_HEIGHT + V_SPACING;
        if self.cursor_y + GLYPH_HEIGHT + V_SPACING >= self.height {
            self.clear_screen();
            self.cursor_y = 0;
        }
    }

    fn clear_screen(&mut self) {
        let bg = self.rgb(0x00, 0x00, 0x00);
        for y in 0..self.height {
            for x in 0..self.width {
                self.put_pixel(x, y, bg);
            }
        }
        self.present_region(0, 0, self.width, self.height);
    }

    // ── Glyph rendering (to backbuffer only) ──────────────────────────────

    fn draw_glyph(&mut self, x: usize, y: usize, glyph: [u8; GLYPH_HEIGHT]) {
        let fg = self.rgb(0xd8, 0xd8, 0xd8);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..GLYPH_WIDTH {
                if bits & (1 << (GLYPH_WIDTH - 1 - col)) != 0 {
                    self.put_pixel(x + col, y + row, fg);
                }
            }
        }
    }

    fn draw_glyph_color(&mut self, x: usize, y: usize, glyph: [u8; GLYPH_HEIGHT], color: u32) {
        for (row, bits) in glyph.iter().enumerate() {
            let py = y + row;
            // Skip rows outside the scissor before testing individual bits.
            if self.sc_active && (py < self.sc_y0 || py >= self.sc_y1) {
                continue;
            }
            for col in 0..GLYPH_WIDTH {
                if bits & (1 << (GLYPH_WIDTH - 1 - col)) != 0 {
                    self.put_pixel(x + col, py, color);
                }
            }
        }
    }

    // ── Text drawing (to backbuffer, no present) ──────────────────────────

    fn draw_text_at(&mut self, x: usize, y: usize, text: &str, color_rgb: u32) -> bool {
        if !self.enabled {
            return false;
        }
        let fg = self.color_from_rgb24(color_rgb);
        let mut cx = x;
        let mut cy = y;
        let step_x = GLYPH_WIDTH + H_SPACING;
        let step_y = GLYPH_HEIGHT + V_SPACING;

        for ch in text.chars() {
            if ch == '\n' {
                cx = x;
                cy = cy.saturating_add(step_y);
                continue;
            }
            if cx.saturating_add(GLYPH_WIDTH) >= self.width {
                cx = x;
                cy = cy.saturating_add(step_y);
            }
            if cy.saturating_add(GLYPH_HEIGHT) >= self.height {
                break;
            }
            self.draw_glyph_color(cx, cy, glyph_for(ch), fg);
            cx = cx.saturating_add(step_x);
        }
        true
    }

    fn draw_text_scaled(
        &mut self,
        x: usize,
        y: usize,
        text: &str,
        color_rgb: u32,
        scale: usize,
    ) -> usize {
        let color = self.color_from_rgb24(color_rgb);
        let char_step = (GLYPH_WIDTH + H_SPACING) * scale;
        let glyph_w = GLYPH_WIDTH * scale;
        let glyph_h = GLYPH_HEIGHT * scale;
        let mut cx = x;
        for ch in text.chars() {
            if cx + glyph_w >= self.width {
                break;
            }
            if y + glyph_h >= self.height {
                break;
            }
            // Skip character entirely if outside scissor.
            if self.sc_active
                && (cx + glyph_w <= self.sc_x0
                    || cx >= self.sc_x1
                    || y + glyph_h <= self.sc_y0
                    || y >= self.sc_y1)
            {
                cx += char_step;
                continue;
            }
            let glyph = glyph_for(ch);
            for (row, bits) in glyph.iter().enumerate() {
                let py = y + row * scale;
                // Skip entire glyph row if outside scissor Y range.
                if self.sc_active && (py + scale <= self.sc_y0 || py >= self.sc_y1) {
                    continue;
                }
                for col in 0..GLYPH_WIDTH {
                    if bits & (1 << (GLYPH_WIDTH - 1 - col)) != 0 {
                        let px = cx + col * scale;
                        for sy in 0..scale {
                            for sx in 0..scale {
                                self.put_pixel(px + sx, py + sy, color);
                            }
                        }
                    }
                }
            }
            cx += char_step;
        }
        cx
    }

    // ── Color helpers ─────────────────────────────────────────────────────

    fn color_from_rgb24(&self, color_rgb: u32) -> u32 {
        self.rgb(
            ((color_rgb >> 16) & 0xFF) as u8,
            ((color_rgb >> 8) & 0xFF) as u8,
            (color_rgb & 0xFF) as u8,
        )
    }

    fn rgb(&self, red: u8, green: u8, blue: u8) -> u32 {
        encode_channel(red, self.red_mask_size, self.red_mask_shift)
            | encode_channel(green, self.green_mask_size, self.green_mask_shift)
            | encode_channel(blue, self.blue_mask_size, self.blue_mask_shift)
    }
}

fn encode_channel(value: u8, size: u8, shift: u8) -> u32 {
    if size == 0 || size > 8 || shift >= 32 {
        return 0;
    }
    let Some(max_shifted) = 1u32.checked_shl(size as u32) else {
        return 0;
    };
    let max = max_shifted.saturating_sub(1);
    let normalized = (value as u32 * max) / 255;
    normalized.checked_shl(shift as u32).unwrap_or(0)
}

fn glyph_for(ch: char) -> [u8; GLYPH_HEIGHT] {
    match ch.to_ascii_lowercase() {
        'a' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'b' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'c' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'd' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'e' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'f' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'g' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'h' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'i' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'j' => [
            0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b10001, 0b01110,
        ],
        'k' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'l' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'm' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'n' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'o' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'p' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'r' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        's' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        't' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'u' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'v' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'w' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'x' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        ':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        ';' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b01000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b01000,
        ],
        '!' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        '?' => [
            0b01110, 0b10001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        '\'' => [
            0b00100, 0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '"' => [
            0b01010, 0b01010, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        '[' => [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
        ']' => [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '\\' => [
            0b10000, 0b01000, 0b01000, 0b00100, 0b00010, 0b00010, 0b00001,
        ],
        '=' => [
            0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000,
        ],
        '+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        '*' => [
            0b00000, 0b10101, 0b01110, 0b00100, 0b01110, 0b10101, 0b00000,
        ],
        '<' => [
            0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010,
        ],
        '>' => [
            0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000,
        ],
        '#' => [
            0b01010, 0b11111, 0b01010, 0b01010, 0b11111, 0b01010, 0b00000,
        ],
        '$' => [
            0b00100, 0b01111, 0b10100, 0b01110, 0b00101, 0b11110, 0b00100,
        ],
        '%' => [
            0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011,
        ],
        '&' => [
            0b01100, 0b10010, 0b01100, 0b01000, 0b10101, 0b10010, 0b01101,
        ],
        '@' => [
            0b01110, 0b10001, 0b10111, 0b10101, 0b10110, 0b10000, 0b01110,
        ],
        '^' => [
            0b00100, 0b01010, 0b10001, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '~' => [
            0b00000, 0b00000, 0b01000, 0b10101, 0b00010, 0b00000, 0b00000,
        ],
        '`' => [
            0b01000, 0b00100, 0b00010, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '{' => [
            0b00110, 0b00100, 0b00100, 0b01000, 0b00100, 0b00100, 0b00110,
        ],
        '}' => [
            0b01100, 0b00100, 0b00100, 0b00010, 0b00100, 0b00100, 0b01100,
        ],
        '|' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        ' ' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        // ── Math / special Unicode symbols used in built-in apps ──────────
        // ÷ (U+00F7) division sign: dot · line · dot
        '÷' => [
            0b00100, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000, 0b00100,
        ],
        // × (U+00D7) multiplication sign: diagonal cross
        '×' => [
            0b00000, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b00000,
        ],
        // − (U+2212) minus sign: same shape as ASCII hyphen
        '−' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        // ← (U+2190) left arrow: arrowhead + horizontal stem
        '←' => [
            0b00100, 0b01000, 0b11111, 0b01000, 0b00100, 0b00000, 0b00000,
        ],
        _ => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
    }
}
