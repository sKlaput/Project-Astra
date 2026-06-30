// ── FAT32 VFS bridge ─────────────────────────────────────────────────────────
//
// FAT32 files are exposed as read-only entries alongside the dynamic layer.
// The File Manager sees them via `fat32_list_dir` / `fat32_read_file`.
//
// We store FAT32-specific metadata (cluster + size) in a static table
// so that `open()` and `read()` can access FAT32 files by NodeId.
// NodeIds for FAT32 entries start at FAT32_ID_BASE (0x4000) and are
// assigned on first list of each directory (evicted on re-list of same dir).
//
// This is intentionally simple: no persistent cache, no write support.

pub const FAT32_ID_BASE: NodeId = 0x4000;
const MAX_FAT32_CACHE: usize = 64;

#[derive(Clone, Copy)]
struct Fat32Cache {
    live: bool,
    id: NodeId,
    cluster: u32,
    size: u32,
    is_dir: bool,
    dir_cluster: u32, // directory that contains this entry
    name: [u8; 64],   // display name (LFN or lowercased 8.3)
    name_len: usize,
}

impl Fat32Cache {
    const EMPTY: Self = Fat32Cache {
        live: false,
        id: 0,
        cluster: 0,
        size: 0,
        is_dir: false,
        dir_cluster: 0,
        name: [0u8; 64],
        name_len: 0,
    };
}

static FAT32_CACHE: Mutex<[Fat32Cache; MAX_FAT32_CACHE]> =
    Mutex::new([Fat32Cache::EMPTY; MAX_FAT32_CACHE]);
static FAT32_NEXT_ID: AtomicUsize = AtomicUsize::new(FAT32_ID_BASE as usize);

fn fat32_alloc_id(
    cluster: u32,
    size: u32,
    is_dir: bool,
    dir_cluster: u32,
    name: [u8; 64],
    name_len: usize,
) -> NodeId {
    let mut cache = FAT32_CACHE.lock();
    // Match by cluster (for non-empty files) or by dir+name (for empty cluster==0 files)
    for e in cache.iter_mut() {
        if e.live && e.is_dir == is_dir {
            let hit = if cluster >= 2 {
                e.cluster == cluster
            } else {
                e.cluster == 0
                    && e.dir_cluster == dir_cluster
                    && e.name_len == name_len
                    && e.name[..name_len] == name[..name_len]
            };
            if hit {
                e.size = size;
                e.dir_cluster = dir_cluster;
                e.name = name;
                e.name_len = name_len;
                return e.id;
            }
        }
    }
    // Evict first free slot
    for e in cache.iter_mut() {
        if !e.live {
            let id = FAT32_NEXT_ID.fetch_add(1, Ordering::Relaxed) as NodeId;
            *e = Fat32Cache {
                live: true,
                id,
                cluster,
                size,
                is_dir,
                dir_cluster,
                name,
                name_len,
            };
            return id;
        }
    }
    // Cache full — evict slot 0
    let id = FAT32_NEXT_ID.fetch_add(1, Ordering::Relaxed) as NodeId;
    cache[0] = Fat32Cache {
        live: true,
        id,
        cluster,
        size,
        is_dir,
        dir_cluster,
        name,
        name_len,
    };
    id
}

fn fat32_lookup_id(id: NodeId) -> Option<Fat32Cache> {
    let cache = FAT32_CACHE.lock();
    for e in cache.iter() {
        if e.live && e.id == id {
            return Some(*e);
        }
    }
    None
}

/// List FAT32 entries in the directory at `fat_cluster`.
/// Appends to `out` starting at `start`, returns number of entries added.
pub fn fat32_list_dir(fat_cluster: u32, out: &mut [DynEntry], start: usize) -> usize {
    if !crate::fat32::is_mounted() {
        return 0;
    }
    let mut n = start;
    crate::fat32::list_dir(fat_cluster, |de| {
        if n >= out.len() {
            return false;
        }
        let mut name32 = [0u8; 32];
        let nlen = de.name_len.min(32);
        name32[..nlen].copy_from_slice(&de.name[..nlen]);
        let mut name64 = [0u8; 64];
        name64[..nlen].copy_from_slice(&de.name[..nlen]);
        let id = fat32_alloc_id(de.cluster, de.size, de.is_dir, fat_cluster, name64, nlen);
        out[n] = DynEntry {
            id,
            name: name32,
            nlen,
            is_dir: de.is_dir,
            size: de.size as usize,
        };
        n += 1;
        true
    });
    n - start
}

