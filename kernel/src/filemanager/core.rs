// ── FileManagerApp ────────────────────────────────────────────────────────────

pub struct FileManagerApp {
    cwd: PathBuf,
    fat32_cluster: u32, // 0 = not in a FAT32 directory
    /// Stack of parent FAT32 clusters, one entry per level pushed.
    /// fat32_cluster_stack[0] = cluster when we first entered FAT32 root.
    fat32_cluster_stack: [u32; 8],
    fat32_stack_depth: usize,
    /// Display names for the FAT32 breadcrumb segments (parallel to stack).
    fat32_crumb_names: [[u8; 32]; 8],
    fat32_crumb_nlens: [usize; 8],
    entries: [Entry; MAX_ENTRIES],
    count: usize,
    selected: usize,
    scroll: usize,
    hover_row: Option<usize>,
    load_err: bool,
    last_click_ms: u64,
    last_click_row: usize,
    prompt: FmPrompt,
    op_err: Option<&'static str>,
    op_ok: Option<&'static str>, // success feedback, cleared on next keypress
    ctx: CtxMenu,
    hover_crumb: Option<usize>,
    clipboard: Clipboard,
    view: FmView,
    tile_hover: Option<usize>, // hovered drive tile in ThisPc view
    tile_sel: Option<usize>,   // selected drive tile
    tile_last_click_ms: u64,
}

impl FileManagerApp {
    /// Open File Manager with the "New File" prompt already showing.
    pub fn new_file() -> Self {
        let mut app = Self::new();
        // Enter Files view first so the prompt is shown in context
        app.view = FmView::Files;
        let target = crate::fs::resolve_node_id(app.cwd.as_str()).unwrap_or(0);
        app.prompt = FmPrompt {
            kind: PromptKind::New,
            buf: [0u8; 32],
            len: 0,
            target,
        };
        app
    }

    /// Open File Manager with the Files view rooted inside a specific FAT32 folder.
    /// `cluster` is the FAT32 cluster of the folder, `name` is its display name.
    pub fn open_dir(cluster: u32, name: &[u8]) -> Self {
        // Build directly without calling new() to avoid the redundant load_dir()
        // that new() runs at the VFS root before we override the cluster.
        let mut app = FileManagerApp {
            cwd: PathBuf::root(),
            fat32_cluster: cluster,
            fat32_cluster_stack: [0u32; 8],
            fat32_stack_depth: 0,
            fat32_crumb_names: [[0u8; 32]; 8],
            fat32_crumb_nlens: [0usize; 8],
            entries: [Entry::EMPTY; MAX_ENTRIES],
            count: 0,
            selected: 0,
            scroll: 0,
            hover_row: None,
            load_err: false,
            last_click_ms: 0,
            last_click_row: usize::MAX,
            prompt: FmPrompt::DEFAULT,
            op_err: None,
            op_ok: None,
            ctx: CtxMenu::hidden(),
            hover_crumb: None,
            clipboard: Clipboard::EMPTY,
            view: FmView::Files,
            tile_hover: None,
            tile_sel: None,
            tile_last_click_ms: 0,
        };
        if cluster != 0 {
            let nlen = name.len().min(32);
            app.fat32_crumb_names[0][..nlen].copy_from_slice(&name[..nlen]);
            app.fat32_crumb_nlens[0] = nlen;
            // Stack[0] stores the cluster to RETURN TO when navigating "..".
            // We were opened directly into `cluster`, so ".." should go back
            // to the VFS/FAT32 root (cluster = 0).
            app.fat32_cluster_stack[0] = 0;
            app.fat32_stack_depth = 1;
        }
        app.load_dir();
        app
    }

    /// Open File Manager with the "New Folder" prompt already showing.
    pub fn new_folder() -> Self {
        let mut app = Self::new();
        app.view = FmView::Files;
        let target = crate::fs::resolve_node_id(app.cwd.as_str()).unwrap_or(0);
        app.prompt = FmPrompt {
            kind: PromptKind::Mkdir,
            buf: [0u8; 32],
            len: 0,
            target,
        };
        app
    }

    pub fn new() -> Self {
        let mut app = FileManagerApp {
            cwd: PathBuf::root(),
            fat32_cluster: 0,
            fat32_cluster_stack: [0u32; 8],
            fat32_stack_depth: 0,
            fat32_crumb_names: [[0u8; 32]; 8],
            fat32_crumb_nlens: [0usize; 8],
            entries: [Entry::EMPTY; MAX_ENTRIES],
            count: 0,
            selected: 0,
            scroll: 0,
            hover_row: None,
            load_err: false,
            last_click_ms: 0,
            last_click_row: usize::MAX,
            prompt: FmPrompt::DEFAULT,
            op_err: None,
            op_ok: None,
            ctx: CtxMenu::hidden(),
            hover_crumb: None,
            clipboard: Clipboard::EMPTY,
            view: FmView::ThisPc,
            tile_hover: None,
            tile_sel: None,
            tile_last_click_ms: 0,
        };
        app.load_dir();
        app
    }

