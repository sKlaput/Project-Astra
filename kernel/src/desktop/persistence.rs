impl Desktop {
    // ── Desktop state persistence ──────────────────────────────────────────
    // Binary file "DESKSTAT" in the FAT32 Desktop/ folder.
    // Format (v2, magic b"DSK2"):
    //   [0..4]  magic b"DSK2"
    //   [4]     app_count (= NUM_APPS)
    //   [5]     desk_item_count
    //   [6..8]  pad
    //   per app icon  (app_count * 8 bytes): x:i32 LE, y:i32 LE
    //   per desk item (item_count * 48 bytes):
    //     x:i32 LE, y:i32 LE, nlen:u8, is_dir:u8, pad:2,
    //     cluster:u32 LE, name:[u8;32]

    fn save_desktop_state(&self) {
        if !crate::fat32::is_mounted() {
            return;
        }
        const SZ: usize = 8 + NUM_APPS * 8 + MAX_DESK_ITEMS * 48;
        let mut buf = [0u8; SZ];
        let mut p = 0usize;
        buf[p..p + 4].copy_from_slice(b"DSK2");
        p += 4;
        buf[p] = NUM_APPS as u8;
        buf[p + 1] = self.desk_item_count as u8;
        p += 4; // [4]=app_count [5]=item_count [6..8]=pad
        for i in 0..NUM_APPS {
            buf[p..p + 4].copy_from_slice(&self.icons[i].x.to_le_bytes());
            p += 4;
            buf[p..p + 4].copy_from_slice(&self.icons[i].y.to_le_bytes());
            p += 4;
        }
        for i in 0..self.desk_item_count {
            let it = &self.desk_items[i];
            buf[p..p + 4].copy_from_slice(&it.x.to_le_bytes());
            p += 4;
            buf[p..p + 4].copy_from_slice(&it.y.to_le_bytes());
            p += 4;
            buf[p] = it.nlen as u8;
            buf[p + 1] = it.is_dir as u8;
            // p+2, p+3 = pad (zeroed)
            buf[p + 4..p + 8].copy_from_slice(&it.fat32_cluster.to_le_bytes());
            buf[p + 8..p + 40].copy_from_slice(&it.name);
            p += 40; // 1+1+2+4+32
        }
        let desk_c = Self::desktop_dir_cluster();
        if desk_c != 0 {
            crate::fat32::write_file(desk_c, b"DESKSTAT", &buf[..p]);
        }
    }

    fn load_desktop_state(&mut self) {
        if !crate::fat32::is_mounted() {
            return;
        }
        const SZ: usize = 8 + NUM_APPS * 8 + MAX_DESK_ITEMS * 48;
        let desk_c = Self::desktop_dir_cluster();
        if desk_c == 0 {
            return;
        }
        let de = match crate::fat32::find_in_dir(desk_c, b"DESKSTAT") {
            Some(d) => d,
            None => return,
        };
        if de.size < 8 {
            return;
        }
        let mut buf = [0u8; SZ];
        let nread = crate::fat32::read_file(de.cluster, de.size, &mut buf);
        if nread < 8 || &buf[0..4] != b"DSK2" {
            return;
        }
        let app_count = buf[4] as usize;
        let item_count = buf[5] as usize;
        let mut p = 8usize;
        // App icon positions
        for i in 0..app_count.min(NUM_APPS) {
            if p + 8 > nread {
                return;
            }
            self.icons[i].x = i32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]);
            self.icons[i].y = i32::from_le_bytes([buf[p + 4], buf[p + 5], buf[p + 6], buf[p + 7]]);
            p += 8;
        }
        if app_count > NUM_APPS {
            p += (app_count - NUM_APPS) * 8;
        }
        // Desk items
        let n = item_count.min(MAX_DESK_ITEMS);
        self.desk_item_count = 0;
        for i in 0..n {
            if p + 48 > nread {
                break;
            }
            let x = i32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]);
            let y = i32::from_le_bytes([buf[p + 4], buf[p + 5], buf[p + 6], buf[p + 7]]);
            let nlen = (buf[p + 8] as usize).min(32);
            let is_dir = buf[p + 9] != 0;
            let fat32_cluster =
                u32::from_le_bytes([buf[p + 12], buf[p + 13], buf[p + 14], buf[p + 15]]);
            let mut name = [0u8; 32];
            name.copy_from_slice(&buf[p + 16..p + 48]);
            let mut item = DesktopItem::blank();
            item.x = x;
            item.y = y;
            item.nlen = nlen;
            item.is_dir = is_dir;
            item.fat32_cluster = fat32_cluster;
            item.name = name;
            self.desk_items[i] = item;
            self.desk_item_count += 1;
            p += 48;
        }
    }

    fn snap_icon(&mut self, idx: usize) {
        let gx = ICON_GRID_X as i32;
        let gy = (BAR_H + ICON_GRID_Y) as i32;
        let sx = ICON_SNAP_STEP_X as i32;
        let sy = ICON_SNAP_STEP_Y as i32;
        let icon = &mut self.icons[idx];
        let col = ((icon.x - gx + sx / 2) / sx).max(0);
        let row = ((icon.y - gy + sy / 2) / sy).max(0);
        icon.x = (gx + col * sx).min((self.sw.saturating_sub(ICON_CELL_W)) as i32);
        icon.y = (gy + row * sy).min((self.sh.saturating_sub(ICON_CELL_H)) as i32);
        self.save_desktop_state();
    }

}
