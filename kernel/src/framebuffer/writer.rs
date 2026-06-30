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