    /// Execute a clipboard paste into the current directory.
    /// For copy: reads the source file and writes it to the destination cluster.
    /// For cut (move): same, then deletes the source entry.
    fn do_paste(&mut self) {
        if !self.clipboard.is_set() {
            return;
        }
        if !crate::fat32::is_mounted() {
            self.op_err = Some("Paste: no FAT32 disk");
            return;
        }
        let dst_cluster = if self.fat32_cluster != 0 {
            self.fat32_cluster
        } else {
            fs::fat32_root_cluster()
        };
        let src_cluster = self.clipboard.src_cluster;
        let name = &self.clipboard.name[..self.clipboard.name_len];

        // Find the source entry
        let de = match crate::fat32::find_in_dir(src_cluster, name) {
            Some(d) => d,
            None => {
                self.op_err = Some("Paste: source not found");
                return;
            }
        };

        if de.is_dir {
            self.op_err = Some("Paste: directories not supported");
            return;
        }

        // Read file content (up to 64 KB)
        const MAX: usize = 65536;
        let mut buf = [0u8; MAX];
        let n = crate::fat32::read_file(de.cluster, de.size, &mut buf);

        // Write to destination
        if !crate::fat32::write_file(dst_cluster, name, &buf[..n]) {
            self.op_err = Some("Paste: write failed");
            return;
        }

        // If cut: delete the source and clear clipboard
        if self.clipboard.is_cut {
            crate::fat32::delete_entry(src_cluster, name);
            self.clipboard = Clipboard::EMPTY;
        }

        self.op_ok = Some("Pasted");
        self.op_err = None;
        self.load_dir();
    }

    fn load_dir(&mut self) {
        self.count = 0;
        self.selected = 0;
        self.scroll = 0;
        self.hover_row = None;
        self.load_err = false;

        // If we're inside a FAT32 subdirectory (not the VFS root or a VFS path),
        // skip VFS resolution entirely — just list FAT32 contents.
        if self.fat32_cluster != 0 {
            // Add ".." entry to navigate back
            let mut back = Entry::EMPTY;
            back.name[..2].copy_from_slice(b"..");
            back.nlen = 2;
            back.is_dir = true;
            self.entries[self.count] = back;
            self.count += 1;

            let fat_cluster = self.fat32_cluster;
            let mut fat_out = [fs::DynEntry {
                id: 0,
                parent: 0,
                name: [0u8; 32],
                nlen: 0,
                is_dir: false,
                size: 0,
            }; 32];
            // Only call into FAT32 for valid cluster numbers (>= 2).
            // fat32_cluster == 1 is the sentinel for "empty dir / no cluster".
            let fat_count = if fat_cluster >= 2 {
                fs::fat32_list_dir(fat_cluster, &mut fat_out, 0)
            } else {
                0
            };
            for i in 0..fat_count {
                if self.count >= MAX_ENTRIES {
                    break;
                }
                let d = &fat_out[i];
                let mut e = Entry::EMPTY;
                let nlen = d.nlen.min(32);
                e.name[..nlen].copy_from_slice(&d.name[..nlen]);
                e.nlen = nlen;
                e.is_dir = d.is_dir;
                e.is_dyn = false;
                e.is_fat32 = true;
                e.node_id = d.id;
                e.size = d.size;
                self.entries[self.count] = e;
                self.count += 1;
            }
            return;
        }

        let dir_id = match fs::resolve_node_id(self.cwd.as_str()) {
            Some(id) => id,
            None => {
                self.load_err = true;
                return;
            }
        };

        // Static VFS nodes (skip hidden system entries like /etc)
        const HIDDEN: &[&str] = &["etc"];
        for node in fs::iter_nodes() {
            if self.count >= MAX_ENTRIES {
                break;
            }
            if node.parent != Some(dir_id) {
                continue;
            }
            if HIDDEN.iter().any(|h| *h == node.name) {
                continue;
            }

            let nb = node.name.as_bytes();
            let nlen = nb.len().min(32);
            let mut e = Entry::EMPTY;
            e.name[..nlen].copy_from_slice(&nb[..nlen]);
            e.nlen = nlen;
            e.is_dir = node.kind == fs::NodeKind::Directory;
            e.is_dyn = false;
            e.node_id = node.id;
            e.size = if e.is_dir {
                fs::iter_nodes()
                    .iter()
                    .filter(|n| n.parent == Some(node.id))
                    .count()
            } else {
                node.data.len()
            };
            self.entries[self.count] = e;
            self.count += 1;
        }

        // Dynamic files and folders in this directory
        let mut dyn_out = [fs::DynEntry {
            id: 0,
            parent: 0,
            name: [0u8; 32],
            nlen: 0,
            is_dir: false,
            size: 0,
        }; 16];
        let dyn_count = fs::dyn_list_dir(dir_id, &mut dyn_out);
        for i in 0..dyn_count {
            if self.count >= MAX_ENTRIES {
                break;
            }
            let d = &dyn_out[i];
            let mut e = Entry::EMPTY;
            let nlen = d.nlen.min(32);
            e.name[..nlen].copy_from_slice(&d.name[..nlen]);
            e.nlen = nlen;
            e.is_dir = d.is_dir;
            e.is_dyn = true;
            e.node_id = d.id;
            e.size = d.size;
            self.entries[self.count] = e;
            self.count += 1;
        }

        // FAT32 disk entries (if a FAT32 volume is mounted at VFS root level)
        // Note: fat32_cluster == 0 here (the early-return above handles the ≠0 case).
        let fat_cluster = fs::fat32_root_cluster();
        if fat_cluster != 0 {
            let mut fat_out = [fs::DynEntry {
                id: 0,
                parent: 0,
                name: [0u8; 32],
                nlen: 0,
                is_dir: false,
                size: 0,
            }; 32];
            let fat_count = fs::fat32_list_dir(fat_cluster, &mut fat_out, 0);
            for i in 0..fat_count {
                if self.count >= MAX_ENTRIES {
                    break;
                }
                let d = &fat_out[i];
                let mut e = Entry::EMPTY;
                let nlen = d.nlen.min(32);
                e.name[..nlen].copy_from_slice(&d.name[..nlen]);
                e.nlen = nlen;
                e.is_dir = d.is_dir;
                e.is_dyn = false;
                e.is_fat32 = true;
                e.node_id = d.id;
                e.size = d.size;
                self.entries[self.count] = e;
                self.count += 1;
            }
        }
    }

