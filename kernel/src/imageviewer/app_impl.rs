// ── App trait ─────────────────────────────────────────────────────────────────

impl App for ImageViewerApp {
    fn title(&self) -> &str {
        if self.title_len > 0 {
            core::str::from_utf8(&self.title_buf[..self.title_len]).unwrap_or("Image Viewer")
        } else {
            "Image Viewer"
        }
    }

    fn app_id(&self) -> &'static str {
        "imageviewer"
    }

    fn preferred_size(&self) -> (usize, usize) {
        (640, 480)
    }

    fn allow_multiple_instances(&self) -> bool {
        true
    }

    fn refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        // Header bar
        self.draw_header(cx, cy, cw);

        // Canvas area (below header, above status)
        let canvas_y = cy + HEADER_H;
        let canvas_h = ch.saturating_sub(HEADER_H + STATUS_H);

        if self.state == ViewState::Loaded {
            self.draw_checkerboard(cx, canvas_y, cw, canvas_h);
            self.draw_image(cx, canvas_y, cw, canvas_h);
        } else {
            framebuffer::fill_rect(cx, canvas_y, cw, canvas_h, BG);
            self.draw_welcome(cx, canvas_y, cw, canvas_h);
        }

        // Status bar
        self.draw_status(cx, cy, cw, ch);
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        match key {
            Key::Char(b'+') | Key::Char(b'=') => {
                if self.state == ViewState::Loaded {
                    self.zoom_in();
                    AppAction::RedrawAll
                } else {
                    AppAction::Nothing
                }
            }
            Key::Char(b'-') | Key::Char(b'_') => {
                if self.state == ViewState::Loaded {
                    self.zoom_out();
                    AppAction::RedrawAll
                } else {
                    AppAction::Nothing
                }
            }
            Key::Char(b'r') | Key::Char(b'R') => {
                if self.state == ViewState::Loaded {
                    self.reset_view();
                    AppAction::RedrawAll
                } else {
                    AppAction::Nothing
                }
            }
            Key::ArrowLeft => {
                if self.state == ViewState::Loaded {
                    self.pan_x -= 16;
                    AppAction::RedrawAll
                } else {
                    AppAction::Nothing
                }
            }
            Key::ArrowRight => {
                if self.state == ViewState::Loaded {
                    self.pan_x += 16;
                    AppAction::RedrawAll
                } else {
                    AppAction::Nothing
                }
            }
            Key::ArrowUp => {
                if self.state == ViewState::Loaded {
                    self.pan_y -= 16;
                    AppAction::RedrawAll
                } else {
                    AppAction::Nothing
                }
            }
            Key::ArrowDown => {
                if self.state == ViewState::Loaded {
                    self.pan_y += 16;
                    AppAction::RedrawAll
                } else {
                    AppAction::Nothing
                }
            }
            Key::Escape => {
                self.state = ViewState::Empty;
                self.title_len = 0;
                self.path_len = 0;
                AppAction::RedrawAll
            }
            _ => AppAction::Nothing,
        }
    }
}

