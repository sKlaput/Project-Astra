impl FileManagerApp {
    fn mouse_click_files(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        self.op_ok = None;
        self.op_err = None;
        // If context menu is open, check for a click on a menu item first.
        if self.ctx.visible {
            let mw = self.ctx.width() as i32;
            let mh = self.ctx.height() as i32;
            // We stored ctx.x/y relative to the client area.
            let mx = self.ctx.x;
            let my = self.ctx.y;
            if rel_x >= mx && rel_x < mx + mw && rel_y >= my && rel_y < my + mh {
                let item_idx = ((rel_y - my - 2) / CTX_ITEM_H as i32) as usize;
                let acted = if item_idx < self.ctx.item_count {
                    let item = self.ctx.items[item_idx];
                    if item.enabled {
                        self.ctx.visible = false;
                        self.execute_ctx_action(item.action)
                    } else {
                        self.ctx.visible = false;
                        AppAction::RedrawAll
                    }
                } else {
                    self.ctx.visible = false;
                    AppAction::RedrawAll
                };
                return acted;
            }
            // Click outside menu — dismiss
            self.ctx.visible = false;
            return AppAction::RedrawAll;
        }

        // Breadcrumb click in the header area
        if rel_y >= 0 && rel_y < HEADER_H as i32 {
            let lbl_w = "Location: ".len() * CHAR_W;
            let thispc_label = "This PC";
            let thispc_w = (thispc_label.len() * CHAR_W) as i32;
            let sep_w = (" > ".len() * CHAR_W) as i32;
            let crumb_x0 = (PAD_X + lbl_w) as i32;
            // Check click on "This PC" root crumb
            if rel_x >= crumb_x0 && rel_x < crumb_x0 + thispc_w {
                self.view = FmView::ThisPc;
                self.hover_crumb = None;
                return AppAction::RedrawAll;
            }
            // The VFS/FAT32 crumbs start after "This PC > "
            let vfs_x0 = crumb_x0 + thispc_w + sep_w;
            // Snapshot all needed state before any mutations
            let mut path_buf = [0u8; 128];
            let path_len = self.cwd.len;
            path_buf[..path_len].copy_from_slice(&self.cwd.data[..path_len]);
            let path = &path_buf[..path_len];
            let mut segs = [(0usize, 0usize); MAX_CRUMBS];
            let vfs_seg_count = parse_crumbs(path, &mut segs);
            let fat32_depth = self.fat32_stack_depth;
            let fat32_crumb_nlens = self.fat32_crumb_nlens;
            let fat32_cluster_stack = self.fat32_cluster_stack;
            let skip_bare_root = fat32_depth > 0 && vfs_seg_count == 1;
            let total_segs = (if skip_bare_root { 0 } else { vfs_seg_count }) + fat32_depth;
            let mut x = vfs_x0;
            for i in 0..total_segs {
                if i > 0 {
                    x += sep_w;
                }
                let seg_w: i32 = if !skip_bare_root && i < vfs_seg_count {
                    let (_, bl) = segs[i];
                    (bl * CHAR_W) as i32
                } else {
                    let fi = if skip_bare_root { i } else { i - vfs_seg_count };
                    (fat32_crumb_nlens[fi] * CHAR_W) as i32
                };
                if rel_x >= x && rel_x < x + seg_w {
                    // Don't navigate on the current (last) segment
                    if i < total_segs - 1 {
                        if !skip_bare_root && i < vfs_seg_count {
                            // ── VFS segment ───────────────────────────────
                            if i == 0 {
                                // Root "/"
                                self.cwd = PathBuf::root();
                            } else if i < vfs_seg_count - 1 {
                                // Intermediate VFS path segment
                                let mut new_path = PathBuf::root();
                                for j in 1..=i {
                                    let (s, l) = segs[j];
                                    let seg_name =
                                        core::str::from_utf8(&path[s..s + l]).unwrap_or("");
                                    new_path.push(seg_name);
                                }
                                self.cwd = new_path;
                            }
                            // Last VFS seg (i == vfs_seg_count - 1) but FAT32
                            // crumbs follow: drop into FAT32 root view.
                            self.fat32_cluster = 0;
                            self.fat32_stack_depth = 0;
                        } else {
                            // ── FAT32 stack segment ───────────────────────
                            let fi = if skip_bare_root { i } else { i - vfs_seg_count };
                            self.fat32_cluster = fat32_cluster_stack[fi + 1];
                            self.fat32_stack_depth = fi + 1;
                        }
                        self.hover_crumb = None;
                        self.load_dir();
                        return AppAction::RedrawAll;
                    }
                    break;
                }
                x += seg_w;
            }
            return AppAction::Nothing;
        }

        let list_top = (HEADER_H + COL_HDR_H) as i32;
        if rel_y < list_top {
            return AppAction::Nothing;
        }
        let row_in_view = ((rel_y - list_top) as usize) / ROW_H;
        let row_abs = self.scroll + row_in_view;
        if row_abs >= self.count {
            return AppAction::Nothing;
        }

        let now = uptime_ms();
        let is_dbl =
            row_abs == self.last_click_row && now.saturating_sub(self.last_click_ms) < DBL_CLICK_MS;

        self.last_click_ms = now;
        self.last_click_row = row_abs;
        let old_sel = self.selected;
        self.selected = row_abs;

        if is_dbl {
            self.last_click_row = usize::MAX;
            self.open_selected()
        } else if self.selected != old_sel {
            AppAction::RedrawAll
        } else {
            AppAction::Nothing
        }
    }

    fn mouse_move_files(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        // Update context menu hover
        if self.ctx.visible {
            let mw = self.ctx.width() as i32;
            let mh = self.ctx.height() as i32;
            let mx = self.ctx.x;
            let my = self.ctx.y;
            let new_hover = if rel_x >= mx && rel_x < mx + mw && rel_y >= my && rel_y < my + mh {
                let idx = ((rel_y - my - 2) / CTX_ITEM_H as i32) as usize;
                if idx < self.ctx.item_count {
                    Some(idx)
                } else {
                    None
                }
            } else {
                None
            };
            if new_hover != self.ctx.hover {
                self.ctx.hover = new_hover;
                return AppAction::RedrawArea(
                    self.ctx.x.max(0) as usize,
                    self.ctx.y.max(0) as usize,
                    self.ctx.width(),
                    self.ctx.height(),
                );
            }
            return AppAction::Nothing;
        }
        let old_hc = self.hover_crumb;
        let old_row = self.hover_row;

        // Breadcrumb hover (header area)
        let new_hc = if rel_y >= 0 && rel_y < HEADER_H as i32 {
            let lbl_w = "Location: ".len() * CHAR_W;
            let sep_w = (" > ".len() * CHAR_W) as i32;
            let thispc_w = ("This PC".len() * CHAR_W) as i32;
            let crumb_x0 = (PAD_X + lbl_w) as i32;
            // Check "This PC" root crumb (always clickable)
            if rel_x >= crumb_x0 && rel_x < crumb_x0 + thispc_w {
                Some(usize::MAX) // sentinel for "This PC"
            } else {
                let vfs_x0 = crumb_x0 + thispc_w + sep_w;
                let path = &self.cwd.data[..self.cwd.len];
                let mut segs = [(0usize, 0usize); MAX_CRUMBS];
                let vfs_seg_count = parse_crumbs(path, &mut segs);
                let fat32_depth = self.fat32_stack_depth;
                let fat32_crumb_nlens = self.fat32_crumb_nlens;
                let skip_bare_root = fat32_depth > 0 && vfs_seg_count == 1;
                let total_segs = (if skip_bare_root { 0 } else { vfs_seg_count }) + fat32_depth;
                let mut x = vfs_x0;
                let mut found = None;
                for i in 0..total_segs {
                    if i > 0 {
                        x += sep_w;
                    }
                    let seg_w: i32 = if !skip_bare_root && i < vfs_seg_count {
                        let (_, bl) = segs[i];
                        (bl * CHAR_W) as i32
                    } else {
                        let fi = if skip_bare_root { i } else { i - vfs_seg_count };
                        (fat32_crumb_nlens[fi] * CHAR_W) as i32
                    };
                    // Only highlight clickable (non-last) segments
                    if rel_x >= x && rel_x < x + seg_w && i < total_segs - 1 {
                        found = Some(i);
                        break;
                    }
                    x += seg_w;
                }
                found
            }
        } else {
            None
        };
        self.hover_crumb = new_hc;

        // Row hover (list area)
        let list_top = (HEADER_H + COL_HDR_H) as i32;
        let new_hover = if rel_y >= list_top {
            let row_in_view = ((rel_y - list_top) as usize) / ROW_H;
            let row_abs = self.scroll + row_in_view;
            if row_abs < self.count {
                Some(row_abs)
            } else {
                None
            }
        } else {
            None
        };
        self.hover_row = new_hover;

        if old_hc != self.hover_crumb || old_row != self.hover_row {
            let crumb_damage = Self::union_damage(
                self.hover_crumb_damage(old_hc),
                self.hover_crumb_damage(self.hover_crumb),
            );
            let row_damage = Self::union_damage(
                self.hover_row_damage(old_row),
                self.hover_row_damage(self.hover_row),
            );
            if let Some((x, y, w, h)) = Self::union_damage(crumb_damage, row_damage) {
                AppAction::RedrawArea(x, y, w, h)
            } else {
                AppAction::Nothing
            }
        } else {
            AppAction::Nothing
        }
    }

    fn right_click_files(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        let list_top = (HEADER_H + COL_HDR_H) as i32;
        let hint_top = (self.preferred_size().1 as i32).saturating_sub(HINT_H as i32);
        if rel_y >= list_top && rel_y < hint_top {
            let row_in_view = ((rel_y - list_top) as usize) / ROW_H;
            let row_abs = self.scroll + row_in_view;
            let row = if row_abs < self.count {
                self.selected = row_abs;
                row_abs
            } else {
                usize::MAX
            };
            self.open_ctx_for(rel_x, rel_y, row);
            return AppAction::RedrawAll;
        }
        AppAction::Nothing
    }
}

