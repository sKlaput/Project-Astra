// ── Command implementations ───────────────────────────────────────────────────

fn cmd_ls(args: &str) {
    // Resolve which cluster to list
    let cluster = if args.is_empty() {
        cwd_cluster()
    } else {
        resolve_cluster_for_path(args)
    };

    match cluster {
        None => {
            TERM.lock().push_str("ls: path not found", ERR_COL);
        }
        Some(clus) => {
            let mut count = 0usize;
            crate::fat32::list_dir(clus, |de| {
                // skip . and .. in listing
                if de.name_len == 1 && de.name[0] == b'.' {
                    return true;
                }
                if de.name_len == 2 && de.name[0] == b'.' && de.name[1] == b'.' {
                    return true;
                }
                // Build display line: "  NAME  <DIR>" or "  NAME  1234 B"
                let mut buf = [0u8; LINE_BUF];
                let mut pos = 0;
                buf[pos] = b' ';
                pos += 1;
                buf[pos] = b' ';
                pos += 1;
                let nn = de.name_len.min(12);
                buf[pos..pos + nn].copy_from_slice(&de.name[..nn]);
                pos += nn;
                // pad to column 14
                while pos < 16 {
                    buf[pos] = b' ';
                    pos += 1;
                }
                if de.is_dir {
                    let d = b"<DIR>";
                    buf[pos..pos + d.len()].copy_from_slice(d);
                    pos += d.len();
                } else {
                    pos += write_dec(&mut buf[pos..], de.size as u64);
                    let b_ = b" B";
                    buf[pos..pos + b_.len()].copy_from_slice(b_);
                    pos += b_.len();
                }
                let s = unsafe { core::str::from_utf8_unchecked(&buf[..pos]) };
                TERM.lock()
                    .push_str(s, if de.is_dir { 0x4FC3F7 } else { TEXT_NORM });
                count += 1;
                true
            });
            if count == 0 {
                TERM.lock().push_str("  (empty)", TEXT_NORM);
            }
        }
    }
}

fn cmd_cd(args: &str) {
    if args.is_empty() {
        // cd with no args → go to root
        let mut t = TERM.lock();
        t.cwd_cluster = 0;
        t.cwd_plen = 0;
        return;
    }
    if args == ".." {
        // go up — we only have the current path, so strip last segment
        let mut t = TERM.lock();
        if t.cwd_plen == 0 {
            return; // already at root
        }
        // find last '/' in path
        let path = &t.cwd_path[..t.cwd_plen];
        let slash = path.iter().rposition(|&b| b == b'/');
        match slash {
            None | Some(0) => {
                t.cwd_cluster = 0;
                t.cwd_plen = 0;
            }
            Some(pos) => {
                t.cwd_plen = pos;
                // re-resolve cluster by walking path from root
                let new_plen = pos;
                let new_path_bytes = {
                    let mut tmp = [0u8; PATH_BUF];
                    tmp[..new_plen].copy_from_slice(&t.cwd_path[..new_plen]);
                    tmp
                };
                drop(t);
                let path_str =
                    unsafe { core::str::from_utf8_unchecked(&new_path_bytes[..new_plen]) };
                let new_clus = walk_path_to_cluster(path_str).unwrap_or(0);
                let mut t2 = TERM.lock();
                t2.cwd_cluster = new_clus;
                t2.cwd_plen = new_plen;
            }
        }
        return;
    }

    // Navigate into a named subdirectory
    let parent_clus = cwd_cluster().unwrap_or_else(|| {
        if crate::fat32::is_mounted() {
            crate::fat32::root_cluster()
        } else {
            0
        }
    });
    let nb = args.as_bytes();
    match crate::fat32::find_in_dir(parent_clus, nb) {
        None => {
            TERM.lock().push_str("cd: not found", ERR_COL);
        }
        Some(de) if !de.is_dir => {
            TERM.lock().push_str("cd: not a directory", ERR_COL);
        }
        Some(de) => {
            let mut t = TERM.lock();
            // append /name to cwd_path
            let nn = de.name_len.min(12);
            let p = t.cwd_plen;
            if p + 1 + nn <= PATH_BUF {
                t.cwd_path[p] = b'/';
                let p1 = p + 1;
                t.cwd_path[p1..p1 + nn].copy_from_slice(&de.name[..nn]);
                t.cwd_plen = p1 + nn;
            }
            t.cwd_cluster = de.cluster;
        }
    }
}

