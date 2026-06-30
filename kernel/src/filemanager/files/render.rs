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
}

