impl Desktop {
    // ── Input handlers ────────────────────────────────────────────────────

    fn on_mouse_move(&mut self, mx: i32, my: i32) {
        self.cursor_x = mx;
        self.cursor_y = my;

        if let Some(ref mut ids) = self.icon_drag {
            let idx = ids.idx;
            let ox = ids.off_x;
            let oy = ids.off_y;
            let old_r = icon_rect_of(&self.icons[idx]);
            let new_x = (mx - ox)
                .max(0)
                .min((self.sw.saturating_sub(ICON_CELL_W)) as i32);
            let new_y = (my - oy)
                .max(BAR_H as i32)
                .min((self.sh.saturating_sub(ICON_CELL_H)) as i32);
            self.icons[idx].x = new_x;
            self.icons[idx].y = new_y;
            ids.moved = true;
            let new_r = icon_rect_of(&self.icons[idx]);
            self.damage.add(old_r);
            self.damage.add(new_r);
        }

        if let Some(ref mut di_drag) = self.desk_item_drag {
            let idx = di_drag.idx;
            let ox = di_drag.off_x;
            let oy = di_drag.off_y;
            if idx < self.desk_item_count {
                let old_r = self.desk_items[idx].rect();
                let new_x = (mx - ox).max(0).min((self.sw.saturating_sub(DI_W)) as i32);
                let new_y = (my - oy)
                    .max(BAR_H as i32)
                    .min((self.sh.saturating_sub(DI_H)) as i32);
                self.desk_items[idx].x = new_x;
                self.desk_items[idx].y = new_y;
                di_drag.moved = true;
                let new_r = self.desk_items[idx].rect();
                self.damage.add(old_r);
                self.damage.add(new_r);
            }
        }

        if let Some(ref ds) = self.drag {
            let idx = ds.win_idx;
            let ox = ds.off_x;
            let oy = ds.off_y;
            if idx < self.windows.len() {
                // Capture old bounds BEFORE updating position so the accumulator
                // includes both the previous and new window positions.
                let old_b = self.windows[idx].bounds();

                // Clamp so at least WIN_BAR_H*3 px of the title bar stays on-screen
                // horizontally — prevents windows being dragged off into the void.
                let win_w = self.windows[idx].w as i32;
                let keep: i32 = (WIN_BAR_H as i32) * 3;
                self.windows[idx].x = (mx - ox).max(keep - win_w).min(self.sw as i32 - keep);
                self.windows[idx].y = (my - oy).max(BAR_H as i32);
                let new_b = self.windows[idx].bounds();

                // Accumulate: union(accum, old_b, new_b) so we track every pixel
                // the window swept through, even across skipped frames.
                let accum = match self.drag_damage_accum {
                    Some(prev) => prev.union(&old_b).union(&new_b),
                    None => old_b.union(&new_b),
                };
                self.drag_damage_accum = Some(accum);

                // Rate-limit: flush to damage list at most every 16 ms (~60 fps).
                let now_ms = uptime_ms();
                if now_ms.wrapping_sub(self.last_drag_present_ms) >= 16 {
                    self.damage.add(accum);
                    // Reset accum to latest bounds so next frame erases from here.
                    self.drag_damage_accum = Some(new_b);
                    self.last_drag_present_ms = now_ms;
                }
            }
        }

        if let Some(ref rs) = self.resize {
            let idx = rs.win_idx;
            let dx = mx - rs.start_mx;
            let dy = my - rs.start_my;
            let sx = rs.start_x;
            let sy = rs.start_y;
            let sw = rs.start_w as i32;
            let sh = rs.start_h as i32;
            if idx < self.windows.len() {
                let old_b = self.windows[idx].bounds();
                let (nx, ny, nw, nh) = match rs.zone {
                    ResizeZone::TL => (
                        sx + dx,
                        sy + dy,
                        (sw - dx).max(WIN_MIN_W as i32),
                        (sh - dy).max(WIN_MIN_H as i32),
                    ),
                    ResizeZone::T => (sx, sy + dy, sw, (sh - dy).max(WIN_MIN_H as i32)),
                    ResizeZone::TR => (
                        sx,
                        sy + dy,
                        (sw + dx).max(WIN_MIN_W as i32),
                        (sh - dy).max(WIN_MIN_H as i32),
                    ),
                    ResizeZone::R => (sx, sy, (sw + dx).max(WIN_MIN_W as i32), sh),
                    ResizeZone::BR => (
                        sx,
                        sy,
                        (sw + dx).max(WIN_MIN_W as i32),
                        (sh + dy).max(WIN_MIN_H as i32),
                    ),
                    ResizeZone::B => (sx, sy, sw, (sh + dy).max(WIN_MIN_H as i32)),
                    ResizeZone::BL => (
                        sx + dx,
                        sy,
                        (sw - dx).max(WIN_MIN_W as i32),
                        (sh + dy).max(WIN_MIN_H as i32),
                    ),
                    ResizeZone::L => (sx + dx, sy, (sw - dx).max(WIN_MIN_W as i32), sh),
                };
                self.windows[idx].x = nx.max(0);
                self.windows[idx].y = ny.max(BAR_H as i32);
                self.windows[idx].w = nw as usize;
                self.windows[idx].h = nh as usize;
                let new_b = self.windows[idx].bounds();
                self.damage.add(old_b);
                self.damage.add(new_b);
            }
        }

        let old_lh = self.launcher_hover;
        let old_ih = self.icon_hover;
        let old_th = self.taskbar_hover;
        let old_ch = self.close_hover;

        self.launcher_hover = self.launcher_item_at(mx, my);
        self.icon_hover = if self.icon_drag.is_some() {
            None
        } else if !self.launcher_open || mx as usize >= LAUNCHER_W {
            self.icon_at(mx, my)
        } else {
            None
        };
        let pb = power_btn_rect(self.sw);
        let on_power = my >= 0
            && (my as usize) < BAR_H
            && mx as usize >= pb.x
            && (mx as usize) < pb.x + pb.w
            && my as usize >= pb.y
            && (my as usize) < pb.y + pb.h;
        // usize::MAX is used as a sentinel meaning "hovering power button"
        self.taskbar_hover = if on_power {
            Some(usize::MAX)
        } else {
            self.taskbar_btn_at(mx, my)
        };
        self.close_hover = None;
        for i in (0..self.windows.len()).rev() {
            let w = &self.windows[i];
            if w.minimized {
                continue;
            }
            let cb = w.close_btn_rect();
            if mx as usize >= cb.x
                && (mx as usize) < cb.x + cb.w
                && my as usize >= cb.y
                && (my as usize) < cb.y + cb.h
            {
                self.close_hover = Some(i);
                break;
            }
        }

        let mut next_app_hover = None;
        if let Some(fidx) = self.focused {
            if fidx < self.windows.len() && !self.windows[fidx].minimized {
                let cr = self.windows[fidx].client_rect();
                let inside_client = mx >= cr.x as i32
                    && mx < (cr.x + cr.w) as i32
                    && my >= cr.y as i32
                    && my < (cr.y + cr.h) as i32;

                if inside_client {
                    next_app_hover = Some(fidx);
                    let rx = mx - cr.x as i32;
                    let ry = my - cr.y as i32;
                    let act = self.windows[fidx].app.handle_mouse_move(rx, ry);
                    self.handle_app_action(fidx, act);
                } else if self.app_hover_target == Some(fidx) {
                    let act = self.windows[fidx].app.handle_mouse_move(-1, -1);
                    self.handle_app_action(fidx, act);
                }
            }
        }
        self.app_hover_target = next_app_hover;

        if old_lh != self.launcher_hover
            || old_ih != self.icon_hover
            || old_th != self.taskbar_hover
            || old_ch != self.close_hover
        {
            self.damage.add(Rect {
                x: 0,
                y: 0,
                w: self.sw,
                h: BAR_H,
            });
            if self.launcher_open {
                self.damage.add(launcher_rect(self.sh));
            }
            for i in 0..NUM_ICONS {
                self.damage.add(icon_rect_of(&self.icons[i]));
            }
        }

        // Desktop context menu hover
        if self.dctx.visible {
            let new_hover = self.dctx.hit_item(mx, my, self.sw, self.sh);
            if new_hover != self.dctx.hover {
                self.dctx.hover = new_hover;
                self.damage.add(self.dctx.rect(self.sw, self.sh));
            }
        }

        self.update_cursor_shape();
    }

