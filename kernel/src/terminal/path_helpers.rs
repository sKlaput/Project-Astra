// ── Path helpers ──────────────────────────────────────────────────────────────

/// Returns the current FAT32 cluster (root if at /).
fn cwd_cluster() -> Option<u32> {
    if !crate::fat32::is_mounted() {
        return None;
    }
    let t = TERM.lock();
    if t.cwd_plen == 0 {
        Some(crate::fat32::root_cluster())
    } else {
        Some(t.cwd_cluster)
    }
}

/// Walk a slash-separated path from root and return its cluster.
fn walk_path_to_cluster(path: &str) -> Option<u32> {
    if !crate::fat32::is_mounted() {
        return None;
    }
    let mut cluster = crate::fat32::root_cluster();
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let de = crate::fat32::find_in_dir(cluster, seg.as_bytes())?;
        if !de.is_dir {
            return None;
        }
        cluster = de.cluster;
    }
    Some(cluster)
}

/// Resolve an optional path argument to its FAT32 cluster.
/// If path is empty, returns cwd. Otherwise resolves relative to cwd.
fn resolve_cluster_for_path(path: &str) -> Option<u32> {
    if path.is_empty() {
        return cwd_cluster();
    }
    if path.starts_with('/') {
        // absolute path
        return walk_path_to_cluster(path);
    }
    // relative: prepend cwd
    let parent = cwd_cluster()?;
    let de = crate::fat32::find_in_dir(parent, path.as_bytes())?;
    if de.is_dir {
        Some(de.cluster)
    } else {
        None
    }
}

fn write_dec(buf: &mut [u8], mut n: u64) -> usize {
    if buf.is_empty() {
        return 0;
    }
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 20];
    let mut pos = tmp.len();
    while n > 0 {
        pos -= 1;
        tmp[pos] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let len = (tmp.len() - pos).min(buf.len());
    buf[..len].copy_from_slice(&tmp[pos..pos + len]);
    len
}

fn write_hex64(buf: &mut [u8], n: u64) -> usize {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    if buf.len() < 18 {
        return 0;
    }
    buf[0] = b'0';
    buf[1] = b'x';
    for i in 0..16 {
        let nyb = ((n >> ((15 - i) * 4)) & 0xF) as usize;
        buf[2 + i] = HEX[nyb];
    }
    18
}