fn cmd_cat(args: &str) {
    if args.is_empty() {
        TERM.lock().push_str("usage: cat <filename>", ERR_COL);
        return;
    }
    let parent_clus = cwd_cluster().unwrap_or_else(|| {
        if crate::fat32::is_mounted() {
            crate::fat32::root_cluster()
        } else {
            0
        }
    });
    let nb = args.as_bytes();
    match crate::fat32::find_in_dir(parent_clus, nb) {
        None => {
            TERM.lock().push_str("cat: file not found", ERR_COL);
        }
        Some(de) if de.is_dir => {
            TERM.lock().push_str("cat: is a directory", ERR_COL);
        }
        Some(de) => {
            // Read up to 4 KB and display line by line
            const READ_MAX: usize = 4096;
            let mut buf = [0u8; READ_MAX];
            let n = crate::fat32::read_file(de.cluster, de.size, &mut buf);
            if n == 0 {
                TERM.lock().push_str("(empty file)", TEXT_NORM);
                return;
            }
            let mut start = 0usize;
            let mut t = TERM.lock();
            for i in 0..n {
                if buf[i] == b'\n' || i == n - 1 {
                    let end = if buf[i] == b'\n' { i } else { i + 1 };
                    let line = &buf[start..end];
                    if !line.is_empty() {
                        // split into LINE_BUF-sized chunks if needed
                        let mut off = 0;
                        while off < line.len() {
                            let chunk = &line[off..(off + (LINE_BUF - 1)).min(line.len())];
                            t.push_bytes(chunk, TEXT_NORM);
                            off += LINE_BUF - 1;
                        }
                    } else {
                        t.push_str("", TEXT_NORM);
                    }
                    start = i + 1;
                }
            }
            if n >= READ_MAX {
                t.push_str("... (truncated at 4 KB)", TEXT_NORM);
            }
        }
    }
}

fn cmd_touch(args: &str) {
    if args.is_empty() {
        TERM.lock().push_str("usage: touch <filename>", ERR_COL);
        return;
    }
    if !crate::fat32::is_mounted() {
        TERM.lock()
            .push_str("touch: no FAT32 disk mounted", ERR_COL);
        return;
    }
    let parent_clus = cwd_cluster().unwrap_or_else(|| crate::fat32::root_cluster());
    let ok = crate::fat32::write_file(parent_clus, args.as_bytes(), &[]);
    let mut t = TERM.lock();
    if ok {
        t.push_str("created", TEXT_NORM);
    } else {
        t.push_str("touch: failed", ERR_COL);
    }
}

fn cmd_mkdir(args: &str) {
    if args.is_empty() {
        TERM.lock().push_str("usage: mkdir <dirname>", ERR_COL);
        return;
    }
    if !crate::fat32::is_mounted() {
        TERM.lock()
            .push_str("mkdir: no FAT32 disk mounted", ERR_COL);
        return;
    }
    let parent_clus = cwd_cluster().unwrap_or_else(|| crate::fat32::root_cluster());
    let ok = crate::fat32::create_dir(parent_clus, args.as_bytes());
    let mut t = TERM.lock();
    if ok {
        t.push_str("created", TEXT_NORM);
    } else {
        t.push_str("mkdir: failed", ERR_COL);
    }
}

