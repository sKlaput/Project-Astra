impl Desktop {
    fn on_button_release(&mut self) {
        // Flush any drag frames that were skipped by the rate-limiter so the
        // window snaps to its final position with no ghost artifact.
        if let Some(ref ds) = self.drag {
            let idx = ds.win_idx;
            if idx < self.windows.len() {
                let final_b = self.windows[idx].bounds();
                let flush = match self.drag_damage_accum {
                    Some(prev) => prev.union(&final_b),
                    None => final_b,
                };
                self.damage.add(flush);
            }
        }
        self.drag_damage_accum = None;
        self.drag = None;
        self.resize = None;
        if let Some(ref ids) = self.icon_drag {
            let ii = ids.idx;
            if ids.moved {
                // Snap to nearest grid cell and mark both old and new positions dirty.
                let pre_snap = icon_rect_of(&self.icons[ii]);
                self.damage.add(pre_snap);
                self.snap_icon(ii);
                self.damage.add(icon_rect_of(&self.icons[ii]));
            } else {
                // Plain click: toggle selection (was set true on press).
                self.icons[ii].selected = !self.icons[ii].selected;
                self.damage.add(icon_rect_of(&self.icons[ii]));
            }
        }
        self.icon_drag = None;
        // Release desk item drag — only save if the item actually moved
        let should_save = self.desk_item_drag.as_ref().map_or(false, |d| d.moved);
        if let Some(ref ddi) = self.desk_item_drag {
            if ddi.idx < self.desk_item_count {
                self.damage.add(self.desk_items[ddi.idx].rect());
            }
        }
        self.desk_item_drag = None;
        if should_save {
            self.save_desktop_state();
        }
        self.update_cursor_shape();
    }

    fn on_mouse_scroll(&mut self, delta: i32) {
        if let Some(fidx) = self.focused {
            if fidx < self.windows.len() && !self.windows[fidx].minimized {
                let act = self.windows[fidx].app.handle_mouse_scroll(delta);
                self.handle_app_action(fidx, act);
            }
        }
    }

    fn on_key(&mut self, key: Key) {
        // ── Desktop name-entry prompt ─────────────────────────────────────
        if self.desk_prompt.active {
            let pr = self.desk_prompt.rect(self.sw, self.sh);
            match key {
                Key::Escape => {
                    self.desk_prompt = DesktopNamePrompt::hidden();
                    self.damage.add(pr);
                    self.damage.add(Rect {
                        x: pr.x,
                        y: pr.y.saturating_sub(16),
                        w: pr.w,
                        h: pr.h + 30,
                    });
                }
                Key::Enter => {
                    self.commit_desk_prompt();
                }
                Key::Backspace => {
                    if self.desk_prompt.len > 0 {
                        self.desk_prompt.len -= 1;
                        self.desk_prompt.buf[self.desk_prompt.len] = 0;
                        self.damage.add(Rect {
                            x: pr.x,
                            y: pr.y.saturating_sub(16),
                            w: pr.w,
                            h: pr.h + 30,
                        });
                    }
                }
                Key::Char(c) => {
                    let invalid_char = matches!(
                        c,
                        b'/' | b'\\' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|'
                    );
                    if c >= 0x20 && c < 0x7F && !invalid_char && self.desk_prompt.len < 28 {
                        self.desk_prompt.buf[self.desk_prompt.len] = c;
                        self.desk_prompt.len += 1;
                        self.damage.add(Rect {
                            x: pr.x,
                            y: pr.y.saturating_sub(16),
                            w: pr.w,
                            h: pr.h + 30,
                        });
                    }
                }
                _ => {}
            }
            return;
        }

        if let Some(fidx) = self.focused {
            if fidx < self.windows.len() && !self.windows[fidx].minimized {
                let act = self.windows[fidx].app.handle_key(key);
                // Only close on Escape if the app returned Close (not handled by app itself)
                if key == Key::Escape && matches!(act, AppAction::Nothing) {
                    self.close_window(fidx);
                    return;
                }
                self.handle_app_action(fidx, act);
            }
        }
    }

    fn handle_app_action(&mut self, win_idx: usize, action: AppAction) {
        match action {
            AppAction::Nothing => {}
            AppAction::Close => {
                self.close_window(win_idx);
            }
            AppAction::RedrawAll => {
                if win_idx < self.windows.len() {
                    self.windows[win_idx].surface_valid = false;
                    let b = self.windows[win_idx].client_rect();
                    self.damage.add(b);
                }
            }
            AppAction::RedrawArea(rx, ry, rw, rh) => {
                if win_idx < self.windows.len() {
                    self.windows[win_idx].surface_valid = false;
                    let cr = self.windows[win_idx].client_rect();
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
            }
            AppAction::RedrawInput => {
                if win_idx < self.windows.len() {
                    self.windows[win_idx].surface_valid = false;
                    let cr = self.windows[win_idx].client_rect();
                    if let Some(ih) = self.windows[win_idx].app.input_region_height() {
                        let iy = cr.y + cr.h.saturating_sub(ih);
                        self.damage.add(Rect {
                            x: cr.x,
                            y: iy,
                            w: cr.w,
                            h: ih,
                        });
                    } else {
                        self.damage.add(cr);
                    }
                }
            }
            AppAction::OpenFile(path_bytes, path_len) => {
                let path = core::str::from_utf8(&path_bytes[..path_len]).unwrap_or("");
                // Route .ppm files to the image viewer; everything else to the editor.
                let lower_path = path;
                if lower_path.ends_with(".ppm") || lower_path.ends_with(".PPM") {
                    let viewer = Box::new(ImageViewerApp::open(path));
                    self.open_window(viewer);
                } else {
                    let editor = Box::new(EditorApp::open(path));
                    self.open_window(editor);
                }
            }
        }
    }

    fn launch_app(&mut self, idx: usize) {
        if idx >= NUM_APPS {
            return;
        }
        // Factory is defined in APP_REGISTRY — adding a new app there is the
        // only change needed; this function never needs to be touched again.
        let app = (APP_REGISTRY[idx].make)();
        self.open_window(app);
    }
}
