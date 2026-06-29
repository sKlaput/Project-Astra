impl App for FileManagerApp {
    fn title(&self) -> &str {
        if self.view == FmView::ThisPc {
            "This PC"
        } else {
            "Files"
        }
    }
    fn preferred_size(&self) -> (usize, usize) {
        (560, 440)
    }
    fn app_id(&self) -> &'static str {
        "filemanager"
    }
    fn allow_multiple_instances(&self) -> bool {
        true
    }
    fn refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        if self.view == FmView::ThisPc {
            self.render_this_pc(cx, cy, cw, ch);
            return;
        }
        self.render_files(cx, cy, cw, ch);
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        if self.view == FmView::ThisPc {
            return self.key_this_pc(key);
        }
        self.key_files(key)
    }

    fn handle_mouse_click(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        if self.view == FmView::ThisPc {
            return self.mouse_click_this_pc(rel_x, rel_y);
        }
        self.mouse_click_files(rel_x, rel_y)
    }

    fn handle_mouse_move(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        if self.view == FmView::ThisPc {
            return self.mouse_move_this_pc(rel_x, rel_y);
        }
        self.mouse_move_files(rel_x, rel_y)
    }

    fn handle_mouse_right_click(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        if self.view == FmView::ThisPc {
            return AppAction::Nothing; // no right-click menu on This PC for now
        }
        self.right_click_files(rel_x, rel_y)
    }
}

