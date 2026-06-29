// ── FAT32 write support ───────────────────────────────────────────────────────

fn write_le32(buf: &mut [u8], off: usize, v: u32) {
    buf[off] = v as u8;
    buf[off + 1] = (v >> 8) as u8;
    buf[off + 2] = (v >> 16) as u8;
    buf[off + 3] = (v >> 24) as u8;
}

fn write_le16(buf: &mut [u8], off: usize, v: u16) {
    buf[off] = v as u8;
    buf[off + 1] = (v >> 8) as u8;
}

/// Write one 512-byte sector at `lba` to disk.
fn write_sector_raw(lba: u64, buf: &[u8; 512]) -> Result<(), &'static str> {
    virtio_blk::write_sector(lba, buf)
}

/// Read-modify-write a single FAT32 entry for `cluster` in all FAT copies.
fn fat_write_entry(cluster: u32, value: u32) {
    let part_start = PART_START.load(Ordering::Relaxed);
    let rsvd = RSVD_SECS.load(Ordering::Relaxed) as u64;
    let num_fats = NUM_FATS.load(Ordering::Relaxed) as u64;
    let fat_size = FAT_SIZE.load(Ordering::Relaxed) as u64;
    let fat_offset = (cluster as u64) * 4;
    let fat_sec_off = fat_offset / 512;
    let entry_off = (fat_offset % 512) as usize;

    // Read FAT copy 0
    let lba0 = part_start + rsvd + fat_sec_off;
    let mut buf = [0u8; 512];
    if read_sector(lba0, &mut buf).is_err() {
        return;
    }

    // Preserve top 4 bits (FAT32 spec says they are reserved)
    let top_bits = read_le32(&buf, entry_off) & 0xF000_0000;
    write_le32(&mut buf, entry_off, top_bits | (value & 0x0FFF_FFFF));

    // Write to all FAT copies
    for i in 0..num_fats {
        let lba = part_start + rsvd + i * fat_size + fat_sec_off;
        let _ = write_sector_raw(lba, &buf);
    }
}

/// Scan FAT for a free cluster (entry == 0), mark it EOC, and return its number.
/// Returns `None` if the disk is full.
fn alloc_cluster() -> Option<u32> {
    let part_start = PART_START.load(Ordering::Relaxed);
    let rsvd = RSVD_SECS.load(Ordering::Relaxed) as u64;
    let fat_size = FAT_SIZE.load(Ordering::Relaxed) as u64;
    let max_cluster = (fat_size * 128) as u32; // 512 bytes / 4 bytes per entry = 128 entries/sector

    for sec_off in 0..fat_size {
        let lba = part_start + rsvd + sec_off;
        let mut buf = [0u8; 512];
        if read_sector(lba, &mut buf).is_err() {
            continue;
        }

        for i in 0..128usize {
            let cluster = (sec_off as u32) * 128 + i as u32;
            if cluster < 2 {
                continue;
            }
            if cluster >= max_cluster {
                return None;
            }
            let off = i * 4;
            let entry = read_le32(&buf, off) & 0x0FFF_FFFF;
            if entry == 0 {
                // Mark as EOC immediately so concurrent allocs skip it
                fat_write_entry(cluster, 0x0FFF_FFFF);
                return Some(cluster);
            }
        }
    }
    None
}

/// Convert a display filename to an 11-byte FAT32 8.3 name (uppercase, space-padded).
/// For names that fit in 8.3 this is exact. For longer names the first 6 chars of
/// the base get a "~1" numeric tail so the 8.3 entry is unique enough to anchor
/// the LFN chain. The numeric tail is not deduplicated — callers should rely on
/// the LFN name for display and identity.
fn make_83_name(name: &[u8]) -> [u8; 11] {
    let mut out = [b' '; 11];
    let dot = name.iter().rposition(|&b| b == b'.');
    let (base, ext) = match dot {
        Some(i) => (&name[..i], &name[i + 1..]),
        None => (name, &b""[..]),
    };

    let needs_lfn = !is_83_compatible(name);

    if needs_lfn {
        // Generate short name: first 6 uppercase valid chars + "~1"
        let mut n = 0usize;
        for &b in base {
            if n >= 6 {
                break;
            }
            let ub = b.to_ascii_uppercase();
            if is_83_valid_char(ub) {
                out[n] = ub;
                n += 1;
            }
        }
        // Pad remainder of base with spaces already done (array init)
        // Append "~1" at position n (may overwrite spaces if n < 6)
        if n <= 6 {
            out[n] = b'~';
            out[n + 1] = b'1';
        }
    } else {
        for (i, &b) in base.iter().take(8).enumerate() {
            out[i] = b.to_ascii_uppercase();
        }
    }
    for (i, &b) in ext.iter().take(3).enumerate() {
        out[8 + i] = b.to_ascii_uppercase();
    }
    out
}

/// Returns true if the character is valid in an 8.3 base/extension byte.
fn is_83_valid_char(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'0'..=b'9' | b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' |
               b'(' | b')' | b'-' | b'@' | b'^' | b'_' | b'`' | b'{' | b'}' | b'~')
}

