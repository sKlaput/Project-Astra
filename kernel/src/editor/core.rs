impl EditorApp {
    pub fn open(path: &str) -> Self {
        let writable = crate::fs::is_writable(path);
        let mut app = EditorApp {
            buf: [0u8; BUF_SIZE],
            buf_len: 0,
            lines: [0usize; MAX_LINES],
            line_count: 0,
            scroll: 0,
            title_buf: [0u8; 80],
            title_len: 0,
            path_buf: [0u8; 128],
            path_len: 0,
            state: LoadState::Ok,
            mode: if writable {
                EditorMode::Edit
            } else {
                EditorMode::View
            },
            cursor_line: 0,
            cursor_col: 0,
            dirty: false,
            writable,
            saved_flash: false,
            close_prompt: ClosePrompt::Hidden,
        };
        app.build_title(path);
        app.load(path);
        app
    }

    fn build_title(&mut self, path: &str) {
        // Store full path for saving
        let pb = path.as_bytes();
        let pn = pb.len().min(self.path_buf.len());
        self.path_buf[..pn].copy_from_slice(&pb[..pn]);
        self.path_len = pn;

        // Title shows the full path so the window bar reads e.g. "Editor - /etc/motd".
        let prefix = b"Editor - ";
        let mut i = 0usize;
        for b in prefix.iter() {
            if i < self.title_buf.len() {
                self.title_buf[i] = *b;
                i += 1;
            }
        }
        for b in path.bytes() {
            if i < self.title_buf.len() {
                self.title_buf[i] = b;
                i += 1;
            }
        }
        self.title_len = i;
    }

    fn path_str(&self) -> &str {
        core::str::from_utf8(&self.path_buf[..self.path_len]).unwrap_or("")
    }

    /// Number of content characters on line `n` (excluding newline).
    fn line_len(&self, n: usize) -> usize {
        self.line_slice(n).len()
    }

    /// Byte offset in buf for the cursor position.
    fn cursor_offset(&self) -> usize {
        if self.line_count == 0 {
            return 0;
        }
        let line = self.cursor_line.min(self.line_count.saturating_sub(1));
        self.lines[line] + self.cursor_col
    }

    /// Insert one byte at the current cursor position.
    fn insert_byte(&mut self, b: u8) {
        if self.buf_len >= BUF_SIZE {
            return;
        }
        let pos = self.cursor_offset();
        // Shift bytes right
        let mut i = self.buf_len;
        while i > pos {
            self.buf[i] = self.buf[i - 1];
            i -= 1;
        }
        self.buf[pos] = b;
        self.buf_len += 1;
        self.index_lines();
        if b == b'\n' {
            self.cursor_line += 1;
            self.cursor_col = 0;
        } else {
            self.cursor_col += 1;
        }
        self.dirty = true;
    }

    /// Delete the byte before the cursor (Backspace).
    fn delete_before_cursor(&mut self) {
        if self.line_count == 0 && self.buf_len == 0 {
            return;
        }
        if self.cursor_col > 0 {
            let pos = self.cursor_offset() - 1;
            let mut i = pos;
            while i + 1 < self.buf_len {
                self.buf[i] = self.buf[i + 1];
                i += 1;
            }
            self.buf_len -= 1;
            self.cursor_col -= 1;
            self.index_lines();
            self.dirty = true;
        } else if self.cursor_line > 0 {
            // Delete the \n at the end of the previous line
            // Previous line's content length before merge
            let prev_len = self.line_len(self.cursor_line - 1);
            let newline_pos = self.lines[self.cursor_line] - 1;
            let mut i = newline_pos;
            while i + 1 < self.buf_len {
                self.buf[i] = self.buf[i + 1];
                i += 1;
            }
            self.buf_len -= 1;
            self.cursor_line -= 1;
            self.cursor_col = prev_len;
            self.index_lines();
            self.dirty = true;
        }
    }

    /// Delete the byte at the cursor (Delete key).
    fn delete_at_cursor(&mut self) {
        if self.line_count == 0 {
            return;
        }
        let pos = self.cursor_offset();
        if pos >= self.buf_len {
            return;
        }
        let mut i = pos;
        while i + 1 < self.buf_len {
            self.buf[i] = self.buf[i + 1];
            i += 1;
        }
        self.buf_len -= 1;
        // Clamp cursor col to new line length
        self.index_lines();
        let ll = self.line_len(self.cursor_line);
        if self.cursor_col > ll {
            self.cursor_col = ll;
        }
        self.dirty = true;
    }

    /// Save buf to VFS.  Only works if writable.
    fn save(&mut self) -> bool {
        if !self.writable {
            return false;
        }
        let path = self.path_str();
        crate::serial::write_str("editor: save path=");
        crate::serial::write_line(path);
        let result = crate::fs::write_file(path, &self.buf[..self.buf_len]);
        match &result {
            Ok(n) => {
                crate::serial::write_str("editor: save OK bytes=");
                crate::serial::write_u64(*n as u64);
                crate::serial::write_line("");
            }
            Err(_) => {
                crate::serial::write_line("editor: save FAILED");
            }
        }
        matches!(result, Ok(_))
    }

    /// Scroll so the cursor line is visible.
    fn ensure_cursor_visible(&mut self, visible: usize) {
        if self.cursor_line < self.scroll {
            self.scroll = self.cursor_line;
        } else if visible > 0 && self.cursor_line >= self.scroll + visible {
            self.scroll = self.cursor_line.saturating_sub(visible - 1);
        }
    }

    fn load(&mut self, path: &str) {
        let mut handle = match fs::open(path) {
            Ok(h) => h,
            Err(_) => {
                self.state = LoadState::NotFound;
                return;
            }
        };
        match fs::read(&mut handle, &mut self.buf) {
            Ok(0) => {
                self.state = LoadState::Empty;
                return;
            }
            Ok(n) => {
                self.buf_len = n;
            }
            Err(_) => {
                self.state = LoadState::ReadError;
                return;
            }
        }
        self.index_lines();
    }

    fn index_lines(&mut self) {
        self.line_count = 0;
        if self.buf_len == 0 {
            return;
        }
        self.lines[0] = 0;
        self.line_count = 1;
        for i in 0..self.buf_len {
            if self.buf[i] == b'\n' {
                if i + 1 < self.buf_len && self.line_count < MAX_LINES {
                    self.lines[self.line_count] = i + 1;
                    self.line_count += 1;
                }
            }
        }
    }

    fn line_slice(&self, n: usize) -> &[u8] {
        if n >= self.line_count {
            return b"";
        }
        let start = self.lines[n];
        let end = if n + 1 < self.line_count {
            let e = self.lines[n + 1];
            if e > start && self.buf[e - 1] == b'\n' {
                e - 1
            } else {
                e
            }
        } else {
            self.buf_len
        };
        &self.buf[start..end]
    }

    fn visible_lines(ch: usize) -> usize {
        ch.saturating_sub(HEADER_H + STATUS_H) / ROW_H
    }

    fn scroll_percent(&self) -> usize {
        if self.line_count <= 1 {
            return 100;
        }
        (self.scroll * 100) / (self.line_count - 1)
    }

    fn page_size(ch: usize) -> usize {
        Self::visible_lines(ch).saturating_sub(2).max(1)
    }
}

