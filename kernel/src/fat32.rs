// ---------------------------------------------------------------------------
// Astra OS — FAT32 read-only driver (Step 2)
//
// Mounts the first FAT32 partition found on the virtio-blk device.
// Provides:
//   fat32::mount()          — parse BPB, verify FAT32, store geometry
//   fat32::list_dir()       — enumerate entries in a directory cluster
//   fat32::read_file()      — read file contents by start cluster + size
//   fat32::root_cluster()   — returns the root directory cluster number
//
// Design constraints:
//   - No heap allocations — all buffers are stack-local or static
//   - Read-only; writes not implemented
//   - Supports long-file-name (LFN) entries — they are silently skipped
//     so short 8.3 names always show correctly
//   - Cluster cache: one 512-byte static sector buffer (no allocation needed
//     for small sequential reads; callers must copy data out before next call)
// ---------------------------------------------------------------------------

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::drivers::virtio_blk;

// ── FAT32 mount state ─────────────────────────────────────────────────────────

static MOUNTED: AtomicBool = AtomicBool::new(false);
static BYTES_PER_SEC: AtomicU32 = AtomicU32::new(512);
static SEC_PER_CLUS: AtomicU32 = AtomicU32::new(0);
static RSVD_SECS: AtomicU32 = AtomicU32::new(0);
static NUM_FATS: AtomicU32 = AtomicU32::new(0);
static FAT_SIZE: AtomicU32 = AtomicU32::new(0); // sectors per FAT
static DATA_START: AtomicU32 = AtomicU32::new(0); // first data sector
static ROOT_CLUS: AtomicU32 = AtomicU32::new(0);
static PART_START: AtomicU64 = AtomicU64::new(0); // LBA of partition sector 0

// ── Sector buffer ─────────────────────────────────────────────────────────────

struct SecBuf([u8; 512]);
unsafe impl Send for SecBuf {}
static SEC_BUF: Mutex<SecBuf> = Mutex::new(SecBuf([0u8; 512]));

// ── Helpers ───────────────────────────────────────────────────────────────────

fn read_le16(buf: &[u8], off: usize) -> u16 {
    (buf[off] as u16) | ((buf[off + 1] as u16) << 8)
}

fn read_le32(buf: &[u8], off: usize) -> u32 {
    (buf[off] as u32)
        | ((buf[off + 1] as u32) << 8)
        | ((buf[off + 2] as u32) << 16)
        | ((buf[off + 3] as u32) << 24)
}

/// Read exactly one sector into `buf` at `lba` (absolute disk LBA).
fn read_sector(lba: u64, buf: &mut [u8; 512]) -> Result<(), &'static str> {
    virtio_blk::read_sector(lba, buf)
}

/// Convert a cluster number to its first absolute LBA on disk.
fn cluster_to_lba(cluster: u32) -> u64 {
    let data_start = DATA_START.load(Ordering::Relaxed) as u64;
    let spc = SEC_PER_CLUS.load(Ordering::Relaxed) as u64;
    let part_start = PART_START.load(Ordering::Relaxed);
    part_start + data_start + (cluster as u64 - 2) * spc
}

