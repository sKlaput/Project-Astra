// ---------------------------------------------------------------------------
// Astra OS — File Manager app  (polished v2)
//
// Features:
//   - Full directory listing with name + size columns
//   - ".." parent entry for non-root dirs
//   - Keyboard navigation (arrows, Enter, Esc, Home, End, g/G)
//   - Mouse click-to-select, double-click to open
//   - Scroll support when entries exceed visible rows
//   - Entry count + scroll position in header
//   - Empty-directory and error states that look intentional
//   - Opens text files by emitting AppAction::OpenFile
// ---------------------------------------------------------------------------

use crate::app::{App, AppAction};
use crate::framebuffer;
use crate::fs;
use crate::input::Key;
use crate::arch::x86_64::interrupts::uptime_ms;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG:          u32 = 0x080C12;
const HEADER_BG:   u32 = 0x0C1420;
const HEADER_COL:  u32 = 0xE8F4FD;
const COLHDR_BG:   u32 = 0x0A1018;
const COLHDR_COL:  u32 = 0x4A6880;
const SEL_BG:      u32 = 0x1C3F62;   // clearly selected
const SEL_BORDER:  u32 = 0x3870B0;   // bright accent bar on left edge
const SEL_COL:     u32 = 0xF0F8FF;
const DIR_COL:     u32 = 0x5CB8FF;
const FILE_COL:    u32 = 0xB8D0E4;
const UP_COL:      u32 = 0xA0C8E8;   // parent-entry text — brighter than normal dir
const SIZE_COL:    u32 = 0x4A6880;
const SIZE_SEL:    u32 = 0x8AB0CC;
const BORDER_COL:  u32 = 0x182840;
const PATH_BG:     u32 = 0x0A1828;   // slightly distinct background for path row
const PATH_COL:    u32 = 0x88BEDD;   // bright enough to read the path clearly
const PATH_LBL:    u32 = 0x3A5870;   // dimmer "Location:" label
const COUNT_COL:   u32 = 0x5A7C98;   // item count, readable
const HINT_COL:    u32 = 0x38566A;
const HINT_KEY:    u32 = 0x607890;
const EMPTY_COL:   u32 = 0x2A4058;
const ERR_COL:     u32 = 0xC04040;
const EVEN_BG:     u32 = 0x0A0F16;
const HOVER_BG:    u32 = 0x10202E;  // mouse-hover row (visibly brighter than even rows)
const SCROLL_BG:   u32 = 0x0C141C;
const SCROLL_FG:   u32 = 0x2A4060;

// Breadcrumb colours
const CRUMB_COL:   u32 = 0x70B0D0;   // clickable segment
const CRUMB_HOV:   u32 = 0xC0E4FC;   // hovered clickable segment
const CRUMB_CUR:   u32 = 0x486880;   // current (last) segment — dimmer
const CRUMB_SEP:   u32 = 0x2A4860;   // " > " separator

const MAX_CRUMBS:  usize = 8;

// Context menu
const CTX_BG:      u32 = 0x111E2E;
const CTX_BORDER:  u32 = 0x2A4870;
const CTX_SEL_BG:  u32 = 0x1E3F62;
const CTX_COL:     u32 = 0xC8E0F4;
const CTX_DIS:     u32 = 0x3A5870;   // disabled item
const CTX_ITEM_H:  usize = 16;
const CTX_PAD_X:   usize = 10;
const CTX_MIN_W:   usize = 130;

// ── Layout ────────────────────────────────────────────────────────────────────

const PAD_X:      usize = 12;
const ROW_H:      usize = 18;
const HEADER_H:   usize = 26;
const COL_HDR_H:  usize = 16;
const HINT_H:     usize = 20;
const CHAR_W:     usize = 6;
const SCROLL_W:   usize = 6;
const SIZE_COL_W: usize = 64;
const PREFIX_W:   usize = 4 * CHAR_W;

const MAX_ENTRIES: usize = 32;
const DBL_CLICK_MS: u64 = 450;

// ── Path buffer ───────────────────────────────────────────────────────────────

#[derive(Clone)]
struct PathBuf {
    data: [u8; 128],
    len:  usize,
}

impl PathBuf {
    fn root() -> Self {
        let mut b = PathBuf { data: [0u8; 128], len: 1 };
        b.data[0] = b'/';
        b
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.data[..self.len]).unwrap_or("/")
    }

    fn push(&mut self, name: &str) {
        if self.len > 0 && self.data[self.len - 1] != b'/' {
            if self.len < self.data.len() { self.data[self.len] = b'/'; self.len += 1; }
        }
        for b in name.bytes() {
            if self.len < self.data.len() { self.data[self.len] = b; self.len += 1; }
        }
    }

    fn pop(&mut self) {
        if self.len <= 1 { return; }
        if self.data[self.len - 1] == b'/' { self.len -= 1; }
        while self.len > 1 && self.data[self.len - 1] != b'/' { self.len -= 1; }
    }
}

// ── Entry ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Entry {
    name:    [u8; 32],
    nlen:    usize,
    is_dir:  bool,
    is_dyn:  bool,   // created via dynamic layer — can be deleted/renamed
    is_fat32: bool,  // backed by FAT32 disk
    node_id: u16,    // VFS NodeId (used for dyn ops)
    size:    usize,
}

impl Entry {
    const EMPTY: Self = Entry {
        name: [0u8; 32], nlen: 0, is_dir: false,
        is_dyn: false, is_fat32: false, node_id: 0, size: 0,
    };

    fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.nlen]).unwrap_or("?")
    }
}

// ── File-operation prompt state ─────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptKind { None, New, Mkdir, Rename, ConfirmDel }

#[derive(Clone, Copy)]
struct FmPrompt {
    kind:   PromptKind,
    buf:    [u8; 32],   // input buffer for name / display name for confirm
    len:    usize,
    target: u16,        // parent NodeId (New) or file NodeId (Rename/Delete)
}

impl FmPrompt {
    const DEFAULT: Self = FmPrompt {
        kind: PromptKind::None, buf: [0u8; 32], len: 0, target: 0,
    };
}

// ── Context menu ──────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum CtxAction {
    Open,
    NewFile,
    NewDir,
    Rename,
    Delete,
    Copy,
    Cut,
    Paste,
}

/// In-process clipboard for copy/move operations.
#[derive(Clone, Copy)]
struct Clipboard {
    /// Display name of the source entry (up to 64 bytes).
    name:     [u8; 64],
    name_len: usize,
    /// FAT32 directory cluster that contains the source.
    src_cluster: u32,
    /// True = move (cut), false = copy.
    is_cut:   bool,
}

impl Clipboard {
    const EMPTY: Self = Clipboard {
        name: [0u8; 64], name_len: 0, src_cluster: 0, is_cut: false,
    };
    fn is_set(&self) -> bool { self.name_len > 0 }
}

#[derive(Clone, Copy)]
struct CtxItem {
    action:   CtxAction,
    label:    &'static str,
    enabled:  bool,
}

#[derive(Clone, Copy)]
struct CtxMenu {
    visible:   bool,
    x:         i32,   // position relative to window client area
    y:         i32,
    hover:     Option<usize>,
    items:     [CtxItem; 5],
    item_count: usize,
    // which row was right-clicked (usize::MAX = empty area)
    target_row: usize,
}

impl CtxMenu {
    const NULL_ITEM: CtxItem = CtxItem { action: CtxAction::Open, label: "", enabled: false };
    const fn hidden() -> Self {
        CtxMenu {
            visible: false, x: 0, y: 0, hover: None,
            items: [Self::NULL_ITEM; 5], item_count: 0,
            target_row: usize::MAX,
        }
    }
    fn width(&self) -> usize {
        let mut max_chars = 0usize;
        for i in 0..self.item_count {
            let l = self.items[i].label.len();
            if l > max_chars { max_chars = l; }
        }
        (max_chars * CHAR_W + CTX_PAD_X * 2).max(CTX_MIN_W)
    }
    fn height(&self) -> usize {
        self.item_count * CTX_ITEM_H + 4
    }
}

// ── View mode ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum FmView {
    /// "This PC" root — shows drive tiles
    ThisPc,
    /// Normal file/directory browser
    Files,
}

// ── Drive tile layout for This PC view ────────────────────────────────────────

const TILE_W:    usize = 160;
const TILE_H:    usize = 80;
const TILE_PAD:  usize = 20;   // horizontal gap between tiles
const TILE_TOP:  usize = HEADER_H + 24; // top margin inside client area
const TILE_BG:   u32   = 0x0C1828;
const TILE_HOV:  u32   = 0x162840;
const TILE_SEL:  u32   = 0x1C3F62;
const TILE_BORD: u32   = 0x1A3050;
const TILE_NAME: u32   = 0xD8EEFF;
const TILE_SUB:  u32   = 0x4A7090;
const TILE_BAR:  u32   = 0x1E5090;
const TILE_USED: u32   = 0x2E78CC;
const THIS_PC_BG: u32  = 0x060A10;

