/// Create or overwrite a file named `name` (display name, e.g. "my file.txt")
/// in the directory at `dir_cluster` with `data` as content.
/// Writes LFN directory entries for names that don't fit in 8.3.
///
/// Returns `true` on success.
pub fn write_file(dir_cluster: u32, name: &[u8], data: &[u8]) -> bool {
    if !is_mounted() {
        return false;
    }
    let name_83 = make_83_name(name);
    let needs_lfn = !is_83_compatible(name);
    let name_len = name.len().min(63);
    let lfn_n = if needs_lfn {
        lfn_entry_count(name_len)
    } else {
        0
    };
    let total_slots = lfn_n + 1;
    let spc = SEC_PER_CLUS.load(Ordering::Relaxed) as usize;

    // ── Step 1: Scan directory for existing 8.3 entry ─────────────────────────
    let mut found = false;
    let mut old_cluster = 0u32;
    let mut dirent_lba = 0u64;
    let mut dirent_off = 0usize;

    'scan: {
        let mut cluster = dir_cluster;
        loop {
            if cluster >= 0x0FFF_FFF8 {
                break;
            }
            for sec_off in 0..spc as u64 {
                let lba = cluster_to_lba(cluster) + sec_off;
                let mut buf = [0u8; 512];
                if read_sector(lba, &mut buf).is_err() {
                    break 'scan;
                }
                for eo in (0..512usize).step_by(32) {
                    let first = buf[eo];
                    if first == 0x00 {
                        break 'scan;
                    } // end of dir — not found
                    if first == 0xE5 {
                        continue;
                    }
                    let attr = buf[eo + 11];
                    if attr == 0x0F || attr & 0x08 != 0 {
                        continue;
                    }
                    if buf[eo..eo + 11] == name_83 {
                        found = true;
                        let hi = read_le16(&buf, eo + 20) as u32;
                        let lo = read_le16(&buf, eo + 26) as u32;
                        old_cluster = (hi << 16) | lo;
                        dirent_lba = lba;
                        dirent_off = eo;
                        break 'scan;
                    }
                }
            }
            cluster = fat_next_cluster(cluster);
        }
    }

    // ── Step 2: Free old cluster chain if overwriting ─────────────────────────
    if found && old_cluster >= 2 {
        let mut c = old_cluster;
        while c < 0x0FFF_FFF8 {
            let next = fat_next_cluster(c);
            fat_write_entry(c, 0);
            c = next;
        }
    }

    // ── Step 3: Allocate clusters and write data ───────────────────────────────
    let bytes_per_clus = spc * 512;
    let n_clusters = if data.is_empty() {
        0
    } else {
        (data.len() + bytes_per_clus - 1) / bytes_per_clus
    };

    let mut first_cluster = 0u32;
    let mut prev_cluster = 0u32;
    let mut written = 0usize;

    for _ in 0..n_clusters {
        let c = match alloc_cluster() {
            Some(c) => c,
            None => return false,
        };
        if first_cluster == 0 {
            first_cluster = c;
        }
        if prev_cluster != 0 {
            fat_write_entry(prev_cluster, c);
        }
        prev_cluster = c;
        for sec_off in 0..spc {
            let lba = cluster_to_lba(c) + sec_off as u64;
            let mut sec = [0u8; 512];
            let take = (data.len().saturating_sub(written)).min(512);
            if take > 0 {
                sec[..take].copy_from_slice(&data[written..written + take]);
                written += take;
            }
            if write_sector_raw(lba, &sec).is_err() {
                return false;
            }
        }
    }

    // ── Step 4: Write directory entry/entries ─────────────────────────────────
    if found {
        // Update existing 8.3 entry in place (LFN chain is still valid by checksum)
        let mut buf = [0u8; 512];
        if read_sector(dirent_lba, &mut buf).is_err() {
            return false;
        }
        write_le16(&mut buf, dirent_off + 20, (first_cluster >> 16) as u16);
        write_le16(&mut buf, dirent_off + 26, (first_cluster & 0xFFFF) as u16);
        write_le32(&mut buf, dirent_off + 28, data.len() as u32);
        return write_sector_raw(dirent_lba, &buf).is_ok();
    }

    // Find a run of total_slots consecutive free directory entries
    let mut run_lba = [0u64; 6];
    let mut run_off = [0usize; 6];
    if !find_slot_run(dir_cluster, total_slots, &mut run_lba, &mut run_off) {
        return false;
    }

    // Write LFN entries in reverse name-order (highest seq first on disk)
    if needs_lfn {
        let checksum = lfn_checksum(&name_83);
        for i in 0..lfn_n {
            let seq = lfn_n - i; // seq lfn_n → 1
            let is_last = i == 0; // first-on-disk entry is the "last" seq marker
            let lba = run_lba[i];
            let eo = run_off[i];
            let mut buf = [0u8; 512];
            if read_sector(lba, &mut buf).is_err() {
                return false;
            }
            let mut entry = [0u8; 32];
            fill_lfn_entry(&mut entry, seq, is_last, name, name_len, checksum);
            buf[eo..eo + 32].copy_from_slice(&entry);
            if write_sector_raw(lba, &buf).is_err() {
                return false;
            }
        }
    }

    // Write the 8.3 directory entry
    let lba = run_lba[lfn_n];
    let eo = run_off[lfn_n];
    let mut buf = [0u8; 512];
    if read_sector(lba, &mut buf).is_err() {
        return false;
    }
    let e = &mut buf[eo..eo + 32];
    for b in e.iter_mut() {
        *b = 0;
    }
    e[..11].copy_from_slice(&name_83);
    e[11] = 0x20; // archive attribute
    write_le16(&mut e[..], 20, (first_cluster >> 16) as u16);
    write_le16(&mut e[..], 26, (first_cluster & 0xFFFF) as u16);
    write_le32(&mut e[..], 28, data.len() as u32);
    write_sector_raw(lba, &buf).is_ok()
}

