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

