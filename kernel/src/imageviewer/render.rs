impl ImageViewerApp {
    /// Draw a checkerboard background (indicates transparency / empty canvas).
    fn draw_checkerboard(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        let tile = 12usize;
        let mut y = 0usize;
        while y < ch {
            let mut x = 0usize;
            while x < cw {
                let col = if (x / tile + y / tile) % 2 == 0 {
                    GRID_A
                } else {
                    GRID_B
                };
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
        let z = self.zoom;

        // Image origin on screen (canvas-relative, may be negative).
        let origin_x = (cw as i32 / 2 - (iw * z) as i32 / 2) + self.pan_x;
        let origin_y = (ch as i32 / 2 - (ih * z) as i32 / 2) + self.pan_y;

        for py in 0..ih {
            for px in 0..iw {
                let sx = origin_x + (px * z) as i32;
                let sy = origin_y + (py * z) as i32;

                // Cull fully off-canvas pixels.
                if sx + z as i32 <= 0 || sx >= cw as i32 {
                    continue;
                }
                if sy + z as i32 <= 0 || sy >= ch as i32 {
                    continue;
                }

                let off = (py * iw + px) * 3;
                let r = buf[off] as u32;
                let g = buf[off + 1] as u32;
                let b = buf[off + 2] as u32;
                let color = (r << 16) | (g << 8) | b;

                // Clip the fill_rect to canvas bounds.
                let fx = sx.max(0) as usize;
                let fy = sy.max(0) as usize;
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
        } else {
            "Image Viewer"
        };
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
            sbuf[si] = b'x';
            si += 1;
            write_usize(&mut sbuf, &mut si, self.img_h);
            // zoom
            let zoom_label = b"  zoom:";
            for &b in zoom_label {
                if si < sbuf.len() {
                    sbuf[si] = b;
                    si += 1;
                }
            }
            write_usize(&mut sbuf, &mut si, self.zoom);
            sbuf[si] = b'x';
            si += 1;
            // path
            let sep = b"   ";
            for &b in sep {
                if si < sbuf.len() {
                    sbuf[si] = b;
                    si += 1;
                }
            }
            for &b in path.as_bytes() {
                if si < sbuf.len() {
                    sbuf[si] = b;
                    si += 1;
                }
            }

            let stat_str = core::str::from_utf8(&sbuf[..si]).unwrap_or("");
            framebuffer::draw_text_at(cx + PAD, sy + (STATUS_H - 8) / 2, stat_str, STATUS_VAL);
        } else {
            let msg = match self.state {
                ViewState::ReadError => "Error: could not read file",
                ViewState::ParseError => "Error: invalid PPM P6 data",
                ViewState::TooBig => "Error: image too large (max 256×256)",
                ViewState::NotPpm => "Error: not a PPM P6 file (.ppm with P6 header required)",
                _ => "No image loaded",
            };
            let col = if self.state == ViewState::Empty {
                STATUS_COL
            } else {
                ERR_COL
            };
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
            let col = if i == 0 {
                HEADER_COL
            } else if line.starts_with(|c: char| c.is_ascii_alphabetic() || c == '+' || c == '-') {
                HELP_KEY_COL
            } else {
                HELP_COL
            };
            let tx = cx + (cw.saturating_sub(line.len() * 6)) / 2;
            framebuffer::draw_text_at(tx, start_y + i * 16, line, col);
        }
    }

    // ── Zoom & pan ────────────────────────────────────────────────────────────

    fn zoom_in(&mut self) {
        if self.zoom < 8 {
            self.zoom += 1;
        }
    }

    fn zoom_out(&mut self) {
        if self.zoom > 1 {
            self.zoom -= 1;
        }
    }

    fn reset_view(&mut self) {
        self.zoom = 2;
        self.pan_x = 0;
        self.pan_y = 0;
    }
}