/// Read up to `buf.len()` bytes from the FAT32 file identified by NodeId.
/// Returns bytes read, or 0 if not found / not mounted.
pub fn fat32_read(id: NodeId, offset: usize, buf: &mut [u8]) -> usize {
    let entry = match fat32_lookup_id(id) {
        Some(e) => e,
        None => return 0,
    };
    if entry.is_dir {
        return 0;
    }

    // Fast path: no offset — read directly into caller's buffer (supports up to BUF_SIZE)
    if offset == 0 {
        return crate::fat32::read_file(entry.cluster, entry.size, buf);
    }

    // Slow path: non-zero offset — use an intermediate buffer
    const MAX_READ: usize = 8192;
    let mut tmp = [0u8; MAX_READ];
    let total = crate::fat32::read_file(entry.cluster, entry.size, &mut tmp);
    if offset >= total {
        return 0;
    }
    let take = (total - offset).min(buf.len());
    buf[..take].copy_from_slice(&tmp[offset..offset + take]);
    take
}

/// Create an empty FAT32 file on disk and register it in the cache.
/// Returns the NodeId to use as the editor path "/fat32/<hex_id>".
/// Returns `None` if FAT32 is not mounted, disk is full, or name is invalid.
pub fn fat32_create_and_open(dir_cluster: u32, name: &[u8]) -> Option<NodeId> {
    // Write zero-byte file to create the directory entry
    if !crate::fat32::write_file(dir_cluster, name, &[]) {
        return None;
    }
    // Find the new entry to read back its cluster (may be 0 for empty)
    let de = crate::fat32::find_in_dir(dir_cluster, name)?;
    let mut name64 = [0u8; 64];
    let nlen = de.name_len.min(64);
    name64[..nlen].copy_from_slice(&de.name[..nlen]);
    let id = fat32_alloc_id(de.cluster, 0, false, dir_cluster, name64, nlen);
    Some(id)
}

/// Find an existing FAT32 file and register it in the NodeId cache.
/// Does NOT write to disk — use this to open files that already exist.
/// Returns `None` if not mounted, not found, or the entry is a directory.
pub fn fat32_find_and_open(dir_cluster: u32, name: &[u8]) -> Option<NodeId> {
    if !crate::fat32::is_mounted() {
        return None;
    }
    let de = crate::fat32::find_in_dir(dir_cluster, name)?;
    if de.is_dir {
        return None;
    }
    let mut name64 = [0u8; 64];
    let nlen = de.name_len.min(64);
    name64[..nlen].copy_from_slice(&de.name[..nlen]);
    Some(fat32_alloc_id(
        de.cluster,
        de.size,
        false,
        dir_cluster,
        name64,
        nlen,
    ))
}

/// Returns the root FAT32 directory cluster (0 if not mounted).
pub fn fat32_root_cluster() -> u32 {
    if crate::fat32::is_mounted() {
        crate::fat32::root_cluster()
    } else {
        0
    }
}

/// Return the FAT32 cluster number for a directory node.
/// Used when navigating into a subdirectory so we store the real cluster, not the cache ID.
pub fn fat32_dir_cluster(id: NodeId) -> u32 {
    match fat32_lookup_id(id) {
        Some(e) if e.is_dir => e.cluster,
        _ => 0,
    }
}

/// Return the cached name and length for any FAT32 node (file or dir).
/// Returns `Some(([u8;13], usize))` or `None` if not cached.
pub fn fat32_entry_name(id: NodeId) -> Option<([u8; 64], usize)> {
    let e = fat32_lookup_id(id)?;
    Some((e.name, e.name_len))
}

/// Returns true if `id` is a FAT32-backed NodeId.
pub fn is_fat32_id(id: NodeId) -> bool {
    id >= FAT32_ID_BASE
}

/// Parse up to 4 hex digits into a u16.  Returns None on invalid input.
fn parse_hex_u16(s: &[u8]) -> Option<NodeId> {
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    let mut v: u16 = 0;
    for &b in s {
        let nibble = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        v = (v << 4) | nibble as u16;
    }
    Some(v)
}
