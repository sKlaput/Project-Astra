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
