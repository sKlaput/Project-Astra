// ── File-operation prompt state ─────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptKind {
    None,
    New,
    Mkdir,
    Rename,
    ConfirmDel,
}

#[derive(Clone, Copy)]
struct FmPrompt {
    kind: PromptKind,
    buf: [u8; 32], // input buffer for name / display name for confirm
    len: usize,
    target: u16, // parent NodeId (New) or file NodeId (Rename/Delete)
}

impl FmPrompt {
    const DEFAULT: Self = FmPrompt {
        kind: PromptKind::None,
        buf: [0u8; 32],
        len: 0,
        target: 0,
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
    name: [u8; 64],
    name_len: usize,
    /// FAT32 directory cluster that contains the source.
    src_cluster: u32,
    /// True = move (cut), false = copy.
    is_cut: bool,
}

impl Clipboard {
    const EMPTY: Self = Clipboard {
        name: [0u8; 64],
        name_len: 0,
        src_cluster: 0,
        is_cut: false,
    };
    fn is_set(&self) -> bool {
        self.name_len > 0
    }
}

#[derive(Clone, Copy)]
struct CtxItem {
    action: CtxAction,
    label: &'static str,
    enabled: bool,
}

#[derive(Clone, Copy)]
struct CtxMenu {
    visible: bool,
    x: i32, // position relative to window client area
    y: i32,
    hover: Option<usize>,
    items: [CtxItem; 5],
    item_count: usize,
    // which row was right-clicked (usize::MAX = empty area)
    target_row: usize,
}

impl CtxMenu {
    const NULL_ITEM: CtxItem = CtxItem {
        action: CtxAction::Open,
        label: "",
        enabled: false,
    };
    const fn hidden() -> Self {
        CtxMenu {
            visible: false,
            x: 0,
            y: 0,
            hover: None,
            items: [Self::NULL_ITEM; 5],
            item_count: 0,
            target_row: usize::MAX,
        }
    }
    fn width(&self) -> usize {
        let mut max_chars = 0usize;
        for i in 0..self.item_count {
            let l = self.items[i].label.len();
            if l > max_chars {
                max_chars = l;
            }
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

const TILE_W: usize = 160;
const TILE_H: usize = 80;
const TILE_PAD: usize = 20; // horizontal gap between tiles
const TILE_TOP: usize = HEADER_H + 24; // top margin inside client area
const TILE_BG: u32 = 0x0C1828;
const TILE_HOV: u32 = 0x162840;
const TILE_SEL: u32 = 0x1C3F62;
const TILE_BORD: u32 = 0x1A3050;
const TILE_NAME: u32 = 0xD8EEFF;
const TILE_SUB: u32 = 0x4A7090;
const TILE_BAR: u32 = 0x1E5090;
const TILE_USED: u32 = 0x2E78CC;
const THIS_PC_BG: u32 = 0x060A10;

