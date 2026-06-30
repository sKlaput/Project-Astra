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