// ── FileManagerApp ────────────────────────────────────────────────────────────

pub struct FileManagerApp {
    cwd:            PathBuf,
    fat32_cluster:  u32,   // 0 = not in a FAT32 directory
    /// Stack of parent FAT32 clusters, one entry per level pushed.
    /// fat32_cluster_stack[0] = cluster when we first entered FAT32 root.
    fat32_cluster_stack: [u32; 8],
    fat32_stack_depth:   usize,
    /// Display names for the FAT32 breadcrumb segments (parallel to stack).
    fat32_crumb_names:   [[u8; 32]; 8],
    fat32_crumb_nlens:   [usize; 8],
    entries:        [Entry; MAX_ENTRIES],
    count:          usize,
    selected:       usize,
    scroll:         usize,
    hover_row:      Option<usize>,
    load_err:       bool,
    last_click_ms:  u64,
    last_click_row: usize,
    prompt:         FmPrompt,
    op_err:         Option<&'static str>,
    op_ok:          Option<&'static str>,  // success feedback, cleared on next keypress
    ctx:            CtxMenu,
    hover_crumb:    Option<usize>,
    clipboard:      Clipboard,
    view:           FmView,
    tile_hover:     Option<usize>,  // hovered drive tile in ThisPc view
    tile_sel:       Option<usize>,  // selected drive tile
    tile_last_click_ms: u64,
}

impl FileManagerApp {
    /// Open File Manager with the "New File" prompt already showing.
    pub fn new_file() -> Self {
        let mut app = Self::new();
        // Enter Files view first so the prompt is shown in context
        app.view = FmView::Files;
        let target = crate::fs::resolve_node_id(app.cwd.as_str()).unwrap_or(0);
        app.prompt = FmPrompt { kind: PromptKind::New, buf: [0u8; 32], len: 0, target };
        app
    }

    /// Open File Manager with the Files view rooted inside a specific FAT32 folder.
    /// `cluster` is the FAT32 cluster of the folder, `name` is its display name.
    pub fn open_dir(cluster: u32, name: &[u8]) -> Self {
        // Build directly without calling new() to avoid the redundant load_dir()
        // that new() runs at the VFS root before we override the cluster.
        let mut app = FileManagerApp {
            cwd:            PathBuf::root(),
            fat32_cluster:  cluster,
            fat32_cluster_stack: [0u32; 8],
            fat32_stack_depth:   0,
            fat32_crumb_names:   [[0u8; 32]; 8],
            fat32_crumb_nlens:   [0usize; 8],
            entries:        [Entry::EMPTY; MAX_ENTRIES],
            count:          0,
            selected:       0,
            scroll:         0,
            hover_row:      None,
            load_err:       false,
            last_click_ms:  0,
            last_click_row: usize::MAX,
            prompt:         FmPrompt::DEFAULT,
            op_err:         None,
            op_ok:          None,
            ctx:            CtxMenu::hidden(),
            hover_crumb:    None,
            clipboard:      Clipboard::EMPTY,
            view:           FmView::Files,
            tile_hover:     None,
            tile_sel:       None,
            tile_last_click_ms: 0,
        };
        if cluster != 0 {
            let nlen = name.len().min(32);
            app.fat32_crumb_names[0][..nlen].copy_from_slice(&name[..nlen]);
            app.fat32_crumb_nlens[0] = nlen;
            // Stack[0] stores the cluster to RETURN TO when navigating "..".
            // We were opened directly into `cluster`, so ".." should go back
            // to the VFS/FAT32 root (cluster = 0).
            app.fat32_cluster_stack[0] = 0;
            app.fat32_stack_depth = 1;
        }
        app.load_dir();
        app
    }

    /// Open File Manager with the "New Folder" prompt already showing.
    pub fn new_folder() -> Self {
        let mut app = Self::new();
        app.view = FmView::Files;
        let target = crate::fs::resolve_node_id(app.cwd.as_str()).unwrap_or(0);
        app.prompt = FmPrompt { kind: PromptKind::Mkdir, buf: [0u8; 32], len: 0, target };
        app
    }

    pub fn new() -> Self {
        let mut app = FileManagerApp {
            cwd:            PathBuf::root(),
            fat32_cluster:  0,
            fat32_cluster_stack: [0u32; 8],
            fat32_stack_depth:   0,
            fat32_crumb_names:   [[0u8; 32]; 8],
            fat32_crumb_nlens:   [0usize; 8],
            entries:        [Entry::EMPTY; MAX_ENTRIES],
            count:          0,
            selected:       0,
            scroll:         0,
            hover_row:      None,
            load_err:       false,
            last_click_ms:  0,
            last_click_row: usize::MAX,
            prompt:         FmPrompt::DEFAULT,
            op_err:         None,
            op_ok:          None,
            ctx:            CtxMenu::hidden(),
            hover_crumb:    None,
            clipboard:      Clipboard::EMPTY,
            view:           FmView::ThisPc,
            tile_hover:     None,
            tile_sel:       None,
            tile_last_click_ms: 0,
        };
        app.load_dir();
        app
    }

    /// Execute a clipboard paste into the current directory.
    /// For copy: reads the source file and writes it to the destination cluster.
    /// For cut (move): same, then deletes the source entry.
    fn do_paste(&mut self) {
        if !self.clipboard.is_set() { return; }
        if !crate::fat32::is_mounted() {
            self.op_err = Some("Paste: no FAT32 disk");
            return;
        }
        let dst_cluster = if self.fat32_cluster != 0 { self.fat32_cluster }
                          else { fs::fat32_root_cluster() };
        let src_cluster = self.clipboard.src_cluster;
        let name = &self.clipboard.name[..self.clipboard.name_len];

        // Find the source entry
        let de = match crate::fat32::find_in_dir(src_cluster, name) {
            Some(d) => d,
            None => { self.op_err = Some("Paste: source not found"); return; }
        };

        if de.is_dir {
            self.op_err = Some("Paste: directories not supported");
            return;
        }

        // Read file content (up to 64 KB)
        const MAX: usize = 65536;
        let mut buf = [0u8; MAX];
        let n = crate::fat32::read_file(de.cluster, de.size, &mut buf);

        // Write to destination
        if !crate::fat32::write_file(dst_cluster, name, &buf[..n]) {
            self.op_err = Some("Paste: write failed");
            return;
        }

        // If cut: delete the source and clear clipboard
        if self.clipboard.is_cut {
            crate::fat32::delete_entry(src_cluster, name);
            self.clipboard = Clipboard::EMPTY;
        }

        self.op_ok = Some("Pasted");
        self.op_err = None;
        self.load_dir();
    }

