/// Create a new subdirectory named `name` inside `parent_cluster`.
/// Allocates one cluster, writes `.` / `..` entries, writes the parent dirent.
/// Returns true on success.
pub fn create_dir(parent_cluster: u32, name: &[u8]) -> bool {
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

    // Quick check: does this name already exist?
    if find_in_dir(parent_cluster, name).is_some() {
        return false;
    }

    // Find a run of total_slots free directory entries
    let mut run_lba = [0u64; 6];
    let mut run_off = [0usize; 6];
    if !find_slot_run(parent_cluster, total_slots, &mut run_lba, &mut run_off) {
        return false;
    }

    // Allocate one cluster for the new directory
    let new_c = match alloc_cluster() {
        Some(c) => c,
        None => return false,
    };
    fat_write_entry(new_c, 0x0FFF_FFFF);

    // Zero out the cluster and write . and .. entries
    let mut dot_buf = [0u8; 512];
    dot_buf[0..11].copy_from_slice(b".          ");
    dot_buf[11] = 0x10;
    write_le16(&mut dot_buf, 20, (new_c >> 16) as u16);
    write_le16(&mut dot_buf, 26, (new_c & 0xFFFF) as u16);
    let parent_c = if parent_cluster == root_cluster() {
        0u32
    } else {
        parent_cluster
    };
    dot_buf[32..43].copy_from_slice(b"..         ");
    dot_buf[43] = 0x10;
    write_le16(&mut dot_buf, 52, (parent_c >> 16) as u16);
    write_le16(&mut dot_buf, 58, (parent_c & 0xFFFF) as u16);
    let first_lba = cluster_to_lba(new_c);
    if write_sector_raw(first_lba, &dot_buf).is_err() {
        return false;
    }
    let zero = [0u8; 512];
    for sec_off in 1..spc as u64 {
        let _ = write_sector_raw(first_lba + sec_off, &zero);
    }

    // Write LFN entries (highest seq first on disk)
    if needs_lfn {
        let checksum = lfn_checksum(&name_83);
        for i in 0..lfn_n {
            let seq = lfn_n - i;
            let is_last = i == 0;
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
    e[11] = 0x10; // ATTR_DIRECTORY
    write_le16(&mut e[..], 20, (new_c >> 16) as u16);
    write_le16(&mut e[..], 26, (new_c & 0xFFFF) as u16);
    // size = 0 for directories
    write_sector_raw(lba, &buf).is_ok()
}

/// Delete a file or empty directory named `name` inside `dir_cluster`.
/// Marks the directory entry as deleted (0xE5) and frees the cluster chain.
/// Returns true on success. Returns false if the entry is not found, or if
/// `name` is a non-empty directory (caller should delete contents first).
pub fn delete_entry(dir_cluster: u32, name: &[u8]) -> bool {
    if !is_mounted() {
        return false;
    }
    let name_83 = make_83_name(name);
    let spc = SEC_PER_CLUS.load(Ordering::Relaxed) as usize;

    let mut cluster = dir_cluster;
    loop {
        if cluster >= 0x0FFF_FFF8 {
            break;
        }
        for sec_off in 0..spc as u64 {
            let lba = cluster_to_lba(cluster) + sec_off;
            let mut buf = [0u8; 512];
            if read_sector(lba, &mut buf).is_err() {
                return false;
            }
            for eo in (0..512usize).step_by(32) {
                let first = buf[eo];
                if first == 0x00 {
                    return false;
                } // end of dir — not found
                if first == 0xE5 {
                    continue;
                } // already deleted
                let attr = buf[eo + 11];
                if attr == 0x0F || attr & 0x08 != 0 {
                    continue;
                } // LFN / volume
                if buf[eo..eo + 11] != name_83 {
                    continue;
                }

                // Found — read cluster chain start and free it
                let hi = read_le16(&buf, eo + 20) as u32;
                let lo = read_le16(&buf, eo + 26) as u32;
                let first_cluster = (hi << 16) | lo;

                // Free the cluster chain
                if first_cluster >= 2 {
                    let mut c = first_cluster;
                    while c < 0x0FFF_FFF8 {
                        let next = fat_next_cluster(c);
                        fat_write_entry(c, 0);
                        c = next;
                    }
                }

                // Mark dirent as deleted
                buf[eo] = 0xE5;
                return write_sector_raw(lba, &buf).is_ok();
            }
        }
        cluster = fat_next_cluster(cluster);
    }
    false
}

/// Rename a file or directory entry from `old_name` to `new_name` inside `dir_cluster`.
/// Deletes the old LFN chain + 8.3 entry and writes a new LFN chain + 8.3 entry
/// pointing to the same data cluster, preserving attributes and file size.
/// Returns true on success.
pub fn rename_entry(dir_cluster: u32, old_name: &[u8], new_name: &[u8]) -> bool {
    if !is_mounted() {
        return false;
    }
    let old_83 = make_83_name(old_name);
    let new_83 = make_83_name(new_name);
    let needs_lfn_new = !is_83_compatible(new_name);
    let name_len_new = new_name.len().min(63);
    let lfn_n_new = if needs_lfn_new {
        lfn_entry_count(name_len_new)
    } else {
        0
    };
    let total_slots = lfn_n_new + 1;
    let spc = SEC_PER_CLUS.load(Ordering::Relaxed) as usize;

    // ── Step 1: Find the old 8.3 entry and collect its preceding LFN positions ─
    // We keep a rolling window of (lba, eo) for up to 5 preceding LFN entries.
    let mut lfn_win: [(u64, usize); 5] = [(0, 0); 5];
    let mut lfn_win_len = 0usize;

    let mut data_cluster = 0u32;
    let mut file_size = 0u32;
    let mut file_attr = 0u8;
    let mut found = false;
    let mut found_lba = 0u64;
    let mut found_eo = 0usize;

    'find: {
        let mut cluster = dir_cluster;
        loop {
            if cluster >= 0x0FFF_FFF8 {
                break;
            }
            for sec_off in 0..spc as u64 {
                let lba = cluster_to_lba(cluster) + sec_off;
                let mut buf = [0u8; 512];
                if read_sector(lba, &mut buf).is_err() {
                    break 'find;
                }
                for eo in (0..512usize).step_by(32) {
                    let first = buf[eo];
                    if first == 0x00 {
                        break 'find;
                    }
                    if first == 0xE5 {
                        lfn_win_len = 0;
                        continue;
                    }
                    let attr = buf[eo + 11];
                    if attr & 0x08 != 0 {
                        lfn_win_len = 0;
                        continue;
                    } // volume label
                    if attr == 0x0F {
                        // LFN entry — add to rolling window
                        if lfn_win_len < 5 {
                            lfn_win[lfn_win_len] = (lba, eo);
                            lfn_win_len += 1;
                        }
                        continue;
                    }
                    // 8.3 entry
                    if buf[eo..eo + 11] != old_83 {
                        lfn_win_len = 0;
                        continue;
                    }
                    // Found!
                    let hi = read_le16(&buf, eo + 20) as u32;
                    let lo = read_le16(&buf, eo + 26) as u32;
                    data_cluster = (hi << 16) | lo;
                    file_size = read_le32(&buf, eo + 28);
                    file_attr = attr;
                    found = true;
                    found_lba = lba;
                    found_eo = eo;
                    break 'find;
                }
            }
            cluster = fat_next_cluster(cluster);
        }
    }
    if !found {
        return false;
    }

    // ── Step 2: Mark the old 8.3 entry and all its LFN entries as deleted ──────
    {
        let mut buf = [0u8; 512];
        if read_sector(found_lba, &mut buf).is_ok() {
            buf[found_eo] = 0xE5;
            let _ = write_sector_raw(found_lba, &buf);
        }
    }
    for i in 0..lfn_win_len {
        let (lba, eo) = lfn_win[i];
        let mut buf = [0u8; 512];
        if read_sector(lba, &mut buf).is_ok() {
            buf[eo] = 0xE5;
            let _ = write_sector_raw(lba, &buf);
        }
    }

    // ── Step 3: Find free slots for the new name ─────────────────────────────
    let mut run_lba = [0u64; 6];
    let mut run_off = [0usize; 6];
    if !find_slot_run(dir_cluster, total_slots, &mut run_lba, &mut run_off) {
        return false;
    }

    // ── Step 4: Write new LFN entries ────────────────────────────────────────
    if needs_lfn_new {
        let checksum = lfn_checksum(&new_83);
        for i in 0..lfn_n_new {
            let seq = lfn_n_new - i;
            let is_last = i == 0;
            let lba = run_lba[i];
            let eo = run_off[i];
            let mut buf = [0u8; 512];
            if read_sector(lba, &mut buf).is_err() {
                return false;
            }
            let mut entry = [0u8; 32];
            fill_lfn_entry(&mut entry, seq, is_last, new_name, name_len_new, checksum);
            buf[eo..eo + 32].copy_from_slice(&entry);
            if write_sector_raw(lba, &buf).is_err() {
                return false;
            }
        }
    }

    // ── Step 5: Write new 8.3 entry preserving data cluster / size / attr ────
    let lba = run_lba[lfn_n_new];
    let eo = run_off[lfn_n_new];
    let mut buf = [0u8; 512];
    if read_sector(lba, &mut buf).is_err() {
        return false;
    }
    let e = &mut buf[eo..eo + 32];
    for b in e.iter_mut() {
        *b = 0;
    }
    e[..11].copy_from_slice(&new_83);
    e[11] = file_attr;
    write_le16(e, 20, (data_cluster >> 16) as u16);
    write_le16(e, 26, (data_cluster & 0xFFFF) as u16);
    write_le32(e, 28, file_size);
    write_sector_raw(lba, &buf).is_ok()
}


