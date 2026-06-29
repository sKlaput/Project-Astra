impl Desktop {
    // ── Rendering ─────────────────────────────────────────────────────────

    fn render_taskbar(&self) {
        framebuffer::fill_rect(0, 0, self.sw, BAR_H, BAR_BG);
        framebuffer::fill_rect(0, BAR_H - 1, self.sw, 1, BAR_BORDER);

        let btns: [&str; 2] = [
            "Desktop",
            if self.launcher_open {
                "Apps v"
            } else {
                "Apps >"
            },
        ];
        for (i, label) in btns.iter().enumerate() {
            let r = taskbar_btn_rect(i);
            let bg = if i == 1 && self.launcher_open {
                BAR_BTN_ACT
            } else if self.taskbar_hover == Some(i) {
                BAR_BTN_HOV
            } else {
                BAR_BTN_BG
            };
            framebuffer::fill_rect(r.x, r.y, r.w, r.h, bg);
            framebuffer::fill_rect(r.x, r.y, r.w, 1, BAR_BORDER);
            framebuffer::fill_rect(r.x, r.y + r.h - 1, r.w, 1, BAR_BORDER);
            framebuffer::fill_rect(r.x, r.y, 1, r.h, BAR_BORDER);
            framebuffer::fill_rect(r.x + r.w - 1, r.y, 1, r.h, BAR_BORDER);
            framebuffer::draw_text_at(
                r.x + 6,
                r.y + (r.h.saturating_sub(8)) / 2,
                label,
                BAR_BTN_TEXT,
            );
        }

        let mut wi = 0;
        for (idx, win) in self.windows.iter().enumerate() {
            if win.minimized {
                continue;
            }
            let r = taskbar_btn_rect(FIXED_BTNS + wi);
            let is_focused = self.focused == Some(idx);
            let bg = if is_focused {
                BAR_BTN_ACT
            } else if self.taskbar_hover == Some(FIXED_BTNS + wi) {
                BAR_BTN_HOV
            } else {
                BAR_BTN_BG
            };
            framebuffer::fill_rect(r.x, r.y, r.w, r.h, bg);
            framebuffer::fill_rect(r.x, r.y, r.w, 1, BAR_BORDER);
            framebuffer::fill_rect(r.x, r.y + r.h - 1, r.w, 1, BAR_BORDER);
            framebuffer::fill_rect(r.x, r.y, 1, r.h, BAR_BORDER);
            framebuffer::fill_rect(r.x + r.w - 1, r.y, 1, r.h, BAR_BORDER);
            let title = win.app.title();
            let max_chars = (r.w.saturating_sub(12)) / 6;
            let disp = if title.len() > max_chars {
                &title[..max_chars]
            } else {
                title
            };
            framebuffer::draw_text_at(
                r.x + 6,
                r.y + (r.h.saturating_sub(8)) / 2,
                disp,
                BAR_BTN_TEXT,
            );
            wi += 1;
        }

        // Taskbar clock — real time from RTC, uptime as secondary.
        {
            let (rh, rm, rs) = crate::rtc::read_time();
            let mut cbuf = [0u8; 24];
            let clen = fmt_hms(&mut cbuf, rh as u64, rm as u64, rs as u64);
            let clock_str = core::str::from_utf8(&cbuf[..clen]).unwrap_or("");

            // Uptime secondary "+H:MM:SS"
            let ms = uptime_ms();
            let us = ms / 1000;
            let um = us / 60;
            let uh = um / 60;
            let mut ubuf = [0u8; 24];
            let mut tmp = [0u8; 24];
            ubuf[0] = b'+';
            let tlen = fmt_hms(&mut tmp, uh, um % 60, us % 60);
            ubuf[1..1 + tlen].copy_from_slice(&tmp[..tlen]);
            let ulen = 1 + tlen;
            let up_str = core::str::from_utf8(&ubuf[..ulen]).unwrap_or("");

            let clock_w = clock_str.len() * 6 + 4;
            let up_w = up_str.len() * 6 + 4;
            let total_w = clock_w + up_w + 8;
            let pb_r = power_btn_rect(self.sw);
            let right_x = pb_r.x.saturating_sub(4);
            let clock_x = right_x.saturating_sub(total_w);
            framebuffer::draw_text_at(clock_x, 11, clock_str, BAR_TEXT);
            framebuffer::draw_text_at(clock_x + clock_w + 4, 11, up_str, BAR_UPTIME);
        }

        // Power button — far right, left of clock
        let pb = power_btn_rect(self.sw);
        let pb_bg = if self.taskbar_hover == Some(usize::MAX) {
            0x5A1010
        } else {
            0x2A0A0A
        };
        framebuffer::fill_rect(pb.x, pb.y, pb.w, pb.h, pb_bg);
        framebuffer::fill_rect(pb.x, pb.y, pb.w, 1, BAR_BORDER);
        framebuffer::fill_rect(pb.x, pb.y + pb.h - 1, pb.w, 1, BAR_BORDER);
        framebuffer::fill_rect(pb.x, pb.y, 1, pb.h, BAR_BORDER);
        framebuffer::fill_rect(pb.x + pb.w - 1, pb.y, 1, pb.h, BAR_BORDER);
        framebuffer::draw_text_at(
            pb.x + (pb.w.saturating_sub(6)) / 2,
            pb.y + (pb.h.saturating_sub(8)) / 2,
            "U",
            0xE05050,
        );
    }

    fn render_icon(&self, i: usize) {
        let r = icon_rect_of(&self.icons[i]);
        if self.launcher_open && r.x + r.w <= LAUNCHER_W {
            return;
        }
        let sel = self.icons[i].selected;
        let hov = self.icon_hover == Some(i);
        let bg = if sel {
            ICON_SEL
        } else if hov {
            0x131C28
        } else {
            ICON_BG
        };
        let border = if sel { ICON_BORDER } else { 0x181E28 };
        framebuffer::fill_rect(r.x, r.y, r.w, r.h, bg);
        framebuffer::fill_rect(r.x, r.y, r.w, 1, border);
        framebuffer::fill_rect(r.x, r.y + r.h - 1, r.w, 1, border);
        framebuffer::fill_rect(r.x, r.y, 1, r.h, border);
        framebuffer::fill_rect(r.x + r.w - 1, r.y, 1, r.h, border);
        framebuffer::fill_rect(r.x + 1, r.y + 1, r.w - 2, 4, ICON_ACCENT);
        draw_app_icon(i, r);
        let label = APP_REGISTRY[i].label;
        let tx = r.x + (r.w.saturating_sub(label.len() * 6)) / 2;
        let ty = r.y + r.h / 2 + 4;
        let col = if sel { ICON_TEXT_SEL } else { ICON_TEXT };
        framebuffer::draw_text_at(tx, ty, label, col);
        let sub = APP_REGISTRY[i].icon_sub;
        let sx = r.x + (r.w.saturating_sub(sub.len() * 6)) / 2;
        framebuffer::draw_text_at(sx, ty + 12, sub, 0x2A4060);
    }

    fn render_icons(&self) {
        for i in 0..NUM_ICONS {
            self.render_icon(i);
        }
    }

    fn render_launcher(&self) {
        let r = launcher_rect(self.sh);
        framebuffer::fill_rect(r.x, r.y, r.w, r.h, LAUNCHER_BG);
        framebuffer::fill_rect(r.x + r.w - 1, r.y, 1, r.h, LAUNCHER_BORD);
        framebuffer::fill_rect(0, BAR_H, LAUNCHER_W, LAUNCHER_HEAD_H, LAUNCHER_HEAD);
        framebuffer::fill_rect(0, BAR_H + LAUNCHER_HEAD_H - 1, LAUNCHER_W, 1, LAUNCHER_SEP);
        framebuffer::draw_text_scaled(LAUNCHER_PAD_X, BAR_H + 10, "ASTRA OS", LAUNCHER_TEXT, 2);
        framebuffer::draw_text_at(
            LAUNCHER_PAD_X,
            BAR_H + LAUNCHER_HEAD_H - 12,
            "Applications",
            LAUNCHER_SUB,
        );
        for i in 0..NUM_LAUNCHER {
            let ir = launcher_item_rect(i);
            let bg = if self.launcher_hover == Some(i) {
                LAUNCHER_HOV
            } else {
                LAUNCHER_BG
            };
            framebuffer::fill_rect(ir.x, ir.y, ir.w, ir.h, bg);
            framebuffer::fill_rect(ir.x, ir.y + ir.h - 1, ir.w - 1, 1, LAUNCHER_SEP);
            framebuffer::draw_text_at(
                ir.x + LAUNCHER_PAD_X,
                ir.y + (ir.h.saturating_sub(16)) / 2,
                APP_REGISTRY[i].label,
                LAUNCHER_TEXT,
            );
            framebuffer::draw_text_at(
                ir.x + LAUNCHER_PAD_X,
                ir.y + (ir.h.saturating_sub(16)) / 2 + 11,
                APP_REGISTRY[i].desc,
                LAUNCHER_SUB,
            );
        }
    }

    fn render_desktop_ctx(&self) {
        let r = self.dctx.rect(self.sw, self.sh);
        // Border
        framebuffer::fill_rect(r.x, r.y, r.w, r.h, DCTX_BORD);
        // Background (inset 1 px)
        framebuffer::fill_rect(r.x + 1, r.y + 1, r.w - 2, r.h - 2, DCTX_BG);
        let labels: [&str; DCTX_ITEMS] = ["New File", "New Folder"];
        for i in 0..DCTX_ITEMS {
            let ir = self.dctx.item_rect(i, self.sw, self.sh);
            let bg = if self.dctx.hover == Some(i) {
                DCTX_HOV
            } else {
                DCTX_BG
            };
            framebuffer::fill_rect(ir.x, ir.y, ir.w, ir.h, bg);
            let ty = ir.y + (DCTX_ITEM_H.saturating_sub(9)) / 2;
            framebuffer::draw_text_at(ir.x + 8, ty, labels[i], DCTX_TEXT);
        }
    }

    fn render_desk_item(&self, i: usize) {
        let item = &self.desk_items[i];
        let r = item.rect();
        // Same pipeline as render_icon ─────────────────────────────────────
        let sel = item.selected;
        let bg = if sel { DI_SEL_BG } else { ICON_BG };
        let border = if sel { ICON_BORDER } else { 0x181E28 };
        framebuffer::fill_rect(r.x, r.y, r.w, r.h, bg);
        // 4-side border
        framebuffer::fill_rect(r.x, r.y, r.w, 1, border);
        framebuffer::fill_rect(r.x, r.y + r.h - 1, r.w, 1, border);
        framebuffer::fill_rect(r.x, r.y, 1, r.h, border);
        framebuffer::fill_rect(r.x + r.w - 1, r.y, 1, r.h, border);
        // Accent strip (same as dock)
        framebuffer::fill_rect(r.x + 1, r.y + 1, r.w - 2, 4, ICON_ACCENT);
        // Pixel-art icon via shared draw_app_icon
        let icon_idx = if item.is_dir {
            DI_ICON_DIR
        } else {
            DI_ICON_FILE
        };
        draw_app_icon(icon_idx, r);
        // Label — centred, up to 14 chars (same position as dock label)
        let name = core::str::from_utf8(&item.name[..item.nlen]).unwrap_or("?");
        const MAX_LABEL: usize = 14;
        let label = if name.len() > MAX_LABEL {
            &name[..MAX_LABEL]
        } else {
            name
        };
        let tx = r.x + (r.w.saturating_sub(label.len() * 6)) / 2;
        let ty = r.y + r.h / 2 + 4;
        let col = if sel { DI_SEL_TEXT } else { DI_TEXT };
        framebuffer::draw_text_at(tx, ty, label, col);
    }

    fn render_desk_items(&self) {
        for i in 0..self.desk_item_count {
            self.render_desk_item(i);
        }
    }

    fn render_desk_prompt(&self) {
        let pr = self.desk_prompt.rect(self.sw, self.sh);
        // Label above the input box
        let lbl = if self.desk_prompt.is_dir {
            "Folder name:"
        } else {
            "File name:"
        };
        framebuffer::draw_text_at(pr.x + 4, pr.y - 12, lbl, DP_LBL);
        // Box border + bg
        framebuffer::fill_rect(pr.x, pr.y, pr.w, pr.h, DP_BORD);
        framebuffer::fill_rect(pr.x + 1, pr.y + 1, pr.w - 2, pr.h - 2, DP_BG);
        // Typed text
        let typed =
            core::str::from_utf8(&self.desk_prompt.buf[..self.desk_prompt.len]).unwrap_or("");
        let tx = pr.x + 6;
        let ty = pr.y + (DP_H.saturating_sub(8)) / 2;
        framebuffer::draw_text_at(tx, ty, typed, DP_TEXT);
        // Cursor
        let cx = tx + self.desk_prompt.len * 6;
        framebuffer::draw_text_at(cx, ty, "_", DP_CUR);
        // Hint
        framebuffer::draw_text_at(pr.x + 4, pr.y + DP_H + 2, "Enter=ok  Esc=cancel", 0x2A5070);
    }

    /// Renders only the chrome (shadow, border, titlebar, close button).
    /// Does NOT call app.render() — client area is filled with WIN_BG only.
    /// Used during drag when the cached surface is blitted separately.
    fn render_window_chrome(&self, idx: usize, focused: bool) {
        let win = &self.windows[idx];
        if win.minimized {
            return;
        }
        let x = win.x.max(0) as usize;
        let y = win.y.max(0) as usize;
        let (w, h) = (win.w, win.h);
        framebuffer::fill_rect(x + WIN_SHADOW_OFS, y + WIN_SHADOW_OFS, w, h, WIN_SHADOW);
        let border = if focused { WIN_BORDER_FOC } else { WIN_BORDER };
        framebuffer::fill_rect(x, y, w, h, border);
        framebuffer::fill_rect(
            x + 1,
            y + 1,
            w.saturating_sub(2),
            h.saturating_sub(2),
            WIN_BG,
        );
        let bar_bg = if focused { WIN_BAR_FOC } else { WIN_BAR_BG };
        framebuffer::fill_rect(x, y, w, WIN_BAR_H, bar_bg);
        framebuffer::fill_rect(x, y + WIN_BAR_H, w, 1, WIN_BAR_BORDER);
        let title = win.app.title();
        let ty = y + (WIN_BAR_H.saturating_sub(14)) / 2;
        framebuffer::draw_text_scaled(x + WIN_PAD_X, ty, title, WIN_TITLE_COL, 2);
        let cb = win.close_btn_rect();
        let close_bg = if self.close_hover == Some(idx) {
            WIN_CLOSE_HOV
        } else {
            bar_bg
        };
        framebuffer::fill_rect(cb.x, cb.y, cb.w, cb.h, close_bg);
        framebuffer::draw_text_at(
            cb.x + (cb.w.saturating_sub(6)) / 2,
            cb.y + (cb.h.saturating_sub(8)) / 2,
            "X",
            WIN_TITLE_COL,
        );
        let hint = "[ESC]";
        let hx = cb.x.saturating_sub(hint.len() * 6 + 6);
        framebuffer::draw_text_at(hx, ty + 2, hint, WIN_HINT_COL);
    }

    fn render_window(&self, idx: usize, focused: bool) {
        let win = &self.windows[idx];
        if win.minimized {
            return;
        }
        let x = win.x.max(0) as usize;
        let y = win.y.max(0) as usize;
        let (w, h) = (win.w, win.h);
        framebuffer::fill_rect(x + WIN_SHADOW_OFS, y + WIN_SHADOW_OFS, w, h, WIN_SHADOW);
        let border = if focused { WIN_BORDER_FOC } else { WIN_BORDER };
        framebuffer::fill_rect(x, y, w, h, border);
        framebuffer::fill_rect(
            x + 1,
            y + 1,
            w.saturating_sub(2),
            h.saturating_sub(2),
            WIN_BG,
        );
        let bar_bg = if focused { WIN_BAR_FOC } else { WIN_BAR_BG };
        framebuffer::fill_rect(x, y, w, WIN_BAR_H, bar_bg);
        framebuffer::fill_rect(x, y + WIN_BAR_H, w, 1, WIN_BAR_BORDER);
        let title = win.app.title();
        let ty = y + (WIN_BAR_H.saturating_sub(14)) / 2;
        framebuffer::draw_text_scaled(x + WIN_PAD_X, ty, title, WIN_TITLE_COL, 2);
        let cb = win.close_btn_rect();
        let close_bg = if self.close_hover == Some(idx) {
            WIN_CLOSE_HOV
        } else {
            bar_bg
        };
        framebuffer::fill_rect(cb.x, cb.y, cb.w, cb.h, close_bg);
        framebuffer::draw_text_at(
            cb.x + (cb.w.saturating_sub(6)) / 2,
            cb.y + (cb.h.saturating_sub(8)) / 2,
            "X",
            WIN_TITLE_COL,
        );
        let hint = "[ESC]";
        let hx = cb.x.saturating_sub(hint.len() * 6 + 6);
        framebuffer::draw_text_at(hx, ty + 2, hint, WIN_HINT_COL);
        let cr = win.client_rect();
        win.app.render(cr.x, cr.y, cr.w, cr.h);
    }

    fn compose_full(&mut self) {
        framebuffer::clear(desktop_bg());
        self.render_icons();
        self.render_desk_items();
        let focused = self.focused;
        for i in 0..self.windows.len() {
            if !self.windows[i].minimized {
                self.render_window(i, focused == Some(i));
            }
        }
        if self.launcher_open {
            self.render_launcher();
        }
        if self.dctx.visible {
            self.render_desktop_ctx();
        }
        if self.desk_prompt.active {
            self.render_desk_prompt();
        }
        self.render_taskbar();
    }

    fn compose_damage(&mut self) {
        let screen = Rect {
            x: 0,
            y: 0,
            w: self.sw,
            h: self.sh,
        };
        let launcher = launcher_rect(self.sh);
        let taskbar = Rect {
            x: 0,
            y: 0,
            w: self.sw,
            h: BAR_H,
        };
        let focused = self.focused;

        for i in 0..self.damage.count {
            let dirty = self.damage.rects[i].clip(&screen);
            if dirty.is_empty() {
                continue;
            }

            // Set scissor so all rendering is clipped to this damage rect.
            // This means app.render() and render_window() only write pixels
            // that will actually be visible — no wasted backbuffer work outside
            // the damaged area.
            framebuffer::set_scissor(dirty.x, dirty.y, dirty.w, dirty.h);

            framebuffer::fill_rect(dirty.x, dirty.y, dirty.w, dirty.h, desktop_bg());

            for icon_idx in 0..NUM_ICONS {
                if icon_rect_of(&self.icons[icon_idx]).intersects(&dirty) {
                    self.render_icon(icon_idx);
                }
            }

            for di in 0..self.desk_item_count {
                if self.desk_items[di].rect().intersects(&dirty) {
                    self.render_desk_item(di);
                }
            }

            for win_idx in 0..self.windows.len() {
                if !self.windows[win_idx].minimized
                    && self.windows[win_idx].bounds().intersects(&dirty)
                {
                    let cr = self.windows[win_idx].client_rect();
                    let cache_ok = self.windows[win_idx].surface_valid
                        && self.windows[win_idx].surface_w == cr.w
                        && self.windows[win_idx].surface_h == cr.h
                        && !self.windows[win_idx].cached_surface.is_empty();

                    if cache_ok {
                        // Chrome (titlebar, border, shadow) — no app.render().
                        // Chrome calls respect scissor, so they self-clip to `dirty`.
                        self.render_window_chrome(win_idx, focused == Some(win_idx));

                        // Blit cached client pixels.  write_rect_sub bypasses the
                        // scissor, so we manually clip to (dirty ∩ client_rect) to
                        // avoid stomping pixels outside the damage area (which could
                        // contain another window/icon not yet recomposed for that rect).
                        let ix0 = cr.x.max(dirty.x);
                        let iy0 = cr.y.max(dirty.y);
                        let ix1 = (cr.x + cr.w).min(dirty.x + dirty.w);
                        let iy1 = (cr.y + cr.h).min(dirty.y + dirty.h);
                        if ix1 > ix0 && iy1 > iy0 {
                            let iw = ix1 - ix0;
                            let ih = iy1 - iy0;
                            let sub_x = ix0 - cr.x;
                            let sub_y = iy0 - cr.y;
                            framebuffer::write_rect_sub(
                                ix0,
                                iy0,
                                iw,
                                ih,
                                &self.windows[win_idx].cached_surface,
                                cr.w,
                                sub_x,
                                sub_y,
                            );
                        }
                    } else {
                        // Full render: chrome + app.render().  Schedule capture so
                        // subsequent frames can blit from the cache instead.
                        self.render_window(win_idx, focused == Some(win_idx));
                        self.windows[win_idx].surface_needs_capture = true;
                    }
                }
            }

            if self.launcher_open && launcher.intersects(&dirty) {
                self.render_launcher();
            }
            if self.dctx.visible && self.dctx.rect(self.sw, self.sh).intersects(&dirty) {
                self.render_desktop_ctx();
            }
            if self.desk_prompt.active && self.desk_prompt.rect(self.sw, self.sh).intersects(&dirty)
            {
                self.render_desk_prompt();
            }
            if taskbar.intersects(&dirty) {
                self.render_taskbar();
            }
        }

        framebuffer::clear_scissor();

        // ── Surface capture ───────────────────────────────────────────────
        // After all damage rects are composited, read back client pixels for
        // any window that was fully re-rendered this pass.  Future drag frames
        // will blit from this cache instead of calling app.render().
        for win_idx in 0..self.windows.len() {
            if !self.windows[win_idx].surface_needs_capture {
                continue;
            }
            self.windows[win_idx].surface_needs_capture = false;
            let cr = self.windows[win_idx].client_rect();
            let n = cr.w * cr.h;
            if n == 0 {
                continue;
            }
            self.windows[win_idx].cached_surface.resize(n, 0);
            framebuffer::read_rect(
                cr.x,
                cr.y,
                cr.w,
                cr.h,
                &mut self.windows[win_idx].cached_surface,
            );
            self.windows[win_idx].surface_w = cr.w;
            self.windows[win_idx].surface_h = cr.h;
            self.windows[win_idx].surface_valid = true;
        }
    }

    // ── Cursor ────────────────────────────────────────────────────────────

    fn cursor_save(&mut self) {
        let cx = self.cursor_x.max(0) as usize;
        let cy = self.cursor_y.max(0) as usize;
        framebuffer::read_rect(cx, cy, CURSOR_W, CURSOR_H, &mut self.cursor_under);
        self.cursor_drawn_x = self.cursor_x;
        self.cursor_drawn_y = self.cursor_y;
    }

    fn cursor_stamp(&self) {
        let cx = self.cursor_x.max(0) as usize;
        let cy = self.cursor_y.max(0) as usize;
        let bmp = cursor_bitmap(self.cursor_shape);
        for row in 0..CURSOR_H {
            for col in 0..CURSOR_W {
                let px = bmp[row][col];
                if px == 0 {
                    continue;
                }
                let color = if px == 1 { CURSOR_WHITE } else { CURSOR_BLACK };
                let px_x = cx + col;
                let px_y = cy + row;
                if px_x < self.sw && px_y < self.sh {
                    framebuffer::fill_rect(px_x, px_y, 1, 1, color);
                }
            }
        }
    }

    fn cursor_erase(&self) {
        let cx = self.cursor_drawn_x.max(0) as usize;
        let cy = self.cursor_drawn_y.max(0) as usize;
        framebuffer::write_rect(cx, cy, CURSOR_W, CURSOR_H, &self.cursor_under);
    }

    fn cursor_move_fast(&mut self) {
        let old_x = self.cursor_drawn_x.max(0) as usize;
        let old_y = self.cursor_drawn_y.max(0) as usize;
        if self.cursor_on_screen {
            self.cursor_erase();
        }
        self.cursor_save();
        self.cursor_stamp();
        self.cursor_on_screen = true;
        if old_x != self.cursor_x.max(0) as usize || old_y != self.cursor_y.max(0) as usize {
            framebuffer::present_rect(old_x, old_y, CURSOR_W, CURSOR_H);
        }
        let nx = self.cursor_x.max(0) as usize;
        let ny = self.cursor_y.max(0) as usize;
        framebuffer::present_rect(nx, ny, CURSOR_W, CURSOR_H);
    }

    fn cursor_rect_at(&self, x: i32, y: i32) -> Rect {
        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        if x0 >= self.sw || y0 >= self.sh {
            return Rect::ZERO;
        }
        Rect {
            x: x0,
            y: y0,
            w: CURSOR_W.min(self.sw - x0),
            h: CURSOR_H.min(self.sh - y0),
        }
    }

    fn present_damage(&mut self) {
        if self.damage.full {
            self.present_full();
            return;
        }

        let screen = Rect {
            x: 0,
            y: 0,
            w: self.sw,
            h: self.sh,
        };

        // ── Cursor erase (backbuffer only, before compose) ────────────────
        // IMPORTANT: do NOT add the cursor rect to the damage list.
        // Adding it would cause compose_damage to call render_window for every
        // window the cursor touches — even when no app content changed.
        // Instead, restore the saved cursor_under pixels directly; compose_damage
        // will overwrite them with correct content if that area is in a damage rect.
        let old_cursor = if self.cursor_on_screen {
            let r = self.cursor_rect_at(self.cursor_drawn_x, self.cursor_drawn_y);
            self.cursor_erase(); // writes cursor_under back to backbuffer
            r
        } else {
            Rect::ZERO
        };
        self.cursor_on_screen = false;

        // Compose only app/window damage — cursor not in the damage list.
        self.compose_damage();

        // Stamp cursor at new position.
        self.cursor_save();
        self.cursor_stamp();
        self.cursor_on_screen = true;

        // Blit app damage rects.
        for i in 0..self.damage.count {
            let r = self.damage.rects[i].clip(&screen);
            if !r.is_empty() {
                framebuffer::present_rect(r.x, r.y, r.w, r.h);
            }
        }

        // Blit old cursor area (backbuffer now has clean background there).
        let old_cr = old_cursor.clip(&screen);
        if !old_cr.is_empty() {
            framebuffer::present_rect(old_cr.x, old_cr.y, old_cr.w, old_cr.h);
        }

        // Blit new cursor area.
        let new_cr = self
            .cursor_rect_at(self.cursor_x, self.cursor_y)
            .clip(&screen);
        if !new_cr.is_empty() {
            framebuffer::present_rect(new_cr.x, new_cr.y, new_cr.w, new_cr.h);
        }

        self.damage.clear();
    }

    fn present_full(&mut self) {
        self.cursor_on_screen = false;
        self.compose_full();
        self.cursor_save();
        self.cursor_stamp();
        framebuffer::present_full();
        self.cursor_on_screen = true;
        self.damage.clear();
    }

    fn tick_live_windows(&mut self, now: u64) {
        for i in 0..self.windows.len() {
            if self.windows[i].minimized {
                continue;
            }
            if let Some(interval) = self.windows[i].app.refresh_interval_ms() {
                if now.wrapping_sub(self.windows[i].last_refresh_ms) >= interval {
                    self.windows[i].last_refresh_ms = now;
                    match self.windows[i].app.tick() {
                        AppAction::Nothing => {}
                        AppAction::RedrawArea(rx, ry, rw, rh) => {
                            self.windows[i].surface_valid = false;
                            let cr = self.windows[i].client_rect();
                            let rx = rx.min(cr.w);
                            let ry = ry.min(cr.h);
                            let rw = rw.min(cr.w.saturating_sub(rx));
                            let rh = rh.min(cr.h.saturating_sub(ry));
                            if rw != 0 && rh != 0 {
                                self.damage.add(Rect {
                                    x: cr.x + rx,
                                    y: cr.y + ry,
                                    w: rw,
                                    h: rh,
                                });
                            }
                        }
                        _ => {
                            self.windows[i].surface_valid = false;
                            self.damage.add(self.windows[i].client_rect());
                        }
                    }
                }
            }
        }
    }

    /// Earliest future time (ms) at which a live window needs its next refresh.
    /// Returns `u64::MAX` when no windows have periodic refresh.
    fn next_wakeup_ms(&self, now: u64) -> u64 {
        let mut earliest = u64::MAX;
        for win in &self.windows {
            if win.minimized {
                continue;
            }
            if let Some(interval) = win.app.refresh_interval_ms() {
                let next = win.last_refresh_ms.saturating_add(interval);
                let next = if next <= now { now } else { next };
                if next < earliest {
                    earliest = next;
                }
            }
        }
        earliest
    }

    fn update_cursor_shape(&mut self) {
        let (mx, my) = (self.cursor_x, self.cursor_y);
        if self.drag.is_some() {
            self.cursor_shape = CursorShape::Move;
            return;
        }
        if let Some(ref rs) = self.resize {
            self.cursor_shape = rs.zone.cursor_shape();
            return;
        }
        for i in (0..self.windows.len()).rev() {
            let w = &self.windows[i];
            if w.minimized {
                continue;
            }
            if let Some(zone) = hit_resize_zone(w, mx, my) {
                self.cursor_shape = zone.cursor_shape();
                return;
            }
            if mx >= w.x && mx < w.x + w.w as i32 && my >= w.y && my < w.y + WIN_BAR_H as i32 {
                self.cursor_shape = CursorShape::Move;
                return;
            }
        }
        if self.launcher_open && self.launcher_item_at(mx, my).is_some() {
            self.cursor_shape = CursorShape::Hand;
            return;
        }
        if self.icon_at(mx, my).is_some() {
            self.cursor_shape = CursorShape::Hand;
            return;
        }
        if self.desk_item_at(mx, my).is_some() {
            self.cursor_shape = CursorShape::Hand;
            return;
        }
        if self.taskbar_btn_at(mx, my).is_some() {
            self.cursor_shape = CursorShape::Hand;
            return;
        }
        let pb = power_btn_rect(self.sw);
        if my >= 0
            && (my as usize) < BAR_H
            && mx as usize >= pb.x
            && (mx as usize) < pb.x + pb.w
            && my as usize >= pb.y
            && (my as usize) < pb.y + pb.h
        {
            self.cursor_shape = CursorShape::Hand;
            return;
        }
        self.cursor_shape = CursorShape::Arrow;
    }

}