/// Returns true if `name` can be stored exactly as a FAT32 8.3 name
/// (base ≤ 8 chars, extension ≤ 3 chars, all uppercase-valid chars).
fn is_83_compatible(name: &[u8]) -> bool {
    let dot = name.iter().rposition(|&b| b == b'.');
    let (base, ext) = match dot {
        Some(i) => (&name[..i], &name[i + 1..]),
        None => (name, &b""[..]),
    };
    if base.len() > 8 || ext.len() > 3 {
        return false;
    }
    for &b in base.iter().chain(ext.iter()) {
        let ub = b.to_ascii_uppercase();
        if !is_83_valid_char(ub) {
            return false;
        }
    }
    true
}

/// Returns the number of LFN directory entries needed for `name_len` chars.
fn lfn_entry_count(name_len: usize) -> usize {
    (name_len + 12) / 13 // ceil(name_len / 13) but max 1 if 0
}

/// Fill a 32-byte LFN directory entry buffer.
/// `seq`      – 1-based sequence number; OR with 0x40 for the last entry (first-on-disk).
/// `name`     – full display name bytes.
/// `name_len` – length of display name.
/// `checksum` – LFN checksum of the paired 8.3 name.
fn fill_lfn_entry(
    buf: &mut [u8; 32],
    seq: usize,
    is_last: bool,
    name: &[u8],
    name_len: usize,
    checksum: u8,
) {
    for b in buf.iter_mut() {
        *b = 0;
    }
    buf[0] = seq as u8 | if is_last { 0x40 } else { 0 };
    buf[11] = 0x0F; // LFN attribute
    buf[13] = checksum;
    buf[26] = 0;
    buf[27] = 0; // cluster (always 0 in LFN)

    // The three UTF-16LE char blocks: offsets/counts
    let blocks: [(usize, usize); 3] = [(1, 5), (14, 6), (28, 2)];
    let base = (seq - 1) * 13;
    let mut pos = base;

    for (off, count) in blocks {
        for i in 0..count {
            if pos < name_len {
                // ASCII → UTF-16LE: high byte = 0
                buf[off + i * 2] = name[pos];
                buf[off + i * 2 + 1] = 0x00;
                pos += 1;
            } else if pos == name_len {
                // Null terminator
                buf[off + i * 2] = 0x00;
                buf[off + i * 2 + 1] = 0x00;
                pos += 1;
            } else {
                // Padding
                buf[off + i * 2] = 0xFF;
                buf[off + i * 2 + 1] = 0xFF;
            }
        }
    }
}

/// Helper: find a run of `needed` consecutive free directory slots in `dir_cluster`.
/// Fills `run_lba`/`run_off` with the (lba, byte-offset) for each slot.
/// Returns true if a sufficient run was found.
///
/// "Free" means: first byte == 0x00 (end-of-dir), 0xE5 (deleted), or attr == 0x0F (orphan LFN).
fn find_slot_run(
    dir_cluster: u32,
    needed: usize,
    run_lba: &mut [u64; 6],
    run_off: &mut [usize; 6],
) -> bool {
    let spc = SEC_PER_CLUS.load(Ordering::Relaxed) as usize;
    let mut run_count = 0usize;
    let mut found = false;
    let mut cluster = dir_cluster;

    'outer: loop {
        if cluster >= 0x0FFF_FFF8 {
            break;
        }
        for sec_off in 0..spc as u64 {
            let lba = cluster_to_lba(cluster) + sec_off;
            let mut buf = [0u8; 512];
            if read_sector(lba, &mut buf).is_err() {
                break 'outer;
            }

            for eo in (0..512usize).step_by(32) {
                let first = buf[eo];
                // A slot is free if deleted (0xE5) or end-of-dir (0x00).
                // Valid LFN entries (attr == 0x0F, first != 0xE5) are NOT free.
                let is_free = first == 0x00 || first == 0xE5;

                if is_free {
                    if run_count < 6 {
                        run_lba[run_count] = lba;
                        run_off[run_count] = eo;
                        run_count += 1;
                    }
                    if run_count >= needed {
                        found = true;
                        break 'outer;
                    }
                    // 0x00 means all subsequent entries are also free — keep iterating
                    // to collect the remaining needed slots without breaking early.
                } else {
                    // Reset the run on any occupied entry
                    run_count = 0;
                }
            }
        }
        cluster = fat_next_cluster(cluster);
    }
    found
}

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

/// Update the size field in an existing directory entry (after in-place rewrite).
/// Used to refresh the cached size after a write_file call.
pub fn update_dirent_size(dir_cluster: u32, name: &[u8], new_size: u32) -> bool {
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
                }
                if first == 0xE5 {
                    continue;
                }
                let attr = buf[eo + 11];
                if attr == 0x0F || attr & 0x08 != 0 {
                    continue;
                }
                if buf[eo..eo + 11] == name_83 {
                    write_le32(&mut buf, eo + 28, new_size);
                    return write_sector_raw(lba, &buf).is_ok();
                }
            }
        }
        cluster = fat_next_cluster(cluster);
    }
    false
}