    fn on_button_press(&mut self, mx: i32, my: i32) {
        // ── Desktop context menu ──────────────────────────────────────────
        if self.dctx.visible {
            let old_r = self.dctx.rect(self.sw, self.sh);
            let hit = self.dctx.hit_item(mx, my, self.sw, self.sh);
            self.dctx.visible = false;
            self.damage.add(old_r);
            if let Some(item) = hit {
                // Start the desktop name-entry prompt instead of opening File Manager
                let is_dir = item == 1; // 0 = New File, 1 = New Folder
                self.desk_prompt = DesktopNamePrompt {
                    active: true,
                    spawn_x: self.dctx.x,
                    spawn_y: self.dctx.y,
                    is_dir,
                    buf: [0u8; 32],
                    len: 0,
                };
                self.damage.add(self.desk_prompt.rect(self.sw, self.sh));
                return;
            }
            // Clicked outside the menu — just dismissed it, fall through.
        }

        if let Some(ch) = self.close_hover {
            if ch < self.windows.len() {
                let b = self.windows[ch].close_btn_rect();
                if mx as usize >= b.x
                    && (mx as usize) < b.x + b.w
                    && my as usize >= b.y
                    && (my as usize) < b.y + b.h
                {
                    self.close_window(ch);
                    return;
                }
            }
        }

        // Power button click
        let pb = power_btn_rect(self.sw);
        if my >= 0
            && (my as usize) < BAR_H
            && mx as usize >= pb.x
            && (mx as usize) < pb.x + pb.w
            && my as usize >= pb.y
            && (my as usize) < pb.y + pb.h
        {
            crate::arch::x86_64::power_off();
        }

        if let Some(tb) = self.taskbar_btn_at(mx, my) {
            match tb {
                0 => self.minimize_all(),
                1 => {
                    self.launcher_open = !self.launcher_open;
                    self.damage.mark_full();
                }
                n => {
                    let mut wi = 0;
                    for i in 0..self.windows.len() {
                        if !self.windows[i].minimized {
                            if wi == n - FIXED_BTNS {
                                let b = self.windows[i].bounds();
                                self.damage.add(b);
                                self.raise_to_front(i);
                                self.focused = Some(self.windows.len() - 1);
                                self.damage
                                    .add(self.windows[self.windows.len() - 1].bounds());
                                return;
                            }
                            wi += 1;
                        }
                    }
                }
            }
            self.damage.add(Rect {
                x: 0,
                y: 0,
                w: self.sw,
                h: BAR_H,
            });
            return;
        }

        if self.launcher_open {
            if let Some(li) = self.launcher_item_at(mx, my) {
                self.launch_app(li);
                self.launcher_open = false;
                self.damage.mark_full();
                return;
            }
            if mx as usize >= LAUNCHER_W {
                self.launcher_open = false;
                self.damage.mark_full();
            }
        }

        if let Some(wi) = self.window_at(mx, my) {
            if self.focused != Some(wi) || wi != self.windows.len() - 1 {
                if let Some(fid) = self.focused {
                    if fid < self.windows.len() {
                        self.damage.add(self.windows[fid].bounds());
                    }
                }
                self.damage.add(self.windows[wi].bounds());
                self.raise_to_front(wi);
                let new_idx = self.windows.len() - 1;
                self.focused = Some(new_idx);
                self.damage.add(self.windows[new_idx].bounds());
            }
            let tidx = self.windows.len() - 1;
            let win = &self.windows[tidx];
            if my >= win.y && my < win.y + WIN_BAR_H as i32 {
                let cb = win.close_btn_rect();
                if !(mx as usize >= cb.x
                    && (mx as usize) < cb.x + cb.w
                    && my as usize >= cb.y
                    && (my as usize) < cb.y + cb.h)
                {
                    let ox = mx - win.x;
                    let oy = my - win.y;
                    self.drag_damage_accum = Some(self.windows[tidx].bounds());
                    self.last_drag_present_ms = uptime_ms();
                    self.drag = Some(DragState {
                        win_idx: tidx,
                        off_x: ox,
                        off_y: oy,
                    });
                }
                return;
            }
            if let Some(zone) = hit_resize_zone(&self.windows[tidx], mx, my) {
                let win = &self.windows[tidx];
                self.resize = Some(ResizeState {
                    win_idx: tidx,
                    zone,
                    start_mx: mx,
                    start_my: my,
                    start_x: win.x,
                    start_y: win.y,
                    start_w: win.w,
                    start_h: win.h,
                });
                return;
            }
            let cr = self.windows[tidx].client_rect();
            let rx = mx - cr.x as i32;
            let ry = my - cr.y as i32;
            let act = self.windows[tidx].app.handle_mouse_click(rx, ry);
            self.handle_app_action(tidx, act);
            return;
        }

        if let Some(ii) = self.icon_at(mx, my) {
            let now = uptime_ms();
            let dbl = now.wrapping_sub(self.icons[ii].last_click_ms) < DBL_CLICK_MS;
            self.icons[ii].last_click_ms = now;
            for j in 0..NUM_ICONS {
                if j != ii {
                    self.icons[j].selected = false;
                }
            }
            if dbl {
                self.icons[ii].selected = false;
                self.launch_app(ii);
            } else {
                self.icons[ii].selected = true;
                let r = icon_rect_of(&self.icons[ii]);
                let off_x = mx - r.x as i32;
                let off_y = my - r.y as i32;
                self.icon_drag = Some(IconDragState {
                    idx: ii,
                    off_x,
                    off_y,
                    moved: false,
                });
            }
            for i in 0..NUM_ICONS {
                self.damage.add(icon_rect_of(&self.icons[i]));
            }
            return;
        }

        // ── Desktop items (user-created files/folders) ────────────────────
        if let Some(di) = self.desk_item_at(mx, my) {
            // Dismiss name prompt if open
            if self.desk_prompt.active {
                let pr = self.desk_prompt.rect(self.sw, self.sh);
                self.desk_prompt = DesktopNamePrompt::hidden();
                self.damage.add(pr);
            }
            let now = uptime_ms();
            let dbl = now.wrapping_sub(self.desk_items[di].last_click_ms) < DBL_CLICK_MS;
            self.desk_items[di].last_click_ms = now;
            for j in 0..self.desk_item_count {
                if j != di {
                    self.desk_items[j].selected = false;
                }
            }
            if dbl {
                self.desk_items[di].selected = false;
                self.open_desk_item(di);
            } else {
                self.desk_items[di].selected = true;
                let r = self.desk_items[di].rect();
                self.desk_item_drag = Some(DeskItemDrag {
                    idx: di,
                    off_x: mx - r.x as i32,
                    off_y: my - r.y as i32,
                    moved: false,
                });
            }
            self.damage.mark_full();
            return;
        }

        // Click on empty desktop area — dismiss prompt, deselect items
        if self.desk_prompt.active {
            let pr = self.desk_prompt.rect(self.sw, self.sh);
            self.desk_prompt = DesktopNamePrompt::hidden();
            self.damage.add(pr);
        }
        // Mark currently-selected items dirty before clearing the flag.
        for i in 0..self.desk_item_count {
            if self.desk_items[i].selected {
                self.damage.add(self.desk_items[i].rect());
            }
        }
        for i in 0..self.desk_item_count {
            self.desk_items[i].selected = false;
        }
    }