    fn load_dir(&mut self) {
        self.count    = 0;
        self.selected = 0;
        self.scroll   = 0;
        self.hover_row = None;
        self.load_err = false;

        // If we're inside a FAT32 subdirectory (not the VFS root or a VFS path),
        // skip VFS resolution entirely — just list FAT32 contents.
        if self.fat32_cluster != 0 {
            // Add ".." entry to navigate back
            let mut back = Entry::EMPTY;
            back.name[..2].copy_from_slice(b"..");
            back.nlen   = 2;
            back.is_dir = true;
            self.entries[self.count] = back;
            self.count += 1;

            let fat_cluster = self.fat32_cluster;
            let mut fat_out = [fs::DynEntry {
                id: 0, parent: 0, name: [0u8; 32], nlen: 0, is_dir: false, size: 0,
            }; 32];
            // Only call into FAT32 for valid cluster numbers (>= 2).
            // fat32_cluster == 1 is the sentinel for "empty dir / no cluster".
            let fat_count = if fat_cluster >= 2 {
                fs::fat32_list_dir(fat_cluster, &mut fat_out, 0)
            } else {
                0
            };
            for i in 0..fat_count {
                if self.count >= MAX_ENTRIES { break; }
                let d = &fat_out[i];
                let mut e = Entry::EMPTY;
                let nlen = d.nlen.min(32);
                e.name[..nlen].copy_from_slice(&d.name[..nlen]);
                e.nlen     = nlen;
                e.is_dir   = d.is_dir;
                e.is_dyn   = false;
                e.is_fat32 = true;
                e.node_id  = d.id;
                e.size     = d.size;
                self.entries[self.count] = e;
                self.count += 1;
            }
            return;
        }

        let dir_id = match fs::resolve_node_id(self.cwd.as_str()) {
            Some(id) => id,
            None => { self.load_err = true; return; }
        };

        // Static VFS nodes (skip hidden system entries like /etc)
        const HIDDEN: &[&str] = &["etc"];
        for node in fs::iter_nodes() {
            if self.count >= MAX_ENTRIES { break; }
            if node.parent != Some(dir_id) { continue; }
            if HIDDEN.iter().any(|h| *h == node.name) { continue; }

            let nb = node.name.as_bytes();
            let nlen = nb.len().min(32);
            let mut e = Entry::EMPTY;
            e.name[..nlen].copy_from_slice(&nb[..nlen]);
            e.nlen    = nlen;
            e.is_dir  = node.kind == fs::NodeKind::Directory;
            e.is_dyn  = false;
            e.node_id = node.id;
            e.size    = if e.is_dir {
                fs::iter_nodes().iter().filter(|n| n.parent == Some(node.id)).count()
            } else {
                node.data.len()
            };
            self.entries[self.count] = e;
            self.count += 1;
        }

        // Dynamic files and folders in this directory
        let mut dyn_out = [fs::DynEntry {
            id: 0, parent: 0, name: [0u8; 32], nlen: 0, is_dir: false, size: 0,
        }; 16];
        let dyn_count = fs::dyn_list_dir(dir_id, &mut dyn_out);
        for i in 0..dyn_count {
            if self.count >= MAX_ENTRIES { break; }
            let d = &dyn_out[i];
            let mut e = Entry::EMPTY;
            let nlen = d.nlen.min(32);
            e.name[..nlen].copy_from_slice(&d.name[..nlen]);
            e.nlen    = nlen;
            e.is_dir  = d.is_dir;
            e.is_dyn  = true;
            e.node_id = d.id;
            e.size    = d.size;
            self.entries[self.count] = e;
            self.count += 1;
        }

        // FAT32 disk entries (if a FAT32 volume is mounted at VFS root level)
        // Note: fat32_cluster == 0 here (the early-return above handles the ≠0 case).
        let fat_cluster = fs::fat32_root_cluster();
        if fat_cluster != 0 {
            let mut fat_out = [fs::DynEntry {
                id: 0, parent: 0, name: [0u8; 32], nlen: 0, is_dir: false, size: 0,
            }; 32];
            let fat_count = fs::fat32_list_dir(fat_cluster, &mut fat_out, 0);
            for i in 0..fat_count {
                if self.count >= MAX_ENTRIES { break; }
                let d = &fat_out[i];
                let mut e = Entry::EMPTY;
                let nlen = d.nlen.min(32);
                e.name[..nlen].copy_from_slice(&d.name[..nlen]);
                e.nlen     = nlen;
                e.is_dir   = d.is_dir;
                e.is_dyn   = false;
                e.is_fat32 = true;
                e.node_id  = d.id;
                e.size     = d.size;
                self.entries[self.count] = e;
                self.count += 1;
            }
        }
    }

    fn navigate_into(&mut self) {
        if self.count == 0 { return; }
        let e = self.entries[self.selected];
        if !e.is_dir { return; }

        // ".." entry — navigate up
        if e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.' {
            if self.fat32_stack_depth > 0 {
                // Pop back one FAT32 level
                self.fat32_stack_depth -= 1;
                self.fat32_cluster = self.fat32_cluster_stack[self.fat32_stack_depth];
                self.fat32_cluster_stack[self.fat32_stack_depth] = 0;
                self.fat32_crumb_nlens[self.fat32_stack_depth] = 0;
            } else {
                // Back to VFS root from first FAT32 level
                self.fat32_cluster = 0;
                if self.cwd.len > 1 { self.cwd.pop(); }
            }
            self.load_dir();
            return;
        }

        if e.is_fat32 && e.is_dir {
            // Push current cluster onto the stack
            if self.fat32_stack_depth < 8 {
                self.fat32_cluster_stack[self.fat32_stack_depth] = self.fat32_cluster;
                // Store the display name for the breadcrumb
                let nlen = e.nlen.min(32);
                self.fat32_crumb_names[self.fat32_stack_depth][..nlen]
                    .copy_from_slice(&e.name[..nlen]);
                self.fat32_crumb_nlens[self.fat32_stack_depth] = nlen;
                self.fat32_stack_depth += 1;
            }
            // Resolve the directory's own cluster. The cache is the fast path;
            // if it returns 0 (stale or cluster-0 on disk), re-read from disk.
            let cached_cluster = fs::fat32_dir_cluster(e.node_id);
            let cluster = if cached_cluster >= 2 {
                cached_cluster
            } else {
                // Fall back to a direct disk lookup using the parent cluster.
                // After the push above, stack[depth-1] holds the parent's cluster
                // (0 means the parent was the FAT32 root).
                let parent = {
                    let stacked = self.fat32_cluster_stack
                        [self.fat32_stack_depth.saturating_sub(1)];
                    if stacked != 0 { stacked } else { fs::fat32_root_cluster() }
                };
                crate::fat32::find_in_dir(parent, &e.name[..e.nlen])
                    .map(|de| de.cluster)
                    .unwrap_or(0)
            };
            // Cluster 0 means the directory entry has no allocated cluster yet
            // (truly empty in an unusual FAT32 state). Use sentinel 1 so that
            // load_dir takes the "inside a FAT32 dir" branch but list_dir
            // safely returns 0 entries (cluster 1 is always invalid/reserved).
            self.fat32_cluster = if cluster >= 2 { cluster } else { 1 };
        } else {
            self.cwd.push(e.name_str());
            self.fat32_cluster = 0;
            self.fat32_stack_depth = 0;
        }
        self.load_dir();
    }

    fn open_selected(&mut self) -> AppAction {
        if self.count == 0 { return AppAction::Nothing; }
        let e = self.entries[self.selected];
        if e.is_dir {
            self.navigate_into();
            return AppAction::RedrawAll;
        }
        // FAT32 files: encode the FAT32 NodeId in the path as a virtual path
        // The editor uses OpenFile(path_bytes, len); for FAT32 we pass a
        // special path "/fat32/<node_id_hex>" that the editor resolves.
        if e.is_fat32 {
            let mut buf = [0u8; 128];
            let prefix = b"/fat32/";
            buf[..prefix.len()].copy_from_slice(prefix);
            let id_hex = hex_u16(e.node_id);
            let total = prefix.len() + id_hex.1;
            buf[prefix.len()..total].copy_from_slice(&id_hex.0[..id_hex.1]);
            return AppAction::OpenFile(buf, total);
        }
        let mut path = self.cwd.clone();
        path.push(e.name_str());
        let bytes = path.as_str().as_bytes();
        let len = bytes.len().min(128);
        let mut buf = [0u8; 128];
        buf[..len].copy_from_slice(&bytes[..len]);
        AppAction::OpenFile(buf, len)
    }

    fn visible_rows(ch: usize) -> usize {
        ch.saturating_sub(HEADER_H + COL_HDR_H + HINT_H) / ROW_H
    }

    fn union_damage(
        a: Option<(usize, usize, usize, usize)>,
        b: Option<(usize, usize, usize, usize)>,
    ) -> Option<(usize, usize, usize, usize)> {
        match (a, b) {
            (Some((ax, ay, aw, ah)), Some((bx, by, bw, bh))) => {
                let x0 = ax.min(bx);
                let y0 = ay.min(by);
                let x1 = (ax + aw).max(bx + bw);
                let y1 = (ay + ah).max(by + bh);
                Some((x0, y0, x1 - x0, y1 - y0))
            }
            (Some(area), None) | (None, Some(area)) => Some(area),
            (None, None) => None,
        }
    }

    fn hover_row_damage(&self, row: Option<usize>) -> Option<(usize, usize, usize, usize)> {
        let row = row?;
        if row < self.scroll {
            return None;
        }
        let row_in_view = row - self.scroll;
        let y = HEADER_H + COL_HDR_H + row_in_view * ROW_H;
        Some((0, y, usize::MAX, ROW_H))
    }

    fn hover_crumb_damage(&self, crumb: Option<usize>) -> Option<(usize, usize, usize, usize)> {
        if crumb.is_some() {
            Some((0, 0, usize::MAX, HEADER_H))
        } else {
            None
        }
    }

    fn clamp_scroll(&mut self, visible: usize) {
        if self.count == 0 { self.scroll = 0; return; }
        if self.selected < self.scroll { self.scroll = self.selected; }
        if self.selected >= self.scroll + visible {
            self.scroll = self.selected.saturating_sub(visible - 1);
        }
        let max_scroll = self.count.saturating_sub(visible);
        if self.scroll > max_scroll { self.scroll = max_scroll; }
    }

    fn fmt_size(buf: &mut [u8; 16], size: usize) -> &str {
        if size == 0 {
            buf[0] = b'-';
            return core::str::from_utf8(&buf[..1]).unwrap_or("-");
        }
        if size < 1024 {
            let n = fmt_uint(buf, 0, size);
            let s = b" B";
            let e = (n + s.len()).min(buf.len());
            buf[n..e].copy_from_slice(&s[..e - n]);
            core::str::from_utf8(&buf[..e]).unwrap_or("?")
        } else {
            let n = fmt_uint(buf, 0, size / 1024);
            let s = b" KB";
            let e = (n + s.len()).min(buf.len());
            buf[n..e].copy_from_slice(&s[..e - n]);
            core::str::from_utf8(&buf[..e]).unwrap_or("?")
        }
    }

    fn tile_rect(i: usize, cw: usize) -> (usize, usize, usize, usize) {
        // Centre one tile per "This PC" (we only have one drive for now)
        let _ = i;
        let tx = (cw.saturating_sub(TILE_W)) / 2;
        (tx, TILE_TOP, TILE_W, TILE_H)
    }
}

