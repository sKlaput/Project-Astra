impl ImageViewerApp {
pub fn new() -> Self {
        ImageViewerApp {
            buf: Vec::new(),
            buf_used: 0,
            img_w: 0,
            img_h: 0,
            px_start: 0,
            zoom: 2,
            pan_x: 0,
            pan_y: 0,
            state: ViewState::Empty,
            title_buf: [0u8; 80],
            title_len: 0,
            path_buf: [0u8; 128],
            path_len: 0,
        }
    }

    pub fn open(path: &str) -> Self {
        let mut app = Self::new();
        app.load(path);
        app
    }

    // ── File loading ──────────────────────────────────────────────────────────

    fn load(&mut self, path: &str) {
        // Store path for display.
        let pb = path.as_bytes();
        let pn = pb.len().min(self.path_buf.len());
        self.path_buf[..pn].copy_from_slice(&pb[..pn]);
        self.path_len = pn;

        // Build window title from last path component.
        self.build_title(path);

        // Allocate / resize the file buffer.
        self.buf.resize(FILE_BUF, 0u8);

        // Read file.
        let mut handle = match fs::open(path) {
            Ok(h) => h,
            Err(_) => {
                self.state = ViewState::ReadError;
                return;
            }
        };
        let n = match fs::read(&mut handle, &mut self.buf) {
            Ok(n) => n,
            Err(_) => {
                self.state = ViewState::ReadError;
                return;
            }
        };
        self.buf_used = n;

        // Parse PPM P6 header.
        match parse_ppm_p6(&self.buf[..n]) {
            Some((w, h, px)) => {
                if w > MAX_W || h > MAX_H {
                    self.state = ViewState::TooBig;
                    return;
                }
                self.img_w = w;
                self.img_h = h;
                self.px_start = px;
                self.state = ViewState::Loaded;
                self.zoom = 2;
                self.pan_x = 0;
                self.pan_y = 0;
            }
            None => {
                // Check if it might be text (not a PPM at all).
                if n < 2 || self.buf[0] != b'P' || self.buf[1] != b'6' {
                    self.state = ViewState::NotPpm;
                } else {
                    self.state = ViewState::ParseError;
                }
            }
        }
    }

    fn build_title(&mut self, path: &str) {
        let name = path.rfind('/').map_or(path, |i| &path[i + 1..]);
        let prefix = b"Viewer - ";
        let mut i = 0usize;
        for &b in prefix {
            if i < self.title_buf.len() {
                self.title_buf[i] = b;
                i += 1;
            }
        }
        for &b in name.as_bytes() {
            if i < self.title_buf.len() {
                self.title_buf[i] = b;
                i += 1;
            }
        }
        self.title_len = i;
    }

    // ── Rendering helpers ─────────────────────────────────────────────────────
}

