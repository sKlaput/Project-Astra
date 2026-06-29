impl FileManagerApp {
    /// Open a context menu for the entry at `row` (or empty-area menu if row == usize::MAX).
    fn open_ctx_for(&mut self, rel_x: i32, rel_y: i32, row: usize) {
        let mut menu = CtxMenu::hidden();
        menu.visible = true;
        menu.x = rel_x;
        menu.y = rel_y;
        menu.target_row = row;
        menu.item_count = 0;
        let pw = self.preferred_size().0 as i32;
        let ph = self.preferred_size().1 as i32;
        // Clamp before storing so render() and click() agree
        let mw = CTX_MIN_W as i32;
        let mh = (5 * CTX_ITEM_H + 4) as i32;
        let x = rel_x.min(pw - mw).max(0);
        let y = rel_y.min(ph - mh).max(0);
        menu.x = x;
        menu.y = y;

        if row == usize::MAX || row >= self.count {
            // Empty-area menu
            macro_rules! push {
                ($act:expr, $lbl:expr, $en:expr) => {
                    if menu.item_count < 5 {
                        menu.items[menu.item_count] = CtxItem {
                            action: $act,
                            label: $lbl,
                            enabled: $en,
                        };
                        menu.item_count += 1;
                    }
                };
            }
            push!(CtxAction::NewFile, "New file", true);
            push!(CtxAction::NewDir, "New folder", true);
            push!(CtxAction::Paste, "Paste", self.clipboard.is_set());
        } else {
            let e = self.entries[row];
            let is_up = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
            macro_rules! push {
                ($act:expr, $lbl:expr, $en:expr) => {
                    if menu.item_count < 5 {
                        menu.items[menu.item_count] = CtxItem {
                            action: $act,
                            label: $lbl,
                            enabled: $en,
                        };
                        menu.item_count += 1;
                    }
                };
            }
            push!(CtxAction::Open, "Open", true);
            push!(CtxAction::Copy, "Copy", e.is_fat32 && !is_up);
            push!(CtxAction::Cut, "Cut", e.is_fat32 && !is_up);
            push!(
                CtxAction::Rename,
                "Rename",
                (e.is_dyn || e.is_fat32) && !is_up
            );
            push!(
                CtxAction::Delete,
                "Delete",
                (e.is_dyn || e.is_fat32) && !is_up
            );
        }
        self.ctx = menu;
    }

    /// Execute a context-menu action (after the menu is dismissed).
    fn execute_ctx_action(&mut self, action: CtxAction) -> AppAction {
        match action {
            CtxAction::Open => {
                if self.ctx.target_row < self.count {
                    let old = self.selected;
                    self.selected = self.ctx.target_row;
                    let act = self.open_selected();
                    if matches!(act, AppAction::Nothing) {
                        self.selected = old;
                    }
                    return act;
                }
            }
            CtxAction::NewFile => {
                let target = fs::resolve_node_id(self.cwd.as_str()).unwrap_or(0);
                self.prompt = FmPrompt {
                    kind: PromptKind::New,
                    buf: [0u8; 32],
                    len: 0,
                    target,
                };
            }
            CtxAction::NewDir => {
                let target = fs::resolve_node_id(self.cwd.as_str()).unwrap_or(0);
                self.prompt = FmPrompt {
                    kind: PromptKind::Mkdir,
                    buf: [0u8; 32],
                    len: 0,
                    target,
                };
            }
            CtxAction::Rename => {
                if self.ctx.target_row < self.count {
                    let e = self.entries[self.ctx.target_row];
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
                    }
                }
            }
            CtxAction::Delete => {
                if self.ctx.target_row < self.count {
                    let e = self.entries[self.ctx.target_row];
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
                    }
                }
            }
            CtxAction::Copy => {
                if self.ctx.target_row < self.count {
                    let e = self.entries[self.ctx.target_row];
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
                    }
                }
            }
            CtxAction::Cut => {
                if self.ctx.target_row < self.count {
                    let e = self.entries[self.ctx.target_row];
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
                    }
                }
            }
            CtxAction::Paste => {
                self.do_paste();
            }
        }
        AppAction::RedrawAll
    }
}

