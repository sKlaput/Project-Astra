use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;

pub type NodeId = u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    NotMounted,
    InvalidPath,
    NotFound,
    NotFile,
    NotDirectory,
    NotEmpty,
}

#[derive(Debug, Clone, Copy)]
pub struct Mount {
    pub name: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Node {
    pub id: NodeId,
    pub name: &'static str,
    pub parent: Option<NodeId>,
    pub kind: NodeKind,
    pub data: &'static [u8],
}

#[derive(Debug, Clone, Copy)]
pub struct FileHandle {
    pub node: NodeId,
    pub offset: usize,
}

const ROOT_NODE_ID: NodeId = 0;
const ETC_NODE_ID: NodeId = 1;
const MOTD_NODE_ID: NodeId = 2;
const HELLO_NODE_ID: NodeId = 3;

static ROOT_MOUNTED: AtomicBool = AtomicBool::new(false);

static NODES: [Node; 4] = [
    Node {
        id: ROOT_NODE_ID,
        name: "",
        parent: None,
        kind: NodeKind::Directory,
        data: b"",
    },
    Node {
        id: ETC_NODE_ID,
        name: "etc",
        parent: Some(ROOT_NODE_ID),
        kind: NodeKind::Directory,
        data: b"",
    },
    Node {
        id: MOTD_NODE_ID,
        name: "motd",
        parent: Some(ETC_NODE_ID),
        kind: NodeKind::File,
        data: b"kernel vfs motd\n",
    },
    Node {
        id: HELLO_NODE_ID,
        name: "hello.txt",
        parent: Some(ROOT_NODE_ID),
        kind: NodeKind::File,
        data: b"hello from rootfs\n",
    },
];

// ── Writable buffer for /hello.txt ───────────────────────────────────────────

pub const WRITABLE_MAX: usize = 8192;
static HELLO_BUF: Mutex<[u8; WRITABLE_MAX]> = Mutex::new([0u8; WRITABLE_MAX]);
static HELLO_BUF_LEN: AtomicUsize = AtomicUsize::new(0);

pub fn mount_root() -> Result<Mount, VfsError> {
    // Seed the writable hello.txt buffer with its static default content
    {
        let src = b"hello from rootfs\n";
        let mut buf = HELLO_BUF.lock();
        buf[..src.len()].copy_from_slice(src);
        HELLO_BUF_LEN.store(src.len(), Ordering::Release);
    }
    ROOT_MOUNTED.store(true, Ordering::Release);
    Ok(Mount { name: "rootfs" })
}

pub fn root_mount() -> Result<Mount, VfsError> {
    if ROOT_MOUNTED.load(Ordering::Acquire) {
        Ok(Mount { name: "rootfs" })
    } else {
        Err(VfsError::NotMounted)
    }
}

pub fn lookup(path: &str) -> Result<&'static Node, VfsError> {
    if !ROOT_MOUNTED.load(Ordering::Acquire) {
        return Err(VfsError::NotMounted);
    }
    if !path.starts_with('/') {
        return Err(VfsError::InvalidPath);
    }
    if path == "/" {
        return Ok(&NODES[ROOT_NODE_ID as usize]);
    }

    let mut current = ROOT_NODE_ID;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let mut found: Option<NodeId> = None;
        for node in NODES.iter() {
            if node.parent == Some(current) && node.name == seg {
                found = Some(node.id);
                break;
            }
        }
        current = match found {
            Some(id) => id,
            None => return Err(VfsError::NotFound),
        };
    }

    Ok(&NODES[current as usize])
}