/// Follow the FAT chain to get the next cluster (returns 0x0FFF_FFFF on EOF).
fn fat_next_cluster(cluster: u32) -> u32 {
    let part_start = PART_START.load(Ordering::Relaxed);
    let rsvd = RSVD_SECS.load(Ordering::Relaxed) as u64;
    let fat_offset = (cluster * 4) as u64; // FAT32: 4 bytes per entry
    let fat_sector = rsvd + fat_offset / 512;
    let entry_off = (fat_offset % 512) as usize;

    let mut buf = [0u8; 512];
    if read_sector(part_start + fat_sector, &mut buf).is_err() {
        return 0x0FFF_FFFF;
    }
    read_le32(&buf, entry_off) & 0x0FFF_FFFF
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Attempt to mount the first FAT32 partition from the virtio-blk device.
/// Returns `true` on success.
pub fn mount() -> bool {
    if virtio_blk::sector_count() == 0 {
        crate::serial::write_line("fat32: no disk sectors available");
        return false;
    }

    // --- Step 1: Read MBR (LBA 0) to find first partition ---
    let mut mbr = [0u8; 512];
    if read_sector(0, &mut mbr).is_err() {
        crate::serial::write_line("fat32: cannot read MBR");
        return false;
    }

    // Signature check
    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        crate::serial::write_line("fat32: MBR signature missing — trying LBA 0 as raw FAT32");
        // Might be a raw FAT32 image without MBR
        return try_mount_at(0);
    }

    // Parse first partition entry at offset 0x1BE
    let part_type = mbr[0x1BE + 4];
    let part_lba = read_le32(&mbr, 0x1BE + 8) as u64;

    crate::serial::write_str("fat32: MBR partition 0 type=0x");
    crate::serial::write_hex64(part_type as u64);
    crate::serial::write_str(" start_lba=");
    crate::serial::write_u64(part_lba);
    crate::serial::write_line("");

    // Type 0x0B / 0x0C = FAT32; 0x00 = unused (try raw)
    if part_type == 0x00 || part_lba == 0 {
        return try_mount_at(0);
    }
    try_mount_at(part_lba)
}

fn try_mount_at(part_start_lba: u64) -> bool {
    let mut vbr = [0u8; 512];
    if read_sector(part_start_lba, &mut vbr).is_err() {
        crate::serial::write_line("fat32: cannot read VBR/BPB");
        return false;
    }

    // --- Validate BPB ---
    let bytes_per_sec = read_le16(&vbr, 11) as u32;
    let sec_per_clus = vbr[13] as u32;
    let rsvd_secs = read_le16(&vbr, 14) as u32;
    let num_fats = vbr[16] as u32;
    let total_secs16 = read_le16(&vbr, 19) as u32;
    let fat_size16 = read_le16(&vbr, 22) as u32;
    let total_secs32 = read_le32(&vbr, 32);
    let fat_size32 = read_le32(&vbr, 36);
    let root_cluster = read_le32(&vbr, 44);
    // FAT32 signature at offset 66 should be 0x28 or 0x29
    let ext_sig = vbr[66];

    if bytes_per_sec != 512 {
        crate::serial::write_line("fat32: only 512-byte sectors supported");
        return false;
    }
    if sec_per_clus == 0 || num_fats == 0 {
        crate::serial::write_line("fat32: invalid BPB (spc=0 or fats=0)");
        return false;
    }
    if fat_size16 != 0 || fat_size32 == 0 {
        // FAT32 always has fat_size16 == 0 and fat_size32 != 0
        crate::serial::write_line("fat32: not a FAT32 volume");
        return false;
    }
    if ext_sig != 0x28 && ext_sig != 0x29 {
        crate::serial::write_line("fat32: extended boot signature mismatch — continuing anyway");
    }

    let total_secs = if total_secs32 != 0 {
        total_secs32
    } else {
        total_secs16
    };
    let fat_size = fat_size32;
    let data_start = rsvd_secs + num_fats * fat_size;

    // Store FS geometry
    BYTES_PER_SEC.store(bytes_per_sec, Ordering::Relaxed);
    SEC_PER_CLUS.store(sec_per_clus, Ordering::Relaxed);
    RSVD_SECS.store(rsvd_secs, Ordering::Relaxed);
    NUM_FATS.store(num_fats, Ordering::Relaxed);
    FAT_SIZE.store(fat_size, Ordering::Relaxed);
    DATA_START.store(data_start, Ordering::Relaxed);
    ROOT_CLUS.store(root_cluster, Ordering::Relaxed);
    PART_START.store(part_start_lba, Ordering::Relaxed);

    MOUNTED.store(true, Ordering::Release);

    crate::serial::write_str("fat32: mounted  spc=");
    crate::serial::write_u64(sec_per_clus as u64);
    crate::serial::write_str(" fats=");
    crate::serial::write_u64(num_fats as u64);
    crate::serial::write_str(" fat_size=");
    crate::serial::write_u64(fat_size as u64);
    crate::serial::write_str(" data_start=");
    crate::serial::write_u64(data_start as u64);
    crate::serial::write_str(" root_clus=");
    crate::serial::write_u64(root_cluster as u64);
    crate::serial::write_str(" total_secs=");
    crate::serial::write_u64(total_secs as u64);
    crate::serial::write_line("");
    true
}

