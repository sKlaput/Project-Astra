impl App for NotesApp {
    fn title(&self) -> &str {
        "Notes"
    }
    fn app_id(&self) -> &'static str {
        "notes"
    }
    fn preferred_size(&self) -> (usize, usize) {
        (600, 440)
    }
    fn allow_multiple_instances(&self) -> bool {
        false
    }
    fn refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        // Header
        framebuffer::fill_rect(cx, cy, cw, HEADER_H, HEADER_BG);
        framebuffer::fill_rect(cx, cy + HEADER_H - 1, cw, 1, BORDER_COL);
        let title = match self.save_state {
            SaveState::Dirty => "Notes  [modified]",
            SaveState::JustSaved => "Notes  [saved]",
            SaveState::Clean => "Notes",
        };
        let title_col = match self.save_state {
            SaveState::Dirty => DIRTY_COL,
            SaveState::JustSaved => SAVED_COL,
            SaveState::Clean => HEADER_COL,
        };
        framebuffer::draw_text_at(cx + PAD_X, cy + (HEADER_H - 8) / 2, title, title_col);
        // Help hint in header
        let hint = "Ctrl+S save  Ctrl+L clear";
        let hx = cx + cw.saturating_sub(hint.len() * 6 + 8);
        framebuffer::draw_text_at(hx, cy + (HEADER_H - 8) / 2, hint, STATUS_COL);

        // Text area background
        let text_y = cy + HEADER_H;
        let text_h = ch.saturating_sub(HEADER_H + STATUS_H);
        framebuffer::fill_rect(cx, text_y, cw, text_h, BG);

        // Line number gutter
        framebuffer::fill_rect(cx, text_y, LNUM_W, text_h, HEADER_BG);
        framebuffer::fill_rect(cx + LNUM_W - 1, text_y, 1, text_h, BORDER_COL);

        let visible = Self::visible_rows(ch);
        let cur_line = self.cursor_line();

        for row in 0..visible {
            let line_idx = self.scroll + row;
            if line_idx >= self.line_count {
                break;
            }

            let ry = text_y + row * ROW_H;

            // Line number
            let mut lbuf = [0u8; 8];
            let llen = fmt_usize(&mut lbuf, line_idx + 1);
            let lstr = core::str::from_utf8(&lbuf[..llen]).unwrap_or("");
            let lnum_col = if line_idx == cur_line {
                HEADER_COL
            } else {
                LINE_NUM
            };
            framebuffer::draw_text_at(cx + 2, ry + 2, lstr, lnum_col);

            // Line text
            let start = self.lines[line_idx];
            let end = if line_idx + 1 < self.line_count {
                self.lines[line_idx + 1].saturating_sub(1)
            } else {
                self.buf_len
            };
            let max_chars = (cw.saturating_sub(LNUM_W + PAD_X)) / CHAR_W;
            let text_bytes = &self.buf[start..end];
            let display_len = text_bytes.len().min(max_chars);
            if let Ok(s) = core::str::from_utf8(&text_bytes[..display_len]) {
                framebuffer::draw_text_at(cx + LNUM_W + PAD_X, ry + 2, s, TEXT_COL);
            }

            // Cursor
            if line_idx == cur_line {
                let col = self.cursor - start;
                let cx2 = cx + LNUM_W + PAD_X + col * CHAR_W;
                framebuffer::fill_rect(cx2, ry + 1, 2, ROW_H - 2, CURSOR_COL);
            }
        }

        // Status bar
        let sy = cy + ch - STATUS_H;
        framebuffer::fill_rect(cx, sy, cw, STATUS_H, STATUS_BG);
        framebuffer::fill_rect(cx, sy, cw, 1, BORDER_COL);
        let mut sbuf = [0u8; 80];
        let mut si = 0usize;
        // "Ln N  Col N  N chars"
        write_label(&mut sbuf, &mut si, b"Ln ");
        write_usize_s(&mut sbuf, &mut si, cur_line + 1);
        write_label(&mut sbuf, &mut si, b"  Col ");
        write_usize_s(&mut sbuf, &mut si, self.cursor_col() + 1);
        write_label(&mut sbuf, &mut si, b"  ");
        write_usize_s(&mut sbuf, &mut si, self.buf_len);
        write_label(&mut sbuf, &mut si, b" chars");
        let stat = core::str::from_utf8(&sbuf[..si]).unwrap_or("");
        framebuffer::draw_text_at(cx + PAD_X, sy + (STATUS_H - 8) / 2, stat, STATUS_VAL);
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        let ch_before = self.buf_len;
        let cur_before = self.cursor;

        match key {
            Key::Char(b'\x13') => {
                // Ctrl+S
                self.save();
                return AppAction::RedrawAll;
            }
            Key::Char(b'\x0C') => {
                // Ctrl+L
                self.clear_all();
                self.scroll = 0;
                return AppAction::RedrawAll;
            }
            Key::Char(b'\x08') | Key::Backspace => {
                self.delete_back();
            }
            Key::Char(b'\x04') => {
                // Ctrl+D
                self.delete_fwd();
            }
            Key::Char(b'\r') | Key::Char(b'\n') | Key::Enter => {
                self.insert_byte(b'\n');
            }
            Key::ArrowLeft => self.move_left(),
            Key::ArrowRight => self.move_right(),
            Key::ArrowUp => self.move_up(),
            Key::ArrowDown => self.move_down(),
            Key::Char(b) if b >= 0x20 => {
                self.insert_byte(b);
            }
            _ => return AppAction::Nothing,
        }

        // Decrement flash counter
        if self.flash_ticks > 0 {
            self.flash_ticks -= 1;
            if self.flash_ticks == 0 {
                if self.save_state == SaveState::JustSaved {
                    self.save_state = SaveState::Clean;
                }
            }
        }

        let visible = Self::visible_rows(440); // use preferred height (440)
        self.ensure_cursor_visible(visible);

        if self.buf_len != ch_before || self.cursor != cur_before {
            AppAction::RedrawAll
        } else {
            AppAction::Nothing
        }
    }
}