impl App for FileManagerApp {
    fn title(&self) -> &str {
        if self.view == FmView::ThisPc { "This PC" } else { "Files" }
    }
    fn preferred_size(&self) -> (usize, usize) { (560, 440) }
    fn app_id(&self) -> &'static str { "filemanager" }
    fn allow_multiple_instances(&self) -> bool { true }
    fn refresh_interval_ms(&self) -> Option<u64> { None }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        if self.view == FmView::ThisPc {
            self.render_this_pc(cx, cy, cw, ch);
            return;
        }
        self.render_files(cx, cy, cw, ch);
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        if self.view == FmView::ThisPc {
            return self.key_this_pc(key);
        }
        self.key_files(key)
    }

    fn handle_mouse_click(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        if self.view == FmView::ThisPc {
            return self.mouse_click_this_pc(rel_x, rel_y);
        }
        self.mouse_click_files(rel_x, rel_y)
    }

    fn handle_mouse_move(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        if self.view == FmView::ThisPc {
            return self.mouse_move_this_pc(rel_x, rel_y);
        }
        self.mouse_move_files(rel_x, rel_y)
    }

    fn handle_mouse_right_click(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        if self.view == FmView::ThisPc {
            return AppAction::Nothing; // no right-click menu on This PC for now
        }
        self.right_click_files(rel_x, rel_y)
    }
}

// ── This PC view ──────────────────────────────────────────────────────────────

impl FileManagerApp {
    fn render_this_pc(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        framebuffer::fill_rect(cx, cy, cw, ch, THIS_PC_BG);

        // Header
        framebuffer::fill_rect(cx, cy, cw, HEADER_H, PATH_BG);
        framebuffer::fill_rect(cx, cy + HEADER_H - 1, cw, 1, BORDER_COL);
        let hdr_ty = cy + (HEADER_H - 8) / 2;
        framebuffer::draw_text_at(cx + PAD_X, hdr_ty, "This PC", TILE_NAME);

        // Section label
        let sec_y = cy + HEADER_H + 8;
        framebuffer::draw_text_at(cx + PAD_X, sec_y, "Devices and drives", TILE_SUB);

        // Drive tile(s)
        let mounted = crate::fat32::is_mounted();
        let (used_kb, total_kb) = if mounted { crate::fat32::disk_space_kb() } else { (0, 0) };
        let (tx, ty_t, tw, th) = Self::tile_rect(0, cw);
        let tx = cx + tx;
        let ty_t = cy + ty_t;
        let bg = if self.tile_sel == Some(0) { TILE_SEL }
                 else if self.tile_hover == Some(0) { TILE_HOV }
                 else { TILE_BG };
        // Outer border
        framebuffer::fill_rect(tx, ty_t, tw, th, TILE_BORD);
        // Inner background
        framebuffer::fill_rect(tx + 1, ty_t + 1, tw - 2, th - 2, bg);

        // Drive icon area (left strip)
        let icon_x = tx + 10;
        let icon_y = ty_t + th / 2 - 10;
        framebuffer::fill_rect(icon_x, icon_y, 20, 20, 0x1A3A60);
        framebuffer::fill_rect(icon_x + 2, icon_y + 2, 16, 16, 0x2A5090);
        framebuffer::draw_text_at(icon_x + 4, icon_y + 6, "C:", 0x88BBEE);

        // Drive name and type
        let name_x = tx + 38;
        let (drive_name, drive_label) = if mounted {
            ("Local Disk (C:)", "FAT32")
        } else {
            ("Local Disk (C:)", "Not mounted")
        };
        framebuffer::draw_text_at(name_x, ty_t + 10, drive_name, TILE_NAME);
        framebuffer::draw_text_at(name_x, ty_t + 22, drive_label, TILE_SUB);

        // Space bar
        if mounted && total_kb > 0 {
            let bar_x = name_x;
            let bar_y = ty_t + 38;
            let bar_w = tw.saturating_sub(name_x - tx + 10);
            let bar_h = 8usize;
            // Background
            framebuffer::fill_rect(bar_x, bar_y, bar_w, bar_h, TILE_BAR);
            // Used fill
            let fill_w = ((used_kb * bar_w as u64) / total_kb.max(1)) as usize;
            if fill_w > 0 {
                framebuffer::fill_rect(bar_x, bar_y, fill_w.min(bar_w), bar_h, TILE_USED);
            }
            // Space text
            let mut ubuf = [0u8; 24];
            let used_mb = used_kb / 1024;
            let total_mb = total_kb / 1024;
            let ulen = fmt_uint_u64(&mut ubuf, 0, used_mb);
            let suf = b" MB used of ";
            let end = (ulen + suf.len()).min(ubuf.len());
            ubuf[ulen..end].copy_from_slice(&suf[..end - ulen]);
            let tlen = end;
            let tstart = tlen;
            let tlen2 = fmt_uint_u64(&mut ubuf, tstart, total_mb);
            let suf2 = b" MB";
            let end2 = (tlen2 + suf2.len()).min(ubuf.len());
            ubuf[tlen2..end2].copy_from_slice(&suf2[..end2 - tlen2]);
            let space_str = core::str::from_utf8(&ubuf[..end2]).unwrap_or("");
            framebuffer::draw_text_at(bar_x, bar_y + 11, space_str, TILE_SUB);
        } else if !mounted {
            framebuffer::draw_text_at(name_x, ty_t + 44, "No disk mounted", TILE_SUB);
        }

        // Hint bar
        let hint_y = cy + ch.saturating_sub(HINT_H);
        framebuffer::fill_rect(cx, hint_y, cw, HINT_H, HEADER_BG);
        framebuffer::fill_rect(cx, hint_y, cw, 1, BORDER_COL);
        let hty = hint_y + (HINT_H - 8) / 2;
        framebuffer::draw_text_at(cx + PAD_X, hty, "Enter=open drive   Esc=close", HINT_KEY);
    }

    fn key_this_pc(&mut self, key: Key) -> AppAction {
        match key {
            Key::Escape => AppAction::Nothing,
            Key::Enter | Key::Char(b' ') => {
                if crate::fat32::is_mounted() {
                    self.view = FmView::Files;
                    self.load_dir();
                    return AppAction::RedrawAll;
                }
                AppAction::Nothing
            }
            _ => AppAction::Nothing,
        }
    }

    fn mouse_click_this_pc(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        let (_, ph) = self.preferred_size();
        let (tx, ty_t, tw, th) = Self::tile_rect(0, self.preferred_size().0);
        if rel_x >= tx as i32 && rel_x < (tx + tw) as i32
            && rel_y >= ty_t as i32 && rel_y < (ty_t + th) as i32
        {
            let now = uptime_ms();
            let is_dbl = now.saturating_sub(self.tile_last_click_ms) < DBL_CLICK_MS;
            self.tile_last_click_ms = now;
            self.tile_sel = Some(0);
            if is_dbl && crate::fat32::is_mounted() {
                self.view = FmView::Files;
                self.load_dir();
            }
            return AppAction::RedrawAll;
        }
        // Click outside tile — deselect
        if self.tile_sel.is_some() {
            self.tile_sel = None;
            return AppAction::RedrawAll;
        }
        AppAction::Nothing
    }

    fn mouse_move_this_pc(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        let (tx, ty_t, tw, th) = Self::tile_rect(0, self.preferred_size().0);
        let new_hover = if rel_x >= tx as i32 && rel_x < (tx + tw) as i32
            && rel_y >= ty_t as i32 && rel_y < (ty_t + th) as i32
        { Some(0) } else { None };
        if new_hover != self.tile_hover {
            self.tile_hover = new_hover;
            return AppAction::RedrawAll;
        }
        AppAction::Nothing
    }
}

// ── Files view (existing logic, renamed from impl App) ────────────────────────