    fn on_right_button_press(&mut self, mx: i32, my: i32) {
        // If an existing desktop ctx menu is visible, dismiss it first.
        if self.dctx.visible {
            let old_r = self.dctx.rect(self.sw, self.sh);
            self.dctx.visible = false;
            self.damage.add(old_r);
        }

        // Route right-click to the focused window's client area first.
        if let Some(fi) = self.focused {
            if fi < self.windows.len() && !self.windows[fi].minimized {
                let cr = self.windows[fi].client_rect();
                if mx as usize >= cr.x
                    && (mx as usize) < cr.x + cr.w
                    && my as usize >= cr.y
                    && (my as usize) < cr.y + cr.h
                {
                    let rx = mx - cr.x as i32;
                    let ry = my - cr.y as i32;
                    let act = self.windows[fi].app.handle_mouse_right_click(rx, ry);
                    self.handle_app_action(fi, act);
                    return;
                }
            }
        }

        // Right-click on the bare desktop (not on taskbar, not on a window) —
        // open the desktop context menu.
        if (my as usize) < BAR_H {
            return;
        } // don't show over taskbar
        if self.launcher_open && (mx as usize) < LAUNCHER_W {
            return;
        } // not over launcher
        self.dctx = DesktopCtxMenu {
            visible: true,
            x: mx,
            y: my,
            hover: None,
        };
        self.damage.add(self.dctx.rect(self.sw, self.sh));
    }

}