pub fn open(path: &str) -> Result<FileHandle, VfsError> {
    // FAT32 virtual path: "/fat32/<4-hex-node-id>"
    if let Some(hex) = path.strip_prefix("/fat32/") {
        let id = parse_hex_u16(hex.as_bytes()).ok_or(VfsError::NotFound)?;
        if is_fat32_id(id) {
            return Ok(FileHandle {
                node: id,
                offset: 0,
            });
        }
        return Err(VfsError::NotFound);
    }
    // Try static VFS first
    match lookup(path) {
        Ok(node) => {
            if node.kind != NodeKind::File {
                return Err(VfsError::NotFile);
            }
            return Ok(FileHandle {
                node: node.id,
                offset: 0,
            });
        }
        Err(VfsError::NotFound) => {}
        Err(e) => return Err(e),
    }
    // Fall through to dynamic layer
    if let Some(id) = dyn_path_to_id(path) {
        return Ok(FileHandle {
            node: id,
            offset: 0,
        });
    }
    Err(VfsError::NotFound)
}

pub fn directory_entry_count(path: &str) -> Result<usize, VfsError> {
    let dir = lookup(path)?;
    if dir.kind != NodeKind::Directory {
        return Err(VfsError::NotDirectory);
    }

    let mut count = 0usize;
    for node in NODES.iter() {
        if node.parent == Some(dir.id) {
            count += 1;
        }
    }

    Ok(count)
}