fn cmd_rm(args: &str) {
    if args.is_empty() {
        TERM.lock().push_str("usage: rm <name>", ERR_COL);
        return;
    }
    if !crate::fat32::is_mounted() {
        TERM.lock().push_str("rm: no FAT32 disk mounted", ERR_COL);
        return;
    }
    let parent_clus = cwd_cluster().unwrap_or_else(|| crate::fat32::root_cluster());
    let ok = crate::fat32::delete_entry(parent_clus, args.as_bytes());
    let mut t = TERM.lock();
    if ok {
        t.push_str("deleted", TEXT_NORM);
    } else {
        t.push_str("rm: not found or failed", ERR_COL);
    }
}

fn cmd_rename(args: &str) {
    // args: "oldname newname"
    let (old, new) = match args.find(' ') {
        Some(pos) => (&args[..pos], args[pos + 1..].trim_start()),
        None => {
            TERM.lock().push_str("usage: rename <old> <new>", ERR_COL);
            return;
        }
    };
    if new.is_empty() {
        TERM.lock().push_str("usage: rename <old> <new>", ERR_COL);
        return;
    }
    if !crate::fat32::is_mounted() {
        TERM.lock()
            .push_str("rename: no FAT32 disk mounted", ERR_COL);
        return;
    }
    let parent_clus = cwd_cluster().unwrap_or_else(|| crate::fat32::root_cluster());
    let ok = crate::fat32::rename_entry(parent_clus, old.as_bytes(), new.as_bytes());
    let mut t = TERM.lock();
    if ok {
        t.push_str("renamed", TEXT_NORM);
    } else {
        t.push_str("rename: failed", ERR_COL);
    }
}

fn cmd_cp(args: &str) {
    let (src, dst) = match args.find(' ') {
        Some(pos) => (&args[..pos], args[pos + 1..].trim_start()),
        None => {
            TERM.lock().push_str("usage: cp <src> <dst>", ERR_COL);
            return;
        }
    };
    if dst.is_empty() {
        TERM.lock().push_str("usage: cp <src> <dst>", ERR_COL);
        return;
    }
    if !crate::fat32::is_mounted() {
        TERM.lock().push_str("cp: no FAT32 disk mounted", ERR_COL);
        return;
    }
    let dir_c = cwd_cluster().unwrap_or_else(|| crate::fat32::root_cluster());
    let de = match crate::fat32::find_in_dir(dir_c, src.as_bytes()) {
        Some(d) => d,
        None => {
            TERM.lock().push_str("cp: source not found", ERR_COL);
            return;
        }
    };
    if de.is_dir {
        TERM.lock()
            .push_str("cp: directories not supported", ERR_COL);
        return;
    }
    const MAX: usize = 65536;
    let mut buf = [0u8; MAX];
    let n = crate::fat32::read_file(de.cluster, de.size, &mut buf);
    if !crate::fat32::write_file(dir_c, dst.as_bytes(), &buf[..n]) {
        TERM.lock().push_str("cp: write failed", ERR_COL);
        return;
    }
    TERM.lock().push_str("copied", TEXT_NORM);
}

fn cmd_mv(args: &str) {
    let (src, dst) = match args.find(' ') {
        Some(pos) => (&args[..pos], args[pos + 1..].trim_start()),
        None => {
            TERM.lock().push_str("usage: mv <src> <dst>", ERR_COL);
            return;
        }
    };
    if dst.is_empty() {
        TERM.lock().push_str("usage: mv <src> <dst>", ERR_COL);
        return;
    }
    if !crate::fat32::is_mounted() {
        TERM.lock().push_str("mv: no FAT32 disk mounted", ERR_COL);
        return;
    }
    let dir_c = cwd_cluster().unwrap_or_else(|| crate::fat32::root_cluster());
    let ok = crate::fat32::rename_entry(dir_c, src.as_bytes(), dst.as_bytes());
    let mut t = TERM.lock();
    if ok {
        t.push_str("moved", TEXT_NORM);
    } else {
        t.push_str("mv: failed", ERR_COL);
    }
}

