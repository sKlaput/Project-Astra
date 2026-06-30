impl App for TetrisApp {
    fn title(&self) -> &str {
        "Tetris"
    }
    fn preferred_size(&self) -> (usize, usize) {
        (WIN_W, WIN_H)
    }
    fn app_id(&self) -> &'static str {
        "tetris"
    }
    fn allow_multiple_instances(&self) -> bool {
        false
    }

    fn refresh_interval_ms(&self) -> Option<u64> {
        Some(50) // 20 fps max
    }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        // Advance game logic via interior-mutability cast (single-threaded kernel).
        let s = unsafe { &mut *self.inner.get() };
        s.tick();

        framebuffer::fill_rect(cx, cy, cw, ch, BG);

        // ── Board background ──────────────────────────────────────────────
        let bx = cx + BOARD_X;
        let by = cy + BOARD_Y;
        framebuffer::fill_rect(bx, by, BOARD_W, BOARD_H, BOARD_BG);
        // Grid lines
        for c in 1..COLS {
            framebuffer::fill_rect(bx + c * CELL, by, 1, BOARD_H, GRID_LINE);
        }
        for r in 1..ROWS {
            framebuffer::fill_rect(bx, by + r * CELL, BOARD_W, 1, GRID_LINE);
        }
        // Board border
        framebuffer::fill_rect(
            bx.saturating_sub(2),
            by.saturating_sub(2),
            BOARD_W + 4,
            2,
            BORDER,
        );
        framebuffer::fill_rect(bx.saturating_sub(2), by + BOARD_H, BOARD_W + 4, 2, BORDER);
        framebuffer::fill_rect(
            bx.saturating_sub(2),
            by.saturating_sub(2),
            2,
            BOARD_H + 4,
            BORDER,
        );
        framebuffer::fill_rect(bx + BOARD_W, by.saturating_sub(2), 2, BOARD_H + 4, BORDER);

        // ── Locked cells ──────────────────────────────────────────────────
        for r in 0..ROWS {
            for c in 0..COLS {
                let v = s.board[r][c];
                if v != 0 {
                    draw_cell(bx + c * CELL, by + r * CELL, v, false);
                }
            }
        }

        // ── Ghost piece ───────────────────────────────────────────────────
        if s.phase == Phase::Playing {
            let gr = s.ghost_row();
            if gr != s.pr {
                for (c, _r) in s.cells(s.piece, s.rot, s.pc, gr) {
                    if c >= 0 && (c as usize) < COLS {
                        let cc = c as usize;
                        let rr = gr.max(0) as usize;
                        if rr < ROWS {
                            draw_cell(bx + cc * CELL, by + rr * CELL, s.piece, true);
                        }
                    }
                }
            }
        }

        // ── Active piece ──────────────────────────────────────────────────
        if s.phase == Phase::Playing || s.phase == Phase::Paused {
            for (c, r) in s.current_cells() {
                if c >= 0 && r >= 0 && (c as usize) < COLS && (r as usize) < ROWS {
                    draw_cell(
                        bx + c as usize * CELL,
                        by + r as usize * CELL,
                        s.piece,
                        false,
                    );
                }
            }
        }

        // ── Right panel ───────────────────────────────────────────────────
        let px = cx + PANEL_X;
        framebuffer::fill_rect(px, cy + BOARD_Y, PANEL_W, BOARD_H, PANEL_BG);

        let mut py = cy + BOARD_Y + 4;

        // Next piece
        draw_label(px + 2, py, "NEXT");
        py += 12;
        framebuffer::fill_rect(px, py, PANEL_W, 36, BOARD_BG);
        draw_mini_piece(s.peek_next(), px + 4, py + 4);
        py += 44;

        // Hold piece
        draw_label(px + 2, py, "HOLD");
        py += 12;
        framebuffer::fill_rect(px, py, PANEL_W, 36, BOARD_BG);
        if s.hold != 0 {
            draw_mini_piece(s.hold, px + 4, py + 4);
            if s.hold_used {
                // Dim the hold box to show it can't be used again this piece
                framebuffer::fill_rect(px, py, PANEL_W, 36, 0x090D12_u32.wrapping_add(0x101010));
            }
        }
        py += 48;

        // Score
        draw_label(px + 2, py, "SCORE");
        py += 12;
        draw_value_u32(px + 2, py, s.score);
        py += 18;

        // Lines
        draw_label(px + 2, py, "LINES");
        py += 12;
        draw_value_u32(px + 2, py, s.lines);
        py += 18;

        // Level
        draw_label(px + 2, py, "LEVEL");
        py += 12;
        draw_value_u32(px + 2, py, s.level as u32);
        py += 24;

        // Controls hint
        let hints: &[&str] = &[
            "\u{2190}\u{2192}=move",
            "\u{2191}=rotate",
            "\u{2193}=soft drop",
            "SPC=hard drop",
            "C=hold",
            "P=pause",
            "R=restart",
        ];
        for h in hints {
            framebuffer::draw_text_at(px + 2, py, h, LABEL_COL);
            py += 11;
        }

        // ── Overlays ──────────────────────────────────────────────────────
        let ov_x = bx + BOARD_W / 4;
        let ov_w = BOARD_W / 2;

        if s.phase == Phase::GameOver {
            framebuffer::fill_rect(ov_x, by + BOARD_H / 3 - 10, ov_w, 54, OVER_BG);
            framebuffer::fill_rect(ov_x, by + BOARD_H / 3 - 10, ov_w, 1, BORDER);
            framebuffer::fill_rect(ov_x, by + BOARD_H / 3 + 44, ov_w, 1, BORDER);
            let ty = by + BOARD_H / 3;
            framebuffer::draw_text_at(ov_x + 4, ty, "GAME OVER", OVER_TITLE);
            framebuffer::draw_text_at(ov_x + 4, ty + 14, "R=restart", OVER_TEXT);
        } else if s.phase == Phase::Paused {
            framebuffer::fill_rect(ov_x, by + BOARD_H / 3 - 10, ov_w, 38, OVER_BG);
            framebuffer::fill_rect(ov_x, by + BOARD_H / 3 - 10, ov_w, 1, BORDER);
            framebuffer::fill_rect(ov_x, by + BOARD_H / 3 + 28, ov_w, 1, BORDER);
            let ty = by + BOARD_H / 3;
            framebuffer::draw_text_at(ov_x + 12, ty, "PAUSED", PAUSE_COL);
            framebuffer::draw_text_at(ov_x + 4, ty + 14, "P=resume", OVER_TEXT);
        }
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        let s = self.state_mut();

        // R always restarts
        if key == Key::Char(b'r') || key == Key::Char(b'R') {
            s.reset();
            return AppAction::RedrawAll;
        }

        match s.phase {
            Phase::GameOver => {
                // Any key other than R (handled above) → nothing
                return AppAction::Nothing;
            }
            Phase::Paused => {
                if key == Key::Char(b'p') || key == Key::Char(b'P') {
                    s.phase = Phase::Playing;
                    s.last_drop_ms = uptime_ms();
                    return AppAction::RedrawAll;
                }
                return AppAction::Nothing;
            }
            Phase::Playing => {}
        }

        match key {
            Key::ArrowLeft => {
                s.try_move(-1, 0);
            }
            Key::ArrowRight => {
                s.try_move(1, 0);
            }
            Key::ArrowDown => {
                if s.try_move(0, 1) {
                    s.score += 1;
                    s.last_drop_ms = uptime_ms();
                }
            }
            Key::ArrowUp => {
                s.try_rotate(true);
            }
            Key::Char(b'z') | Key::Char(b'Z') => {
                s.try_rotate(false);
            }
            Key::Char(b' ') => {
                s.hard_drop();
            }
            Key::Char(b'c') | Key::Char(b'C') => {
                s.do_hold();
            }
            Key::Char(b'p') | Key::Char(b'P') => {
                s.phase = Phase::Paused;
            }
            _ => return AppAction::Nothing,
        }

        AppAction::RedrawAll
    }
}

