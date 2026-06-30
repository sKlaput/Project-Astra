impl App for CalculatorApp {
    fn title(&self) -> &str {
        "Calculator"
    }
    fn preferred_size(&self) -> (usize, usize) {
        let w = PAD * 2 + COLS * BTN_W + (COLS - 1) * BTN_GAP;
        let h = PAD * 3 + DISP_H + ROWS * BTN_H + (ROWS - 1) * BTN_GAP + PAD;
        (w, h)
    }
    fn app_id(&self) -> &'static str {
        "calculator"
    }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        // Background
        framebuffer::fill_rect(cx, cy, cw, ch, BG);

        // ── Display area ──────────────────────────────────────────────────
        let dx = cx + PAD;
        let dy = cy + PAD;
        let dw = COLS * BTN_W + (COLS - 1) * BTN_GAP;
        let dh = DISP_H;
        framebuffer::fill_rect(dx, dy, dw, dh, DISPLAY_BG);
        // Thin top border
        framebuffer::fill_rect(dx, dy, dw, 2, 0x2A3F5F);

        // Pending operator in top-right of display
        if let Some(op) = self.pending_op {
            let op_ch = [op as u8];
            let op_str = unsafe { core::str::from_utf8_unchecked(&op_ch) };
            framebuffer::draw_text_at(dx + dw.saturating_sub(14), dy + 4, op_str, DISPLAY_OP);
        }

        // Main display value (scale-2 text, right-aligned)
        let display_text: &str = if self.error {
            "Error"
        } else if self.input_len == 0 {
            "0"
        } else {
            unsafe { core::str::from_utf8_unchecked(&self.input[..self.input_len]) }
        };

        let text_col = if self.error { ERR_COL } else { DISPLAY_TXT };
        // Scale-2 rendering: draw each character at 2× by drawing 4 fill_rect quads
        let char_w = 6usize;
        let char_h = 7usize;
        let scale = 2usize;
        let sw = char_w * scale;
        let sh = char_h * scale;
        let text_bytes = display_text.as_bytes();
        let text_pixel_w = text_bytes.len() * sw;
        let text_x = if text_pixel_w < dw.saturating_sub(8) {
            dx + dw.saturating_sub(text_pixel_w + 8)
        } else {
            dx + 4
        };
        let text_y = dy + (dh - sh) / 2;
        framebuffer::draw_text_at(text_x, text_y, display_text, text_col);

        // ── Buttons ───────────────────────────────────────────────────────
        for row in 0..ROWS {
            for col in 0..COLS {
                let btn = &BTNS[row][col];
                let (bx, by, bw, bh) = self.btn_rect(row, col, cx, cy);
                let hov = self.hovered == Some((row, col));

                let bg = match btn.kind {
                    BtnKind::Clear => {
                        if hov {
                            BTN_CLR_HOV
                        } else {
                            BTN_CLR_BG
                        }
                    }
                    BtnKind::Eq => {
                        if hov {
                            BTN_EQ_HOV
                        } else {
                            BTN_EQ_BG
                        }
                    }
                    BtnKind::Op(_) => {
                        if hov {
                            BTN_OP_HOV
                        } else {
                            BTN_OP_BG
                        }
                    }
                    _ => {
                        if hov {
                            BTN_HOV
                        } else {
                            BTN_BG
                        }
                    }
                };

                framebuffer::fill_rect(bx, by, bw, bh, bg);
                // Border
                framebuffer::fill_rect(bx, by, bw, 1, BTN_BORDER);
                framebuffer::fill_rect(bx, by, 1, bh, BTN_BORDER);
                framebuffer::fill_rect(bx, by + bh - 1, bw, 1, BTN_BORDER);
                framebuffer::fill_rect(bx + bw - 1, by, 1, bh, BTN_BORDER);

                // Label — centered
                let label = btn.label;
                let lw = label.len() * 6;
                let lx = bx + (bw.saturating_sub(lw)) / 2;
                let ly = by + (bh.saturating_sub(7)) / 2;
                framebuffer::draw_text_at(lx, ly, label, BTN_TXT);
            }
        }
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        let kind = match key {
            Key::Char(b'0') | Key::Char(b')') => Some(BtnKind::Digit(0)),
            Key::Char(b'1') | Key::Char(b'!') => Some(BtnKind::Digit(1)),
            Key::Char(b'2') | Key::Char(b'@') => Some(BtnKind::Digit(2)),
            Key::Char(b'3') | Key::Char(b'#') => Some(BtnKind::Digit(3)),
            Key::Char(b'4') | Key::Char(b'$') => Some(BtnKind::Digit(4)),
            Key::Char(b'5') => Some(BtnKind::Digit(5)),
            Key::Char(b'6') => Some(BtnKind::Digit(6)),
            Key::Char(b'7') => Some(BtnKind::Digit(7)),
            Key::Char(b'8') => Some(BtnKind::Digit(8)),
            Key::Char(b'9') => Some(BtnKind::Digit(9)),
            Key::Char(b'.') | Key::Char(b',') => Some(BtnKind::Dot),
            Key::Char(b'+') => Some(BtnKind::Op('+')),
            Key::Char(b'-') => Some(BtnKind::Op('-')),
            Key::Char(b'*') => Some(BtnKind::Op('*')),
            Key::Char(b'/') => Some(BtnKind::Op('/')),
            Key::Char(b'=') | Key::Enter => Some(BtnKind::Eq),
            Key::Backspace => Some(BtnKind::Backspace),
            Key::Escape => Some(BtnKind::Clear),
            Key::Char(b'%') => Some(BtnKind::Percent),
            _ => None,
        };

        if let Some(k) = kind {
            if self.press(k) {
                return AppAction::RedrawAll;
            }
        }
        AppAction::Nothing
    }

    fn handle_mouse_click(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        if let Some((row, col)) = self.hit_test(rel_x, rel_y) {
            let kind = BTNS[row][col].kind;
            self.press(kind);
            AppAction::RedrawAll
        } else {
            AppAction::Nothing
        }
    }

    fn handle_mouse_move(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        let new_hov = self.hit_test(rel_x, rel_y);
        if new_hov != self.hovered {
            let old_hov = self.hovered;
            self.hovered = new_hov;
            if let Some((x, y, w, h)) = self.hover_damage_rect(old_hov, new_hov) {
                AppAction::RedrawArea(x, y, w, h)
            } else {
                AppAction::Nothing
            }
        } else {
            AppAction::Nothing
        }
    }
}

