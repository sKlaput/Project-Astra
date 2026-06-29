// ── Files view (existing logic, renamed from impl App) ────────────────────────

impl FileManagerApp {
    fn render_files(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        framebuffer::fill_rect(cx, cy, cw, ch, BG);

        // ── Path header (breadcrumb navigation) ──────────────────────────
        framebuffer::fill_rect(cx, cy, cw, HEADER_H, PATH_BG);
        framebuffer::fill_rect(cx, cy + HEADER_H - 1, cw, 1, BORDER_COL);
        let hdr_ty = cy + (HEADER_H - 8) / 2;
        let lbl = "Location: ";
        let lbl_w = lbl.len() * CHAR_W;
        framebuffer::draw_text_at(cx + PAD_X, hdr_ty, lbl, PATH_LBL);
        // Draw item count on the right first so we know the right boundary
        let crumb_clip = if !self.load_err {
            let mut cbuf = [0u8; 20];
            let cstr = fmt_count(&mut cbuf, self.count);
            let rx = cx + cw.saturating_sub(PAD_X + cstr.len() * CHAR_W);
            framebuffer::draw_text_at(rx, hdr_ty, cstr, COUNT_COL);
            rx.saturating_sub(PAD_X / 2)
        } else {
            cx + cw
        };
        // "This PC" as the first clickable breadcrumb
        let thispc_label = "This PC";
        let thispc_w = thispc_label.len() * CHAR_W;
        let mut draw_x = cx + PAD_X + lbl_w;
        // hover_crumb == Some(usize::MAX) signals hovering over "This PC" crumb
        let thispc_hover = self.hover_crumb == Some(usize::MAX);
        if thispc_hover {
            framebuffer::fill_rect(
                draw_x.saturating_sub(2),
                hdr_ty.saturating_sub(2),
                thispc_w + 4,
                12,
                0x142A40,
            );
        }
        framebuffer::draw_text_at(
            draw_x,
            hdr_ty,
            thispc_label,
            if thispc_hover { CRUMB_HOV } else { CRUMB_COL },
        );
        draw_x += thispc_w;
        // Render breadcrumb segments (VFS path + any FAT32 subdir levels)
        let path = &self.cwd.data[..self.cwd.len];
        let mut segs = [(0usize, 0usize); MAX_CRUMBS];
        let vfs_seg_count = parse_crumbs(path, &mut segs);
        // When we're directly inside a FAT32 directory from the VFS root ("/"),
        // suppress the lone "/" VFS segment so the crumb reads
        // "This PC > FolderName" instead of "This PC > / > FolderName".
        let skip_bare_root = self.fat32_stack_depth > 0 && vfs_seg_count == 1;
        // Total segment count = VFS segs (possibly suppressed) + FAT32 stack depth
        let total_segs = (if skip_bare_root { 0 } else { vfs_seg_count }) + self.fat32_stack_depth;
        let sep = " > ";
        let sep_w = sep.len() * CHAR_W;
        for i in 0..total_segs {
            // Always draw a separator before each VFS/FAT32 segment
            if draw_x + sep_w > crumb_clip {
                break;
            }
            framebuffer::draw_text_at(draw_x, hdr_ty, sep, CRUMB_SEP);
            draw_x += sep_w;
            let is_last = i == total_segs - 1;
            // FAT32 stack segment?
            let seg_str_buf: [u8; 32];
            let seg_str: &str = if !skip_bare_root && i < vfs_seg_count {
                let (bs, bl) = segs[i];
                core::str::from_utf8(&path[bs..bs + bl]).unwrap_or("?")
            } else {
                // FAT32 stack index: when skip_bare_root, i maps directly to fi;
                // otherwise offset by vfs_seg_count.
                let fi = if skip_bare_root { i } else { i - vfs_seg_count };
                seg_str_buf = self.fat32_crumb_names[fi];
                let flen = self.fat32_crumb_nlens[fi];
                core::str::from_utf8(&seg_str_buf[..flen]).unwrap_or("?")
            };
            let seg_px = seg_str.len() * CHAR_W;
            if draw_x + seg_px > crumb_clip {
                break;
            }
            let col = if is_last {
                CRUMB_CUR
            } else if Some(i) == self.hover_crumb {
                CRUMB_HOV
            } else {
                CRUMB_COL
            };
            if Some(i) == self.hover_crumb && !is_last {
                framebuffer::fill_rect(
                    draw_x.saturating_sub(2),
                    hdr_ty.saturating_sub(2),
                    seg_px + 4,
                    12,
                    0x142A40,
                );
            }
            framebuffer::draw_text_at(draw_x, hdr_ty, seg_str, col);
            draw_x += seg_px;
        }

        // ── Column header ─────────────────────────────────────────────────
        let col_y = cy + HEADER_H;
        framebuffer::fill_rect(cx, col_y, cw, COL_HDR_H, COLHDR_BG);
        framebuffer::draw_text_at(
            cx + PAD_X + PREFIX_W,
            col_y + (COL_HDR_H - 8) / 2,
            "Name",
            COLHDR_COL,
        );
        let sz_x = cx + cw.saturating_sub(PAD_X + SIZE_COL_W);
        framebuffer::draw_text_at(sz_x, col_y + (COL_HDR_H - 8) / 2, "Size", COLHDR_COL);
        framebuffer::fill_rect(cx, col_y + COL_HDR_H - 1, cw, 1, BORDER_COL);

        // ── List area ─────────────────────────────────────────────────────
        let list_y = col_y + COL_HDR_H;
        let list_h = ch.saturating_sub(HEADER_H + COL_HDR_H + HINT_H);
        let visible = list_h / ROW_H;
        let scroll = self.scroll;

        if self.load_err {
            let ey = list_y + list_h / 3;
            framebuffer::draw_text_at(cx + PAD_X, ey, "[!]  Could not read directory", ERR_COL);
            framebuffer::draw_text_at(
                cx + PAD_X,
                ey + ROW_H + 4,
                "     Check that the path is valid.",
                COLHDR_COL,
            );
        } else if self.count == 0 {
            framebuffer::draw_text_at(
                cx + PAD_X,
                list_y + list_h / 3,
                "(empty directory)",
                EMPTY_COL,
            );
        } else {
            for vi in 0..visible {
                let ei = scroll + vi;
                if ei >= self.count {
                    break;
                }
                let e = &self.entries[ei];
                let ry = list_y + vi * ROW_H;
                let is_sel = ei == self.selected;

                let row_bg = if is_sel {
                    SEL_BG
                } else if Some(ei) == self.hover_row {
                    HOVER_BG
                } else if vi % 2 == 1 {
                    EVEN_BG
                } else {
                    BG
                };
                framebuffer::fill_rect(cx, ry, cw.saturating_sub(SCROLL_W), ROW_H, row_bg);
                if is_sel {
                    // Thick left-edge accent bar (5 px) + right-edge accent (2 px)
                    framebuffer::fill_rect(cx, ry, 5, ROW_H, SEL_BORDER);
                    framebuffer::fill_rect(
                        cx + cw.saturating_sub(SCROLL_W + 2),
                        ry,
                        2,
                        ROW_H,
                        SEL_BORDER,
                    );
                } else if Some(ei) == self.hover_row {
                    // Thin left indicator for hovered row so it doesn't look selected
                    framebuffer::fill_rect(cx, ry, 2, ROW_H, 0x1A3A58);
                }

                let ty = ry + (ROW_H - 8) / 2;
                let (icon, base_col) = if e.is_dir {
                    ("[>] ", DIR_COL)
                } else {
                    ("    ", FILE_COL)
                };
                let tcol = if is_sel { SEL_COL } else { base_col };

                framebuffer::draw_text_at(cx + PAD_X, ty, icon, tcol);
                let name_max =
                    cw.saturating_sub(PAD_X + PREFIX_W + SIZE_COL_W + PAD_X + SCROLL_W) / CHAR_W;
                framebuffer::draw_text_at(
                    cx + PAD_X + PREFIX_W,
                    ty,
                    truncate_str(e.name_str(), name_max),
                    tcol,
                );

                {
                    let mut sbuf = [0u8; 16];
                    let sstr = if e.is_dir {
                        let n = fmt_uint(&mut sbuf, 0, e.size);
                        let suf = b" items";
                        let end = (n + suf.len()).min(sbuf.len());
                        sbuf[n..end].copy_from_slice(&suf[..end - n]);
                        core::str::from_utf8(&sbuf[..end]).unwrap_or("")
                    } else {
                        FileManagerApp::fmt_size(&mut sbuf, e.size)
                    };
                    let sz_col = if is_sel { SIZE_SEL } else { SIZE_COL };
                    let srx = cx + cw.saturating_sub(PAD_X + sstr.len() * CHAR_W + SCROLL_W);
                    framebuffer::draw_text_at(srx, ty, sstr, sz_col);
                }
            } // end for vi

            // Scrollbar
            let sb_x = cx + cw.saturating_sub(SCROLL_W);
            framebuffer::fill_rect(sb_x, list_y, SCROLL_W, list_h, SCROLL_BG);
            if visible < self.count && list_h > 0 {
                let thumb_h = ((visible * list_h) / self.count).max(6);
                let thumb_y = if self.count > visible {
                    (scroll * (list_h - thumb_h)) / (self.count - visible)
                } else {
                    0
                };
                framebuffer::fill_rect(
                    sb_x + 1,
                    list_y + thumb_y,
                    SCROLL_W - 2,
                    thumb_h,
                    SCROLL_FG,
                );
            }
        }

        // ── Hint bar (or inline prompt) ─────────────────────────────────────────
        let hint_y = cy + ch.saturating_sub(HINT_H);
        framebuffer::fill_rect(cx, hint_y, cw, HINT_H, HEADER_BG);
        framebuffer::fill_rect(cx, hint_y, cw, 1, BORDER_COL);
        let ty = hint_y + (HINT_H - 8) / 2;

        match self.prompt.kind {
            PromptKind::None => {
                if let Some(err) = self.op_err {
                    // Red error bar background
                    framebuffer::fill_rect(cx, hint_y, cw, HINT_H, 0x2A0A0A);
                    framebuffer::fill_rect(cx, hint_y, cw, 1, 0x8A1A1A);
                    framebuffer::draw_text_at(cx + PAD_X, ty, "[!] ", 0xFF4444);
                    framebuffer::draw_text_at(cx + PAD_X + 4 * CHAR_W, ty, err, 0xFF8888);
                    let dismiss = "(any key to dismiss)";
                    let dx = cx + cw.saturating_sub(PAD_X + dismiss.len() * CHAR_W);
                    framebuffer::draw_text_at(dx, ty, dismiss, 0x885555);
                } else if let Some(msg) = self.op_ok {
                    // Green success bar
                    framebuffer::fill_rect(cx, hint_y, cw, HINT_H, 0x061206);
                    framebuffer::fill_rect(cx, hint_y, cw, 1, 0x1A5A1A);
                    framebuffer::draw_text_at(cx + PAD_X, ty, "OK  ", 0x44FF44);
                    framebuffer::draw_text_at(cx + PAD_X + 4 * CHAR_W, ty, msg, 0x88FF88);
                } else {
                    let mut hx = cx + PAD_X;
                    macro_rules! hkey {
                        ($s:expr) => {{
                            framebuffer::draw_text_at(hx, ty, $s, HINT_KEY);
                            hx += $s.len() * CHAR_W;
                        }};
                    }
                    macro_rules! hsep {
                        ($s:expr) => {{
                            framebuffer::draw_text_at(hx, ty, $s, HINT_COL);
                            hx += $s.len() * CHAR_W;
                        }};
                    }
                    hkey!("Enter");
                    hsep!("=open  ");
                    hkey!("\u{2191}\u{2193}");
                    hsep!("=nav  ");
                    hkey!("N");
                    hsep!("=file  ");
                    hkey!("M");
                    hsep!("=dir");
                    let sel_can_edit = self.count > 0 && {
                        let e = &self.entries[self.selected];
                        let is_back = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
                        (e.is_dyn || e.is_fat32) && !is_back
                    };
                    if sel_can_edit {
                        hsep!("  ");
                        hkey!("Del");
                        hsep!("=del  ");
                        hkey!("R");
                        hsep!("=ren");
                    }
                    let _ = hx;
                    let esc = "Esc=close";
                    let ex = cx + cw.saturating_sub(PAD_X + esc.len() * CHAR_W);
                    framebuffer::draw_text_at(ex, ty, esc, HINT_KEY);
                }
            }
            PromptKind::ConfirmDel => {
                // Display target filename in red
                let lbl = "Delete \"";
                framebuffer::draw_text_at(cx + PAD_X, ty, lbl, ERR_COL);
                let mut hx = cx + PAD_X + lbl.len() * CHAR_W;
                let fname =
                    core::str::from_utf8(&self.prompt.buf[..self.prompt.len]).unwrap_or("?");
                framebuffer::draw_text_at(hx, ty, fname, ERR_COL);
                hx += self.prompt.len * CHAR_W;
                framebuffer::draw_text_at(hx, ty, "\"?", ERR_COL);
                let ok = "Enter=yes  Esc=no";
                let ox = cx + cw.saturating_sub(PAD_X + ok.len() * CHAR_W);
                framebuffer::draw_text_at(ox, ty, ok, HINT_KEY);
            }
            PromptKind::New | PromptKind::Mkdir | PromptKind::Rename => {
                let lbl = match self.prompt.kind {
                    PromptKind::New => "New file: ",
                    PromptKind::Mkdir => "New folder: ",
                    _ => "Rename to: ",
                };
                framebuffer::draw_text_at(cx + PAD_X, ty, lbl, PATH_LBL);
                let ix = cx + PAD_X + lbl.len() * CHAR_W;
                let input = core::str::from_utf8(&self.prompt.buf[..self.prompt.len]).unwrap_or("");
                framebuffer::draw_text_at(ix, ty, input, PATH_COL);
                // Blinking cursor placeholder
                let cur_x = ix + self.prompt.len * CHAR_W;
                framebuffer::draw_text_at(cur_x, ty, "_", HINT_KEY);
                let ok = "Enter=ok  Esc=cancel";
                let ox = cx + cw.saturating_sub(PAD_X + ok.len() * CHAR_W);
                framebuffer::draw_text_at(ox, ty, ok, HINT_KEY);
            }
        } // end match self.prompt.kind

        // ── Context menu overlay (drawn on top of everything) ─────────────────
        if self.ctx.visible {
            let mw = self.ctx.width();
            let mh = self.ctx.height();
            let mx = (cx as i32 + self.ctx.x).max(cx as i32) as usize;
            let my = (cy as i32 + self.ctx.y).max(cy as i32) as usize;
            // Clamp so the menu never overflows the window
            let mx = if mx + mw > cx + cw {
                (cx + cw).saturating_sub(mw)
            } else {
                mx
            };
            let my = if my + mh > cy + ch {
                (cy + ch).saturating_sub(mh)
            } else {
                my
            };
            // Background + border
            framebuffer::fill_rect(mx, my, mw, mh, CTX_BORDER);
            framebuffer::fill_rect(mx + 1, my + 1, mw - 2, mh - 2, CTX_BG);
            for i in 0..self.ctx.item_count {
                let item = &self.ctx.items[i];
                let iy = my + 2 + i * CTX_ITEM_H;
                if self.ctx.hover == Some(i) && item.enabled {
                    framebuffer::fill_rect(mx + 1, iy, mw - 2, CTX_ITEM_H, CTX_SEL_BG);
                }
                let text_col = if item.enabled { CTX_COL } else { CTX_DIS };
                framebuffer::draw_text_at(
                    mx + CTX_PAD_X,
                    iy + (CTX_ITEM_H - 8) / 2,
                    item.label,
                    text_col,
                );
            }
        }
    }

