impl Desktop {
    // ── Desktop item helpers ──────────────────────────────────────────────

    fn desk_item_at(&self, mx: i32, my: i32) -> Option<usize> {
        for i in 0..self.desk_item_count {
            let r = self.desk_items[i].rect();
            if mx as usize >= r.x
                && (mx as usize) < r.x + r.w
                && my as usize >= r.y
                && (my as usize) < r.y + r.h
            {
                return Some(i);
            }
        }
        None
    }

    fn commit_desk_prompt(&mut self) {
        if self.desk_prompt.len == 0 || self.desk_item_count >= MAX_DESK_ITEMS {
            let pr = self.desk_prompt.rect(self.sw, self.sh);
            self.desk_prompt = DesktopNamePrompt::hidden();
            self.damage.add(Rect {
                x: pr.x,
                y: pr.y.saturating_sub(16),
                w: pr.w,
                h: pr.h + 30,
            });
            return;
        }
        let is_dir = self.desk_prompt.is_dir;
        let nlen = self.desk_prompt.len;
        let mut name_bytes = [0u8; 32];
        name_bytes[..nlen].copy_from_slice(&self.desk_prompt.buf[..nlen]);
        // Create on FAT32 (inside the Desktop/ folder) and immediately look up
        // the cluster so double-click can open the folder directly.
        let fat32_cluster = if crate::fat32::is_mounted() {
            let desk_c = Self::desktop_dir_cluster();
            if desk_c == 0 {
                0
            } else {
                if is_dir {
                    crate::fat32::create_dir(desk_c, &name_bytes[..nlen]);
                } else {
                    crate::fs::fat32_create_and_open(desk_c, &name_bytes[..nlen]);
                }
                // find_in_dir right after creation — the entry is guaranteed to exist now.
                crate::fat32::find_in_dir(desk_c, &name_bytes[..nlen])
                    .map(|de| de.cluster)
                    .unwrap_or(0)
            }
        } else {
            0
        };
        // Place the desktop item near the spawn position
        let pr = self.desk_prompt.rect(self.sw, self.sh);
        let ix = (self.desk_prompt.spawn_x - (DI_W as i32 / 2))
            .max(0)
            .min((self.sw.saturating_sub(DI_W)) as i32);
        let iy = (self.desk_prompt.spawn_y - DI_H as i32 - 8)
            .max(BAR_H as i32)
            .min((self.sh.saturating_sub(DI_H)) as i32);
        let mut item = DesktopItem::blank();
        item.x = ix;
        item.y = iy;
        item.nlen = nlen;
        item.name = name_bytes;
        item.is_dir = is_dir;
        item.fat32_cluster = fat32_cluster;
        let item_rect = item.rect();
        self.desk_items[self.desk_item_count] = item;
        self.desk_item_count += 1;
        self.desk_prompt = DesktopNamePrompt::hidden();
        self.damage.add(Rect {
            x: pr.x,
            y: pr.y.saturating_sub(16),
            w: pr.w,
            h: pr.h + 30,
        });
        self.damage.add(item_rect);
        self.save_desktop_state();
    }

    fn open_desk_item(&mut self, idx: usize) {
        if idx >= self.desk_item_count {
            return;
        }
        let item = self.desk_items[idx];
        if item.is_dir {
            // Use the cluster stored at creation time. Fall back to a fresh
            // find_in_dir only for items loaded from an older DESKSTAT that
            // didn't record the cluster.
            let dir_cluster = if item.fat32_cluster != 0 {
                item.fat32_cluster
            } else if crate::fat32::is_mounted() {
                let desk_c = Self::desktop_dir_cluster();
                if desk_c != 0 {
                    crate::fat32::find_in_dir(desk_c, &item.name[..item.nlen])
                        .map(|de| de.cluster)
                        .unwrap_or(0)
                } else {
                    0
                }
            } else {
                0
            };
            let app = Box::new(crate::filemanager::FileManagerApp::open_dir(
                dir_cluster,
                &item.name[..item.nlen],
            ));
            self.open_window(app);
        } else {
            // Open the file in the editor — find it without overwriting its content.
            let desk_c = Self::desktop_dir_cluster();
            if desk_c != 0 {
                if let Some(fid) = crate::fs::fat32_find_and_open(desk_c, &item.name[..item.nlen]) {
                    // Build /fat32/<hex-id> path
                    let mut buf = [0u8; 32];
                    let prefix = b"/fat32/";
                    buf[..prefix.len()].copy_from_slice(prefix);
                    let mut hi = prefix.len();
                    let mut v = fid as u32;
                    let mut tmp = [0u8; 8];
                    let mut tlen = 0usize;
                    loop {
                        let n = (v & 0xF) as u8;
                        tmp[tlen] = if n < 10 { b'0' + n } else { b'a' + n - 10 };
                        tlen += 1;
                        v >>= 4;
                        if v == 0 {
                            break;
                        }
                    }
                    tmp[..tlen].reverse();
                    buf[hi..hi + tlen].copy_from_slice(&tmp[..tlen]);
                    hi += tlen;
                    if let Some(path) = core::str::from_utf8(&buf[..hi]).ok() {
                        let ed = Box::new(crate::editor::EditorApp::open(path));
                        self.open_window(ed);
                    }
                }
            } // if desk_c != 0
        }
        self.damage.mark_full();
    }

    // ── Desktop directory helper ──────────────────────────────────────────────
    // All desktop items and DESKSTAT live inside a "Desktop" folder on FAT32.
    // This helper ensures the folder exists and returns its cluster.
    fn desktop_dir_cluster() -> u32 {
        if !crate::fat32::is_mounted() {
            return 0;
        }
        let root_c = crate::fat32::root_cluster();
        if root_c == 0 {
            return 0;
        }
        // Return existing Desktop folder cluster, creating it if absent.
        if let Some(de) = crate::fat32::find_in_dir(root_c, b"Desktop") {
            if de.cluster >= 2 {
                return de.cluster;
            }
        }
        crate::fat32::create_dir(root_c, b"Desktop");
        crate::fat32::find_in_dir(root_c, b"Desktop")
            .map(|de| de.cluster)
            .unwrap_or(0)
    }

}