pub fn directory_contains(path: &str, name: &str) -> Result<bool, VfsError> {
    let dir = lookup(path)?;
    if dir.kind != NodeKind::Directory {
        return Err(VfsError::NotDirectory);
    }

    for node in NODES.iter() {
        if node.parent == Some(dir.id) && node.name == name {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn read(handle: &mut FileHandle, buf: &mut [u8]) -> Result<usize, VfsError> {
    if !ROOT_MOUNTED.load(Ordering::Acquire) {
        return Err(VfsError::NotMounted);
    }
    // FAT32-backed nodes
    if is_fat32_id(handle.node) {
        let n = fat32_read(handle.node, handle.offset, buf);
        handle.offset += n;
        return Ok(n);
    }
    // Dispatch dynamic nodes before indexing into NODES
    if handle.node >= DYN_ID_BASE {
        let n = dyn_read_by_id(handle.node, handle.offset, buf)?;
        handle.offset += n;
        return Ok(n);
    }
    let node = &NODES[handle.node as usize];
    if node.kind != NodeKind::File {
        return Err(VfsError::NotFile);
    }

    if node.id == HELLO_NODE_ID {
        // Read from the dynamic writable buffer
        let total = HELLO_BUF_LEN.load(Ordering::Acquire);
        if handle.offset >= total {
            return Ok(0);
        }
        let remaining = total - handle.offset;
        let take = remaining.min(buf.len());
        let content = HELLO_BUF.lock();
        buf[..take].copy_from_slice(&content[handle.offset..handle.offset + take]);
        handle.offset += take;
        return Ok(take);
    }

    let data = node.data;
    if handle.offset >= data.len() {
        return Ok(0);
    }

    let remaining = data.len() - handle.offset;
    let take = remaining.min(buf.len());
    let start = handle.offset;
    let end = start + take;

    buf[..take].copy_from_slice(&data[start..end]);
    handle.offset = end;
    Ok(take)
}

/// Write (overwrite) a writable file — static hello.txt or any dynamic file.
pub fn write_file(path: &str, data: &[u8]) -> Result<usize, VfsError> {
    if !ROOT_MOUNTED.load(Ordering::Acquire) {
        return Err(VfsError::NotMounted);
    }
    // FAT32 file write-back: path = "/fat32/<hex_id>"
    if let Some(hex) = path.strip_prefix("/fat32/") {
        if let Some(id) = parse_hex_u16(hex.as_bytes()) {
            if let Some(entry) = fat32_lookup_id(id) {
                if entry.is_dir {
                    return Err(VfsError::NotFile);
                }
                let ok = crate::fat32::write_file(
                    entry.dir_cluster,
                    &entry.name[..entry.name_len],
                    data,
                );
                if !ok {
                    return Err(VfsError::NotFound);
                }
                // Refresh cached size
                {
                    let mut cache = FAT32_CACHE.lock();
                    for e in cache.iter_mut() {
                        if e.live && e.id == id {
                            e.size = data.len() as u32;
                        }
                    }
                }
                return Ok(data.len());
            }
        }
        return Err(VfsError::NotFound);
    }
    // Try static writable node first
    if let Ok(node) = lookup(path) {
        if node.kind != NodeKind::File {
            return Err(VfsError::NotFile);
        }
        if node.id == HELLO_NODE_ID {
            let len = data.len().min(WRITABLE_MAX);
            let mut buf = HELLO_BUF.lock();
            buf[..len].copy_from_slice(&data[..len]);
            HELLO_BUF_LEN.store(len, Ordering::Release);
            return Ok(len);
        }
        return Err(VfsError::NotFile); // static but read-only
    }
    // Try dynamic layer — write to RAM and also persist to FAT32
    if let Some(id) = dyn_path_to_id(path) {
        let result = dyn_write_file(id, data);
        if result.is_ok() && crate::fat32::is_mounted() {
            // Derive filename from path and write to FAT32 root for persistence
            let fname = path.rsplit('/').next().unwrap_or("");
            if !fname.is_empty() {
                let root = crate::fat32::root_cluster();
                crate::fat32::write_file(root, fname.as_bytes(), data);
            }
        }
        return result;
    }
    Err(VfsError::NotFound)
}

/// Returns true if the given path is writable (static hello.txt or any dynamic file).
pub fn is_writable(path: &str) -> bool {
    // FAT32 files are writable
    if let Some(hex) = path.strip_prefix("/fat32/") {
        if let Some(id) = parse_hex_u16(hex.as_bytes()) {
            if let Some(e) = fat32_lookup_id(id) {
                return !e.is_dir;
            }
        }
        return false;
    }
    matches!(lookup(path), Ok(n) if n.id == HELLO_NODE_ID) || dyn_path_to_id(path).is_some()
}

/// Exposes the static VFS node table so GUI apps can enumerate entries.
pub fn iter_nodes() -> &'static [Node] {
    &NODES
}

/// Resolve an absolute path through both static and dynamic VFS layers.
/// Returns `Some(NodeId)` for both files and directories.
pub fn resolve_node_id(path: &str) -> Option<NodeId> {
    if !ROOT_MOUNTED.load(Ordering::Acquire) {
        return None;
    }
    if path == "/" {
        return Some(ROOT_NODE_ID);
    }
    if !path.starts_with('/') {
        return None;
    }
    let mut current: NodeId = ROOT_NODE_ID;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let nb = seg.as_bytes();
        let mut found = false;
        for node in NODES.iter() {
            if node.parent == Some(current) && node.name.as_bytes() == nb {
                current = node.id;
                found = true;
                break;
            }
        }
        if found {
            continue;
        }
        // Not in static nodes — check dynamic layer
        let mut dyn_found = false;
        {
            let files = DYN_FILES.lock();
            for f in files.iter() {
                if f.live && f.parent == current && &f.name[..f.nlen] == nb {
                    current = f.id;
                    dyn_found = true;
                    break;
                }
            }
        }
        if !dyn_found {
            return None;
        }
    }
    Some(current)
}

// ── Dynamic file layer ───────────────────────────────────────────────────────
// Supports up to MAX_DYN_FILES user-created files, each up to DYN_FILE_MAX bytes.
// Dynamic NodeIds start at DYN_ID_BASE (≥ 100) so they never alias static nodes.

const MAX_DYN_FILES: usize = 16;
const DYN_ID_BASE: NodeId = 100;
const DYN_FILE_MAX: usize = 4096;

#[derive(Clone, Copy)]
struct DynFile {
    live: bool,
    is_dir: bool,
    id: NodeId,
    parent: NodeId,
    name: [u8; 32],
    nlen: usize,
    data: [u8; DYN_FILE_MAX],
    dlen: usize,
}

impl DynFile {
    const EMPTY: Self = DynFile {
        live: false,
        is_dir: false,
        id: 0,
        parent: 0,
        name: [0u8; 32],
        nlen: 0,
        data: [0u8; DYN_FILE_MAX],
        dlen: 0,
    };
}

/// Public descriptor returned by `dyn_list_dir`.
#[derive(Clone, Copy)]
pub struct DynEntry {
    pub id: NodeId,
    pub name: [u8; 32],
    pub nlen: usize,
    pub is_dir: bool,
    pub size: usize,
}

static DYN_FILES: Mutex<[DynFile; MAX_DYN_FILES]> = Mutex::new([DynFile::EMPTY; MAX_DYN_FILES]);
static DYN_NEXT_ID: AtomicUsize = AtomicUsize::new(DYN_ID_BASE as usize);

/// List dynamic files and directories whose parent is `parent_id`.
/// Fills `out` and returns how many entries were written.
pub fn dyn_list_dir(parent_id: NodeId, out: &mut [DynEntry]) -> usize {
    let files = DYN_FILES.lock();
    let mut n = 0usize;
    for f in files.iter() {
        if n >= out.len() {
            break;
        }
        if !f.live || f.parent != parent_id {
            continue;
        }
        let fid = f.id;
        let size = if f.is_dir {
            files.iter().filter(|c| c.live && c.parent == fid).count()
        } else {
            f.dlen
        };
        out[n] = DynEntry {
            id: f.id,
            name: f.name,
            nlen: f.nlen,
            is_dir: f.is_dir,
            size,
        };
        n += 1;
    }
    n
}

/// Create a new empty dynamic file under `parent_id`.
/// Returns the new NodeId on success, or an error if the name is taken or no slot is free.
pub fn dyn_create_file(parent_id: NodeId, name: &str) -> Result<NodeId, VfsError> {
    if !ROOT_MOUNTED.load(Ordering::Acquire) {
        return Err(VfsError::NotMounted);
    }
    let nb = name.as_bytes();
    if nb.is_empty() || nb.len() > 32 {
        return Err(VfsError::InvalidPath);
    }
    // Reject names that collide with static nodes in the same directory
    for node in NODES.iter() {
        if node.parent == Some(parent_id) && node.name.as_bytes() == nb {
            return Err(VfsError::InvalidPath);
        }
    }
    let mut files = DYN_FILES.lock();
    // Reject duplicate dynamic names
    for f in files.iter() {
        if f.live && f.parent == parent_id && &f.name[..f.nlen] == nb {
            return Err(VfsError::InvalidPath);
        }
    }
    // Assign to the first free slot
    for f in files.iter_mut() {
        if !f.live {
            let id = DYN_NEXT_ID.fetch_add(1, Ordering::Relaxed) as NodeId;
            *f = DynFile::EMPTY;
            f.live = true;
            f.is_dir = false;
            f.id = id;
            f.parent = parent_id;
            f.nlen = nb.len();
            f.name[..f.nlen].copy_from_slice(nb);
            return Ok(id);
        }
    }
    Err(VfsError::NotFound) // no free slot
}

/// Create a new empty dynamic directory under `parent_id`.
pub fn dyn_create_dir(parent_id: NodeId, name: &str) -> Result<NodeId, VfsError> {
    if !ROOT_MOUNTED.load(Ordering::Acquire) {
        return Err(VfsError::NotMounted);
    }
    let nb = name.as_bytes();
    if nb.is_empty() || nb.len() > 32 {
        return Err(VfsError::InvalidPath);
    }
    for node in NODES.iter() {
        if node.parent == Some(parent_id) && node.name.as_bytes() == nb {
            return Err(VfsError::InvalidPath);
        }
    }
    let mut files = DYN_FILES.lock();
    for f in files.iter() {
        if f.live && f.parent == parent_id && &f.name[..f.nlen] == nb {
            return Err(VfsError::InvalidPath);
        }
    }
    for f in files.iter_mut() {
        if !f.live {
            let id = DYN_NEXT_ID.fetch_add(1, Ordering::Relaxed) as NodeId;
            *f = DynFile::EMPTY;
            f.live = true;
            f.is_dir = true;
            f.id = id;
            f.parent = parent_id;
            f.nlen = nb.len();
            f.name[..f.nlen].copy_from_slice(nb);
            return Ok(id);
        }
    }
    Err(VfsError::NotFound) // no free slot
}

/// Delete a dynamic file or empty directory by NodeId.
/// Returns `VfsError::NotEmpty` if the target directory still has children.
pub fn dyn_delete_node(id: NodeId) -> Result<(), VfsError> {
    let mut files = DYN_FILES.lock();
    let mut target_idx: Option<usize> = None;
    let mut is_dir = false;
    for (i, f) in files.iter().enumerate() {
        if f.live && f.id == id {
            target_idx = Some(i);
            is_dir = f.is_dir;
            break;
        }
    }
    let idx = target_idx.ok_or(VfsError::NotFound)?;
    if is_dir {
        for f in files.iter() {
            if f.live && f.parent == id {
                return Err(VfsError::NotEmpty);
            }
        }
    }
    files[idx] = DynFile::EMPTY;
    Ok(())
}

/// Rename a dynamic file.  Fails if the new name already exists in the same directory.
pub fn dyn_rename_file(id: NodeId, new_name: &str) -> Result<(), VfsError> {
    let nb = new_name.as_bytes();
    if nb.is_empty() || nb.len() > 32 {
        return Err(VfsError::InvalidPath);
    }
    let mut files = DYN_FILES.lock();
    // Find parent for collision checks
    let parent = {
        let mut p = 0u16;
        for f in files.iter() {
            if f.live && f.id == id {
                p = f.parent;
                break;
            }
        }
        p
    };
    for f in files.iter() {
        if f.live && f.id != id && f.parent == parent && &f.name[..f.nlen] == nb {
            return Err(VfsError::InvalidPath);
        }
    }
    for node in NODES.iter() {
        if node.parent == Some(parent) && node.name.as_bytes() == nb {
            return Err(VfsError::InvalidPath);
        }
    }
    for f in files.iter_mut() {
        if f.live && f.id == id {
            let nlen = nb.len();
            f.name = [0u8; 32];
            f.name[..nlen].copy_from_slice(nb);
            f.nlen = nlen;
            return Ok(());
        }
    }
    Err(VfsError::NotFound)
}

/// Write data to a dynamic file (full overwrite).
pub fn dyn_write_file(id: NodeId, data: &[u8]) -> Result<usize, VfsError> {
    let mut files = DYN_FILES.lock();
    for f in files.iter_mut() {
        if f.live && f.id == id {
            let n = data.len().min(DYN_FILE_MAX);
            f.data[..n].copy_from_slice(&data[..n]);
            f.dlen = n;
            return Ok(n);
        }
    }
    Err(VfsError::NotFound)
}

/// Read from a dynamic file starting at `offset`.  Used by `read()`.
fn dyn_read_by_id(id: NodeId, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
    let files = DYN_FILES.lock();
    for f in files.iter() {
        if f.live && f.id == id {
            if offset >= f.dlen {
                return Ok(0);
            }
            let take = (f.dlen - offset).min(buf.len());
            buf[..take].copy_from_slice(&f.data[offset..offset + take]);
            return Ok(take);
        }
    }
    Err(VfsError::NotFound)
}

/// Resolve an absolute path to a dynamic **file** NodeId (directories excluded).
fn dyn_path_to_id(path: &str) -> Option<NodeId> {
    let (dir, name) = rsplit_path(path)?;
    let dir_id = resolve_node_id(dir)?;
    let nb = name.as_bytes();
    let files = DYN_FILES.lock();
    for f in files.iter() {
        if f.live && !f.is_dir && f.parent == dir_id && &f.name[..f.nlen] == nb {
            return Some(f.id);
        }
    }
    None
}

/// Split "/foo/bar" into ("/foo", "bar").  Returns `None` for root or empty paths.
fn rsplit_path(path: &str) -> Option<(&str, &str)> {
    let b = path.as_bytes();
    if b.len() <= 1 {
        return None;
    } // just "/" or empty
    let mut i = b.len() - 1;
    while i > 0 && b[i] != b'/' {
        i -= 1;
    }
    if i == 0 {
        Some(("/", &path[1..]))
    } else {
        Some((&path[..i], &path[i + 1..]))
    }
}

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
