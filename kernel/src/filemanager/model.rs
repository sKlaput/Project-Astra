// ── Path buffer ───────────────────────────────────────────────────────────────

#[derive(Clone)]
struct PathBuf {
    data: [u8; 128],
    len: usize,
}

impl PathBuf {
    fn root() -> Self {
        let mut b = PathBuf {
            data: [0u8; 128],
            len: 1,
        };
        b.data[0] = b'/';
        b
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.data[..self.len]).unwrap_or("/")
    }

    fn push(&mut self, name: &str) {
        if self.len > 0 && self.data[self.len - 1] != b'/' {
            if self.len < self.data.len() {
                self.data[self.len] = b'/';
                self.len += 1;
            }
        }
        for b in name.bytes() {
            if self.len < self.data.len() {
                self.data[self.len] = b;
                self.len += 1;
            }
        }
    }

    fn pop(&mut self) {
        if self.len <= 1 {
            return;
        }
        if self.data[self.len - 1] == b'/' {
            self.len -= 1;
        }
        while self.len > 1 && self.data[self.len - 1] != b'/' {
            self.len -= 1;
        }
    }
}

// ── Entry ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Entry {
    name: [u8; 32],
    nlen: usize,
    is_dir: bool,
    is_dyn: bool,   // created via dynamic layer — can be deleted/renamed
    is_fat32: bool, // backed by FAT32 disk
    node_id: u16,   // VFS NodeId (used for dyn ops)
    size: usize,
}

impl Entry {
    const EMPTY: Self = Entry {
        name: [0u8; 32],
        nlen: 0,
        is_dir: false,
        is_dyn: false,
        is_fat32: false,
        node_id: 0,
        size: 0,
    };

    fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.nlen]).unwrap_or("?")
    }
}
