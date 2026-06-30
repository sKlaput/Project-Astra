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

