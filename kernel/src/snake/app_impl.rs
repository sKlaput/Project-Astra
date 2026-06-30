impl App for SnakeApp {
    fn title(&self) -> &str {
        "Snake"
    }
    fn app_id(&self) -> &'static str {
        "snake"
    }
    fn preferred_size(&self) -> (usize, usize) {
        (WIN_W, WIN_H)
    }
    fn allow_multiple_instances(&self) -> bool {
        false
    }
    fn refresh_interval_ms(&self) -> Option<u64> {
        Some(50)
    }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        let g = self.g();
        // Advance tick
        g.tick();

        // ── Background ────────────────────────────────────────────────────
        framebuffer::fill_rect(cx, cy, cw, ch, BG);

        // ── Grid lines ────────────────────────────────────────────────────
        let gx0 = cx + X_OFF;
        let gy0 = cy + Y_OFF;
        let gw = COLS * CELL;
        let gh = ROWS * CELL;

        // Vertical lines
        for col in 0..=COLS {
            framebuffer::fill_rect(gx0 + col * CELL, gy0, 1, gh, GRID_LINE);
        }
        // Horizontal lines
        for row in 0..=ROWS {
            framebuffer::fill_rect(gx0, gy0 + row * CELL, gw, 1, GRID_LINE);
        }
        // Border
        framebuffer::fill_rect(gx0 - 2, gy0 - 2, gw + 4, 2, BORDER);
        framebuffer::fill_rect(gx0 - 2, gy0 + gh, gw + 4, 2, BORDER);
        framebuffer::fill_rect(gx0 - 2, gy0 - 2, 2, gh + 4, BORDER);
        framebuffer::fill_rect(gx0 + gw, gy0 - 2, 2, gh + 4, BORDER);

        // ── Snake body ────────────────────────────────────────────────────
        for i in 0..g.len {
            let idx = (g.tail_idx + i) % MAX_LEN;
            let (bx, by) = g.body[idx];
            let frac = i * 3 / g.len.max(1); // 0,1,2 for dim/mid/bright
            let col = if i + 1 == g.len {
                SNAKE_HEAD
            } else if frac < 1 {
                SNAKE_DIM
            } else {
                SNAKE_BODY
            };
            draw_cell(bx as usize, by as usize, cx, cy, col);
        }

        // ── Food ──────────────────────────────────────────────────────────
        {
            let (fx, fy) = g.food;
            let px = cx + X_OFF + fx as usize * CELL;
            let py = cy + Y_OFF + fy as usize * CELL;
            framebuffer::fill_rect(px + 2, py + 2, CELL - 4, CELL - 4, FOOD);
            framebuffer::fill_rect(px + 5, py + 5, CELL - 10, CELL - 10, FOOD_INNER);
        }

        // ── Score bar (top) ───────────────────────────────────────────────
        {
            let mut buf = [0u8; 32];
            let len = fmt_score(&mut buf, b"SCORE ", g.score);
            let s = core::str::from_utf8(&buf[..len]).unwrap_or("");
            framebuffer::draw_text_at(cx + X_OFF, cy + 6, s, SCORE_COL);

            let mut hbuf = [0u8; 32];
            let hlen = fmt_score(&mut hbuf, b"BEST  ", g.high_score);
            let hs = core::str::from_utf8(&hbuf[..hlen]).unwrap_or("");
            framebuffer::draw_text_at(cx + X_OFF + 140, cy + 6, hs, HI_COL);

            let lvl = (g.score / 5) + 1;
            let mut lbuf = [0u8; 32];
            let llen = fmt_score(&mut lbuf, b"LVL ", lvl);
            let ls = core::str::from_utf8(&lbuf[..llen]).unwrap_or("");
            framebuffer::draw_text_at(cx + X_OFF + 280, cy + 6, ls, TITLE_COL);
        }

        // ── Hint bar (bottom) ─────────────────────────────────────────────
        framebuffer::draw_text_at(
            cx + X_OFF,
            cy + Y_OFF + gh + 5,
            "ARROWS/WASD: steer   SPACE: pause   R: restart",
            HINT_COL,
        );

        // ── Overlay: Ready / Paused / Game Over ───────────────────────────
        match g.phase {
            Phase::Ready => {
                draw_overlay(
                    cx,
                    cy,
                    cw,
                    ch,
                    "SNAKE",
                    TITLE_COL,
                    "Press any direction to start",
                    OVER_TEXT,
                );
            }
            Phase::Paused => {
                draw_overlay(
                    cx,
                    cy,
                    cw,
                    ch,
                    "PAUSED",
                    PAUSE_COL,
                    "Press SPACE to resume",
                    OVER_TEXT,
                );
            }
            Phase::GameOver => {
                draw_overlay(
                    cx,
                    cy,
                    cw,
                    ch,
                    "GAME OVER",
                    OVER_TITLE,
                    "Press R to play again",
                    OVER_TEXT,
                );
            }
            Phase::Playing => {}
        }
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        let g = self.g();
        match key {
            Key::Char(b'r') | Key::Char(b'R') => {
                let hi = g.high_score;
                g.reset();
                g.high_score = hi;
                AppAction::RedrawAll
            }
            Key::Char(b' ') => match g.phase {
                Phase::Playing => {
                    g.phase = Phase::Paused;
                    AppAction::RedrawAll
                }
                Phase::Paused => {
                    g.phase = Phase::Playing;
                    g.last_move_ms = uptime_ms();
                    AppAction::RedrawAll
                }
                Phase::GameOver => AppAction::Nothing,
                Phase::Ready => AppAction::Nothing,
            },
            Key::ArrowUp | Key::Char(b'w') | Key::Char(b'W') => steer(g, Dir::Up),
            Key::ArrowDown | Key::Char(b's') | Key::Char(b'S') => steer(g, Dir::Down),
            Key::ArrowLeft | Key::Char(b'a') | Key::Char(b'A') => steer(g, Dir::Left),
            Key::ArrowRight | Key::Char(b'd') | Key::Char(b'D') => steer(g, Dir::Right),
            _ => AppAction::Nothing,
        }
    }
}