/// Returns the root directory cluster number.
pub fn root_cluster() -> u32 {
    ROOT_CLUS.load(Ordering::Relaxed)
}

/// Returns `true` if a FAT32 volume is currently mounted.
pub fn is_mounted() -> bool {
    MOUNTED.load(Ordering::Acquire)
}

/// Return (used_kb, total_kb) for the mounted FAT32 volume.
/// Scans the FAT to count free clusters — O(fat_size) but fast enough for display.
/// Returns (0, 0) if not mounted.
pub fn disk_space_kb() -> (u64, u64) {
    if !is_mounted() {
        return (0, 0);
    }
    let part_start = PART_START.load(Ordering::Relaxed);
    let rsvd = RSVD_SECS.load(Ordering::Relaxed) as u64;
    let fat_size = FAT_SIZE.load(Ordering::Relaxed) as u64;
    let spc = SEC_PER_CLUS.load(Ordering::Relaxed) as u64;
    if fat_size == 0 || spc == 0 {
        return (0, 0);
    }

    let bytes_per_clus = spc * 512;
    let total_clusters = fat_size * 128; // 128 entries per 512-byte sector
    let mut free_clusters: u64 = 0;

    for sec_off in 0..fat_size {
        let lba = part_start + rsvd + sec_off;
        let mut buf = [0u8; 512];
        if read_sector(lba, &mut buf).is_err() {
            continue;
        }
        for i in 0..128usize {
            let cluster = (sec_off * 128) as u32 + i as u32;
            if cluster < 2 {
                continue;
            }
            let off = i * 4;
            let entry = read_le32(&buf, off) & 0x0FFF_FFFF;
            if entry == 0 {
                free_clusters += 1;
            }
        }
    }

    let total_kb = (total_clusters * bytes_per_clus) / 1024;
    let used_kb = ((total_clusters - free_clusters) * bytes_per_clus) / 1024;
    (used_kb, total_kb)
}

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

// ── FAT32 mkfs ────────────────────────────────────────────────────────────────

