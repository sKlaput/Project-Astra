impl App for SettingsApp {
    fn title(&self) -> &str {
        "Settings"
    }
    fn preferred_size(&self) -> (usize, usize) {
        (760, 520)
    }
    fn app_id(&self) -> &'static str {
        "settings"
    }
    fn allow_multiple_instances(&self) -> bool {
        false
    }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        framebuffer::fill_rect(cx, cy, cw, ch, BG);

        // ── Sidebar ───────────────────────────────────────────────────────
        framebuffer::fill_rect(cx, cy, SIDEBAR_W, ch, SIDEBAR_BG);
        framebuffer::fill_rect(cx + SIDEBAR_W, cy, 1, ch, SEP);

        for i in 0..NUM_TABS {
            let ty = cy + i * TAB_H + 4;
            let (bg, tc) = if i == self.tab {
                (TAB_SEL_BG, TAB_SEL_TXT)
            } else {
                (SIDEBAR_BG, TAB_TXT)
            };
            framebuffer::fill_rect(cx, ty, SIDEBAR_W, TAB_H - 2, bg);
            framebuffer::draw_text_at(cx + 10, ty + 7, TAB_LABELS[i], tc);
        }

        // Hint at bottom of sidebar
        let hint_y = cy + ch.saturating_sub(20);
        framebuffer::draw_text_at(cx + 4, hint_y, "Tab=next tab", HINT);

        // ── Content ───────────────────────────────────────────────────────
        let cx2 = cx + SIDEBAR_W + 1;
        let cw2 = cw.saturating_sub(SIDEBAR_W + 1);
        match self.tab {
            0 => render_system(cx2, cy, cw2, ch, self.row),
            1 => render_display(cx2, cy, cw2, ch, self.row),
            2 => render_input(cx2, cy, cw2, ch, self.row),
            3 => render_about(cx2, cy, cw2, ch),
            _ => {}
        }
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        match key {
            Key::Tab => {
                self.tab = (self.tab + 1) % NUM_TABS;
                self.row = 0;
                AppAction::RedrawAll
            }
            Key::ArrowUp => {
                let m = self.max_rows();
                if m > 0 {
                    self.row = if self.row == 0 { m - 1 } else { self.row - 1 };
                }
                AppAction::RedrawAll
            }
            Key::ArrowDown => {
                let m = self.max_rows();
                if m > 0 {
                    self.row = (self.row + 1) % m;
                }
                AppAction::RedrawAll
            }
            Key::Char(b'\r') | Key::Char(b' ') => {
                if self.tab == 1 && self.row < NUM_THEMES {
                    crate::desktop::DESKTOP_BG_COLOR.store(THEMES[self.row].0, AO::Relaxed);
                }
                AppAction::RedrawAll
            }
            Key::Escape => AppAction::Nothing,
            _ => AppAction::Nothing,
        }
    }
}
