// ── Directory entry ───────────────────────────────────────────────────────────

/// A single directory entry returned by `list_dir`.
/// `name` holds the long filename (LFN) if present, otherwise the 8.3 short name.
#[derive(Clone, Copy)]
pub struct DirEntry {
    /// Display name — up to 63 chars of LFN, or the 8.3 short name.
    pub name: [u8; 64],
    pub name_len: usize,
    pub is_dir: bool,
    pub size: u32,
    pub cluster: u32,
}

impl DirEntry {
    pub const EMPTY: Self = DirEntry {
        name: [0u8; 64],
        name_len: 0,
        is_dir: false,
        size: 0,
        cluster: 0,
    };

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
}

/// Parse a raw 8.3 directory entry (32 bytes at `e`) and fill a `DirEntry`.
/// Returns `None` if the entry is empty, deleted, LFN, or a volume label.
fn parse_dirent(e: &[u8]) -> Option<DirEntry> {
    debug_assert!(e.len() >= 32);
    let first = e[0];
    if first == 0x00 {
        return None;
    } // end of directory
    if first == 0xE5 {
        return None;
    } // deleted
    let attr = e[11];
    if attr == 0x0F {
        return None;
    } // LFN entry — handled in list_dir
    if attr & 0x08 != 0 {
        return None;
    } // volume label

    // Skip . and ..
    if e[0] == b'.' {
        return None;
    }

    let is_dir = attr & 0x10 != 0;

    // Build trimmed 8.3 name: "NAME    .EXT" → "NAME.EXT" or "NAME"
    let mut name = [0u8; 64];
    let mut nlen = 0usize;
    // Base name (bytes 0..8), trim trailing spaces
    let mut base_end = 8usize;
    while base_end > 0 && e[base_end - 1] == b' ' {
        base_end -= 1;
    }
    for i in 0..base_end {
        let c = e[i];
        // Convert to lowercase for display
        name[nlen] = if c >= b'A' && c <= b'Z' { c + 32 } else { c };
        nlen += 1;
    }
    // Extension (bytes 8..11), trim trailing spaces
    let mut ext_end = 3usize;
    while ext_end > 0 && e[8 + ext_end - 1] == b' ' {
        ext_end -= 1;
    }
    if ext_end > 0 && !is_dir {
        name[nlen] = b'.';
        nlen += 1;
        for i in 0..ext_end {
            let c = e[8 + i];
            name[nlen] = if c >= b'A' && c <= b'Z' { c + 32 } else { c };
            nlen += 1;
        }
    }

    // Cluster: high word at offset 20, low word at offset 26
    let cluster_hi = read_le16(e, 20) as u32;
    let cluster_lo = read_le16(e, 26) as u32;
    let cluster = (cluster_hi << 16) | cluster_lo;

    let size = read_le32(e, 28);

    Some(DirEntry {
        name,
        name_len: nlen,
        is_dir,
        size,
        cluster,
    })
}

// ── LFN helpers ───────────────────────────────────────────────────────────────

/// Maximum number of LFN directory entries supported per filename.
const MAX_LFN_ENTRIES: usize = 5; // 5 × 13 chars = 65 chars (fits in [u8; 64])

/// Compute the FAT32 LFN checksum over an 11-byte 8.3 directory-name field.
pub fn lfn_checksum(name83: &[u8; 11]) -> u8 {
    let mut sum = 0u8;
    for &b in name83 {
        sum = sum.rotate_right(1).wrapping_add(b);
    }
    sum
}

/// Decode 13 UTF-16LE characters from an LFN entry into ASCII bytes.
/// `seq` is the 1-based sequence index (so chars land at (seq-1)*13 in `out`).
fn decode_lfn_chars(e: &[u8], seq: usize, out: &mut [u8; 64]) {
    // The three blocks of UTF-16LE chars in an LFN entry:
    //   bytes  1..11  → 5 chars
    //   bytes 14..26  → 6 chars
    //   bytes 28..32  → 2 chars
    let chunks: [(usize, usize); 3] = [(1, 5), (14, 6), (28, 2)];
    let base = (seq - 1) * 13;
    let mut pos = base;
    for (off, count) in chunks {
        for i in 0..count {
            if pos >= 64 {
                return;
            }
            let lo = e[off + i * 2];
            let hi = e[off + i * 2 + 1];
            if lo == 0xFF && hi == 0xFF {
                return;
            } // padding sentinel
            if lo == 0x00 && hi == 0x00 {
                return;
            } // null terminator
              // Represent non-ASCII (hi!=0) as '?' for now
            out[pos] = if hi == 0 && lo >= 0x20 { lo } else { b'?' };
            pos += 1;
        }
    }
}