    fn key_files(&mut self, key: Key) -> AppAction {
        let (_, ph) = self.preferred_size();
        let visible = Self::visible_rows(ph).max(1);

        // ── Prompt mode: all keys go to the active prompt ───────────────────────
        if self.prompt.kind != PromptKind::None {
            match key {
                Key::Escape => {
                    self.prompt = FmPrompt::DEFAULT;
                    return AppAction::RedrawAll;
                }
                Key::Enter => {
                    let mut open_file_action: Option<AppAction> = None;
                    match self.prompt.kind {
                        PromptKind::New => {
                            if self.prompt.len > 0 {
                                let name =
                                    core::str::from_utf8(&self.prompt.buf[..self.prompt.len])
                                        .unwrap_or("");
                                if crate::fat32::is_mounted() {
                                    // Create directly on disk so it survives reboot
                                    let dir_c = if self.fat32_cluster != 0 {
                                        self.fat32_cluster
                                    } else {
                                        fs::fat32_root_cluster()
                                    };
                                    if let Some(id) =
                                        fs::fat32_create_and_open(dir_c, name.as_bytes())
                                    {
                                        let mut buf = [0u8; 128];
                                        let prefix = b"/fat32/";
                                        buf[..prefix.len()].copy_from_slice(prefix);
                                        let (hex, hlen) = hex_u16(id);
                                        let total = prefix.len() + hlen;
                                        buf[prefix.len()..total].copy_from_slice(&hex[..hlen]);
                                        open_file_action = Some(AppAction::OpenFile(buf, total));
                                        self.op_ok = Some("File created");
                                        self.op_err = None;
                                    } else {
                                        self.op_err = Some(
                                            "Create failed — name may already exist or be invalid",
                                        );
                                    }
                                } else {
                                    if fs::dyn_create_file(self.prompt.target, name).is_err() {
                                        self.op_err = Some("Create failed");
                                    } else {
                                        self.op_ok = Some("File created");
                                        self.op_err = None;
                                    }
                                }
                            } else {
                                self.op_err = Some("Name cannot be empty");
                            }
                        }
                        PromptKind::Mkdir => {
                            if self.prompt.len > 0 {
                                let name =
                                    core::str::from_utf8(&self.prompt.buf[..self.prompt.len])
                                        .unwrap_or("");
                                if crate::fat32::is_mounted() {
                                    let dir_c = if self.fat32_cluster != 0 {
                                        self.fat32_cluster
                                    } else {
                                        fs::fat32_root_cluster()
                                    };
                                    if !crate::fat32::create_dir(dir_c, name.as_bytes()) {
                                        self.op_err =
                                            Some("Create folder failed — name may already exist");
                                    } else {
                                        self.op_ok = Some("Folder created");
                                        self.op_err = None;
                                    }
                                } else {
                                    if fs::dyn_create_dir(self.prompt.target, name).is_err() {
                                        self.op_err = Some("Create folder failed");
                                    } else {
                                        self.op_ok = Some("Folder created");
                                        self.op_err = None;
                                    }
                                }
                            } else {
                                self.op_err = Some("Name cannot be empty");
                            }
                        }
                        PromptKind::Rename => {
                            if self.prompt.len > 0 {
                                let new_name =
                                    core::str::from_utf8(&self.prompt.buf[..self.prompt.len])
                                        .unwrap_or("");
                                if fs::is_fat32_id(self.prompt.target) {
                                    // FAT32 rename: need old name from cache
                                    if let Some(old_name) = fs::fat32_entry_name(self.prompt.target)
                                    {
                                        let dir_c = if self.fat32_cluster != 0 {
                                            self.fat32_cluster
                                        } else {
                                            fs::fat32_root_cluster()
                                        };
                                        if !crate::fat32::rename_entry(
                                            dir_c,
                                            &old_name.0[..old_name.1],
                                            new_name.as_bytes(),
                                        ) {
                                            self.op_err = Some("Rename failed");
                                        } else {
                                            self.op_ok = Some("Renamed");
                                            self.op_err = None;
                                        }
                                    } else {
                                        self.op_err =
                                            Some("Rename failed — entry not found in cache");
                                    }
                                } else {
                                    if fs::dyn_rename_file(self.prompt.target, new_name).is_err() {
                                        self.op_err = Some("Rename failed");
                                    } else {
                                        self.op_ok = Some("Renamed");
                                        self.op_err = None;
                                    }
                                }
                            } else {
                                self.op_err = Some("Name cannot be empty");
                            }
                        }
                        PromptKind::ConfirmDel => {
                            if fs::is_fat32_id(self.prompt.target) {
                                // FAT32 delete: name is in prompt.buf (pre-filled in Delete handler)
                                let name = &self.prompt.buf[..self.prompt.len];
                                let dir_c = if self.fat32_cluster != 0 {
                                    self.fat32_cluster
                                } else {
                                    fs::fat32_root_cluster()
                                };
                                if !crate::fat32::delete_entry(dir_c, name) {
                                    self.op_err = Some("Delete failed");
                                } else {
                                    self.op_ok = Some("Deleted");
                                    self.op_err = None;
                                }
                            } else {
                                match fs::dyn_delete_node(self.prompt.target) {
                                    Err(crate::fs::VfsError::NotEmpty) => {
                                        self.op_err =
                                            Some("Not empty \u{2014} delete contents first");
                                    }
                                    _ => {
                                        self.op_ok = Some("Deleted");
                                        self.op_err = None;
                                    }
                                }
                            }
                        }
                        PromptKind::None => {}
                    }
                    self.prompt = FmPrompt::DEFAULT;
                    self.load_dir();
                    if let Some(act) = open_file_action {
                        return act;
                    }
                    return AppAction::RedrawAll;
                }
                Key::Backspace => {
                    if self.prompt.kind != PromptKind::ConfirmDel && self.prompt.len > 0 {
                        self.prompt.len -= 1;
                        self.prompt.buf[self.prompt.len] = 0;
                        return AppAction::RedrawAll;
                    }
                }
                Key::Char(c) if self.prompt.kind != PromptKind::ConfirmDel => {
                    // Accept printable ASCII except '/' (invalid in filenames)
                    let invalid_char = matches!(
                        c,
                        b'/' | b'\\' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|'
                    );
                    if c >= 0x20 && c < 0x7F && !invalid_char && self.prompt.len < 32 {
                        self.prompt.buf[self.prompt.len] = c;
                        self.prompt.len += 1;
                        return AppAction::RedrawAll;
                    }
                }
                _ => {}
            }
            return AppAction::Nothing;
        }

        // ── Normal navigation mode ────────────────────────────────────────────
        self.op_err = None; // clear stale error on any new keypress
        self.op_ok = None; // clear stale success message on any new keypress
        let old_sel = self.selected;
        let old_scroll = self.scroll;
        match key {
            Key::Escape => {
                // Go back to "This PC" root view
                self.view = FmView::ThisPc;
                self.hover_crumb = None;
                return AppAction::RedrawAll;
            }
            Key::ArrowUp => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            Key::ArrowDown => {
                if self.selected + 1 < self.count {
                    self.selected += 1;
                }
            }
            Key::Char(b'g') | Key::Home => {
                self.selected = 0;
            }
            Key::Char(b'G') | Key::End => {
                self.selected = self.count.saturating_sub(1);
            }
            Key::Tab | Key::PageDown => {
                self.selected = (self.selected + visible).min(self.count.saturating_sub(1));
            }
            Key::PageUp => {
                self.selected = self.selected.saturating_sub(visible);
            }
            Key::Backspace => {
                // Navigate to parent directory
                if self.fat32_stack_depth > 0 || self.fat32_cluster != 0 {
                    // Go up one FAT32 level using navigate_into's ".." logic
                    // synthesise a ".." selection
                    if self.count > 0
                        && self.entries[0].nlen == 2
                        && self.entries[0].name[0] == b'.'
                        && self.entries[0].name[1] == b'.'
                    {
                        self.selected = 0;
                        self.navigate_into();
                    } else {
                        self.fat32_cluster = 0;
                        self.fat32_stack_depth = 0;
                        self.load_dir();
                    }
                    self.hover_crumb = None;
                    return AppAction::RedrawAll;
                } else if self.cwd.len > 1 {
                    self.cwd.pop();
                    self.hover_crumb = None;
                    self.fat32_cluster = 0;
                    self.fat32_stack_depth = 0;
                    self.load_dir();
                    return AppAction::RedrawAll;
                }
            }
            Key::Enter | Key::Char(b' ') => {
                return self.open_selected();
            }

            // ── File management ──────────────────────────────────────────
            Key::Char(b'n') | Key::Char(b'N') => {
                // Start "new file" prompt.
                let target = fs::resolve_node_id(self.cwd.as_str()).unwrap_or(0);
                self.prompt = FmPrompt {
                    kind: PromptKind::New,
                    buf: [0u8; 32],
                    len: 0,
                    target,
                };
                return AppAction::RedrawAll;
            }
            Key::Char(b'm') | Key::Char(b'M') => {
                // Start "new folder" (mkdir) prompt.
                let target = fs::resolve_node_id(self.cwd.as_str()).unwrap_or(0);
                self.prompt = FmPrompt {
                    kind: PromptKind::Mkdir,
                    buf: [0u8; 32],
                    len: 0,
                    target,
                };
                return AppAction::RedrawAll;
            }
            Key::Delete => {
                // Delete: dynamic files or FAT32 entries
                if self.count > 0 {
                    let e = self.entries[self.selected];
                    let is_back = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
                    if !is_back && (e.is_dyn || e.is_fat32) {
                        let mut buf = [0u8; 32];
                        buf[..e.nlen].copy_from_slice(&e.name[..e.nlen]);
                        self.prompt = FmPrompt {
                            kind: PromptKind::ConfirmDel,
                            buf,
                            len: e.nlen,
                            target: e.node_id,
                        };
                        return AppAction::RedrawAll;
                    }
                }
            }
            Key::Char(b'r') | Key::Char(b'R') => {
                // Rename: dynamic files or FAT32 entries
                if self.count > 0 {
                    let e = self.entries[self.selected];
                    let is_back = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
                    if !is_back && (e.is_dyn || e.is_fat32) {
                        let mut buf = [0u8; 32];
                        buf[..e.nlen].copy_from_slice(&e.name[..e.nlen]);
                        self.prompt = FmPrompt {
                            kind: PromptKind::Rename,
                            buf,
                            len: e.nlen,
                            target: e.node_id,
                        };
                        return AppAction::RedrawAll;
                    }
                }
            }

            // ── Clipboard ────────────────────────────────────────────────────────────
            Key::Ctrl(b'c') | Key::Ctrl(b'C') => {
                if self.count > 0 {
                    let e = self.entries[self.selected];
                    let is_up = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
                    if e.is_fat32 && !is_up {
                        let mut name = [0u8; 64];
                        name[..e.nlen].copy_from_slice(&e.name[..e.nlen]);
                        let src_cluster = if self.fat32_cluster != 0 {
                            self.fat32_cluster
                        } else {
                            fs::fat32_root_cluster()
                        };
                        self.clipboard = Clipboard {
                            name,
                            name_len: e.nlen,
                            src_cluster,
                            is_cut: false,
                        };
                        return AppAction::RedrawAll;
                    }
                }
            }
            Key::Ctrl(b'x') | Key::Ctrl(b'X') => {
                if self.count > 0 {
                    let e = self.entries[self.selected];
                    let is_up = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
                    if e.is_fat32 && !is_up {
                        let mut name = [0u8; 64];
                        name[..e.nlen].copy_from_slice(&e.name[..e.nlen]);
                        let src_cluster = if self.fat32_cluster != 0 {
                            self.fat32_cluster
                        } else {
                            fs::fat32_root_cluster()
                        };
                        self.clipboard = Clipboard {
                            name,
                            name_len: e.nlen,
                            src_cluster,
                            is_cut: true,
                        };
                        return AppAction::RedrawAll;
                    }
                }
            }
            Key::Ctrl(b'v') | Key::Ctrl(b'V') => {
                self.do_paste(); // do_paste() calls load_dir() internally on success
                return AppAction::RedrawAll;
            }

            _ => return AppAction::Nothing,
        }
        self.clamp_scroll(visible);
        if self.selected != old_sel || self.scroll != old_scroll {
            AppAction::RedrawAll
        } else {
            AppAction::Nothing
        }
    }

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

