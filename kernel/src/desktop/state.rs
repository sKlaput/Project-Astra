// ── Drag / resize state ───────────────────────────────────────────────────────

struct DragState {
    win_idx: usize,
    off_x: i32,
    off_y: i32,
}

struct IconDragState {
    idx: usize,
    off_x: i32,
    off_y: i32,
    moved: bool,
}

// ── Desktop items (user-created files/folders on the desktop) ─────────────────

const MAX_DESK_ITEMS: usize = 32;
const DI_W: usize = ICON_CELL_W; // match dock icon cell width (88)
const DI_H: usize = ICON_CELL_H; // match dock icon cell height (78)
const DI_ICON_FILE: usize = 100; // draw_app_icon index for generic file
const DI_ICON_DIR: usize = 101; // draw_app_icon index for folder
const DI_SEL_BG: u32 = ICON_SEL; // same selection colour as dock
const DI_TEXT: u32 = ICON_TEXT;
const DI_SEL_TEXT: u32 = ICON_TEXT_SEL;

// Desktop name-entry prompt (shown when user picks "New File" / "New Folder")
const DP_W: usize = 140;
const DP_H: usize = 36;
const DP_BG: u32 = 0x08121E;
const DP_BORD: u32 = 0x2A6090;
const DP_LBL: u32 = 0x4A90B8;
const DP_TEXT: u32 = 0xD8EEFF;
const DP_CUR: u32 = 0x60AADD;

#[derive(Clone, Copy)]
struct DesktopItem {
    x: i32,
    y: i32,
    name: [u8; 32],
    nlen: usize,
    is_dir: bool,
    /// FAT32 first cluster of this entry (stored at creation so double-click
    /// opens directly inside the folder without another find_in_dir lookup).
    fat32_cluster: u32,
    selected: bool,
    last_click_ms: u64,
}

impl DesktopItem {
    const fn blank() -> Self {
        DesktopItem {
            x: 0,
            y: 0,
            name: [0u8; 32],
            nlen: 0,
            is_dir: false,
            fat32_cluster: 0,
            selected: false,
            last_click_ms: 0,
        }
    }
    fn rect(&self) -> Rect {
        Rect {
            x: self.x as usize,
            y: self.y as usize,
            w: DI_W,
            h: DI_H,
        }
    }
}

struct DesktopNamePrompt {
    active: bool,
    spawn_x: i32, // right-click position where we'll place the item
    spawn_y: i32,
    is_dir: bool,
    buf: [u8; 32],
    len: usize,
}

impl DesktopNamePrompt {
    const fn hidden() -> Self {
        DesktopNamePrompt {
            active: false,
            spawn_x: 0,
            spawn_y: 0,
            is_dir: false,
            buf: [0u8; 32],
            len: 0,
        }
    }
    /// Bounding rect of the rendered prompt box (clamped to screen).
    fn rect(&self, sw: usize, sh: usize) -> Rect {
        let x = (self.spawn_x as usize).min(sw.saturating_sub(DP_W));
        let y = (self.spawn_y as usize).min(sh.saturating_sub(DP_H + 14));
        Rect {
            x,
            y: y + 14,
            w: DP_W,
            h: DP_H,
        }
    }
}

struct DeskItemDrag {
    idx: usize,
    off_x: i32,
    off_y: i32,
    moved: bool,
}

struct ResizeState {
    win_idx: usize,
    zone: ResizeZone,
    start_mx: i32,
    start_my: i32,
    start_x: i32,
    start_y: i32,
    start_w: usize,
    start_h: usize,
}

// ── Taskbar button geometry ───────────────────────────────────────────────────

const BTN_W: usize = 84;
const BTN_H: usize = 22;
const BTN_GAP: usize = 4;
const BTN_START_X: usize = 8;
const FIXED_BTNS: usize = 2;
const WIN_BTN_START_X: usize = BTN_START_X + (BTN_W + BTN_GAP) * FIXED_BTNS + 8;

fn taskbar_btn_rect(idx: usize) -> Rect {
    let y = (BAR_H - BTN_H) / 2;
    if idx < FIXED_BTNS {
        Rect {
            x: BTN_START_X + idx * (BTN_W + BTN_GAP),
            y,
            w: BTN_W,
            h: BTN_H,
        }
    } else {
        let wi = idx - FIXED_BTNS;
        Rect {
            x: WIN_BTN_START_X + wi * (BTN_W + BTN_GAP),
            y,
            w: BTN_W,
            h: BTN_H,
        }
    }
}

const POWER_BTN_W: usize = 28;
const POWER_BTN_MARGIN: usize = 6;

fn power_btn_rect(sw: usize) -> Rect {
    let y = (BAR_H - BTN_H) / 2;
    Rect {
        x: sw.saturating_sub(POWER_BTN_W + POWER_BTN_MARGIN),
        y,
        w: POWER_BTN_W,
        h: BTN_H,
    }
}
