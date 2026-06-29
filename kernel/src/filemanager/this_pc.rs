// ── This PC view ──────────────────────────────────────────────────────────────

impl FileManagerApp {
    fn render_this_pc(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        framebuffer::fill_rect(cx, cy, cw, ch, THIS_PC_BG);

        // Header
        framebuffer::fill_rect(cx, cy, cw, HEADER_H, PATH_BG);
        framebuffer::fill_rect(cx, cy + HEADER_H - 1, cw, 1, BORDER_COL);
        let hdr_ty = cy + (HEADER_H - 8) / 2;
        framebuffer::draw_text_at(cx + PAD_X, hdr_ty, "This PC", TILE_NAME);

        // Section label
        let sec_y = cy + HEADER_H + 8;
        framebuffer::draw_text_at(cx + PAD_X, sec_y, "Devices and drives", TILE_SUB);

        // Drive tile(s)
        let mounted = crate::fat32::is_mounted();
        let (used_kb, total_kb) = if mounted {
            crate::fat32::disk_space_kb()
        } else {
            (0, 0)
        };
        let (tx, ty_t, tw, th) = Self::tile_rect(0, cw);
        let tx = cx + tx;
        let ty_t = cy + ty_t;
        let bg = if self.tile_sel == Some(0) {
            TILE_SEL
        } else if self.tile_hover == Some(0) {
            TILE_HOV
        } else {
            TILE_BG
        };
        // Outer border
        framebuffer::fill_rect(tx, ty_t, tw, th, TILE_BORD);
        // Inner background
        framebuffer::fill_rect(tx + 1, ty_t + 1, tw - 2, th - 2, bg);

        // Drive icon area (left strip)
        let icon_x = tx + 10;
        let icon_y = ty_t + th / 2 - 10;
        framebuffer::fill_rect(icon_x, icon_y, 20, 20, 0x1A3A60);
        framebuffer::fill_rect(icon_x + 2, icon_y + 2, 16, 16, 0x2A5090);
        framebuffer::draw_text_at(icon_x + 4, icon_y + 6, "C:", 0x88BBEE);

        // Drive name and type
        let name_x = tx + 38;
        let (drive_name, drive_label) = if mounted {
            ("Local Disk (C:)", "FAT32")
        } else {
            ("Local Disk (C:)", "Not mounted")
        };
        framebuffer::draw_text_at(name_x, ty_t + 10, drive_name, TILE_NAME);
        framebuffer::draw_text_at(name_x, ty_t + 22, drive_label, TILE_SUB);

        // Space bar
        if mounted && total_kb > 0 {
            let bar_x = name_x;
            let bar_y = ty_t + 38;
            let bar_w = tw.saturating_sub(name_x - tx + 10);
            let bar_h = 8usize;
            // Background
            framebuffer::fill_rect(bar_x, bar_y, bar_w, bar_h, TILE_BAR);
            // Used fill
            let fill_w = ((used_kb * bar_w as u64) / total_kb.max(1)) as usize;
            if fill_w > 0 {
                framebuffer::fill_rect(bar_x, bar_y, fill_w.min(bar_w), bar_h, TILE_USED);
            }
            // Space text
            let mut ubuf = [0u8; 24];
            let used_mb = used_kb / 1024;
            let total_mb = total_kb / 1024;
            let ulen = fmt_uint_u64(&mut ubuf, 0, used_mb);
            let suf = b" MB used of ";
            let end = (ulen + suf.len()).min(ubuf.len());
            ubuf[ulen..end].copy_from_slice(&suf[..end - ulen]);
            let tlen = end;
            let tstart = tlen;
            let tlen2 = fmt_uint_u64(&mut ubuf, tstart, total_mb);
            let suf2 = b" MB";
            let end2 = (tlen2 + suf2.len()).min(ubuf.len());
            ubuf[tlen2..end2].copy_from_slice(&suf2[..end2 - tlen2]);
            let space_str = core::str::from_utf8(&ubuf[..end2]).unwrap_or("");
            framebuffer::draw_text_at(bar_x, bar_y + 11, space_str, TILE_SUB);
        } else if !mounted {
            framebuffer::draw_text_at(name_x, ty_t + 44, "No disk mounted", TILE_SUB);
        }

        // Hint bar
        let hint_y = cy + ch.saturating_sub(HINT_H);
        framebuffer::fill_rect(cx, hint_y, cw, HINT_H, HEADER_BG);
        framebuffer::fill_rect(cx, hint_y, cw, 1, BORDER_COL);
        let hty = hint_y + (HINT_H - 8) / 2;
        framebuffer::draw_text_at(cx + PAD_X, hty, "Enter=open drive   Esc=close", HINT_KEY);
    }

    fn key_this_pc(&mut self, key: Key) -> AppAction {
        match key {
            Key::Escape => AppAction::Nothing,
            Key::Enter | Key::Char(b' ') => {
                if crate::fat32::is_mounted() {
                    self.view = FmView::Files;
                    self.load_dir();
                    return AppAction::RedrawAll;
                }
                AppAction::Nothing
            }
            _ => AppAction::Nothing,
        }
    }

    fn mouse_click_this_pc(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        let (tx, ty_t, tw, th) = Self::tile_rect(0, self.preferred_size().0);
        if rel_x >= tx as i32
            && rel_x < (tx + tw) as i32
            && rel_y >= ty_t as i32
            && rel_y < (ty_t + th) as i32
        {
            let now = uptime_ms();
            let is_dbl = now.saturating_sub(self.tile_last_click_ms) < DBL_CLICK_MS;
            self.tile_last_click_ms = now;
            self.tile_sel = Some(0);
            if is_dbl && crate::fat32::is_mounted() {
                self.view = FmView::Files;
                self.load_dir();
            }
            return AppAction::RedrawAll;
        }
        // Click outside tile — deselect
        if self.tile_sel.is_some() {
            self.tile_sel = None;
            return AppAction::RedrawAll;
        }
        AppAction::Nothing
    }

    fn mouse_move_this_pc(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        let (tx, ty_t, tw, th) = Self::tile_rect(0, self.preferred_size().0);
        let new_hover = if rel_x >= tx as i32
            && rel_x < (tx + tw) as i32
            && rel_y >= ty_t as i32
            && rel_y < (ty_t + th) as i32
        {
            Some(0)
        } else {
            None
        };
        if new_hover != self.tile_hover {
            self.tile_hover = new_hover;
            return AppAction::RedrawAll;
        }
        AppAction::Nothing
    }
}
