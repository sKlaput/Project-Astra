impl NotesApp {
    pub fn new() -> Self {
        let mut app = NotesApp {
            buf: [0u8; BUF_CAP],
            buf_len: 0,
            cursor: 0,
            scroll: 0,
            lines: [0usize; MAX_LINES],
            line_count: 1,
            save_state: SaveState::Clean,
            flash_ticks: 0,
        };
        app.lines[0] = 0;
        app.try_load();
        app
    }

    // ── Load / Save ───────────────────────────────────────────────────────────

    fn try_load(&mut self) {
        // Try FAT32 root directly.
        if crate::fat32::is_mounted() {
            let root = crate::fat32::root_cluster();
            if let Some(de) = crate::fat32::find_in_dir(root, b"notes.txt") {
                let n = crate::fat32::read_file(de.cluster, de.size, &mut self.buf);
                if n > 0 {
                    self.buf_len = n;
                    self.cursor = n;
                    self.reindex();
                    return;
                }
            }
        }
        // Try dynamic VFS.
        if let Ok(mut h) = fs::open(NOTES_DYN) {
            if let Ok(n) = fs::read(&mut h, &mut self.buf) {
                self.buf_len = n;
                self.cursor = n;
                self.reindex();
            }
        }
    }

    fn save(&mut self) {
        let data = &self.buf[..self.buf_len];
        // Try FAT32 root directly.
        if crate::fat32::is_mounted() {
            let root = crate::fat32::root_cluster();
            if crate::fat32::write_file(root, b"notes.txt", data) {
                self.save_state = SaveState::JustSaved;
                self.flash_ticks = 30;
                return;
            }
        }
        // Fallback: dynamic VFS.
        if fs::write_file(NOTES_DYN, data).is_ok() {
            self.save_state = SaveState::JustSaved;
            self.flash_ticks = 30;
        }
    }

    // ── Text editing ──────────────────────────────────────────────────────────

    fn insert_byte(&mut self, b: u8) {
        if self.buf_len >= BUF_CAP {
            return;
        }
        // Shift bytes right from cursor
        let pos = self.cursor;
        for i in (pos..self.buf_len).rev() {
            self.buf[i + 1] = self.buf[i];
        }
        self.buf[pos] = b;
        self.buf_len += 1;
        self.cursor += 1;
        self.reindex();
        self.save_state = SaveState::Dirty;
    }

    fn delete_back(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let pos = self.cursor - 1;
        for i in pos..self.buf_len - 1 {
            self.buf[i] = self.buf[i + 1];
        }
        self.buf_len -= 1;
        self.cursor = pos;
        self.reindex();
        self.save_state = SaveState::Dirty;
    }

    fn delete_fwd(&mut self) {
        if self.cursor >= self.buf_len {
            return;
        }
        let pos = self.cursor;
        for i in pos..self.buf_len - 1 {
            self.buf[i] = self.buf[i + 1];
        }
        self.buf_len -= 1;
        self.reindex();
        self.save_state = SaveState::Dirty;
    }

    fn clear_all(&mut self) {
        self.buf_len = 0;
        self.cursor = 0;
        self.lines[0] = 0;
        self.line_count = 1;
        self.save_state = SaveState::Dirty;
    }

    fn reindex(&mut self) {
        self.line_count = 0;
        self.lines[0] = 0;
        self.line_count = 1;
        for i in 0..self.buf_len {
            if self.buf[i] == b'\n' && self.line_count < MAX_LINES {
                self.lines[self.line_count] = i + 1;
                self.line_count += 1;
            }
        }
    }

    fn cursor_line(&self) -> usize {
        let mut line = 0usize;
        for i in (0..self.line_count).rev() {
            if self.lines[i] <= self.cursor {
                line = i;
                break;
            }
        }
        line
    }

    fn cursor_col(&self) -> usize {
        let line = self.cursor_line();
        self.cursor - self.lines[line]
    }

    fn move_up(&mut self) {
        let line = self.cursor_line();
        if line == 0 {
            self.cursor = 0;
            return;
        }
        let col = self.cursor_col();
        let prev_start = self.lines[line - 1];
        let prev_end = if line - 1 + 1 < self.line_count {
            self.lines[line] - 1
        } else {
            self.buf_len
        };
        let prev_len = prev_end - prev_start;
        self.cursor = prev_start + col.min(prev_len);
    }

    fn move_down(&mut self) {
        let line = self.cursor_line();
        if line + 1 >= self.line_count {
            self.cursor = self.buf_len;
            return;
        }
        let col = self.cursor_col();
        let next_start = self.lines[line + 1];
        let next_end = if line + 2 < self.line_count {
            self.lines[line + 2] - 1
        } else {
            self.buf_len
        };
        let next_len = next_end - next_start;
        self.cursor = next_start + col.min(next_len);
    }

    fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    fn move_right(&mut self) {
        if self.cursor < self.buf_len {
            self.cursor += 1;
        }
    }

    fn ensure_cursor_visible(&mut self, visible_rows: usize) {
        let line = self.cursor_line();
        if line < self.scroll {
            self.scroll = line;
        } else if visible_rows > 0 && line >= self.scroll + visible_rows {
            self.scroll = line + 1 - visible_rows;
        }
    }

    fn visible_rows(ch: usize) -> usize {
        ch.saturating_sub(HEADER_H + STATUS_H) / ROW_H
    }
}