/// Write a minimal FAT32 filesystem to the raw virtio-blk device.
/// Assumes a 64 MiB device (131072 × 512-byte sectors).
/// Safe to call on any size ≥ 4 MiB; uses the actual sector count.
/// Returns `true` on success.
pub fn mkfs() -> bool {
    let total_secs = virtio_blk::sector_count();
    if total_secs < 8192 {
        crate::serial::write_line("fat32: mkfs: disk too small");
        return false;
    }
    let total_secs = total_secs as u32;

    // FAT32 geometry
    let bps: u32 = 512;
    let spc: u32 = 8; // sectors per cluster → 4 KiB clusters
    let rsvd: u32 = 32; // reserved sectors (standard FAT32)
    let nfats: u32 = 2;

    // Calculate FAT size: each cluster needs a 4-byte FAT entry.
    // fat_size = ceil((max_clusters + 2) * 4 / 512)
    // Iterate once to get exact value (no floating point).
    let fat_size = {
        let approx_data = total_secs.saturating_sub(rsvd + nfats * 128);
        let approx_clus = approx_data / spc + 2;
        (approx_clus * 4 + 511) / 512
    };

    let data_start = rsvd + nfats * fat_size;
    let root_clus: u32 = 2;

    crate::serial::write_str("fat32: mkfs  total_secs=");
    crate::serial::write_u64(total_secs as u64);
    crate::serial::write_str(" fat_size=");
    crate::serial::write_u64(fat_size as u64);
    crate::serial::write_str(" data_start=");
    crate::serial::write_u64(data_start as u64);
    crate::serial::write_line("");

    // ── Sector 0: VBR / BPB ──────────────────────────────────────────────────
    let mut vbr = [0u8; 512];
    // Jump boot + OEM
    vbr[0] = 0xEB;
    vbr[1] = 0x58;
    vbr[2] = 0x90;
    vbr[3..11].copy_from_slice(b"MSDOS5.0");
    // BPB
    write_le16(&mut vbr, 11, bps as u16);
    vbr[13] = spc as u8;
    write_le16(&mut vbr, 14, rsvd as u16);
    vbr[16] = nfats as u8;
    write_le16(&mut vbr, 17, 0); // root entry count = 0 (FAT32)
    write_le16(&mut vbr, 19, 0); // total sectors 16 = 0 (use 32-bit)
    vbr[21] = 0xF8; // media descriptor
    write_le16(&mut vbr, 22, 0); // fat_size_16 = 0 (FAT32)
    write_le16(&mut vbr, 24, 63); // sectors per track (dummy)
    write_le16(&mut vbr, 26, 255); // number of heads (dummy)
    write_le32(&mut vbr, 28, 0); // hidden sectors
    write_le32(&mut vbr, 32, total_secs);
    // FAT32 extended BPB
    write_le32(&mut vbr, 36, fat_size);
    write_le16(&mut vbr, 40, 0); // ext flags
    write_le16(&mut vbr, 42, 0); // fs version 0.0
    write_le32(&mut vbr, 44, root_clus);
    write_le16(&mut vbr, 48, 1); // FSInfo sector
    write_le16(&mut vbr, 50, 6); // backup boot sector
    vbr[64] = 0x80; // drive number
    vbr[66] = 0x29; // extended boot signature
                    // Volume ID (dummy 4 bytes)
    vbr[67] = 0x41;
    vbr[68] = 0x53;
    vbr[69] = 0x54;
    vbr[70] = 0x52;
    // Volume label (11 bytes)
    vbr[71..82].copy_from_slice(b"ASTRA OS   ");
    // FS type string (8 bytes)
    vbr[82..90].copy_from_slice(b"FAT32   ");
    // Boot signature
    vbr[510] = 0x55;
    vbr[511] = 0xAA;

    if write_sector_raw(0, &vbr).is_err() {
        crate::serial::write_line("fat32: mkfs: failed to write VBR");
        return false;
    }

    // ── Sector 1: FSInfo ─────────────────────────────────────────────────────
    let mut fsi = [0u8; 512];
    write_le32(&mut fsi, 0, 0x4161_5252); // FSInfo lead signature
    write_le32(&mut fsi, 484, 0x6141_7272); // FSInfo structure signature
    write_le32(&mut fsi, 488, 0xFFFF_FFFF); // free cluster count (unknown)
    write_le32(&mut fsi, 492, 0xFFFF_FFFF); // next free cluster (unknown)
    write_le32(&mut fsi, 508, 0xAA55_0000); // trail signature (note: bytes 508..511)
    fsi[510] = 0x55;
    fsi[511] = 0xAA;
    let _ = write_sector_raw(1, &fsi);

    // ── FAT copies ───────────────────────────────────────────────────────────
    // Write FAT sector 0 (media descriptor + reserved entries) for each copy
    let mut fat0 = [0u8; 512];
    write_le32(&mut fat0, 0, 0x0FFF_FFF8); // entry 0: media
    write_le32(&mut fat0, 4, 0x0FFF_FFFF); // entry 1: reserved
    write_le32(&mut fat0, 8, 0x0FFF_FFFF); // entry 2: root dir EOC

    for i in 0..nfats {
        let lba = rsvd as u64 + i as u64 * fat_size as u64;
        if write_sector_raw(lba, &fat0).is_err() {
            crate::serial::write_line("fat32: mkfs: failed to write FAT");
            return false;
        }
        // Zero remaining FAT sectors
        let zero = [0u8; 512];
        for s in 1..fat_size as u64 {
            let _ = write_sector_raw(lba + s, &zero);
        }
    }

    // ── Root directory cluster (cluster 2) ───────────────────────────────────
    let zero = [0u8; 512];
    let root_lba = data_start as u64; // cluster 2 → sector data_start + (2-2)*spc
    for s in 0..spc as u64 {
        if write_sector_raw(root_lba + s, &zero).is_err() {
            crate::serial::write_line("fat32: mkfs: failed to zero root dir");
            return false;
        }
    }

    crate::serial::write_line("fat32: mkfs complete");
    true
}
