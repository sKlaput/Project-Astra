impl FileManagerApp {
    fn key_files(&mut self, key: Key) -> AppAction {
        let (_, ph) = self.preferred_size();
        let visible = Self::visible_rows(ph).max(1);

        // ── Prompt mode: all keys go to the active prompt ───────────────────────
        if self.prompt.kind != PromptKind::None {
            match key {
                Key::Escape => {
                    self.prompt = FmPrompt::DEFAULT;
                    return AppAction::RedrawAll;
                }
                Key::Enter => {
                    let mut open_file_action: Option<AppAction> = None;
                    match self.prompt.kind {
                        PromptKind::New => {
                            if self.prompt.len > 0 {
                                let name =
                                    core::str::from_utf8(&self.prompt.buf[..self.prompt.len])
                                        .unwrap_or("");
                                if crate::fat32::is_mounted() {
                                    // Create directly on disk so it survives reboot
                                    let dir_c = if self.fat32_cluster != 0 {
                                        self.fat32_cluster
                                    } else {
                                        fs::fat32_root_cluster()
                                    };
                                    if let Some(id) =
                                        fs::fat32_create_and_open(dir_c, name.as_bytes())
                                    {
                                        let mut buf = [0u8; 128];
                                        let prefix = b"/fat32/";
                                        buf[..prefix.len()].copy_from_slice(prefix);
                                        let (hex, hlen) = hex_u16(id);
                                        let total = prefix.len() + hlen;
                                        buf[prefix.len()..total].copy_from_slice(&hex[..hlen]);
                                        open_file_action = Some(AppAction::OpenFile(buf, total));
                                        self.op_ok = Some("File created");
                                        self.op_err = None;
                                    } else {
                                        self.op_err = Some(
                                            "Create failed — name may already exist or be invalid",
                                        );
                                    }
                                } else {
                                    if fs::dyn_create_file(self.prompt.target, name).is_err() {
                                        self.op_err = Some("Create failed");
                                    } else {
                                        self.op_ok = Some("File created");
                                        self.op_err = None;
                                    }
                                }
                            } else {
                                self.op_err = Some("Name cannot be empty");
                            }
                        }
                        PromptKind::Mkdir => {
                            if self.prompt.len > 0 {
                                let name =
                                    core::str::from_utf8(&self.prompt.buf[..self.prompt.len])
                                        .unwrap_or("");
                                if crate::fat32::is_mounted() {
                                    let dir_c = if self.fat32_cluster != 0 {
                                        self.fat32_cluster
                                    } else {
                                        fs::fat32_root_cluster()
                                    };
                                    if !crate::fat32::create_dir(dir_c, name.as_bytes()) {
                                        self.op_err =
                                            Some("Create folder failed — name may already exist");
                                    } else {
                                        self.op_ok = Some("Folder created");
                                        self.op_err = None;
                                    }
                                } else {
                                    if fs::dyn_create_dir(self.prompt.target, name).is_err() {
                                        self.op_err = Some("Create folder failed");
                                    } else {
                                        self.op_ok = Some("Folder created");
                                        self.op_err = None;
                                    }
                                }
                            } else {
                                self.op_err = Some("Name cannot be empty");
                            }
                        }
                        PromptKind::Rename => {
                            if self.prompt.len > 0 {
                                let new_name =
                                    core::str::from_utf8(&self.prompt.buf[..self.prompt.len])
                                        .unwrap_or("");
                                if fs::is_fat32_id(self.prompt.target) {
                                    // FAT32 rename: need old name from cache
                                    if let Some(old_name) = fs::fat32_entry_name(self.prompt.target)
                                    {
                                        let dir_c = if self.fat32_cluster != 0 {
                                            self.fat32_cluster
                                        } else {
                                            fs::fat32_root_cluster()
                                        };
                                        if !crate::fat32::rename_entry(
                                            dir_c,
                                            &old_name.0[..old_name.1],
                                            new_name.as_bytes(),
                                        ) {
                                            self.op_err = Some("Rename failed");
                                        } else {
                                            self.op_ok = Some("Renamed");
                                            self.op_err = None;
                                        }
                                    } else {
                                        self.op_err =
                                            Some("Rename failed — entry not found in cache");
                                    }
                                } else {
                                    if fs::dyn_rename_file(self.prompt.target, new_name).is_err() {
                                        self.op_err = Some("Rename failed");
                                    } else {
                                        self.op_ok = Some("Renamed");
                                        self.op_err = None;
                                    }
                                }
                            } else {
                                self.op_err = Some("Name cannot be empty");
                            }
                        }
                        PromptKind::ConfirmDel => {
                            if fs::is_fat32_id(self.prompt.target) {
                                // FAT32 delete: name is in prompt.buf (pre-filled in Delete handler)
                                let name = &self.prompt.buf[..self.prompt.len];
                                let dir_c = if self.fat32_cluster != 0 {
                                    self.fat32_cluster
                                } else {
                                    fs::fat32_root_cluster()
                                };
                                if !crate::fat32::delete_entry(dir_c, name) {
                                    self.op_err = Some("Delete failed");
                                } else {
                                    self.op_ok = Some("Deleted");
                                    self.op_err = None;
                                }
                            } else {
                                match fs::dyn_delete_node(self.prompt.target) {
                                    Err(crate::fs::VfsError::NotEmpty) => {
                                        self.op_err =
                                            Some("Not empty \u{2014} delete contents first");
                                    }
                                    _ => {
                                        self.op_ok = Some("Deleted");
                                        self.op_err = None;
                                    }
                                }
                            }
                        }
                        PromptKind::None => {}
                    }
                    self.prompt = FmPrompt::DEFAULT;
                    self.load_dir();
                    if let Some(act) = open_file_action {
                        return act;
                    }
                    return AppAction::RedrawAll;
                }
                Key::Backspace => {
                    if self.prompt.kind != PromptKind::ConfirmDel && self.prompt.len > 0 {
                        self.prompt.len -= 1;
                        self.prompt.buf[self.prompt.len] = 0;
                        return AppAction::RedrawAll;
                    }
                }
                Key::Char(c) if self.prompt.kind != PromptKind::ConfirmDel => {
                    // Accept printable ASCII except '/' (invalid in filenames)
                    let invalid_char = matches!(
                        c,
                        b'/' | b'\\' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|'
                    );
                    if c >= 0x20 && c < 0x7F && !invalid_char && self.prompt.len < 32 {
                        self.prompt.buf[self.prompt.len] = c;
                        self.prompt.len += 1;
                        return AppAction::RedrawAll;
                    }
                }
                _ => {}
            }
            return AppAction::Nothing;
        }

        // ── Normal navigation mode ────────────────────────────────────────────
        self.op_err = None; // clear stale error on any new keypress
        self.op_ok = None; // clear stale success message on any new keypress
        let old_sel = self.selected;
        let old_scroll = self.scroll;
        match key {
            Key::Escape => {
                // Go back to "This PC" root view
                self.view = FmView::ThisPc;
                self.hover_crumb = None;
                return AppAction::RedrawAll;
            }
            Key::ArrowUp => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            Key::ArrowDown => {
                if self.selected + 1 < self.count {
                    self.selected += 1;
                }
            }
            Key::Char(b'g') | Key::Home => {
                self.selected = 0;
            }
            Key::Char(b'G') | Key::End => {
                self.selected = self.count.saturating_sub(1);
            }
            Key::Tab | Key::PageDown => {
                self.selected = (self.selected + visible).min(self.count.saturating_sub(1));
            }
            Key::PageUp => {
                self.selected = self.selected.saturating_sub(visible);
            }
            Key::Backspace => {
                // Navigate to parent directory
                if self.fat32_stack_depth > 0 || self.fat32_cluster != 0 {
                    // Go up one FAT32 level using navigate_into's ".." logic
                    // synthesise a ".." selection
                    if self.count > 0
                        && self.entries[0].nlen == 2
                        && self.entries[0].name[0] == b'.'
                        && self.entries[0].name[1] == b'.'
                    {
                        self.selected = 0;
                        self.navigate_into();
                    } else {
                        self.fat32_cluster = 0;
                        self.fat32_stack_depth = 0;
                        self.load_dir();
                    }
                    self.hover_crumb = None;
                    return AppAction::RedrawAll;
                } else if self.cwd.len > 1 {
                    self.cwd.pop();
                    self.hover_crumb = None;
                    self.fat32_cluster = 0;
                    self.fat32_stack_depth = 0;
                    self.load_dir();
                    return AppAction::RedrawAll;
                }
            }
            Key::Enter | Key::Char(b' ') => {
                return self.open_selected();
            }

            // ── File management ──────────────────────────────────────────
            Key::Char(b'n') | Key::Char(b'N') => {
                // Start "new file" prompt.
                let target = fs::resolve_node_id(self.cwd.as_str()).unwrap_or(0);
                self.prompt = FmPrompt {
                    kind: PromptKind::New,
                    buf: [0u8; 32],
                    len: 0,
                    target,
                };
                return AppAction::RedrawAll;
            }
            Key::Char(b'm') | Key::Char(b'M') => {
                // Start "new folder" (mkdir) prompt.
                let target = fs::resolve_node_id(self.cwd.as_str()).unwrap_or(0);
                self.prompt = FmPrompt {
                    kind: PromptKind::Mkdir,
                    buf: [0u8; 32],
                    len: 0,
                    target,
                };
                return AppAction::RedrawAll;
            }
            Key::Delete => {
                // Delete: dynamic files or FAT32 entries
                if self.count > 0 {
                    let e = self.entries[self.selected];
                    let is_back = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
                    if !is_back && (e.is_dyn || e.is_fat32) {
                        let mut buf = [0u8; 32];
                        buf[..e.nlen].copy_from_slice(&e.name[..e.nlen]);
                        self.prompt = FmPrompt {
                            kind: PromptKind::ConfirmDel,
                            buf,
                            len: e.nlen,
                            target: e.node_id,
                        };
                        return AppAction::RedrawAll;
                    }
                }
            }
            Key::Char(b'r') | Key::Char(b'R') => {
                // Rename: dynamic files or FAT32 entries
                if self.count > 0 {
                    let e = self.entries[self.selected];
                    let is_back = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
                    if !is_back && (e.is_dyn || e.is_fat32) {
                        let mut buf = [0u8; 32];
                        buf[..e.nlen].copy_from_slice(&e.name[..e.nlen]);
                        self.prompt = FmPrompt {
                            kind: PromptKind::Rename,
                            buf,
                            len: e.nlen,
                            target: e.node_id,
                        };
                        return AppAction::RedrawAll;
                    }
                }
            }

            // ── Clipboard ────────────────────────────────────────────────────────────
            Key::Ctrl(b'c') | Key::Ctrl(b'C') => {
                if self.count > 0 {
                    let e = self.entries[self.selected];
                    let is_up = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
                    if e.is_fat32 && !is_up {
                        let mut name = [0u8; 64];
                        name[..e.nlen].copy_from_slice(&e.name[..e.nlen]);
                        let src_cluster = if self.fat32_cluster != 0 {
                            self.fat32_cluster
                        } else {
                            fs::fat32_root_cluster()
                        };
                        self.clipboard = Clipboard {
                            name,
                            name_len: e.nlen,
                            src_cluster,
                            is_cut: false,
                        };
                        return AppAction::RedrawAll;
                    }
                }
            }
            Key::Ctrl(b'x') | Key::Ctrl(b'X') => {
                if self.count > 0 {
                    let e = self.entries[self.selected];
                    let is_up = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
                    if e.is_fat32 && !is_up {
                        let mut name = [0u8; 64];
                        name[..e.nlen].copy_from_slice(&e.name[..e.nlen]);
                        let src_cluster = if self.fat32_cluster != 0 {
                            self.fat32_cluster
                        } else {
                            fs::fat32_root_cluster()
                        };
                        self.clipboard = Clipboard {
                            name,
                            name_len: e.nlen,
                            src_cluster,
                            is_cut: true,
                        };
                        return AppAction::RedrawAll;
                    }
                }
            }
            Key::Ctrl(b'v') | Key::Ctrl(b'V') => {
                self.do_paste(); // do_paste() calls load_dir() internally on success
                return AppAction::RedrawAll;
            }

            _ => return AppAction::Nothing,
        }
        self.clamp_scroll(visible);
        if self.selected != old_sel || self.scroll != old_scroll {
            AppAction::RedrawAll
        } else {
            AppAction::Nothing
        }
    }
}