/// List entries in the directory starting at `start_cluster`.
/// Calls `callback(entry)` for each valid entry (LFN names are resolved).
/// Stops early if callback returns `false`.
pub fn list_dir<F>(start_cluster: u32, mut callback: F)
where
    F: FnMut(DirEntry) -> bool,
{
    if !is_mounted() {
        return;
    }
    let spc = SEC_PER_CLUS.load(Ordering::Relaxed);
    let mut cluster = start_cluster;

    // LFN accumulation state (reset on each non-LFN entry)
    let mut lfn_buf: [u8; 64] = [0u8; 64];
    let mut lfn_len: usize = 0;
    let mut lfn_expected: u8 = 0; // checksum from LFN entries
    let mut lfn_seqs: u8 = 0; // how many seq entries seen

    loop {
        if cluster >= 0x0FFF_FFF8 {
            break;
        }

        for sec_off in 0..spc {
            let lba = cluster_to_lba(cluster) + sec_off as u64;
            let mut buf = [0u8; 512];
            if read_sector(lba, &mut buf).is_err() {
                return;
            }

            for entry_off in (0..512usize).step_by(32) {
                let e = &buf[entry_off..entry_off + 32];
                if e[0] == 0x00 {
                    return;
                } // end of directory

                let attr = e[11];
                if attr == 0x0F {
                    // LFN entry
                    let seq_raw = e[0];
                    let seq = (seq_raw & 0x1F) as usize;
                    if seq == 0 || seq > MAX_LFN_ENTRIES {
                        // Invalid — reset
                        lfn_len = 0;
                        lfn_seqs = 0;
                        continue;
                    }
                    if seq_raw & 0x40 != 0 {
                        // First-on-disk = last part of name — start fresh
                        lfn_buf = [0u8; 64];
                        lfn_seqs = 0;
                        lfn_expected = e[13]; // checksum
                    } else if e[13] != lfn_expected {
                        // Checksum mismatch — orphaned entry, reset
                        lfn_len = 0;
                        lfn_seqs = 0;
                        continue;
                    }
                    decode_lfn_chars(e, seq, &mut lfn_buf);
                    lfn_seqs += 1;
                    // Update lfn_len: scan from start for null terminator
                    lfn_len = 0;
                    for i in 0..64 {
                        if lfn_buf[i] == 0 {
                            break;
                        }
                        lfn_len = i + 1;
                    }
                    continue;
                }

                // Regular or deleted entry
                if e[0] == 0xE5 {
                    lfn_len = 0;
                    lfn_seqs = 0;
                    continue;
                }
                if attr & 0x08 != 0 {
                    // Volume label
                    lfn_len = 0;
                    lfn_seqs = 0;
                    continue;
                }

                if let Some(mut de) = parse_dirent(e) {
                    // Validate LFN by checksum
                    let have_lfn = lfn_len > 0 && lfn_seqs > 0 && {
                        let mut short_name = [b' '; 11];
                        short_name.copy_from_slice(&e[0..11]);
                        lfn_checksum(&short_name) == lfn_expected
                    };
                    if have_lfn {
                        de.name = lfn_buf;
                        de.name_len = lfn_len;
                    }
                    lfn_len = 0;
                    lfn_seqs = 0;
                    if !callback(de) {
                        return;
                    }
                } else {
                    lfn_len = 0;
                    lfn_seqs = 0;
                }
            }
        }

        cluster = fat_next_cluster(cluster);
    }
}

/// Read up to `buf.len()` bytes from a file starting at `start_cluster`.
/// `file_size` is used to avoid reading past EOF.
/// Returns the number of bytes actually read.
pub fn read_file(start_cluster: u32, file_size: u32, buf: &mut [u8]) -> usize {
    if !is_mounted() {
        return 0;
    }
    let spc = SEC_PER_CLUS.load(Ordering::Relaxed) as usize;
    let max_read = (file_size as usize).min(buf.len());
    if max_read == 0 {
        return 0;
    }

    let mut total = 0usize;
    let mut cluster = start_cluster;

    'outer: loop {
        if cluster >= 0x0FFF_FFF8 {
            break;
        }

        for sec_off in 0..spc {
            if total >= max_read {
                break 'outer;
            }
            let lba = cluster_to_lba(cluster) + sec_off as u64;
            let mut sec = [0u8; 512];
            if read_sector(lba, &mut sec).is_err() {
                break 'outer;
            }

            let take = (max_read - total).min(512);
            buf[total..total + take].copy_from_slice(&sec[..take]);
            total += take;
        }

        cluster = fat_next_cluster(cluster);
    }
    total
}

/// Find a named entry (case-insensitive) in a directory cluster.
/// Returns the entry if found.
pub fn find_in_dir(dir_cluster: u32, name: &[u8]) -> Option<DirEntry> {
    let mut result = None;
    list_dir(dir_cluster, |de| {
        // Case-insensitive compare
        if de.name_len == name.len() {
            let mut eq = true;
            for i in 0..de.name_len {
                let a = de.name[i].to_ascii_lowercase();
                let b = name[i].to_ascii_lowercase();
                if a != b {
                    eq = false;
                    break;
                }
            }
            if eq {
                result = Some(de);
                return false;
            }
        }
        true
    });
    result
}

/// Walk a FAT32 path (e.g. "/docs/readme.txt") and return the final entry.
/// Path must start with '/'.
pub fn resolve_path(path: &str) -> Option<DirEntry> {
    if !is_mounted() {
        return None;
    }
    let mut cluster = ROOT_CLUS.load(Ordering::Relaxed);
    let segs: &[&str] = &[];
    let _ = segs; // silence lint

    let mut last_de: Option<DirEntry> = None;

    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let nb = seg.as_bytes();
        let de = find_in_dir(cluster, nb)?;
        cluster = de.cluster;
        last_de = Some(de);
    }
    last_de
}
