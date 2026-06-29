// ── App-trait wrapper ─────────────────────────────────────────────────────────

use crate::app::{App, AppAction};

/// Single-instance terminal window.  Delegates all state to module-level globals.
pub struct TerminalApp;

impl TerminalApp {
    pub fn new() -> Self {
        init_if_needed();
        TerminalApp
    }
}

impl App for TerminalApp {
    fn title(&self) -> &str {
        "Terminal"
    }
    fn preferred_size(&self) -> (usize, usize) {
        (700, 460)
    }
    fn app_id(&self) -> &'static str {
        "terminal"
    }
    fn allow_multiple_instances(&self) -> bool {
        false
    }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        render(cx, cy, cw, ch);
    }

    fn input_region_height(&self) -> Option<usize> {
        Some(INPUT_REGION_H)
    }

    fn render_input_region(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        render_input_line(cx, cy, cw, ch);
    }

    fn handle_key(&mut self, key: crate::input::Key) -> AppAction {
        match handle_key(key) {
            TermAction::Close => AppAction::Close,
            TermAction::RedrawAll => AppAction::RedrawAll,
            TermAction::RedrawInput => AppAction::RedrawInput,
            TermAction::Nothing => AppAction::Nothing,
        }
    }

    fn handle_mouse_scroll(&mut self, delta: i32) -> AppAction {
        let mut t = TERM.lock();
        if delta > 0 {
            t.scroll_off = t.scroll_off.saturating_add(delta as usize);
        } else if delta < 0 {
            t.scroll_off = t.scroll_off.saturating_sub((-delta) as usize);
        }
        AppAction::RedrawAll
    }

    fn refresh_interval_ms(&self) -> Option<u64> {
        None
    }
}