    fn navigate_into(&mut self) {
        if self.count == 0 {
            return;
        }
        let e = self.entries[self.selected];
        if !e.is_dir {
            return;
        }

        // ".." entry — navigate up
        if e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.' {
            if self.fat32_stack_depth > 0 {
                // Pop back one FAT32 level
                self.fat32_stack_depth -= 1;
                self.fat32_cluster = self.fat32_cluster_stack[self.fat32_stack_depth];
                self.fat32_cluster_stack[self.fat32_stack_depth] = 0;
                self.fat32_crumb_nlens[self.fat32_stack_depth] = 0;
            } else {
                // Back to VFS root from first FAT32 level
                self.fat32_cluster = 0;
                if self.cwd.len > 1 {
                    self.cwd.pop();
                }
            }
            self.load_dir();
            return;
        }

        if e.is_fat32 && e.is_dir {
            // Push current cluster onto the stack
            if self.fat32_stack_depth < 8 {
                self.fat32_cluster_stack[self.fat32_stack_depth] = self.fat32_cluster;
                // Store the display name for the breadcrumb
                let nlen = e.nlen.min(32);
                self.fat32_crumb_names[self.fat32_stack_depth][..nlen]
                    .copy_from_slice(&e.name[..nlen]);
                self.fat32_crumb_nlens[self.fat32_stack_depth] = nlen;
                self.fat32_stack_depth += 1;
            }
            // Resolve the directory's own cluster. The cache is the fast path;
            // if it returns 0 (stale or cluster-0 on disk), re-read from disk.
            let cached_cluster = fs::fat32_dir_cluster(e.node_id);
            let cluster = if cached_cluster >= 2 {
                cached_cluster
            } else {
                // Fall back to a direct disk lookup using the parent cluster.
                // After the push above, stack[depth-1] holds the parent's cluster
                // (0 means the parent was the FAT32 root).
                let parent = {
                    let stacked =
                        self.fat32_cluster_stack[self.fat32_stack_depth.saturating_sub(1)];
                    if stacked != 0 {
                        stacked
                    } else {
                        fs::fat32_root_cluster()
                    }
                };
                crate::fat32::find_in_dir(parent, &e.name[..e.nlen])
                    .map(|de| de.cluster)
                    .unwrap_or(0)
            };
            // Cluster 0 means the directory entry has no allocated cluster yet
            // (truly empty in an unusual FAT32 state). Use sentinel 1 so that
            // load_dir takes the "inside a FAT32 dir" branch but list_dir
            // safely returns 0 entries (cluster 1 is always invalid/reserved).
            self.fat32_cluster = if cluster >= 2 { cluster } else { 1 };
        } else {
            self.cwd.push(e.name_str());
            self.fat32_cluster = 0;
            self.fat32_stack_depth = 0;
        }
        self.load_dir();
    }

    fn open_selected(&mut self) -> AppAction {
        if self.count == 0 {
            return AppAction::Nothing;
        }
        let e = self.entries[self.selected];
        if e.is_dir {
            self.navigate_into();
            return AppAction::RedrawAll;
        }
        // FAT32 files: encode the FAT32 NodeId in the path as a virtual path
        // The editor uses OpenFile(path_bytes, len); for FAT32 we pass a
        // special path "/fat32/<node_id_hex>" that the editor resolves.
        if e.is_fat32 {
            let mut buf = [0u8; 128];
            let prefix = b"/fat32/";
            buf[..prefix.len()].copy_from_slice(prefix);
            let id_hex = hex_u16(e.node_id);
            let total = prefix.len() + id_hex.1;
            buf[prefix.len()..total].copy_from_slice(&id_hex.0[..id_hex.1]);
            return AppAction::OpenFile(buf, total);
        }
        let mut path = self.cwd.clone();
        path.push(e.name_str());
        let bytes = path.as_str().as_bytes();
        let len = bytes.len().min(128);
        let mut buf = [0u8; 128];
        buf[..len].copy_from_slice(&bytes[..len]);
        AppAction::OpenFile(buf, len)
    }

    fn visible_rows(ch: usize) -> usize {
        ch.saturating_sub(HEADER_H + COL_HDR_H + HINT_H) / ROW_H
    }

    fn union_damage(
        a: Option<(usize, usize, usize, usize)>,
        b: Option<(usize, usize, usize, usize)>,
    ) -> Option<(usize, usize, usize, usize)> {
        match (a, b) {
            (Some((ax, ay, aw, ah)), Some((bx, by, bw, bh))) => {
                let x0 = ax.min(bx);
                let y0 = ay.min(by);
                let x1 = (ax + aw).max(bx + bw);
                let y1 = (ay + ah).max(by + bh);
                Some((x0, y0, x1 - x0, y1 - y0))
            }
            (Some(area), None) | (None, Some(area)) => Some(area),
            (None, None) => None,
        }
    }

    fn hover_row_damage(&self, row: Option<usize>) -> Option<(usize, usize, usize, usize)> {
        let row = row?;
        if row < self.scroll {
            return None;
        }
        let row_in_view = row - self.scroll;
        let y = HEADER_H + COL_HDR_H + row_in_view * ROW_H;
        Some((0, y, usize::MAX, ROW_H))
    }

    fn hover_crumb_damage(&self, crumb: Option<usize>) -> Option<(usize, usize, usize, usize)> {
        if crumb.is_some() {
            Some((0, 0, usize::MAX, HEADER_H))
        } else {
            None
        }
    }

    fn clamp_scroll(&mut self, visible: usize) {
        if self.count == 0 {
            self.scroll = 0;
            return;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        if self.selected >= self.scroll + visible {
            self.scroll = self.selected.saturating_sub(visible - 1);
        }
        let max_scroll = self.count.saturating_sub(visible);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    fn fmt_size(buf: &mut [u8; 16], size: usize) -> &str {
        if size == 0 {
            buf[0] = b'-';
            return core::str::from_utf8(&buf[..1]).unwrap_or("-");
        }
        if size < 1024 {
            let n = fmt_uint(buf, 0, size);
            let s = b" B";
            let e = (n + s.len()).min(buf.len());
            buf[n..e].copy_from_slice(&s[..e - n]);
            core::str::from_utf8(&buf[..e]).unwrap_or("?")
        } else {
            let n = fmt_uint(buf, 0, size / 1024);
            let s = b" KB";
            let e = (n + s.len()).min(buf.len());
            buf[n..e].copy_from_slice(&s[..e - n]);
            core::str::from_utf8(&buf[..e]).unwrap_or("?")
        }
    }

    fn tile_rect(i: usize, cw: usize) -> (usize, usize, usize, usize) {
        // Centre one tile per "This PC" (we only have one drive for now)
        let _ = i;
        let tx = (cw.saturating_sub(TILE_W)) / 2;
        (tx, TILE_TOP, TILE_W, TILE_H)
    }
}
