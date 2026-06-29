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