impl FileManagerApp {
    fn render_files(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        framebuffer::fill_rect(cx, cy, cw, ch, BG);

        // ── Path header (breadcrumb navigation) ──────────────────────────
        framebuffer::fill_rect(cx, cy, cw, HEADER_H, PATH_BG);
        framebuffer::fill_rect(cx, cy + HEADER_H - 1, cw, 1, BORDER_COL);
        let hdr_ty = cy + (HEADER_H - 8) / 2;
        let lbl = "Location: ";
        let lbl_w = lbl.len() * CHAR_W;
        framebuffer::draw_text_at(cx + PAD_X, hdr_ty, lbl, PATH_LBL);
        // Draw item count on the right first so we know the right boundary
        let crumb_clip = if !self.load_err {
            let mut cbuf = [0u8; 20];
            let cstr = fmt_count(&mut cbuf, self.count);
            let rx = cx + cw.saturating_sub(PAD_X + cstr.len() * CHAR_W);
            framebuffer::draw_text_at(rx, hdr_ty, cstr, COUNT_COL);
            rx.saturating_sub(PAD_X / 2)
        } else {
            cx + cw
        };
        // "This PC" as the first clickable breadcrumb
        let thispc_label = "This PC";
        let thispc_w = thispc_label.len() * CHAR_W;
        let mut draw_x = cx + PAD_X + lbl_w;
        // hover_crumb == Some(usize::MAX) signals hovering over "This PC" crumb
        let thispc_hover = self.hover_crumb == Some(usize::MAX);
        if thispc_hover {
            framebuffer::fill_rect(
                draw_x.saturating_sub(2), hdr_ty.saturating_sub(2),
                thispc_w + 4, 12, 0x142A40);
        }
        framebuffer::draw_text_at(draw_x, hdr_ty, thispc_label,
            if thispc_hover { CRUMB_HOV } else { CRUMB_COL });
        draw_x += thispc_w;
        // Render breadcrumb segments (VFS path + any FAT32 subdir levels)
        let path = &self.cwd.data[..self.cwd.len];
        let mut segs = [(0usize, 0usize); MAX_CRUMBS];
        let vfs_seg_count = parse_crumbs(path, &mut segs);
        // When we're directly inside a FAT32 directory from the VFS root ("/"),
        // suppress the lone "/" VFS segment so the crumb reads
        // "This PC > FolderName" instead of "This PC > / > FolderName".
        let skip_bare_root = self.fat32_stack_depth > 0 && vfs_seg_count == 1;
        // Total segment count = VFS segs (possibly suppressed) + FAT32 stack depth
        let total_segs = (if skip_bare_root { 0 } else { vfs_seg_count }) + self.fat32_stack_depth;
        let sep = " > ";
        let sep_w = sep.len() * CHAR_W;
        for i in 0..total_segs {
            // Always draw a separator before each VFS/FAT32 segment
            if draw_x + sep_w > crumb_clip { break; }
            framebuffer::draw_text_at(draw_x, hdr_ty, sep, CRUMB_SEP);
            draw_x += sep_w;
            let is_last = i == total_segs - 1;
            // FAT32 stack segment?
            let seg_str_buf: [u8; 32];
            let seg_str: &str = if !skip_bare_root && i < vfs_seg_count {
                let (bs, bl) = segs[i];
                core::str::from_utf8(&path[bs..bs + bl]).unwrap_or("?")
            } else {
                // FAT32 stack index: when skip_bare_root, i maps directly to fi;
                // otherwise offset by vfs_seg_count.
                let fi = if skip_bare_root { i } else { i - vfs_seg_count };
                seg_str_buf = self.fat32_crumb_names[fi];
                let flen = self.fat32_crumb_nlens[fi];
                core::str::from_utf8(&seg_str_buf[..flen]).unwrap_or("?")
            };
            let seg_px = seg_str.len() * CHAR_W;
            if draw_x + seg_px > crumb_clip { break; }
            let col = if is_last { CRUMB_CUR }
                      else if Some(i) == self.hover_crumb { CRUMB_HOV }
                      else { CRUMB_COL };
            if Some(i) == self.hover_crumb && !is_last {
                framebuffer::fill_rect(
                    draw_x.saturating_sub(2), hdr_ty.saturating_sub(2),
                    seg_px + 4, 12, 0x142A40);
            }
            framebuffer::draw_text_at(draw_x, hdr_ty, seg_str, col);
            draw_x += seg_px;
        }

        // ── Column header ─────────────────────────────────────────────────
        let col_y = cy + HEADER_H;
        framebuffer::fill_rect(cx, col_y, cw, COL_HDR_H, COLHDR_BG);
        framebuffer::draw_text_at(cx + PAD_X + PREFIX_W,
                                  col_y + (COL_HDR_H - 8) / 2, "Name", COLHDR_COL);
        let sz_x = cx + cw.saturating_sub(PAD_X + SIZE_COL_W);
        framebuffer::draw_text_at(sz_x, col_y + (COL_HDR_H - 8) / 2, "Size", COLHDR_COL);
        framebuffer::fill_rect(cx, col_y + COL_HDR_H - 1, cw, 1, BORDER_COL);

        // ── List area ─────────────────────────────────────────────────────
        let list_y    = col_y + COL_HDR_H;
        let list_h    = ch.saturating_sub(HEADER_H + COL_HDR_H + HINT_H);
        let visible   = list_h / ROW_H;
        let scroll    = self.scroll;

        if self.load_err {
            let ey = list_y + list_h / 3;
            framebuffer::draw_text_at(cx + PAD_X, ey,
                "[!]  Could not read directory", ERR_COL);
            framebuffer::draw_text_at(cx + PAD_X, ey + ROW_H + 4,
                "     Check that the path is valid.", COLHDR_COL);
        } else if self.count == 0 {
            framebuffer::draw_text_at(cx + PAD_X, list_y + list_h / 3,
                "(empty directory)", EMPTY_COL);
        } else {
            for vi in 0..visible {
                let ei = scroll + vi;
                if ei >= self.count { break; }
                let e  = &self.entries[ei];
                let ry = list_y + vi * ROW_H;
                let is_sel = ei == self.selected;

                let row_bg = if is_sel { SEL_BG }
                             else if Some(ei) == self.hover_row { HOVER_BG }
                             else if vi % 2 == 1 { EVEN_BG }
                             else { BG };
                framebuffer::fill_rect(cx, ry, cw.saturating_sub(SCROLL_W), ROW_H, row_bg);
                if is_sel {
                    // Thick left-edge accent bar (5 px) + right-edge accent (2 px)
                    framebuffer::fill_rect(cx, ry, 5, ROW_H, SEL_BORDER);
                    framebuffer::fill_rect(cx + cw.saturating_sub(SCROLL_W + 2), ry, 2, ROW_H, SEL_BORDER);
                } else if Some(ei) == self.hover_row {
                    // Thin left indicator for hovered row so it doesn't look selected
                    framebuffer::fill_rect(cx, ry, 2, ROW_H, 0x1A3A58);
                }

                let ty  = ry + (ROW_H - 8) / 2;
                let (icon, base_col) = if e.is_dir { ("[>] ", DIR_COL) }
                                       else        { ("    ", FILE_COL) };
                let tcol = if is_sel { SEL_COL } else { base_col };

                framebuffer::draw_text_at(cx + PAD_X, ty, icon, tcol);
                let name_max = cw.saturating_sub(PAD_X + PREFIX_W + SIZE_COL_W + PAD_X + SCROLL_W) / CHAR_W;
                framebuffer::draw_text_at(cx + PAD_X + PREFIX_W, ty,
                                          truncate_str(e.name_str(), name_max), tcol);

                {
                    let mut sbuf = [0u8; 16];
                    let sstr = if e.is_dir {
                        let n = fmt_uint(&mut sbuf, 0, e.size);
                        let suf = b" items";
                        let end = (n + suf.len()).min(sbuf.len());
                        sbuf[n..end].copy_from_slice(&suf[..end - n]);
                        core::str::from_utf8(&sbuf[..end]).unwrap_or("")
                    } else {
                        FileManagerApp::fmt_size(&mut sbuf, e.size)
                    };
                    let sz_col = if is_sel { SIZE_SEL } else { SIZE_COL };
                    let srx = cx + cw.saturating_sub(PAD_X + sstr.len() * CHAR_W + SCROLL_W);
                    framebuffer::draw_text_at(srx, ty, sstr, sz_col);
                }
            }   // end for vi

            // Scrollbar
            let sb_x = cx + cw.saturating_sub(SCROLL_W);
            framebuffer::fill_rect(sb_x, list_y, SCROLL_W, list_h, SCROLL_BG);
            if visible < self.count && list_h > 0 {
                let thumb_h = ((visible * list_h) / self.count).max(6);
                let thumb_y = if self.count > visible {
                    (scroll * (list_h - thumb_h)) / (self.count - visible)
                } else { 0 };
                framebuffer::fill_rect(sb_x + 1, list_y + thumb_y,
                                       SCROLL_W - 2, thumb_h, SCROLL_FG);
            }
        }

        // ── Hint bar (or inline prompt) ─────────────────────────────────────────
        let hint_y = cy + ch.saturating_sub(HINT_H);
        framebuffer::fill_rect(cx, hint_y, cw, HINT_H, HEADER_BG);
        framebuffer::fill_rect(cx, hint_y, cw, 1, BORDER_COL);
        let ty = hint_y + (HINT_H - 8) / 2;

        match self.prompt.kind {
            PromptKind::None => {
                if let Some(err) = self.op_err {
                    // Red error bar background
                    framebuffer::fill_rect(cx, hint_y, cw, HINT_H, 0x2A0A0A);
                    framebuffer::fill_rect(cx, hint_y, cw, 1, 0x8A1A1A);
                    framebuffer::draw_text_at(cx + PAD_X, ty, "[!] ", 0xFF4444);
                    framebuffer::draw_text_at(cx + PAD_X + 4 * CHAR_W, ty, err, 0xFF8888);
                    let dismiss = "(any key to dismiss)";
                    let dx = cx + cw.saturating_sub(PAD_X + dismiss.len() * CHAR_W);
                    framebuffer::draw_text_at(dx, ty, dismiss, 0x885555);
                } else if let Some(msg) = self.op_ok {
                    // Green success bar
                    framebuffer::fill_rect(cx, hint_y, cw, HINT_H, 0x061206);
                    framebuffer::fill_rect(cx, hint_y, cw, 1, 0x1A5A1A);
                    framebuffer::draw_text_at(cx + PAD_X, ty, "OK  ", 0x44FF44);
                    framebuffer::draw_text_at(cx + PAD_X + 4 * CHAR_W, ty, msg, 0x88FF88);
                } else {
                    let mut hx = cx + PAD_X;
                    macro_rules! hkey { ($s:expr) => { { framebuffer::draw_text_at(hx, ty, $s, HINT_KEY); hx += $s.len() * CHAR_W; } } }
                    macro_rules! hsep { ($s:expr) => { { framebuffer::draw_text_at(hx, ty, $s, HINT_COL); hx += $s.len() * CHAR_W; } } }
                    hkey!("Enter"); hsep!("=open  ");
                    hkey!("\u{2191}\u{2193}");    hsep!("=nav  ");
                    hkey!("N");     hsep!("=file  ");
                    hkey!("M");     hsep!("=dir");
                    let sel_can_edit = self.count > 0 && {
                        let e = &self.entries[self.selected];
                        let is_back = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
                        (e.is_dyn || e.is_fat32) && !is_back
                    };
                    if sel_can_edit {
                        hsep!("  ");
                        hkey!("Del"); hsep!("=del  ");
                        hkey!("R");   hsep!("=ren");
                    }
                    let esc = "Esc=close";
                    let ex = cx + cw.saturating_sub(PAD_X + esc.len() * CHAR_W);
                    framebuffer::draw_text_at(ex, ty, esc, HINT_KEY);
                }
            }
            PromptKind::ConfirmDel => {
                // Display target filename in red
                let lbl = "Delete \"";
                framebuffer::draw_text_at(cx + PAD_X, ty, lbl, ERR_COL);
                let mut hx = cx + PAD_X + lbl.len() * CHAR_W;
                let fname = core::str::from_utf8(
                    &self.prompt.buf[..self.prompt.len]).unwrap_or("?");
                framebuffer::draw_text_at(hx, ty, fname, ERR_COL);
                hx += self.prompt.len * CHAR_W;
                framebuffer::draw_text_at(hx, ty, "\"?", ERR_COL);
                let ok = "Enter=yes  Esc=no";
                let ox = cx + cw.saturating_sub(PAD_X + ok.len() * CHAR_W);
                framebuffer::draw_text_at(ox, ty, ok, HINT_KEY);
            }
            PromptKind::New | PromptKind::Mkdir | PromptKind::Rename => {
                let lbl = match self.prompt.kind {
                    PromptKind::New   => "New file: ",
                    PromptKind::Mkdir => "New folder: ",
                    _                 => "Rename to: ",
                };
                framebuffer::draw_text_at(cx + PAD_X, ty, lbl, PATH_LBL);
                let ix = cx + PAD_X + lbl.len() * CHAR_W;
                let input = core::str::from_utf8(
                    &self.prompt.buf[..self.prompt.len]).unwrap_or("");
                framebuffer::draw_text_at(ix, ty, input, PATH_COL);
                // Blinking cursor placeholder
                let cur_x = ix + self.prompt.len * CHAR_W;
                framebuffer::draw_text_at(cur_x, ty, "_", HINT_KEY);
                let ok = "Enter=ok  Esc=cancel";
                let ox = cx + cw.saturating_sub(PAD_X + ok.len() * CHAR_W);
                framebuffer::draw_text_at(ox, ty, ok, HINT_KEY);
            }
        }   // end match self.prompt.kind

        // ── Context menu overlay (drawn on top of everything) ─────────────────
        if self.ctx.visible {
            let mw = self.ctx.width();
            let mh = self.ctx.height();
            let mx = (cx as i32 + self.ctx.x).max(cx as i32) as usize;
            let my = (cy as i32 + self.ctx.y).max(cy as i32) as usize;
            // Clamp so the menu never overflows the window
            let mx = if mx + mw > cx + cw { (cx + cw).saturating_sub(mw) } else { mx };
            let my = if my + mh > cy + ch { (cy + ch).saturating_sub(mh) } else { my };
            // Background + border
            framebuffer::fill_rect(mx, my, mw, mh, CTX_BORDER);
            framebuffer::fill_rect(mx + 1, my + 1, mw - 2, mh - 2, CTX_BG);
            for i in 0..self.ctx.item_count {
                let item = &self.ctx.items[i];
                let iy = my + 2 + i * CTX_ITEM_H;
                if self.ctx.hover == Some(i) && item.enabled {
                    framebuffer::fill_rect(mx + 1, iy, mw - 2, CTX_ITEM_H, CTX_SEL_BG);
                }
                let text_col = if item.enabled { CTX_COL } else { CTX_DIS };
                framebuffer::draw_text_at(mx + CTX_PAD_X, iy + (CTX_ITEM_H - 8) / 2,
                                          item.label, text_col);
            }
        }
    }

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
                                let name = core::str::from_utf8(
                                    &self.prompt.buf[..self.prompt.len]).unwrap_or("");
                                if crate::fat32::is_mounted() {
                                    // Create directly on disk so it survives reboot
                                    let dir_c = if self.fat32_cluster != 0 {
                                        self.fat32_cluster
                                    } else {
                                        fs::fat32_root_cluster()
                                    };
                                    if let Some(id) = fs::fat32_create_and_open(dir_c, name.as_bytes()) {
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
                                        self.op_err = Some("Create failed — name may already exist or be invalid");
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
                                let name = core::str::from_utf8(
                                    &self.prompt.buf[..self.prompt.len]).unwrap_or("");
                                if crate::fat32::is_mounted() {
                                    let dir_c = if self.fat32_cluster != 0 {
                                        self.fat32_cluster
                                    } else {
                                        fs::fat32_root_cluster()
                                    };
                                    if !crate::fat32::create_dir(dir_c, name.as_bytes()) {
                                        self.op_err = Some("Create folder failed — name may already exist");
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
                                let new_name = core::str::from_utf8(
                                    &self.prompt.buf[..self.prompt.len]).unwrap_or("");
                                if fs::is_fat32_id(self.prompt.target) {
                                    // FAT32 rename: need old name from cache
                                    if let Some(old_name) = fs::fat32_entry_name(self.prompt.target) {
                                        let dir_c = if self.fat32_cluster != 0 {
                                            self.fat32_cluster
                                        } else {
                                            fs::fat32_root_cluster()
                                        };
                                        if !crate::fat32::rename_entry(
                                            dir_c, &old_name.0[..old_name.1], new_name.as_bytes()) {
                                            self.op_err = Some("Rename failed");
                                        } else {
                                            self.op_ok = Some("Renamed");
                                            self.op_err = None;
                                        }
                                    } else {
                                        self.op_err = Some("Rename failed — entry not found in cache");
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
                                        self.op_err = Some("Not empty \u{2014} delete contents first");
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
                    if let Some(act) = open_file_action { return act; }
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
                    if c >= 0x20 && c < 0x7F && c != b'/' && self.prompt.len < 32 {
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
        self.op_err = None;  // clear stale error on any new keypress
        self.op_ok  = None;  // clear stale success message on any new keypress
        let old_sel    = self.selected;
        let old_scroll = self.scroll;
        match key {
            Key::Escape => {
                // Go back to "This PC" root view
                self.view = FmView::ThisPc;
                self.hover_crumb = None;
                return AppAction::RedrawAll;
            }
            Key::ArrowUp => {
                if self.selected > 0 { self.selected -= 1; }
            }
            Key::ArrowDown => {
                if self.selected + 1 < self.count { self.selected += 1; }
            }
            Key::Char(b'g') | Key::Home => { self.selected = 0; }
            Key::Char(b'G') | Key::End  => { self.selected = self.count.saturating_sub(1); }
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
                    if self.count > 0 && self.entries[0].nlen == 2
                        && self.entries[0].name[0] == b'.' && self.entries[0].name[1] == b'.'
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
                    kind:   PromptKind::New,
                    buf:    [0u8; 32],
                    len:    0,
                    target,
                };
                return AppAction::RedrawAll;
            }
            Key::Char(b'm') | Key::Char(b'M') => {
                // Start "new folder" (mkdir) prompt.
                let target = fs::resolve_node_id(self.cwd.as_str()).unwrap_or(0);
                self.prompt = FmPrompt {
                    kind:   PromptKind::Mkdir,
                    buf:    [0u8; 32],
                    len:    0,
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
                            kind:   PromptKind::ConfirmDel,
                            buf,
                            len:    e.nlen,
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
                            kind:   PromptKind::Rename,
                            buf,
                            len:    e.nlen,
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
                        let src_cluster = if self.fat32_cluster != 0 { self.fat32_cluster }
                                          else { fs::fat32_root_cluster() };
                        self.clipboard = Clipboard { name, name_len: e.nlen, src_cluster, is_cut: false };
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
                        let src_cluster = if self.fat32_cluster != 0 { self.fat32_cluster }
                                          else { fs::fat32_root_cluster() };
                        self.clipboard = Clipboard { name, name_len: e.nlen, src_cluster, is_cut: true };
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

    fn mouse_click_files(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        self.op_ok  = None;
        self.op_err = None;
        // If context menu is open, check for a click on a menu item first.
        if self.ctx.visible {
            let mw = self.ctx.width() as i32;
            let mh = self.ctx.height() as i32;
            let (_, ph) = self.preferred_size();
            let cw = ph as i32; // approximation not needed — use stored values
            // We stored ctx.x/y relative to the client area
            // Clamp the menu origin the same way render() does (simplified: just use ctx.x/y)
            let mx = self.ctx.x;
            let my = self.ctx.y;
            if rel_x >= mx && rel_x < mx + mw && rel_y >= my && rel_y < my + mh {
                let item_idx = ((rel_y - my - 2) / CTX_ITEM_H as i32) as usize;
                let acted = if item_idx < self.ctx.item_count {
                    let item = self.ctx.items[item_idx];
                    if item.enabled {
                        self.ctx.visible = false;
                        self.execute_ctx_action(item.action)
                    } else {
                        self.ctx.visible = false;
                        AppAction::RedrawAll
                    }
                } else {
                    self.ctx.visible = false;
                    AppAction::RedrawAll
                };
                return acted;
            }
            // Click outside menu — dismiss
            self.ctx.visible = false;
            return AppAction::RedrawAll;
        }

        // Breadcrumb click in the header area
        if rel_y >= 0 && rel_y < HEADER_H as i32 {
            let lbl_w = "Location: ".len() * CHAR_W;
            let thispc_label = "This PC";
            let thispc_w = (thispc_label.len() * CHAR_W) as i32;
            let sep_w = (" > ".len() * CHAR_W) as i32;
            let crumb_x0 = (PAD_X + lbl_w) as i32;
            // Check click on "This PC" root crumb
            if rel_x >= crumb_x0 && rel_x < crumb_x0 + thispc_w {
                self.view = FmView::ThisPc;
                self.hover_crumb = None;
                return AppAction::RedrawAll;
            }
            // The VFS/FAT32 crumbs start after "This PC > "
            let vfs_x0 = crumb_x0 + thispc_w + sep_w;
            // Snapshot all needed state before any mutations
            let mut path_buf = [0u8; 128];
            let path_len = self.cwd.len;
            path_buf[..path_len].copy_from_slice(&self.cwd.data[..path_len]);
            let path = &path_buf[..path_len];
            let mut segs = [(0usize, 0usize); MAX_CRUMBS];
            let vfs_seg_count = parse_crumbs(path, &mut segs);
            let fat32_depth        = self.fat32_stack_depth;
            let fat32_crumb_nlens  = self.fat32_crumb_nlens;
            let fat32_cluster_stack = self.fat32_cluster_stack;
            let skip_bare_root = fat32_depth > 0 && vfs_seg_count == 1;
            let total_segs = (if skip_bare_root { 0 } else { vfs_seg_count }) + fat32_depth;
            let mut x = vfs_x0;
            for i in 0..total_segs {
                if i > 0 { x += sep_w; }
                let seg_w: i32 = if !skip_bare_root && i < vfs_seg_count {
                    let (_, bl) = segs[i];
                    (bl * CHAR_W) as i32
                } else {
                    let fi = if skip_bare_root { i } else { i - vfs_seg_count };
                    (fat32_crumb_nlens[fi] * CHAR_W) as i32
                };
                if rel_x >= x && rel_x < x + seg_w {
                    // Don't navigate on the current (last) segment
                    if i < total_segs - 1 {
                        if !skip_bare_root && i < vfs_seg_count {
                            // ── VFS segment ───────────────────────────────
                            if i == 0 {
                                // Root "/"
                                self.cwd = PathBuf::root();
                            } else if i < vfs_seg_count - 1 {
                                // Intermediate VFS path segment
                                let mut new_path = PathBuf::root();
                                for j in 1..=i {
                                    let (s, l) = segs[j];
                                    let seg_name = core::str::from_utf8(&path[s..s + l]).unwrap_or("");
                                    new_path.push(seg_name);
                                }
                                self.cwd = new_path;
                            }
                            // Last VFS seg (i == vfs_seg_count - 1) but FAT32
                            // crumbs follow: drop into FAT32 root view.
                            self.fat32_cluster = 0;
                            self.fat32_stack_depth = 0;
                        } else {
                            // ── FAT32 stack segment ───────────────────────
                            let fi = if skip_bare_root { i } else { i - vfs_seg_count };
                            self.fat32_cluster = fat32_cluster_stack[fi + 1];
                            self.fat32_stack_depth = fi + 1;
                        }
                        self.hover_crumb = None;
                        self.load_dir();
                        return AppAction::RedrawAll;
                    }
                    break;
                }
                x += seg_w;
            }
            return AppAction::Nothing;
        }

        let list_top = (HEADER_H + COL_HDR_H) as i32;
        if rel_y < list_top { return AppAction::Nothing; }
        let row_in_view = ((rel_y - list_top) as usize) / ROW_H;
        let row_abs = self.scroll + row_in_view;
        if row_abs >= self.count { return AppAction::Nothing; }

        let now    = uptime_ms();
        let is_dbl = row_abs == self.last_click_row
            && now.saturating_sub(self.last_click_ms) < DBL_CLICK_MS;

        self.last_click_ms  = now;
        self.last_click_row = row_abs;
        let old_sel = self.selected;
        self.selected = row_abs;

        if is_dbl {
            self.last_click_row = usize::MAX;
            self.open_selected()
        } else if self.selected != old_sel {
            AppAction::RedrawAll
        } else {
            AppAction::Nothing
        }
    }

    fn mouse_move_files(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        // Update context menu hover
        if self.ctx.visible {
            let mw = self.ctx.width() as i32;
            let mh = self.ctx.height() as i32;
            let mx = self.ctx.x;
            let my = self.ctx.y;
            let new_hover = if rel_x >= mx && rel_x < mx + mw && rel_y >= my && rel_y < my + mh {
                let idx = ((rel_y - my - 2) / CTX_ITEM_H as i32) as usize;
                if idx < self.ctx.item_count { Some(idx) } else { None }
            } else { None };
            if new_hover != self.ctx.hover {
                self.ctx.hover = new_hover;
                return AppAction::RedrawArea(
                    self.ctx.x.max(0) as usize,
                    self.ctx.y.max(0) as usize,
                    self.ctx.width(),
                    self.ctx.height(),
                );
            }
            return AppAction::Nothing;
        }
        let old_hc = self.hover_crumb;
        let old_row = self.hover_row;

        // Breadcrumb hover (header area)
        let new_hc = if rel_y >= 0 && rel_y < HEADER_H as i32 {
            let lbl_w = "Location: ".len() * CHAR_W;
            let sep_w = (" > ".len() * CHAR_W) as i32;
            let thispc_w = ("This PC".len() * CHAR_W) as i32;
            let crumb_x0 = (PAD_X + lbl_w) as i32;
            // Check "This PC" root crumb (always clickable)
            if rel_x >= crumb_x0 && rel_x < crumb_x0 + thispc_w {
                Some(usize::MAX) // sentinel for "This PC"
            } else {
                let vfs_x0 = crumb_x0 + thispc_w + sep_w;
                let path = &self.cwd.data[..self.cwd.len];
                let mut segs = [(0usize, 0usize); MAX_CRUMBS];
                let vfs_seg_count = parse_crumbs(path, &mut segs);
                let fat32_depth       = self.fat32_stack_depth;
                let fat32_crumb_nlens = self.fat32_crumb_nlens;
                let skip_bare_root = fat32_depth > 0 && vfs_seg_count == 1;
                let total_segs = (if skip_bare_root { 0 } else { vfs_seg_count }) + fat32_depth;
                let mut x = vfs_x0;
                let mut found = None;
                for i in 0..total_segs {
                    if i > 0 { x += sep_w; }
                    let seg_w: i32 = if !skip_bare_root && i < vfs_seg_count {
                        let (_, bl) = segs[i];
                        (bl * CHAR_W) as i32
                    } else {
                        let fi = if skip_bare_root { i } else { i - vfs_seg_count };
                        (fat32_crumb_nlens[fi] * CHAR_W) as i32
                    };
                    // Only highlight clickable (non-last) segments
                    if rel_x >= x && rel_x < x + seg_w && i < total_segs - 1 {
                        found = Some(i);
                        break;
                    }
                    x += seg_w;
                }
                found
            }
        } else {
            None
        };
        self.hover_crumb = new_hc;

        // Row hover (list area)
        let list_top = (HEADER_H + COL_HDR_H) as i32;
        let new_hover = if rel_y >= list_top {
            let row_in_view = ((rel_y - list_top) as usize) / ROW_H;
            let row_abs = self.scroll + row_in_view;
            if row_abs < self.count { Some(row_abs) } else { None }
        } else {
            None
        };
        self.hover_row = new_hover;

        if old_hc != self.hover_crumb || old_row != self.hover_row {
            let crumb_damage = Self::union_damage(
                self.hover_crumb_damage(old_hc),
                self.hover_crumb_damage(self.hover_crumb),
            );
            let row_damage = Self::union_damage(
                self.hover_row_damage(old_row),
                self.hover_row_damage(self.hover_row),
            );
            if let Some((x, y, w, h)) = Self::union_damage(crumb_damage, row_damage) {
                AppAction::RedrawArea(x, y, w, h)
            } else {
                AppAction::Nothing
            }
        } else {
            AppAction::Nothing
        }
    }

    fn right_click_files(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        let list_top = (HEADER_H + COL_HDR_H) as i32;
        let hint_top = (self.preferred_size().1 as i32).saturating_sub(HINT_H as i32);
        if rel_y >= list_top && rel_y < hint_top {
            let row_in_view = ((rel_y - list_top) as usize) / ROW_H;
            let row_abs = self.scroll + row_in_view;
            let row = if row_abs < self.count {
                self.selected = row_abs;
                row_abs
            } else {
                usize::MAX
            };
            self.open_ctx_for(rel_x, rel_y, row);
            return AppAction::RedrawAll;
        }
        AppAction::Nothing
    }
}

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
            macro_rules! push { ($act:expr, $lbl:expr, $en:expr) => {
                if menu.item_count < 5 {
                    menu.items[menu.item_count] = CtxItem { action: $act, label: $lbl, enabled: $en };
                    menu.item_count += 1;
                }
            }; }
            push!(CtxAction::NewFile, "New file",   true);
            push!(CtxAction::NewDir,  "New folder", true);
            push!(CtxAction::Paste,   "Paste",      self.clipboard.is_set());
        } else {
            let e = self.entries[row];
            let is_up = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
            macro_rules! push { ($act:expr, $lbl:expr, $en:expr) => {
                if menu.item_count < 5 {
                    menu.items[menu.item_count] = CtxItem { action: $act, label: $lbl, enabled: $en };
                    menu.item_count += 1;
                }
            }; }
            push!(CtxAction::Open,    "Open",   true);
            push!(CtxAction::Copy,    "Copy",   e.is_fat32 && !is_up);
            push!(CtxAction::Cut,     "Cut",    e.is_fat32 && !is_up);
            push!(CtxAction::Rename,  "Rename", (e.is_dyn || e.is_fat32) && !is_up);
            push!(CtxAction::Delete,  "Delete", (e.is_dyn || e.is_fat32) && !is_up);
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
                    if matches!(act, AppAction::Nothing) { self.selected = old; }
                    return act;
                }
            }
            CtxAction::NewFile => {
                let target = fs::resolve_node_id(self.cwd.as_str()).unwrap_or(0);
                self.prompt = FmPrompt { kind: PromptKind::New, buf: [0u8; 32],
                                         len: 0, target };
            }
            CtxAction::NewDir => {
                let target = fs::resolve_node_id(self.cwd.as_str()).unwrap_or(0);
                self.prompt = FmPrompt { kind: PromptKind::Mkdir, buf: [0u8; 32],
                                         len: 0, target };
            }
            CtxAction::Rename => {
                if self.ctx.target_row < self.count {
                    let e = self.entries[self.ctx.target_row];
                    let is_back = e.nlen == 2 && e.name[0] == b'.' && e.name[1] == b'.';
                    if !is_back && (e.is_dyn || e.is_fat32) {
                        let mut buf = [0u8; 32];
                        buf[..e.nlen].copy_from_slice(&e.name[..e.nlen]);
                        self.prompt = FmPrompt { kind: PromptKind::Rename,
                                                 buf, len: e.nlen, target: e.node_id };
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
                        self.prompt = FmPrompt { kind: PromptKind::ConfirmDel,
                                                 buf, len: e.nlen, target: e.node_id };
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
                        let src_cluster = if self.fat32_cluster != 0 { self.fat32_cluster }
                                          else { fs::fat32_root_cluster() };
                        self.clipboard = Clipboard { name, name_len: e.nlen, src_cluster, is_cut: false };
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
                        let src_cluster = if self.fat32_cluster != 0 { self.fat32_cluster }
                                          else { fs::fat32_root_cluster() };
                        self.clipboard = Clipboard { name, name_len: e.nlen, src_cluster, is_cut: true };
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

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse a VFS path into breadcrumb segments.
/// Returns (byte_start, byte_len) for each segment in `path`.
/// Segment 0 is always the root "/".
fn parse_crumbs(path: &[u8], out: &mut [(usize, usize); MAX_CRUMBS]) -> usize {
    out[0] = (0, 1); // root "/"
    let mut count = 1usize;
    let mut i = 1usize;
    while i < path.len() && count < MAX_CRUMBS {
        let start = i;
        while i < path.len() && path[i] != b'/' { i += 1; }
        if start < i {
            out[count] = (start, i - start);
            count += 1;
        }
        if i < path.len() { i += 1; } // skip '/'
    }
    count
}

fn truncate_str(s: &str, max: usize) -> &str {
    let b = s.as_bytes();
    if b.len() <= max { return s; }
    let mut end = max;
    while end > 0 && (b[end] & 0xC0) == 0x80 { end -= 1; }
    core::str::from_utf8(&b[..end]).unwrap_or("")
}

fn fmt_uint(buf: &mut [u8; 16], pos: usize, mut n: usize) -> usize {
    if n == 0 {
        if pos < buf.len() { buf[pos] = b'0'; }
        return pos + 1;
    }
    let start = pos;
    let mut i = pos;
    while n > 0 && i < buf.len() { buf[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
    buf[start..i].reverse();
    i
}

fn fmt_uint_u64(buf: &mut [u8; 24], pos: usize, mut n: u64) -> usize {
    if n == 0 {
        if pos < buf.len() { buf[pos] = b'0'; }
        return pos + 1;
    }
    let start = pos;
    let mut i = pos;
    while n > 0 && i < buf.len() { buf[i] = b'0' + (n % 10) as u8; n /= 10; i += 1; }
    buf[start..i].reverse();
    i
}

fn fmt_count(buf: &mut [u8; 20], n: usize) -> &str {
    let mut tmp = [0u8; 16];
    let end = {
        let mut i = 0usize;
        let mut v = n;
        if v == 0 { tmp[0] = b'0'; i = 1; }
        else { while v > 0 && i < tmp.len() { tmp[i] = b'0' + (v % 10) as u8; v /= 10; i += 1; } tmp[..i].reverse(); }
        i
    };
    let nstr = core::str::from_utf8(&tmp[..end]).unwrap_or("0");
    let mut i = 0usize;
    for b in nstr.bytes() { if i < buf.len() { buf[i] = b; i += 1; } }
    for b in b" items" { if i < buf.len() { buf[i] = *b; i += 1; } }
    core::str::from_utf8(&buf[..i]).unwrap_or("")
}

/// Encode a u16 as lowercase hex into a 4-byte array.
/// Returns (buf, len) where len is always 4.
fn hex_u16(v: u16) -> ([u8; 4], usize) {
    let hex = b"0123456789abcdef";
    let buf = [
        hex[((v >> 12) & 0xF) as usize],
        hex[((v >>  8) & 0xF) as usize],
        hex[((v >>  4) & 0xF) as usize],
        hex[( v        & 0xF) as usize],
    ];
    (buf, 4)
}
