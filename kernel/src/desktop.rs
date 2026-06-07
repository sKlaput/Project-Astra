// ---------------------------------------------------------------------------
// Astra OS — Desktop Compositor  (v3 — App-trait windowing)
//
// Manages:
//   - Taskbar (top)
//   - Desktop icons (left column, Windows-style)
//   - Launcher panel (left slide-out, toggled by "Apps >" in taskbar)
//   - Windows — each backed by a Box<dyn App>
//   - Input routing, drag, 8-zone resize, cursor shapes
//   - Damage-aware partial composition
// ---------------------------------------------------------------------------

extern crate alloc;
use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::app::{App, AppAction};
use crate::framebuffer;
use crate::input::{self, Event, Key};
use crate::arch::x86_64::interrupts::{uptime_ms, timer_ticks, timer_hz};
use crate::arch::x86_64::halt::idle_once;
use crate::rtc;
use crate::terminal::TerminalApp;
use crate::filemanager::FileManagerApp;
use crate::settings::SettingsApp;
use crate::sysmonitor::SysMonitorApp;
use crate::calculator::CalculatorApp;
use crate::imageviewer::ImageViewerApp;
use crate::notes::NotesApp;
use crate::logviewer::LogViewerApp;
use crate::about::AboutApp;
use crate::snake::SnakeApp;
use crate::tetris::TetrisApp;
use crate::editor::EditorApp;

use core::sync::atomic::{AtomicU32, Ordering as AO};
use spin::Mutex;

// Runtime-mutable desktop background colour (written by Settings app).
pub static DESKTOP_BG_COLOR: AtomicU32 = AtomicU32::new(0x0D1117);

// ── Window snapshot table (read by SysMonitor) ────────────────────────────────

pub const WIN_SNAP_MAX: usize = 20;

#[derive(Clone, Copy)]
pub struct WinSnap {
    pub title:     [u8; 32],
    pub title_len: usize,
    pub app_id:    [u8; 16],
    pub id_len:    usize,
    pub minimized: bool,
}

impl WinSnap {
    const fn empty() -> Self {
        WinSnap { title: [0u8; 32], title_len: 0, app_id: [0u8; 16], id_len: 0, minimized: false }
    }
}

pub struct WinTable {
    pub snaps: [WinSnap; WIN_SNAP_MAX],
    pub count: usize,
}

pub static WIN_TABLE: Mutex<WinTable> = Mutex::new(WinTable {
    snaps: [WinSnap::empty(); WIN_SNAP_MAX],
    count: 0,
});

// ── Colour palette ────────────────────────────────────────────────────────────

#[inline(always)] fn desktop_bg() -> u32 { DESKTOP_BG_COLOR.load(AO::Relaxed) }
const DESKTOP_BG:     u32 = 0x0D1117;  // kept for other callers
const BAR_BG:         u32 = 0x0A0E14;
const BAR_BORDER:     u32 = 0x1E3A5F;
const BAR_TEXT:       u32 = 0xD0E8FF;
const BAR_BTN_BG:     u32 = 0x1A2A3A;
const BAR_BTN_HOV:    u32 = 0x253848;
const BAR_BTN_ACT:    u32 = 0x1E3A5F;
const BAR_BTN_TEXT:   u32 = 0xC8E0F8;
const BAR_UPTIME:     u32 = 0x3A6080;

const WIN_SHADOW:     u32 = 0x06090E;
const WIN_BORDER:     u32 = 0x253848;
const WIN_BORDER_FOC: u32 = 0x2E5888;
const WIN_BG:         u32 = 0x0A0E14;
const WIN_BAR_BG:     u32 = 0x0C1320;
const WIN_BAR_FOC:    u32 = 0x0F1E36;
const WIN_BAR_BORDER: u32 = 0x1E3A5F;
const WIN_TITLE_COL:  u32 = 0xD8EEFF;
const WIN_HINT_COL:   u32 = 0x3A5878;
const WIN_CLOSE_HOV:  u32 = 0x7A1E1E;

const ICON_BG:        u32 = 0x111820;
const ICON_SEL:       u32 = 0x1A3050;
const ICON_BORDER:    u32 = 0x1E3A5F;
const ICON_TEXT:      u32 = 0x90B8D8;
const ICON_TEXT_SEL:  u32 = 0xD8EEFF;
const ICON_ACCENT:    u32 = 0x2E5888;

const LAUNCHER_BG:    u32 = 0x0A0F18;
const LAUNCHER_BORD:  u32 = 0x1A2F48;
const LAUNCHER_HEAD:  u32 = 0x0C1830;
const LAUNCHER_TEXT:  u32 = 0xD8EEFF;
const LAUNCHER_SUB:   u32 = 0x4A7090;
const LAUNCHER_HOV:   u32 = 0x162840;
const LAUNCHER_SEP:   u32 = 0x1A2F48;

const CURSOR_WHITE:   u32 = 0xFFFFFF;
const CURSOR_BLACK:   u32 = 0x000000;

// ── Layout ────────────────────────────────────────────────────────────────────

const BAR_H:          usize = 30;
const WIN_BAR_H:      usize = 28;
const WIN_SHADOW_OFS: usize = 4;
const WIN_MIN_W:      usize = 240;
const WIN_MIN_H:      usize = 180;
const WIN_PAD_X:      usize = 8;
const RESIZE_ZONE:    usize = 6;

const ICON_CELL_W:    usize = 88;
const ICON_CELL_H:    usize = 78;
const ICON_GRID_X:    usize = 8;
const ICON_GRID_Y:    usize = 16;
const DBL_CLICK_MS:   u64  = 450;

const LAUNCHER_W:     usize = 220;
const LAUNCHER_HEAD_H: usize = 48;
const LAUNCHER_ITEM_H: usize = 40;
const LAUNCHER_PAD_X: usize = 14;

const ICON_SNAP_STEP_X: usize = ICON_CELL_W + 8;   // = 96  (column stride)
const ICON_SNAP_STEP_Y: usize = ICON_CELL_H + 6;   // = 84  (row stride, matches original spacing)

// All four apps share a single source-of-truth descriptor table.  Every
// launch path (desktop icon, launcher panel, taskbar) pulls from here so
// label, identity, and subtitle are never out of sync.
const NUM_APPS:       usize = 11;
const NUM_ICONS:      usize = NUM_APPS;
const NUM_LAUNCHER:   usize = NUM_APPS;
const MAX_WINDOWS:    usize = 12;
const MAX_DAMAGE:     usize = 16;

const CURSOR_W:       usize = 10;
const CURSOR_H:       usize = 16;

// ── Desktop context menu ─────────────────────────────────────────────────────

const DCTX_W:      usize = 130;
const DCTX_ITEM_H: usize = 22;
const DCTX_ITEMS:  usize = 2;
const DCTX_H:      usize = DCTX_ITEMS * DCTX_ITEM_H + 4; // 2 px top/bottom padding
const DCTX_BG:     u32   = 0x0A0F18;
const DCTX_BORD:   u32   = 0x1A2F48;
const DCTX_HOV:    u32   = 0x162840;
const DCTX_TEXT:   u32   = 0xD8EEFF;

#[derive(Copy, Clone)]
struct DesktopCtxMenu {
    visible: bool,
    x: i32,
    y: i32,
    hover: Option<usize>,
}

impl DesktopCtxMenu {
    const fn hidden() -> Self { DesktopCtxMenu { visible: false, x: 0, y: 0, hover: None } }

    fn rect(&self, sw: usize, sh: usize) -> Rect {
        let x = (self.x as usize).min(sw.saturating_sub(DCTX_W));
        let y = (self.y as usize).min(sh.saturating_sub(DCTX_H));
        Rect { x, y, w: DCTX_W, h: DCTX_H }
    }

    fn item_rect(&self, i: usize, sw: usize, sh: usize) -> Rect {
        let r = self.rect(sw, sh);
        Rect { x: r.x + 1, y: r.y + 2 + i * DCTX_ITEM_H, w: r.w - 2, h: DCTX_ITEM_H }
    }

    fn hit_item(&self, mx: i32, my: i32, sw: usize, sh: usize) -> Option<usize> {
        for i in 0..DCTX_ITEMS {
            let ir = self.item_rect(i, sw, sh);
            if mx >= ir.x as i32 && mx < (ir.x + ir.w) as i32
                && my >= ir.y as i32 && my < (ir.y + ir.h) as i32
            { return Some(i); }
        }
        None
    }
}

// ── Damage tracking ───────────────────────────────────────────────────────────

#[derive(Copy, Clone)]
struct Rect { x: usize, y: usize, w: usize, h: usize }

impl Rect {
    const ZERO: Self = Rect { x: 0, y: 0, w: 0, h: 0 };

    fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w
            && self.y < o.y + o.h && o.y < self.y + self.h
    }

    fn union(&self, o: &Rect) -> Rect {
        if self.w == 0 || self.h == 0 { return *o; }
        if o.w == 0 || o.h == 0 { return *self; }
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = (self.x + self.w).max(o.x + o.w);
        let y1 = (self.y + self.h).max(o.y + o.h);
        Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }

    fn clip(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        if x0 >= x1 || y0 >= y1 { Rect::ZERO } else { Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 } }
    }

    fn is_empty(&self) -> bool { self.w == 0 || self.h == 0 }
}

struct DamageList {
    rects: [Rect; MAX_DAMAGE],
    count: usize,
    full: bool,
}

impl DamageList {
    fn new() -> Self { DamageList { rects: [Rect::ZERO; MAX_DAMAGE], count: 0, full: false } }
    fn clear(&mut self) { self.count = 0; self.full = false; }
    fn mark_full(&mut self) { self.full = true; }
    fn is_empty(&self) -> bool { !self.full && self.count == 0 }

    fn add(&mut self, r: Rect) {
        if r.w == 0 || r.h == 0 || self.full { return; }
        for i in 0..self.count {
            if self.rects[i].intersects(&r) {
                self.rects[i] = self.rects[i].union(&r);
                self.cascade(i);
                return;
            }
        }
        if self.count < MAX_DAMAGE {
            self.rects[self.count] = r;
            self.count += 1;
        } else {
            self.full = true;
        }
    }

    fn cascade(&mut self, idx: usize) {
        let mut i = 0;
        while i < self.count {
            if i != idx && self.rects[idx].intersects(&self.rects[i]) {
                self.rects[idx] = self.rects[idx].union(&self.rects[i]);
                self.count -= 1;
                if i < self.count { self.rects[i] = self.rects[self.count]; }
            } else { i += 1; }
        }
    }
}

// ── Cursor shapes ─────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq)]
enum CursorShape { Arrow, Move, ResizeH, ResizeV, ResizeDiagA, ResizeDiagB, Hand }

const CURSOR_ARROW: [[u8; CURSOR_W]; CURSOR_H] = [
    [1,0,0,0,0,0,0,0,0,0],
    [1,1,0,0,0,0,0,0,0,0],
    [1,2,1,0,0,0,0,0,0,0],
    [1,2,2,1,0,0,0,0,0,0],
    [1,2,2,2,1,0,0,0,0,0],
    [1,2,2,2,2,1,0,0,0,0],
    [1,2,2,2,2,2,1,0,0,0],
    [1,2,2,2,2,2,2,1,0,0],
    [1,2,2,2,1,1,0,0,0,0],
    [1,0,1,2,2,1,0,0,0,0],
    [0,0,0,1,2,1,0,0,0,0],
    [0,0,0,1,1,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
];

const CURSOR_MOVE: [[u8; CURSOR_W]; CURSOR_H] = [
    [0,0,0,0,0,1,0,0,0,0],
    [0,0,0,0,1,2,1,0,0,0],
    [0,0,0,1,2,2,2,1,0,0],
    [0,0,0,0,0,1,0,0,0,0],
    [0,1,0,0,0,1,0,0,1,0],
    [1,2,1,1,1,2,1,1,2,1],
    [0,1,0,0,0,1,0,0,1,0],
    [0,0,0,0,0,1,0,0,0,0],
    [0,0,0,1,2,2,2,1,0,0],
    [0,0,0,0,1,2,1,0,0,0],
    [0,0,0,0,0,1,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
];

const CURSOR_RESIZE_H: [[u8; CURSOR_W]; CURSOR_H] = [
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,1,0,0,0,0,0,0,1,0],
    [1,2,1,1,1,1,1,1,2,1],
    [0,1,0,0,0,0,0,0,1,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
];

const CURSOR_RESIZE_V: [[u8; CURSOR_W]; CURSOR_H] = [
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,1,0,0,0,0,0],
    [0,0,0,1,2,1,0,0,0,0],
    [0,0,1,2,2,2,1,0,0,0],
    [0,0,0,0,1,0,0,0,0,0],
    [0,0,0,0,1,0,0,0,0,0],
    [0,0,0,0,1,0,0,0,0,0],
    [0,0,0,0,1,0,0,0,0,0],
    [0,0,0,0,1,0,0,0,0,0],
    [0,0,0,0,1,0,0,0,0,0],
    [0,0,1,2,2,2,1,0,0,0],
    [0,0,0,1,2,1,0,0,0,0],
    [0,0,0,0,1,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
];

const CURSOR_DIAG_A: [[u8; CURSOR_W]; CURSOR_H] = [
    [1,1,1,1,1,0,0,0,0,0],
    [1,2,2,2,1,0,0,0,0,0],
    [1,2,1,0,0,0,0,0,0,0],
    [1,2,0,1,0,0,0,0,0,0],
    [1,1,0,0,1,0,0,0,0,0],
    [0,0,0,0,0,1,0,0,1,1],
    [0,0,0,0,0,0,1,0,2,1],
    [0,0,0,0,0,0,0,1,2,1],
    [0,0,0,0,0,0,2,2,2,1],
    [0,0,0,0,0,1,1,1,1,1],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
];

const CURSOR_DIAG_B: [[u8; CURSOR_W]; CURSOR_H] = [
    [0,0,0,0,0,1,1,1,1,1],
    [0,0,0,0,0,1,2,2,2,1],
    [0,0,0,0,0,0,0,1,2,1],
    [0,0,0,0,0,0,1,0,2,1],
    [0,0,0,0,0,1,0,0,1,1],
    [1,1,0,0,1,0,0,0,0,0],
    [1,2,0,1,0,0,0,0,0,0],
    [1,2,1,0,0,0,0,0,0,0],
    [1,2,2,2,1,0,0,0,0,0],
    [1,1,1,1,1,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
];

const CURSOR_HAND: [[u8; CURSOR_W]; CURSOR_H] = [
    [0,0,0,1,0,0,0,0,0,0],
    [0,0,1,2,1,0,0,0,0,0],
    [0,0,1,2,1,0,0,0,0,0],
    [0,0,1,2,1,0,0,0,0,0],
    [0,0,1,2,1,1,1,0,0,0],
    [0,0,1,2,1,2,1,1,0,0],
    [0,0,1,2,1,2,1,2,1,0],
    [0,1,2,2,2,2,2,2,2,1],
    [0,1,2,2,2,2,2,2,2,1],
    [0,1,2,2,2,2,2,2,2,1],
    [0,0,1,2,2,2,2,2,1,0],
    [0,0,0,1,2,2,2,1,0,0],
    [0,0,0,0,1,2,1,0,0,0],
    [0,0,0,0,0,1,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0],
];

fn cursor_bitmap(shape: CursorShape) -> &'static [[u8; CURSOR_W]; CURSOR_H] {
    match shape {
        CursorShape::Arrow      => &CURSOR_ARROW,
        CursorShape::Move       => &CURSOR_MOVE,
        CursorShape::ResizeH    => &CURSOR_RESIZE_H,
        CursorShape::ResizeV    => &CURSOR_RESIZE_V,
        CursorShape::ResizeDiagA => &CURSOR_DIAG_A,
        CursorShape::ResizeDiagB => &CURSOR_DIAG_B,
        CursorShape::Hand       => &CURSOR_HAND,
    }
}

// ── Unified app registry ─────────────────────────────────────────────────────
//
// Single source of truth for all launch paths.
// `app_id`   – must match App::app_id() in the corresponding module.
// `label`    – shown on desktop icon, launcher title, and taskbar button.
// `icon_sub` – short subtitle drawn inside the icon cell (≤10 chars).
// `desc`     – one-line description shown in the launcher panel.

struct AppDesc {
    app_id:   &'static str,
    label:    &'static str,
    icon_sub: &'static str,
    desc:     &'static str,
    /// Factory: creates a fresh instance of this app.
    /// The compositor calls this every time the icon is activated.
    make:     fn() -> Box<dyn App>,
}

const APP_REGISTRY: [AppDesc; NUM_APPS] = [
    AppDesc { app_id: "terminal",    label: "Terminal",    icon_sub: "Shell",    desc: "Open a shell",     make: || Box::new(TerminalApp::new())     },
    AppDesc { app_id: "filemanager", label: "This PC",     icon_sub: "ThisPc",   desc: "Browse files",    make: || Box::new(FileManagerApp::new())  },
    AppDesc { app_id: "settings",    label: "Settings",    icon_sub: "Config",   desc: "Preferences",     make: || Box::new(SettingsApp::new())     },
    AppDesc { app_id: "sysmonitor",  label: "Sys Monitor", icon_sub: "Stats",    desc: "Performance",     make: || Box::new(SysMonitorApp::new())   },
    AppDesc { app_id: "calculator",  label: "Calculator",  icon_sub: "Calc",     desc: "4-function calc", make: || Box::new(CalculatorApp::new())   },
    AppDesc { app_id: "imageviewer", label: "Viewer",      icon_sub: "Images",   desc: "View PPM images", make: || Box::new(ImageViewerApp::new())  },
    AppDesc { app_id: "notes",       label: "Notes",       icon_sub: "Notepad",  desc: "Scratchpad",      make: || Box::new(NotesApp::new())        },
    AppDesc { app_id: "logviewer",   label: "Log Viewer",  icon_sub: "Logs",     desc: "Kernel log",      make: || Box::new(LogViewerApp::new())    },
    AppDesc { app_id: "about",       label: "About",       icon_sub: "Info",     desc: "About Astra OS",  make: || Box::new(AboutApp::new())        },
    AppDesc { app_id: "snake",       label: "Snake",       icon_sub: "Game",     desc: "Classic snake",   make: || Box::new(SnakeApp::new())        },
    AppDesc { app_id: "tetris",      label: "Tetris",      icon_sub: "Game",     desc: "Classic Tetris",  make: || Box::new(TetrisApp::new())       },
];

// ── Desktop icons ─────────────────────────────────────────────────────────────

struct DesktopIcon {
    x: i32,
    y: i32,
    selected: bool,
    last_click_ms: u64,
}

fn icon_rect(row: usize, bar_h: usize) -> Rect {
    Rect {
        x: ICON_GRID_X,
        y: bar_h + ICON_GRID_Y + row * (ICON_CELL_H + 6),
        w: ICON_CELL_W,
        h: ICON_CELL_H,
    }
}

fn icon_rect_of(icon: &DesktopIcon) -> Rect {
    Rect { x: icon.x as usize, y: icon.y as usize, w: ICON_CELL_W, h: ICON_CELL_H }
}

fn launcher_item_rect(i: usize) -> Rect {
    Rect {
        x: 0,
        y: BAR_H + LAUNCHER_HEAD_H + i * LAUNCHER_ITEM_H,
        w: LAUNCHER_W,
        h: LAUNCHER_ITEM_H,
    }
}

fn launcher_rect(sh: usize) -> Rect {
    Rect { x: 0, y: BAR_H, w: LAUNCHER_W, h: sh.saturating_sub(BAR_H) }
}

// ── App pixel-art icons ───────────────────────────────────────────────────────
//
// Each icon is drawn with fill_rect only, inside a 44×28 area centred in the
// icon cell above the label text. Origin passed as (ix, iy).
//
//  0 – Terminal     : monitor frame with ">_" prompt
//  1 – This PC      : computer monitor with stand
//  2 – Settings     : gear with cardinal + diagonal teeth
//  3 – Sys Monitor  : bar chart with Y/X axes
//  4 – Calculator   : frame with display + button grid
//  5 – Image Viewer : picture frame with sky/mountain scene
//  6 – Notes        : notepad with binding strip + ruled lines
//  7 – Log Viewer   : monitor with scrolling log lines
//  8 – About        : info “i” badge
//  9 – Snake        : game grid with S-shaped snake + apple
// 10 – Tetris        : coloured block stack in a well
// 100 – Desktop File   : document page with folded corner + text lines
// 101 – Desktop Folder : classic folder shape with tab and depth shadow

fn draw_app_icon(idx: usize, r: Rect) {
    // Centre the 44×28 drawing area horizontally; start 7px below accent strip.
    let iw: usize = 44;
    let ih: usize = 28;
    let ix = r.x + (r.w.saturating_sub(iw)) / 2;
    let iy = r.y + 7;

    match idx {
        // ── 0: Terminal ───────────────────────────────────────────────────
        // Outer monitor frame (dark blue-grey)
        // Inner screen (deep green-black)
        // ">_" prompt in green
        0 => {
            const FRAME:  u32 = 0x2A4060;
            const SCREEN: u32 = 0x061008;
            const GREEN:  u32 = 0x00CC66;
            const CURSOR: u32 = 0x00FF88;
            // Monitor frame
            framebuffer::fill_rect(ix,        iy,        iw,     ih - 4, FRAME);
            // Screen inset (2px border)
            framebuffer::fill_rect(ix + 2,    iy + 2,    iw - 4, ih - 8, SCREEN);
            // Stand neck
            framebuffer::fill_rect(ix + iw/2 - 3, iy + ih - 4, 6, 2, FRAME);
            // Stand base
            framebuffer::fill_rect(ix + iw/2 - 7, iy + ih - 2, 14, 2, FRAME);
            // ">" chevron — two diagonal 2×2 pixel blocks each side
            let px = ix + 4;
            let py = iy + 6;
            framebuffer::fill_rect(px,     py,     2, 2, GREEN); // top-left arm
            framebuffer::fill_rect(px + 2, py + 2, 2, 2, GREEN); // point top
            framebuffer::fill_rect(px,     py + 4, 2, 2, GREEN); // bottom-left arm
            // underscore cursor (blinks visually via colour difference)
            framebuffer::fill_rect(px + 4, py + 4, 6, 2, CURSOR);
        }

        // ── 1: This PC (Computer monitor) ────────────────────────────────
        1 => {
            const FRAME:  u32 = 0x2A5080;
            const SCREEN: u32 = 0x0A1828;
            const GLOW:   u32 = 0x1A6090;
            const STAND:  u32 = 0x1E3A58;
            const BASE:   u32 = 0x182E48;
            // Monitor outer frame
            framebuffer::fill_rect(ix,         iy,         iw,     ih - 6, FRAME);
            // Screen inset
            framebuffer::fill_rect(ix + 2,     iy + 2,     iw - 4, ih - 10, SCREEN);
            // Screen glow line at top of screen
            framebuffer::fill_rect(ix + 2,     iy + 2,     iw - 4, 2, GLOW);
            // Stand neck
            framebuffer::fill_rect(ix + iw/2 - 2, iy + ih - 6, 4, 3, STAND);
            // Stand base
            framebuffer::fill_rect(ix + iw/2 - 8, iy + ih - 3, 16, 3, BASE);
            // Small HDD cylinder at bottom-right of screen
            framebuffer::fill_rect(ix + iw - 10, iy + ih - 12, 6, 4, STAND);
            framebuffer::fill_rect(ix + iw - 9,  iy + ih - 13, 4, 2, FRAME);
        }

        // ── 2: Settings (Gear) ────────────────────────────────────────────
        // Hub square + 4 cardinal teeth + 4 diagonal teeth + dark hole
        2 => {
            const GEAR:  u32 = 0x5090C8;
            const GEAR2: u32 = 0x70B0E0;
            const HOLE:  u32 = 0x0A1020;
            let cx = ix + iw / 2;
            let cy = iy + ih / 2 - 1;
            // Cardinal teeth (wider)
            framebuffer::fill_rect(cx - 3, cy - 11, 6, 5, GEAR);  // top
            framebuffer::fill_rect(cx - 3, cy + 6,  6, 5, GEAR);  // bottom
            framebuffer::fill_rect(cx - 11, cy - 3, 5, 6, GEAR);  // left
            framebuffer::fill_rect(cx + 6,  cy - 3, 5, 6, GEAR);  // right
            // Hub body
            framebuffer::fill_rect(cx - 7, cy - 7, 14, 14, GEAR);
            // Diagonal teeth (narrower)
            framebuffer::fill_rect(cx - 9,  cy - 9,  4, 4, GEAR2);
            framebuffer::fill_rect(cx + 5,  cy - 9,  4, 4, GEAR2);
            framebuffer::fill_rect(cx - 9,  cy + 5,  4, 4, GEAR2);
            framebuffer::fill_rect(cx + 5,  cy + 5,  4, 4, GEAR2);
            // Centre hole
            framebuffer::fill_rect(cx - 3, cy - 3, 6, 6, HOLE);
        }

        // ── 3: Sys Monitor (Bar chart) ────────────────────────────────────
        // Y axis + X axis + 4 bars at different heights
        3 => {
            const AXIS:  u32 = 0x304860;
            const BAR1:  u32 = 0x00A8C0;
            const BAR2:  u32 = 0x00C8A0;
            const BAR3:  u32 = 0x0080E0;
            const BAR4:  u32 = 0x40D0FF;
            let base_y = iy + ih - 4; // X-axis y
            // Y axis
            framebuffer::fill_rect(ix + 2, iy + 1, 2, ih - 4, AXIS);
            // X axis
            framebuffer::fill_rect(ix + 2, base_y - 2, iw - 4, 2, AXIS);
            // Bar 1 — medium
            framebuffer::fill_rect(ix + 6,  base_y - 10, 6, 8, BAR1);
            // Bar 2 — tall
            framebuffer::fill_rect(ix + 14, base_y - 16, 6, 14, BAR2);
            // Bar 3 — short
            framebuffer::fill_rect(ix + 22, base_y -  8, 6, 6, BAR3);
            // Bar 4 — tallest
            framebuffer::fill_rect(ix + 30, base_y - 20, 6, 18, BAR4);
        }

        // ── 4: Calculator — grid of buttons with "=" accent ──────────────
        4 => {
            const FRAME:  u32 = 0x1A2F48;
            const BTN_C:  u32 = 0x243448;
            const BTN_OP: u32 = 0x1A3A5F;
            const BTN_EQ: u32 = 0x1A5F3F;
            const DIGIT:  u32 = 0x90B8D8;
            // Calculator body
            framebuffer::fill_rect(ix, iy, iw, ih, FRAME);
            // Display strip
            framebuffer::fill_rect(ix + 2, iy + 2, iw - 4, 7, 0x060B10);
            // "=" character hint in display
            framebuffer::fill_rect(ix + iw - 8, iy + 3, 4, 1, DIGIT);
            framebuffer::fill_rect(ix + iw - 8, iy + 5, 4, 1, DIGIT);
            // Button grid: 3 cols × 3 rows of small squares
            for row in 0..3usize {
                for col in 0..3usize {
                    let bx = ix + 2 + col * 8;
                    let by = iy + 11 + row * 6;
                    let bg = if col == 2 && row == 2 { BTN_EQ }
                             else if col == 2 { BTN_OP }
                             else { BTN_C };
                    framebuffer::fill_rect(bx, by, 6, 4, bg);
                }
            }
        }

        // ── 5: Image Viewer — picture frame with a sun/mountain scene ────
        5 => {
            const FRAME_COL:  u32 = 0x2A3A50;
            const SKY:        u32 = 0x0A2040;
            const GROUND:     u32 = 0x1A3020;
            const SUN:        u32 = 0xF0B030;
            const MTN:        u32 = 0x304858;
            // Outer frame border
            framebuffer::fill_rect(ix,     iy,     iw,     ih,     FRAME_COL);
            // Image area inset
            framebuffer::fill_rect(ix + 2, iy + 2, iw - 4, ih - 4, SKY);
            // Ground strip
            framebuffer::fill_rect(ix + 2, iy + ih - 8, iw - 4, 6, GROUND);
            // Sun (small square)
            framebuffer::fill_rect(ix + iw - 10, iy + 4, 5, 5, SUN);
            // Mountain silhouette (two triangles via diagonal lines approximation)
            for row in 0..8usize {
                let w2 = (row * 2).min(iw - 4);
                framebuffer::fill_rect(ix + 2 + (iw - 4).saturating_sub(w2) / 2, iy + (ih - 8) - row - 2, w2, 1, MTN);
            }
        }

        // ── 6: Notes — notepad with lines ────────────────────────────────
        6 => {
            const PAGE:  u32 = 0x0E1E14;
            const RULE:  u32 = 0x1A4028;
            const CURL:  u32 = 0x2A6040;
            const BIND:  u32 = 0x0A1810;
            // Page background
            framebuffer::fill_rect(ix, iy, iw, ih, PAGE);
            // Binding strip on left
            framebuffer::fill_rect(ix, iy, 6, ih, BIND);
            // Ruled lines
            for row in 0..5usize {
                framebuffer::fill_rect(ix + 8, iy + 6 + row * 4, iw - 10, 1, RULE);
            }
            // Corner curl (top-right triangle approximation)
            framebuffer::fill_rect(ix + iw - 6, iy,     6, 2, CURL);
            framebuffer::fill_rect(ix + iw - 4, iy + 2, 4, 2, CURL);
            framebuffer::fill_rect(ix + iw - 2, iy + 4, 2, 2, CURL);
        }

        // ── 7: Log Viewer — terminal scroll output ────────────────────────
        7 => {
            const FRAME:  u32 = 0x0C1810;
            const SCREEN: u32 = 0x050C06;
            const LINE1:  u32 = 0x206030;
            const LINE2:  u32 = 0x184028;
            const PROMPT: u32 = 0x30A050;
            // Monitor frame
            framebuffer::fill_rect(ix,        iy,        iw,     ih - 4, FRAME);
            // Screen inset
            framebuffer::fill_rect(ix + 2,    iy + 2,    iw - 4, ih - 8, SCREEN);
            // Log lines  (alternating green shades)
            for row in 0..5usize {
                let col = if row % 2 == 0 { LINE1 } else { LINE2 };
                framebuffer::fill_rect(ix + 4, iy + 4 + row * 3, iw - 8, 2, col);
            }
            // Prompt at bottom
            framebuffer::fill_rect(ix + 4, iy + ih - 8, 8, 2, PROMPT);
            // Stand
            framebuffer::fill_rect(ix + iw/2 - 3, iy + ih - 4, 6, 2, FRAME);
            framebuffer::fill_rect(ix + iw/2 - 7, iy + ih - 2, 14, 2, FRAME);
        }

        // ── 8: About — stylised info "i" badge ───────────────────────────
        8 => {
            const BADGE:  u32 = 0x0C2040;
            const RING:   u32 = 0x1E5090;
            const LETTER: u32 = 0x80C0FF;
            const DOT:    u32 = 0xA0D8FF;
            let cx = ix + iw / 2;
            let cy = iy + ih / 2 - 1;
            // Outer ring (circle approx via concentric rects)
            framebuffer::fill_rect(cx - 10, cy - 12, 20, 24, RING);
            framebuffer::fill_rect(cx - 8,  cy - 10, 16, 20, BADGE);
            // "i" dot
            framebuffer::fill_rect(cx - 2, cy - 7, 4, 4, DOT);
            // "i" stem
            framebuffer::fill_rect(cx - 2, cy - 1, 4, 9, LETTER);
            // Serif base
            framebuffer::fill_rect(cx - 4, cy + 8, 8, 2, LETTER);
        }

        // ── 9: Snake — winding snake on a dark grid ───────────────────────
        9 => {
            const GRID_BG:  u32 = 0x060B06;
            const GRID_L:   u32 = 0x0C150C;
            const S_HEAD:   u32 = 0x50F870;
            const S_BODY:   u32 = 0x28A840;
            const APPLE:    u32 = 0xFF4444;
            // Grid background
            framebuffer::fill_rect(ix, iy, iw, ih, GRID_BG);
            // Grid lines (4x4 subcells)
            for g in 0..5usize {
                framebuffer::fill_rect(ix, iy + g * 5, iw, 1, GRID_L);
                framebuffer::fill_rect(ix + g * 8, iy, 1, ih, GRID_L);
            }
            // Snake body — S-shaped winding path
            // Row 1 (left to right)
            framebuffer::fill_rect(ix + 2,  iy + 2,  20, 3, S_BODY);
            // Turn down right side
            framebuffer::fill_rect(ix + 19, iy + 2,  3, 9, S_BODY);
            // Row 2 (right to left)
            framebuffer::fill_rect(ix + 4,  iy + 8,  18, 3, S_BODY);
            // Turn down left side
            framebuffer::fill_rect(ix + 2,  iy + 8,  3, 9, S_BODY);
            // Row 3 (left to right) — tail
            framebuffer::fill_rect(ix + 2,  iy + 14, 14, 3, S_BODY);
            // Head (brighter, at end of row 1)
            framebuffer::fill_rect(ix + 2,  iy + 2,  5, 3, S_HEAD);
            // Apple
            framebuffer::fill_rect(ix + iw - 8, iy + ih - 8, 5, 5, APPLE);
        }

        // ── 10: Tetris — stacked coloured blocks in a well ────────────────
        10 => {
            const WELL:  u32 = 0x060810;
            const WALL:  u32 = 0x1A2A3A;
            const C1:    u32 = 0x00B8D8;  // cyan  (I-piece)
            const C2:    u32 = 0xE8B000;  // yellow (O-piece)
            const C3:    u32 = 0xB000D8;  // purple (T-piece)
            const C4:    u32 = 0x00C840;  // green  (S-piece)
            const C5:    u32 = 0xE04000;  // red    (Z-piece)
            // Well background
            framebuffer::fill_rect(ix + 4, iy, iw - 8, ih, WELL);
            // Well walls
            framebuffer::fill_rect(ix,     iy, 4,      ih, WALL);
            framebuffer::fill_rect(ix + iw - 4, iy, 4, ih, WALL);
            // Block size = 5x4 with 1px gap
            // Row 3 (bottom) — full row: cyan + yellow
            framebuffer::fill_rect(ix + 5,  iy + ih - 5,  8, 4, C1);
            framebuffer::fill_rect(ix + 14, iy + ih - 5,  8, 4, C2);
            framebuffer::fill_rect(ix + 23, iy + ih - 5,  8, 4, C1);
            // Row 2 — partial: purple + green
            framebuffer::fill_rect(ix + 5,  iy + ih - 10, 8, 4, C3);
            framebuffer::fill_rect(ix + 14, iy + ih - 10, 8, 4, C4);
            // Row 1 — sparse: red block + falling piece
            framebuffer::fill_rect(ix + 5,  iy + ih - 15, 8, 4, C5);
            // Falling I-piece (top, centred)
            framebuffer::fill_rect(ix + 14, iy + 2,       8, 4, C1);
            framebuffer::fill_rect(ix + 14, iy + 7,       8, 4, C1);
        }

        // ── 100: Desktop File — document page with folded corner ────────
        100 => {
            const PAGE:  u32 = 0x0E1E30;
            const FOLD:  u32 = 0x060C18;
            const LINE:  u32 = 0x2A5888;
            const LINE2: u32 = 0x1E3A60;
            // Page body (slightly narrower than full iw to look like paper)
            framebuffer::fill_rect(ix,          iy,          iw - 8, ih, PAGE);
            // Right edge (below fold)
            framebuffer::fill_rect(ix + iw - 8, iy + 8,     8, ih - 8, PAGE);
            // Folded corner — dark triangle approximation
            framebuffer::fill_rect(ix + iw - 8, iy,         8, 8, FOLD);
            framebuffer::fill_rect(ix + iw - 8, iy,         2, 8, PAGE);  // edge of fold
            framebuffer::fill_rect(ix + iw - 8, iy + 6,     8, 2, PAGE);  // bottom of fold
            // Text lines
            framebuffer::fill_rect(ix + 4, iy + 6,  iw - 18, 2, LINE);
            framebuffer::fill_rect(ix + 4, iy + 11, iw - 18, 2, LINE);
            framebuffer::fill_rect(ix + 4, iy + 16, iw - 18, 2, LINE2);
            framebuffer::fill_rect(ix + 4, iy + 21, iw - 22, 2, LINE2);
        }

        // ── 101: Desktop Folder — classic folder with tab and shadow ──────
        101 => {
            const BODY:  u32 = 0x7A4C10;
            const BODY2: u32 = 0x9A6418;
            const TAB:   u32 = 0xF0B830;
            const SHAD:  u32 = 0x3A2008;
            const EDGE:  u32 = 0xC08020;
            // Shadow offset
            framebuffer::fill_rect(ix + 3,  iy + 9,  iw - 2, ih - 9, SHAD);
            // Folder tab (top-left flap)
            framebuffer::fill_rect(ix,      iy + 4,  18, 5, TAB);
            // Main body
            framebuffer::fill_rect(ix,      iy + 9,  iw - 3, ih - 9, BODY);
            // Top highlight edge
            framebuffer::fill_rect(ix,      iy + 9,  iw - 3, 2, EDGE);
            // Inner lighter area (open folder depth suggestion)
            framebuffer::fill_rect(ix + 4,  iy + 13, iw - 11, ih - 18, BODY2);
        }

        _ => {}
    }
}

// ── Window ────────────────────────────────────────────────────────────────────

struct Window {
    x: i32,
    y: i32,
    w: usize,
    h: usize,
    minimized: bool,
    last_refresh_ms: u64,
    app: Box<dyn App>,
    // ── Surface cache ─────────────────────────────────────────────────
    // Stores the last-rendered client-area pixels (row-major, ARGB32).
    // When valid, compose_damage can blit this instead of calling app.render(),
    // which eliminates redundant text/background rendering during drag/resize.
    cached_surface: Vec<u32>,
    surface_valid: bool,      // false = must call app.render() and re-capture
    surface_w: usize,         // client width at capture time
    surface_h: usize,         // client height at capture time
    surface_needs_capture: bool, // set after a full render; triggers read_rect capture
}

impl Window {
    fn client_rect(&self) -> Rect {
        let x = (self.x.max(0) as usize) + 1;
        let y = (self.y.max(0) as usize) + WIN_BAR_H + 1;
        let w = self.w.saturating_sub(2);
        let h = self.h.saturating_sub(WIN_BAR_H + 2);
        Rect { x, y, w, h }
    }

    fn bounds(&self) -> Rect {
        Rect {
            x: self.x.max(0) as usize,
            y: self.y.max(0) as usize,
            w: self.w + WIN_SHADOW_OFS,
            h: self.h + WIN_SHADOW_OFS,
        }
    }

    fn close_btn_rect(&self) -> Rect {
        let wx = self.x.max(0) as usize;
        let wy = self.y.max(0) as usize;
        let bw = 16usize; let bh = 16usize;
        let bx = wx + self.w.saturating_sub(bw + 6);
        let by = wy + (WIN_BAR_H.saturating_sub(bh)) / 2;
        Rect { x: bx, y: by, w: bw, h: bh }
    }
}

// ── Resize zone ───────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq)]
enum ResizeZone { TL, T, TR, R, BR, B, BL, L }

impl ResizeZone {
    fn cursor_shape(self) -> CursorShape {
        match self {
            ResizeZone::TL | ResizeZone::BR => CursorShape::ResizeDiagA,
            ResizeZone::TR | ResizeZone::BL => CursorShape::ResizeDiagB,
            ResizeZone::L  | ResizeZone::R  => CursorShape::ResizeH,
            ResizeZone::T  | ResizeZone::B  => CursorShape::ResizeV,
        }
    }
}

fn hit_resize_zone(win: &Window, mx: i32, my: i32) -> Option<ResizeZone> {
    let wx = win.x; let wy = win.y;
    let ww = win.w as i32; let wh = win.h as i32;
    let z = RESIZE_ZONE as i32;
    if mx < wx || mx > wx + ww || my < wy || my > wy + wh { return None; }
    let lft = mx - wx < z;
    let rgt = wx + ww - mx < z;
    let top = my - wy < z;
    let bot = wy + wh - my < z;
    if !lft && !rgt && !top && !bot { return None; }
    if top && my - wy < WIN_BAR_H as i32 { return None; }
    match (lft, rgt, top, bot) {
        (true,  false, true,  false) => Some(ResizeZone::TL),
        (false, false, true,  false) => Some(ResizeZone::T),
        (false, true,  true,  false) => Some(ResizeZone::TR),
        (false, true,  false, false) => Some(ResizeZone::R),
        (false, true,  false, true ) => Some(ResizeZone::BR),
        (false, false, false, true ) => Some(ResizeZone::B),
        (true,  false, false, true ) => Some(ResizeZone::BL),
        (true,  false, false, false) => Some(ResizeZone::L),
        _                            => None,
    }
}

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

const MAX_DESK_ITEMS:  usize = 32;
const DI_W:            usize = ICON_CELL_W; // match dock icon cell width (88)
const DI_H:            usize = ICON_CELL_H; // match dock icon cell height (78)
const DI_ICON_FILE:    usize = 100;         // draw_app_icon index for generic file
const DI_ICON_DIR:     usize = 101;         // draw_app_icon index for folder
const DI_SEL_BG:       u32   = ICON_SEL;    // same selection colour as dock
const DI_TEXT:         u32   = ICON_TEXT;
const DI_SEL_TEXT:     u32   = ICON_TEXT_SEL;

// Desktop name-entry prompt (shown when user picks "New File" / "New Folder")
const DP_W:    usize = 140;
const DP_H:    usize = 36;
const DP_BG:   u32   = 0x08121E;
const DP_BORD: u32   = 0x2A6090;
const DP_LBL:  u32   = 0x4A90B8;
const DP_TEXT: u32   = 0xD8EEFF;
const DP_CUR:  u32   = 0x60AADD;

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
        DesktopItem { x: 0, y: 0, name: [0u8; 32], nlen: 0,
                      is_dir: false, fat32_cluster: 0, selected: false, last_click_ms: 0 }
    }
    fn rect(&self) -> Rect {
        Rect { x: self.x as usize, y: self.y as usize, w: DI_W, h: DI_H }
    }
}

struct DesktopNamePrompt {
    active: bool,
    spawn_x: i32,   // right-click position where we'll place the item
    spawn_y: i32,
    is_dir: bool,
    buf: [u8; 32],
    len: usize,
}

impl DesktopNamePrompt {
    const fn hidden() -> Self {
        DesktopNamePrompt { active: false, spawn_x: 0, spawn_y: 0,
                            is_dir: false, buf: [0u8; 32], len: 0 }
    }
    /// Bounding rect of the rendered prompt box (clamped to screen).
    fn rect(&self, sw: usize, sh: usize) -> Rect {
        let x = (self.spawn_x as usize).min(sw.saturating_sub(DP_W));
        let y = (self.spawn_y as usize).min(sh.saturating_sub(DP_H + 14));
        Rect { x, y: y + 14, w: DP_W, h: DP_H }
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
    start_mx: i32, start_my: i32,
    start_x: i32,  start_y: i32,
    start_w: usize, start_h: usize,
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
        Rect { x: BTN_START_X + idx * (BTN_W + BTN_GAP), y, w: BTN_W, h: BTN_H }
    } else {
        let wi = idx - FIXED_BTNS;
        Rect { x: WIN_BTN_START_X + wi * (BTN_W + BTN_GAP), y, w: BTN_W, h: BTN_H }
    }
}

const POWER_BTN_W: usize = 28;
const POWER_BTN_MARGIN: usize = 6;

fn power_btn_rect(sw: usize) -> Rect {
    let y = (BAR_H - BTN_H) / 2;
    Rect { x: sw.saturating_sub(POWER_BTN_W + POWER_BTN_MARGIN), y, w: POWER_BTN_W, h: BTN_H }
}

// ── Desktop struct ────────────────────────────────────────────────────────────

struct Desktop {
    sw: usize,
    sh: usize,
    windows: Vec<Window>,
    focused: Option<usize>,
    app_hover_target: Option<usize>,
    cursor_x: i32,
    cursor_y: i32,
    cursor_shape: CursorShape,
    drag: Option<DragState>,
    resize: Option<ResizeState>,
    icon_drag: Option<IconDragState>,
    launcher_open: bool,
    launcher_hover: Option<usize>,
    icons: [DesktopIcon; NUM_ICONS],
    icon_hover: Option<usize>,
    taskbar_hover: Option<usize>,
    close_hover: Option<usize>,
    cascade_x: i32,
    cascade_y: i32,
    damage: DamageList,
    cursor_under: [u32; CURSOR_W * CURSOR_H],
    cursor_drawn_x: i32,
    cursor_drawn_y: i32,
    cursor_on_screen: bool,
    prev_btn_state: u8,
    dctx: DesktopCtxMenu,
    desk_items: [DesktopItem; MAX_DESK_ITEMS],
    desk_item_count: usize,
    desk_prompt: DesktopNamePrompt,
    desk_item_drag: Option<DeskItemDrag>,
    // ── Drag rate-limiter ─────────────────────────────────────────────────
    // Accumulates the union of all window positions (old + new) that have not
    // yet been composited.  Flushed to the damage list at most every 16 ms
    // (~60 fps cap) so QEMU's VGA emulation is not overwhelmed by 100 Hz
    // MMIO storms.  Reset to Some(latest_bounds) after each present so the
    // next frame correctly erases the last-drawn position.
    drag_damage_accum: Option<Rect>,
    last_drag_present_ms: u64,
}

impl Desktop {
    fn new(sw: usize, sh: usize) -> Self {
        Desktop {
            sw, sh,
            windows: Vec::new(),
            focused: None,
            app_hover_target: None,
            cursor_x: (sw / 2) as i32,
            cursor_y: (sh / 2) as i32,
            cursor_shape: CursorShape::Arrow,
            drag: None, resize: None, icon_drag: None,
            launcher_open: false,
            launcher_hover: None,
            icons: {
                let mk = |row: usize| {
                    let r = icon_rect(row, BAR_H);
                    DesktopIcon { x: r.x as i32, y: r.y as i32, selected: false, last_click_ms: 0 }
                };
                [mk(0), mk(1), mk(2), mk(3), mk(4), mk(5), mk(6), mk(7), mk(8), mk(9), mk(10)]
            },
            icon_hover: None,
            taskbar_hover: None,
            close_hover: None,
            cascade_x: 120,
            cascade_y: BAR_H as i32 + 20,
            damage: DamageList::new(),
            cursor_under: [0u32; CURSOR_W * CURSOR_H],
            cursor_drawn_x: 0, cursor_drawn_y: 0,
            cursor_on_screen: false,
            prev_btn_state: 0,
            dctx: DesktopCtxMenu::hidden(),
            desk_items: [DesktopItem::blank(); MAX_DESK_ITEMS],
            desk_item_count: 0,
            desk_prompt: DesktopNamePrompt::hidden(),
            desk_item_drag: None,
            drag_damage_accum: None,
            last_drag_present_ms: 0,
        }
    }

    // ── Window management ─────────────────────────────────────────────────

    fn open_window(&mut self, app: Box<dyn App>) {
        // Singleton policy: if a window with this app_id is already open,
        // un-minimize it and bring it to the front instead of spawning again.
        if !app.allow_multiple_instances() {
            let id = app.app_id();
            for i in 0..self.windows.len() {
                if self.windows[i].app.app_id() == id {
                    if self.windows[i].minimized { self.windows[i].minimized = false; }
                    let b = self.windows[i].bounds();
                    self.damage.add(b);
                    self.raise_to_front(i);
                    self.focused = Some(self.windows.len() - 1);
                    self.damage.add(self.windows[self.windows.len() - 1].bounds());
                    return;
                }
            }
        }
        if self.windows.len() >= MAX_WINDOWS { return; }
        let (pw, ph) = app.preferred_size();
        let w = pw.min(self.sw.saturating_sub(40));
        let h = ph.min(self.sh.saturating_sub(BAR_H + 40));

        // Smart cascade placement:
        // - Both axes wrap together (avoids corner pile-up).
        // - Nudge forward if proposed position would exactly overlap an existing
        //   window's title bar (within 8 px).
        const CASCADE_STEP: i32 = 28;
        const CASCADE_BASE_X: i32 = 120;
        const CASCADE_BASE_Y: i32 = BAR_H as i32 + 20;
        let max_x = (self.sw.saturating_sub(w + 20)) as i32;
        let max_y = (self.sh.saturating_sub(h + 20)) as i32;

        let mut cx = self.cascade_x;
        let mut cy = self.cascade_y;
        // Wrap both axes together when either hits the edge.
        if cx > max_x || cy > max_y {
            cx = CASCADE_BASE_X;
            cy = CASCADE_BASE_Y;
        }
        // Nudge until no existing window title-bar overlaps within 8 px.
        for _ in 0..MAX_WINDOWS {
            let overlap = self.windows.iter().any(|win| {
                !win.minimized
                    && (win.x - cx).abs() < 8
                    && (win.y - cy).abs() < 8
            });
            if !overlap { break; }
            cx = (cx + CASCADE_STEP).min(max_x);
            cy = (cy + CASCADE_STEP).min(max_y);
        }

        self.cascade_x = cx + CASCADE_STEP;
        self.cascade_y = cy + CASCADE_STEP;

        let win = Window { x: cx, y: cy, w, h, minimized: false, last_refresh_ms: 0, app,
            cached_surface: Vec::new(), surface_valid: false,
            surface_w: 0, surface_h: 0, surface_needs_capture: false };
        let b = win.bounds();
        self.windows.push(win);
        self.focused = Some(self.windows.len() - 1);
        self.damage.add(b);
    }

    fn close_window(&mut self, idx: usize) {
        if idx >= self.windows.len() { return; }
        // Give the app a chance to intercept (e.g. unsaved-changes prompt)
        let action = self.windows[idx].app.request_close();
        if action != crate::app::AppAction::Close {
            self.handle_app_action(idx, action);
            return;
        }
        let b = self.windows[idx].bounds();
        self.damage.add(b);
        self.windows.remove(idx);
        // Clear any drag/resize that was tracking the removed window.
        if matches!(&self.drag,   Some(d) if d.win_idx == idx) { self.drag   = None; }
        if matches!(&self.resize, Some(r) if r.win_idx == idx) { self.resize = None; }
        if self.windows.is_empty() {
            self.focused = None;
        } else {
            self.focused = Some(self.windows.len() - 1);
        }
        self.damage.mark_full();
    }

    fn raise_to_front(&mut self, idx: usize) {
        if idx >= self.windows.len() || idx == self.windows.len() - 1 { return; }
        let win = self.windows.remove(idx);
        self.windows.push(win);
    }

    fn minimize_all(&mut self) {
        for w in &mut self.windows { w.minimized = true; }
        self.focused = None;
        self.damage.mark_full();
    }

    // ── Hit testing ───────────────────────────────────────────────────────

    fn window_at(&self, mx: i32, my: i32) -> Option<usize> {
        for i in (0..self.windows.len()).rev() {
            let w = &self.windows[i];
            if w.minimized { continue; }
            if mx >= w.x && mx < w.x + w.w as i32
                && my >= w.y && my < w.y + w.h as i32 { return Some(i); }
        }
        None
    }

    fn icon_at(&self, mx: i32, my: i32) -> Option<usize> {
        if mx < 0 || my < 0 { return None; }
        for i in 0..NUM_ICONS {
            let r = icon_rect_of(&self.icons[i]);
            if mx as usize >= r.x && (mx as usize) < r.x + r.w
                && my as usize >= r.y && (my as usize) < r.y + r.h { return Some(i); }
        }
        None
    }

    fn launcher_item_at(&self, mx: i32, my: i32) -> Option<usize> {
        if !self.launcher_open { return None; }
        if mx < 0 || mx as usize >= LAUNCHER_W { return None; }
        let item_start_y = BAR_H + LAUNCHER_HEAD_H;
        let item_end_y = item_start_y + NUM_LAUNCHER * LAUNCHER_ITEM_H;
        if my as usize >= item_start_y && (my as usize) < item_end_y {
            Some(((my as usize) - item_start_y) / LAUNCHER_ITEM_H)
        } else { None }
    }

    fn taskbar_btn_at(&self, mx: i32, my: i32) -> Option<usize> {
        if my < 0 || my as usize >= BAR_H { return None; }
        let total = FIXED_BTNS + self.windows.iter().filter(|w| !w.minimized).count();
        for i in 0..total {
            let r = taskbar_btn_rect(i);
            if mx as usize >= r.x && (mx as usize) < r.x + r.w
                && my as usize >= r.y && (my as usize) < r.y + r.h { return Some(i); }
        }
        None
    }

    // ── Rendering ─────────────────────────────────────────────────────────

    fn render_taskbar(&self) {
        framebuffer::fill_rect(0, 0, self.sw, BAR_H, BAR_BG);
        framebuffer::fill_rect(0, BAR_H - 1, self.sw, 1, BAR_BORDER);

        let btns: [&str; 2] = ["Desktop", if self.launcher_open { "Apps v" } else { "Apps >" }];
        for (i, label) in btns.iter().enumerate() {
            let r = taskbar_btn_rect(i);
            let bg = if i == 1 && self.launcher_open { BAR_BTN_ACT }
                     else if self.taskbar_hover == Some(i) { BAR_BTN_HOV }
                     else { BAR_BTN_BG };
            framebuffer::fill_rect(r.x, r.y, r.w, r.h, bg);
            framebuffer::fill_rect(r.x, r.y, r.w, 1, BAR_BORDER);
            framebuffer::fill_rect(r.x, r.y + r.h - 1, r.w, 1, BAR_BORDER);
            framebuffer::fill_rect(r.x, r.y, 1, r.h, BAR_BORDER);
            framebuffer::fill_rect(r.x + r.w - 1, r.y, 1, r.h, BAR_BORDER);
            framebuffer::draw_text_at(r.x + 6, r.y + (r.h.saturating_sub(8)) / 2, label, BAR_BTN_TEXT);
        }

        let mut wi = 0;
        for (idx, win) in self.windows.iter().enumerate() {
            if win.minimized { continue; }
            let r = taskbar_btn_rect(FIXED_BTNS + wi);
            let is_focused = self.focused == Some(idx);
            let bg = if is_focused { BAR_BTN_ACT }
                     else if self.taskbar_hover == Some(FIXED_BTNS + wi) { BAR_BTN_HOV }
                     else { BAR_BTN_BG };
            framebuffer::fill_rect(r.x, r.y, r.w, r.h, bg);
            framebuffer::fill_rect(r.x, r.y, r.w, 1, BAR_BORDER);
            framebuffer::fill_rect(r.x, r.y + r.h - 1, r.w, 1, BAR_BORDER);
            framebuffer::fill_rect(r.x, r.y, 1, r.h, BAR_BORDER);
            framebuffer::fill_rect(r.x + r.w - 1, r.y, 1, r.h, BAR_BORDER);
            let title = win.app.title();
            let max_chars = (r.w.saturating_sub(12)) / 6;
            let disp = if title.len() > max_chars { &title[..max_chars] } else { title };
            framebuffer::draw_text_at(r.x + 6, r.y + (r.h.saturating_sub(8)) / 2, disp, BAR_BTN_TEXT);
            wi += 1;
        }

        // Taskbar clock — real time from RTC, uptime as secondary.
        {
            let (rh, rm, rs) = crate::rtc::read_time();
            let mut cbuf = [0u8; 24];
            let clen = fmt_hms(&mut cbuf, rh as u64, rm as u64, rs as u64);
            let clock_str = core::str::from_utf8(&cbuf[..clen]).unwrap_or("");

            // Uptime secondary "+H:MM:SS"
            let ms = uptime_ms();
            let us = ms / 1000;
            let um = us / 60;
            let uh = um / 60;
            let mut ubuf = [0u8; 24];
            let mut tmp  = [0u8; 24];
            ubuf[0] = b'+';
            let tlen = fmt_hms(&mut tmp, uh, um % 60, us % 60);
            ubuf[1..1 + tlen].copy_from_slice(&tmp[..tlen]);
            let ulen = 1 + tlen;
            let up_str = core::str::from_utf8(&ubuf[..ulen]).unwrap_or("");

            let clock_w = clock_str.len() * 6 + 4;
            let up_w    = up_str.len() * 6 + 4;
            let total_w = clock_w + up_w + 8;
            let pb_r    = power_btn_rect(self.sw);
            let right_x = pb_r.x.saturating_sub(4);
            let clock_x = right_x.saturating_sub(total_w);
            framebuffer::draw_text_at(clock_x, 11, clock_str, BAR_TEXT);
            framebuffer::draw_text_at(clock_x + clock_w + 4, 11, up_str, BAR_UPTIME);
        }

        // Power button — far right, left of clock
        let pb = power_btn_rect(self.sw);
        let pb_bg = if self.taskbar_hover == Some(usize::MAX) { 0x5A1010 } else { 0x2A0A0A };
        framebuffer::fill_rect(pb.x, pb.y, pb.w, pb.h, pb_bg);
        framebuffer::fill_rect(pb.x, pb.y, pb.w, 1, BAR_BORDER);
        framebuffer::fill_rect(pb.x, pb.y + pb.h - 1, pb.w, 1, BAR_BORDER);
        framebuffer::fill_rect(pb.x, pb.y, 1, pb.h, BAR_BORDER);
        framebuffer::fill_rect(pb.x + pb.w - 1, pb.y, 1, pb.h, BAR_BORDER);
        framebuffer::draw_text_at(pb.x + (pb.w.saturating_sub(6)) / 2,
                                   pb.y + (pb.h.saturating_sub(8)) / 2, "U", 0xE05050);
    }

    fn render_icon(&self, i: usize) {
        let r = icon_rect_of(&self.icons[i]);
        if self.launcher_open && r.x + r.w <= LAUNCHER_W { return; }
        let sel = self.icons[i].selected;
        let hov = self.icon_hover == Some(i);
        let bg = if sel { ICON_SEL } else if hov { 0x131C28 } else { ICON_BG };
        let border = if sel { ICON_BORDER } else { 0x181E28 };
        framebuffer::fill_rect(r.x, r.y, r.w, r.h, bg);
        framebuffer::fill_rect(r.x, r.y, r.w, 1, border);
        framebuffer::fill_rect(r.x, r.y + r.h - 1, r.w, 1, border);
        framebuffer::fill_rect(r.x, r.y, 1, r.h, border);
        framebuffer::fill_rect(r.x + r.w - 1, r.y, 1, r.h, border);
        framebuffer::fill_rect(r.x + 1, r.y + 1, r.w - 2, 4, ICON_ACCENT);
        draw_app_icon(i, r);
        let label = APP_REGISTRY[i].label;
        let tx = r.x + (r.w.saturating_sub(label.len() * 6)) / 2;
        let ty = r.y + r.h / 2 + 4;
        let col = if sel { ICON_TEXT_SEL } else { ICON_TEXT };
        framebuffer::draw_text_at(tx, ty, label, col);
        let sub = APP_REGISTRY[i].icon_sub;
        let sx = r.x + (r.w.saturating_sub(sub.len() * 6)) / 2;
        framebuffer::draw_text_at(sx, ty + 12, sub, 0x2A4060);
    }

    fn render_icons(&self) {
        for i in 0..NUM_ICONS {
            self.render_icon(i);
        }
    }

    fn render_launcher(&self) {
        let r = launcher_rect(self.sh);
        framebuffer::fill_rect(r.x, r.y, r.w, r.h, LAUNCHER_BG);
        framebuffer::fill_rect(r.x + r.w - 1, r.y, 1, r.h, LAUNCHER_BORD);
        framebuffer::fill_rect(0, BAR_H, LAUNCHER_W, LAUNCHER_HEAD_H, LAUNCHER_HEAD);
        framebuffer::fill_rect(0, BAR_H + LAUNCHER_HEAD_H - 1, LAUNCHER_W, 1, LAUNCHER_SEP);
        framebuffer::draw_text_scaled(LAUNCHER_PAD_X, BAR_H + 10, "ASTRA OS", LAUNCHER_TEXT, 2);
        framebuffer::draw_text_at(LAUNCHER_PAD_X, BAR_H + LAUNCHER_HEAD_H - 12, "Applications", LAUNCHER_SUB);
        for i in 0..NUM_LAUNCHER {
            let ir = launcher_item_rect(i);
            let bg = if self.launcher_hover == Some(i) { LAUNCHER_HOV } else { LAUNCHER_BG };
            framebuffer::fill_rect(ir.x, ir.y, ir.w, ir.h, bg);
            framebuffer::fill_rect(ir.x, ir.y + ir.h - 1, ir.w - 1, 1, LAUNCHER_SEP);
            framebuffer::draw_text_at(ir.x + LAUNCHER_PAD_X, ir.y + (ir.h.saturating_sub(16)) / 2, APP_REGISTRY[i].label, LAUNCHER_TEXT);
            framebuffer::draw_text_at(ir.x + LAUNCHER_PAD_X, ir.y + (ir.h.saturating_sub(16)) / 2 + 11, APP_REGISTRY[i].desc, LAUNCHER_SUB);
        }
    }

    fn render_desktop_ctx(&self) {
        let r = self.dctx.rect(self.sw, self.sh);
        // Border
        framebuffer::fill_rect(r.x, r.y, r.w, r.h, DCTX_BORD);
        // Background (inset 1 px)
        framebuffer::fill_rect(r.x + 1, r.y + 1, r.w - 2, r.h - 2, DCTX_BG);
        let labels: [&str; DCTX_ITEMS] = ["New File", "New Folder"];
        for i in 0..DCTX_ITEMS {
            let ir = self.dctx.item_rect(i, self.sw, self.sh);
            let bg = if self.dctx.hover == Some(i) { DCTX_HOV } else { DCTX_BG };
            framebuffer::fill_rect(ir.x, ir.y, ir.w, ir.h, bg);
            let ty = ir.y + (DCTX_ITEM_H.saturating_sub(9)) / 2;
            framebuffer::draw_text_at(ir.x + 8, ty, labels[i], DCTX_TEXT);
        }
    }

    fn render_desk_item(&self, i: usize) {
        let item = &self.desk_items[i];
        let r = item.rect();
        // Same pipeline as render_icon ─────────────────────────────────────
        let sel = item.selected;
        let bg     = if sel { DI_SEL_BG } else { ICON_BG };
        let border = if sel { ICON_BORDER } else { 0x181E28 };
        framebuffer::fill_rect(r.x, r.y, r.w, r.h, bg);
        // 4-side border
        framebuffer::fill_rect(r.x, r.y,             r.w, 1,   border);
        framebuffer::fill_rect(r.x, r.y + r.h - 1,   r.w, 1,   border);
        framebuffer::fill_rect(r.x, r.y,             1,   r.h,  border);
        framebuffer::fill_rect(r.x + r.w - 1, r.y,  1,   r.h,  border);
        // Accent strip (same as dock)
        framebuffer::fill_rect(r.x + 1, r.y + 1, r.w - 2, 4, ICON_ACCENT);
        // Pixel-art icon via shared draw_app_icon
        let icon_idx = if item.is_dir { DI_ICON_DIR } else { DI_ICON_FILE };
        draw_app_icon(icon_idx, r);
        // Label — centred, up to 14 chars (same position as dock label)
        let name = core::str::from_utf8(&item.name[..item.nlen]).unwrap_or("?");
        const MAX_LABEL: usize = 14;
        let label = if name.len() > MAX_LABEL { &name[..MAX_LABEL] } else { name };
        let tx = r.x + (r.w.saturating_sub(label.len() * 6)) / 2;
        let ty = r.y + r.h / 2 + 4;
        let col = if sel { DI_SEL_TEXT } else { DI_TEXT };
        framebuffer::draw_text_at(tx, ty, label, col);
    }

    fn render_desk_items(&self) {
        for i in 0..self.desk_item_count {
            self.render_desk_item(i);
        }
    }

    fn render_desk_prompt(&self) {
        let pr = self.desk_prompt.rect(self.sw, self.sh);
        // Label above the input box
        let lbl = if self.desk_prompt.is_dir { "Folder name:" } else { "File name:" };
        framebuffer::draw_text_at(pr.x + 4, pr.y - 12, lbl, DP_LBL);
        // Box border + bg
        framebuffer::fill_rect(pr.x, pr.y, pr.w, pr.h, DP_BORD);
        framebuffer::fill_rect(pr.x + 1, pr.y + 1, pr.w - 2, pr.h - 2, DP_BG);
        // Typed text
        let typed = core::str::from_utf8(&self.desk_prompt.buf[..self.desk_prompt.len]).unwrap_or("");
        let tx = pr.x + 6;
        let ty = pr.y + (DP_H.saturating_sub(8)) / 2;
        framebuffer::draw_text_at(tx, ty, typed, DP_TEXT);
        // Cursor
        let cx = tx + self.desk_prompt.len * 6;
        framebuffer::draw_text_at(cx, ty, "_", DP_CUR);
        // Hint
        framebuffer::draw_text_at(pr.x + 4, pr.y + DP_H + 2, "Enter=ok  Esc=cancel", 0x2A5070);
    }

    /// Renders only the chrome (shadow, border, titlebar, close button).
    /// Does NOT call app.render() — client area is filled with WIN_BG only.
    /// Used during drag when the cached surface is blitted separately.
    fn render_window_chrome(&self, idx: usize, focused: bool) {
        let win = &self.windows[idx];
        if win.minimized { return; }
        let x = win.x.max(0) as usize;
        let y = win.y.max(0) as usize;
        let (w, h) = (win.w, win.h);
        framebuffer::fill_rect(x + WIN_SHADOW_OFS, y + WIN_SHADOW_OFS, w, h, WIN_SHADOW);
        let border = if focused { WIN_BORDER_FOC } else { WIN_BORDER };
        framebuffer::fill_rect(x, y, w, h, border);
        framebuffer::fill_rect(x + 1, y + 1, w.saturating_sub(2), h.saturating_sub(2), WIN_BG);
        let bar_bg = if focused { WIN_BAR_FOC } else { WIN_BAR_BG };
        framebuffer::fill_rect(x, y, w, WIN_BAR_H, bar_bg);
        framebuffer::fill_rect(x, y + WIN_BAR_H, w, 1, WIN_BAR_BORDER);
        let title = win.app.title();
        let ty = y + (WIN_BAR_H.saturating_sub(14)) / 2;
        framebuffer::draw_text_scaled(x + WIN_PAD_X, ty, title, WIN_TITLE_COL, 2);
        let cb = win.close_btn_rect();
        let close_bg = if self.close_hover == Some(idx) { WIN_CLOSE_HOV } else { bar_bg };
        framebuffer::fill_rect(cb.x, cb.y, cb.w, cb.h, close_bg);
        framebuffer::draw_text_at(cb.x + (cb.w.saturating_sub(6)) / 2, cb.y + (cb.h.saturating_sub(8)) / 2, "X", WIN_TITLE_COL);
        let hint = "[ESC]";
        let hx = cb.x.saturating_sub(hint.len() * 6 + 6);
        framebuffer::draw_text_at(hx, ty + 2, hint, WIN_HINT_COL);
    }

    fn render_window(&self, idx: usize, focused: bool) {        let win = &self.windows[idx];
        if win.minimized { return; }
        let x = win.x.max(0) as usize;
        let y = win.y.max(0) as usize;
        let (w, h) = (win.w, win.h);
        framebuffer::fill_rect(x + WIN_SHADOW_OFS, y + WIN_SHADOW_OFS, w, h, WIN_SHADOW);
        let border = if focused { WIN_BORDER_FOC } else { WIN_BORDER };
        framebuffer::fill_rect(x, y, w, h, border);
        framebuffer::fill_rect(x + 1, y + 1, w.saturating_sub(2), h.saturating_sub(2), WIN_BG);
        let bar_bg = if focused { WIN_BAR_FOC } else { WIN_BAR_BG };
        framebuffer::fill_rect(x, y, w, WIN_BAR_H, bar_bg);
        framebuffer::fill_rect(x, y + WIN_BAR_H, w, 1, WIN_BAR_BORDER);
        let title = win.app.title();
        let ty = y + (WIN_BAR_H.saturating_sub(14)) / 2;
        framebuffer::draw_text_scaled(x + WIN_PAD_X, ty, title, WIN_TITLE_COL, 2);
        let cb = win.close_btn_rect();
        let close_bg = if self.close_hover == Some(idx) { WIN_CLOSE_HOV } else { bar_bg };
        framebuffer::fill_rect(cb.x, cb.y, cb.w, cb.h, close_bg);
        framebuffer::draw_text_at(cb.x + (cb.w.saturating_sub(6)) / 2, cb.y + (cb.h.saturating_sub(8)) / 2, "X", WIN_TITLE_COL);
        let hint = "[ESC]";
        let hx = cb.x.saturating_sub(hint.len() * 6 + 6);
        framebuffer::draw_text_at(hx, ty + 2, hint, WIN_HINT_COL);
        let cr = win.client_rect();
        win.app.render(cr.x, cr.y, cr.w, cr.h);
    }

    fn compose_full(&mut self) {
        framebuffer::clear(desktop_bg());
        self.render_icons();
        self.render_desk_items();
        let focused = self.focused;
        for i in 0..self.windows.len() {
            if !self.windows[i].minimized {
                self.render_window(i, focused == Some(i));
            }
        }
        if self.launcher_open { self.render_launcher(); }
        if self.dctx.visible  { self.render_desktop_ctx(); }
        if self.desk_prompt.active { self.render_desk_prompt(); }
        self.render_taskbar();
    }

    fn compose_damage(&mut self) {
        let screen = Rect { x: 0, y: 0, w: self.sw, h: self.sh };
        let launcher = launcher_rect(self.sh);
        let taskbar = Rect { x: 0, y: 0, w: self.sw, h: BAR_H };
        let focused = self.focused;

        for i in 0..self.damage.count {
            let dirty = self.damage.rects[i].clip(&screen);
            if dirty.is_empty() { continue; }

            // Set scissor so all rendering is clipped to this damage rect.
            // This means app.render() and render_window() only write pixels
            // that will actually be visible — no wasted backbuffer work outside
            // the damaged area.
            framebuffer::set_scissor(dirty.x, dirty.y, dirty.w, dirty.h);

            framebuffer::fill_rect(dirty.x, dirty.y, dirty.w, dirty.h, desktop_bg());

            for icon_idx in 0..NUM_ICONS {
                if icon_rect_of(&self.icons[icon_idx]).intersects(&dirty) {
                    self.render_icon(icon_idx);
                }
            }

            for di in 0..self.desk_item_count {
                if self.desk_items[di].rect().intersects(&dirty) {
                    self.render_desk_item(di);
                }
            }

            for win_idx in 0..self.windows.len() {
                if !self.windows[win_idx].minimized && self.windows[win_idx].bounds().intersects(&dirty) {
                    let cr = self.windows[win_idx].client_rect();
                    let cache_ok = self.windows[win_idx].surface_valid
                        && self.windows[win_idx].surface_w == cr.w
                        && self.windows[win_idx].surface_h == cr.h
                        && !self.windows[win_idx].cached_surface.is_empty();

                    if cache_ok {
                        // Chrome (titlebar, border, shadow) — no app.render().
                        // Chrome calls respect scissor, so they self-clip to `dirty`.
                        self.render_window_chrome(win_idx, focused == Some(win_idx));

                        // Blit cached client pixels.  write_rect_sub bypasses the
                        // scissor, so we manually clip to (dirty ∩ client_rect) to
                        // avoid stomping pixels outside the damage area (which could
                        // contain another window/icon not yet recomposed for that rect).
                        let ix0 = cr.x.max(dirty.x);
                        let iy0 = cr.y.max(dirty.y);
                        let ix1 = (cr.x + cr.w).min(dirty.x + dirty.w);
                        let iy1 = (cr.y + cr.h).min(dirty.y + dirty.h);
                        if ix1 > ix0 && iy1 > iy0 {
                            let iw = ix1 - ix0;
                            let ih = iy1 - iy0;
                            let sub_x = ix0 - cr.x;
                            let sub_y = iy0 - cr.y;
                            framebuffer::write_rect_sub(
                                ix0, iy0, iw, ih,
                                &self.windows[win_idx].cached_surface,
                                cr.w, sub_x, sub_y,
                            );
                        }
                    } else {
                        // Full render: chrome + app.render().  Schedule capture so
                        // subsequent frames can blit from the cache instead.
                        self.render_window(win_idx, focused == Some(win_idx));
                        self.windows[win_idx].surface_needs_capture = true;
                    }
                }
            }

            if self.launcher_open && launcher.intersects(&dirty) {
                self.render_launcher();
            }
            if self.dctx.visible && self.dctx.rect(self.sw, self.sh).intersects(&dirty) {
                self.render_desktop_ctx();
            }
            if self.desk_prompt.active && self.desk_prompt.rect(self.sw, self.sh).intersects(&dirty) {
                self.render_desk_prompt();
            }
            if taskbar.intersects(&dirty) {
                self.render_taskbar();
            }
        }

        framebuffer::clear_scissor();

        // ── Surface capture ───────────────────────────────────────────────
        // After all damage rects are composited, read back client pixels for
        // any window that was fully re-rendered this pass.  Future drag frames
        // will blit from this cache instead of calling app.render().
        for win_idx in 0..self.windows.len() {
            if !self.windows[win_idx].surface_needs_capture { continue; }
            self.windows[win_idx].surface_needs_capture = false;
            let cr = self.windows[win_idx].client_rect();
            let n = cr.w * cr.h;
            if n == 0 { continue; }
            self.windows[win_idx].cached_surface.resize(n, 0);
            framebuffer::read_rect(cr.x, cr.y, cr.w, cr.h,
                &mut self.windows[win_idx].cached_surface);
            self.windows[win_idx].surface_w = cr.w;
            self.windows[win_idx].surface_h = cr.h;
            self.windows[win_idx].surface_valid = true;
        }
    }

    // ── Cursor ────────────────────────────────────────────────────────────

    fn cursor_save(&mut self) {
        let cx = self.cursor_x.max(0) as usize;
        let cy = self.cursor_y.max(0) as usize;
        framebuffer::read_rect(cx, cy, CURSOR_W, CURSOR_H, &mut self.cursor_under);
        self.cursor_drawn_x = self.cursor_x;
        self.cursor_drawn_y = self.cursor_y;
    }

    fn cursor_stamp(&self) {
        let cx = self.cursor_x.max(0) as usize;
        let cy = self.cursor_y.max(0) as usize;
        let bmp = cursor_bitmap(self.cursor_shape);
        for row in 0..CURSOR_H {
            for col in 0..CURSOR_W {
                let px = bmp[row][col];
                if px == 0 { continue; }
                let color = if px == 1 { CURSOR_WHITE } else { CURSOR_BLACK };
                let px_x = cx + col; let px_y = cy + row;
                if px_x < self.sw && px_y < self.sh {
                    framebuffer::fill_rect(px_x, px_y, 1, 1, color);
                }
            }
        }
    }

    fn cursor_erase(&self) {
        let cx = self.cursor_drawn_x.max(0) as usize;
        let cy = self.cursor_drawn_y.max(0) as usize;
        framebuffer::write_rect(cx, cy, CURSOR_W, CURSOR_H, &self.cursor_under);
    }

    fn cursor_move_fast(&mut self) {
        let old_x = self.cursor_drawn_x.max(0) as usize;
        let old_y = self.cursor_drawn_y.max(0) as usize;
        if self.cursor_on_screen { self.cursor_erase(); }
        self.cursor_save();
        self.cursor_stamp();
        self.cursor_on_screen = true;
        if old_x != self.cursor_x.max(0) as usize || old_y != self.cursor_y.max(0) as usize {
            framebuffer::present_rect(old_x, old_y, CURSOR_W, CURSOR_H);
        }
        let nx = self.cursor_x.max(0) as usize;
        let ny = self.cursor_y.max(0) as usize;
        framebuffer::present_rect(nx, ny, CURSOR_W, CURSOR_H);
    }

    fn cursor_rect_at(&self, x: i32, y: i32) -> Rect {
        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        if x0 >= self.sw || y0 >= self.sh {
            return Rect::ZERO;
        }
        Rect {
            x: x0,
            y: y0,
            w: CURSOR_W.min(self.sw - x0),
            h: CURSOR_H.min(self.sh - y0),
        }
    }

    fn present_damage(&mut self) {
        if self.damage.full {
            self.present_full();
            return;
        }

        let screen = Rect { x: 0, y: 0, w: self.sw, h: self.sh };

        // ── Cursor erase (backbuffer only, before compose) ────────────────
        // IMPORTANT: do NOT add the cursor rect to the damage list.
        // Adding it would cause compose_damage to call render_window for every
        // window the cursor touches — even when no app content changed.
        // Instead, restore the saved cursor_under pixels directly; compose_damage
        // will overwrite them with correct content if that area is in a damage rect.
        let old_cursor = if self.cursor_on_screen {
            let r = self.cursor_rect_at(self.cursor_drawn_x, self.cursor_drawn_y);
            self.cursor_erase(); // writes cursor_under back to backbuffer
            r
        } else {
            Rect::ZERO
        };
        self.cursor_on_screen = false;

        // Compose only app/window damage — cursor not in the damage list.
        self.compose_damage();

        // Stamp cursor at new position.
        self.cursor_save();
        self.cursor_stamp();
        self.cursor_on_screen = true;

        // Blit app damage rects.
        for i in 0..self.damage.count {
            let r = self.damage.rects[i].clip(&screen);
            if !r.is_empty() {
                framebuffer::present_rect(r.x, r.y, r.w, r.h);
            }
        }

        // Blit old cursor area (backbuffer now has clean background there).
        let old_cr = old_cursor.clip(&screen);
        if !old_cr.is_empty() {
            framebuffer::present_rect(old_cr.x, old_cr.y, old_cr.w, old_cr.h);
        }

        // Blit new cursor area.
        let new_cr = self.cursor_rect_at(self.cursor_x, self.cursor_y).clip(&screen);
        if !new_cr.is_empty() {
            framebuffer::present_rect(new_cr.x, new_cr.y, new_cr.w, new_cr.h);
        }

        self.damage.clear();
    }

    fn present_full(&mut self) {
        self.cursor_on_screen = false;
        self.compose_full();
        self.cursor_save();
        self.cursor_stamp();
        framebuffer::present_full();
        self.cursor_on_screen = true;
        self.damage.clear();
    }

    fn tick_live_windows(&mut self, now: u64) {
        for i in 0..self.windows.len() {
            if self.windows[i].minimized { continue; }
            if let Some(interval) = self.windows[i].app.refresh_interval_ms() {
                if now.wrapping_sub(self.windows[i].last_refresh_ms) >= interval {
                    self.windows[i].last_refresh_ms = now;
                    match self.windows[i].app.tick() {
                        AppAction::Nothing => {}
                        AppAction::RedrawArea(rx, ry, rw, rh) => {
                            self.windows[i].surface_valid = false;
                            let cr = self.windows[i].client_rect();
                            let rx = rx.min(cr.w);
                            let ry = ry.min(cr.h);
                            let rw = rw.min(cr.w.saturating_sub(rx));
                            let rh = rh.min(cr.h.saturating_sub(ry));
                            if rw != 0 && rh != 0 {
                                self.damage.add(Rect { x: cr.x + rx, y: cr.y + ry, w: rw, h: rh });
                            }
                        }
                        _ => {
                            self.windows[i].surface_valid = false;
                            self.damage.add(self.windows[i].client_rect());
                        }
                    }
                }
            }
        }
    }

    /// Earliest future time (ms) at which a live window needs its next refresh.
    /// Returns `u64::MAX` when no windows have periodic refresh.
    fn next_wakeup_ms(&self, now: u64) -> u64 {
        let mut earliest = u64::MAX;
        for win in &self.windows {
            if win.minimized { continue; }
            if let Some(interval) = win.app.refresh_interval_ms() {
                let next = win.last_refresh_ms.saturating_add(interval);
                let next = if next <= now { now } else { next };
                if next < earliest { earliest = next; }
            }
        }
        earliest
    }

    fn update_cursor_shape(&mut self) {
        let (mx, my) = (self.cursor_x, self.cursor_y);
        if self.drag.is_some() { self.cursor_shape = CursorShape::Move; return; }
        if let Some(ref rs) = self.resize { self.cursor_shape = rs.zone.cursor_shape(); return; }
        for i in (0..self.windows.len()).rev() {
            let w = &self.windows[i];
            if w.minimized { continue; }
            if let Some(zone) = hit_resize_zone(w, mx, my) { self.cursor_shape = zone.cursor_shape(); return; }
            if mx >= w.x && mx < w.x + w.w as i32 && my >= w.y && my < w.y + WIN_BAR_H as i32 {
                self.cursor_shape = CursorShape::Move; return;
            }
        }
        if self.launcher_open && self.launcher_item_at(mx, my).is_some() { self.cursor_shape = CursorShape::Hand; return; }
        if self.icon_at(mx, my).is_some() { self.cursor_shape = CursorShape::Hand; return; }
        if self.desk_item_at(mx, my).is_some() { self.cursor_shape = CursorShape::Hand; return; }
        if self.taskbar_btn_at(mx, my).is_some() { self.cursor_shape = CursorShape::Hand; return; }
        let pb = power_btn_rect(self.sw);
        if my >= 0 && (my as usize) < BAR_H && mx as usize >= pb.x && (mx as usize) < pb.x + pb.w
            && my as usize >= pb.y && (my as usize) < pb.y + pb.h
        { self.cursor_shape = CursorShape::Hand; return; }
        self.cursor_shape = CursorShape::Arrow;
    }

    // ── Input handlers ────────────────────────────────────────────────────

    fn on_mouse_move(&mut self, mx: i32, my: i32) {
        self.cursor_x = mx;
        self.cursor_y = my;

        if let Some(ref mut ids) = self.icon_drag {
            let idx = ids.idx;
            let ox = ids.off_x;
            let oy = ids.off_y;
            let old_r = icon_rect_of(&self.icons[idx]);
            let new_x = (mx - ox)
                .max(0)
                .min((self.sw.saturating_sub(ICON_CELL_W)) as i32);
            let new_y = (my - oy)
                .max(BAR_H as i32)
                .min((self.sh.saturating_sub(ICON_CELL_H)) as i32);
            self.icons[idx].x = new_x;
            self.icons[idx].y = new_y;
            ids.moved = true;
            let new_r = icon_rect_of(&self.icons[idx]);
            self.damage.add(old_r);
            self.damage.add(new_r);
        }

        if let Some(ref mut di_drag) = self.desk_item_drag {
            let idx = di_drag.idx;
            let ox = di_drag.off_x;
            let oy = di_drag.off_y;
            if idx < self.desk_item_count {
                let old_r = self.desk_items[idx].rect();
                let new_x = (mx - ox).max(0).min((self.sw.saturating_sub(DI_W)) as i32);
                let new_y = (my - oy).max(BAR_H as i32).min((self.sh.saturating_sub(DI_H)) as i32);
                self.desk_items[idx].x = new_x;
                self.desk_items[idx].y = new_y;
                di_drag.moved = true;
                let new_r = self.desk_items[idx].rect();
                self.damage.add(old_r);
                self.damage.add(new_r);
            }
        }

        if let Some(ref ds) = self.drag {
            let idx = ds.win_idx; let ox = ds.off_x; let oy = ds.off_y;
            if idx < self.windows.len() {
                // Capture old bounds BEFORE updating position so the accumulator
                // includes both the previous and new window positions.
                let old_b = self.windows[idx].bounds();

                // Clamp so at least WIN_BAR_H*3 px of the title bar stays on-screen
                // horizontally — prevents windows being dragged off into the void.
                let win_w = self.windows[idx].w as i32;
                let keep: i32 = (WIN_BAR_H as i32) * 3;
                self.windows[idx].x = (mx - ox)
                    .max(keep - win_w)
                    .min(self.sw as i32 - keep);
                self.windows[idx].y = (my - oy).max(BAR_H as i32);
                let new_b = self.windows[idx].bounds();

                // Accumulate: union(accum, old_b, new_b) so we track every pixel
                // the window swept through, even across skipped frames.
                let accum = match self.drag_damage_accum {
                    Some(prev) => prev.union(&old_b).union(&new_b),
                    None       => old_b.union(&new_b),
                };
                self.drag_damage_accum = Some(accum);

                // Rate-limit: flush to damage list at most every 16 ms (~60 fps).
                let now_ms = uptime_ms();
                if now_ms.wrapping_sub(self.last_drag_present_ms) >= 16 {
                    self.damage.add(accum);
                    // Reset accum to latest bounds so next frame erases from here.
                    self.drag_damage_accum = Some(new_b);
                    self.last_drag_present_ms = now_ms;
                }
            }
        }

        if let Some(ref rs) = self.resize {
            let idx = rs.win_idx;
            let dx = mx - rs.start_mx; let dy = my - rs.start_my;
            let sx = rs.start_x; let sy = rs.start_y;
            let sw = rs.start_w as i32; let sh = rs.start_h as i32;
            if idx < self.windows.len() {
                let old_b = self.windows[idx].bounds();
                let (nx, ny, nw, nh) = match rs.zone {
                    ResizeZone::TL => (sx+dx, sy+dy, (sw-dx).max(WIN_MIN_W as i32), (sh-dy).max(WIN_MIN_H as i32)),
                    ResizeZone::T  => (sx,    sy+dy, sw,                            (sh-dy).max(WIN_MIN_H as i32)),
                    ResizeZone::TR => (sx,    sy+dy, (sw+dx).max(WIN_MIN_W as i32), (sh-dy).max(WIN_MIN_H as i32)),
                    ResizeZone::R  => (sx,    sy,    (sw+dx).max(WIN_MIN_W as i32), sh),
                    ResizeZone::BR => (sx,    sy,    (sw+dx).max(WIN_MIN_W as i32), (sh+dy).max(WIN_MIN_H as i32)),
                    ResizeZone::B  => (sx,    sy,    sw,                            (sh+dy).max(WIN_MIN_H as i32)),
                    ResizeZone::BL => (sx+dx, sy,    (sw-dx).max(WIN_MIN_W as i32), (sh+dy).max(WIN_MIN_H as i32)),
                    ResizeZone::L  => (sx+dx, sy,    (sw-dx).max(WIN_MIN_W as i32), sh),
                };
                self.windows[idx].x = nx.max(0);
                self.windows[idx].y = ny.max(BAR_H as i32);
                self.windows[idx].w = nw as usize;
                self.windows[idx].h = nh as usize;
                let new_b = self.windows[idx].bounds();
                self.damage.add(old_b);
                self.damage.add(new_b);
            }
        }

        let old_lh = self.launcher_hover;
        let old_ih = self.icon_hover;
        let old_th = self.taskbar_hover;
        let old_ch = self.close_hover;

        self.launcher_hover = self.launcher_item_at(mx, my);
        self.icon_hover = if self.icon_drag.is_some() {
            None
        } else if !self.launcher_open || mx as usize >= LAUNCHER_W {
            self.icon_at(mx, my)
        } else { None };
        let pb = power_btn_rect(self.sw);
        let on_power = my >= 0 && (my as usize) < BAR_H
            && mx as usize >= pb.x && (mx as usize) < pb.x + pb.w
            && my as usize >= pb.y && (my as usize) < pb.y + pb.h;
        // usize::MAX is used as a sentinel meaning "hovering power button"
        self.taskbar_hover = if on_power { Some(usize::MAX) } else { self.taskbar_btn_at(mx, my) };
        self.close_hover = None;
        for i in (0..self.windows.len()).rev() {
            let w = &self.windows[i];
            if w.minimized { continue; }
            let cb = w.close_btn_rect();
            if mx as usize >= cb.x && (mx as usize) < cb.x + cb.w
                && my as usize >= cb.y && (my as usize) < cb.y + cb.h {
                self.close_hover = Some(i); break;
            }
        }

        let mut next_app_hover = None;
        if let Some(fidx) = self.focused {
            if fidx < self.windows.len() && !self.windows[fidx].minimized {
                let cr = self.windows[fidx].client_rect();
                let inside_client = mx >= cr.x as i32
                    && mx < (cr.x + cr.w) as i32
                    && my >= cr.y as i32
                    && my < (cr.y + cr.h) as i32;

                if inside_client {
                    next_app_hover = Some(fidx);
                    let rx = mx - cr.x as i32;
                    let ry = my - cr.y as i32;
                    let act = self.windows[fidx].app.handle_mouse_move(rx, ry);
                    self.handle_app_action(fidx, act);
                } else if self.app_hover_target == Some(fidx) {
                    let act = self.windows[fidx].app.handle_mouse_move(-1, -1);
                    self.handle_app_action(fidx, act);
                }
            }
        }
        self.app_hover_target = next_app_hover;

        if old_lh != self.launcher_hover || old_ih != self.icon_hover
            || old_th != self.taskbar_hover || old_ch != self.close_hover {
            self.damage.add(Rect { x: 0, y: 0, w: self.sw, h: BAR_H });
            if self.launcher_open { self.damage.add(launcher_rect(self.sh)); }
            for i in 0..NUM_ICONS { self.damage.add(icon_rect_of(&self.icons[i])); }
        }

        // Desktop context menu hover
        if self.dctx.visible {
            let new_hover = self.dctx.hit_item(mx, my, self.sw, self.sh);
            if new_hover != self.dctx.hover {
                self.dctx.hover = new_hover;
                self.damage.add(self.dctx.rect(self.sw, self.sh));
            }
        }

        self.update_cursor_shape();
    }

    fn on_button_press(&mut self, mx: i32, my: i32) {
        // ── Desktop context menu ──────────────────────────────────────────
        if self.dctx.visible {
            let old_r = self.dctx.rect(self.sw, self.sh);
            let hit = self.dctx.hit_item(mx, my, self.sw, self.sh);
            self.dctx.visible = false;
            self.damage.add(old_r);
            if let Some(item) = hit {
                // Start the desktop name-entry prompt instead of opening File Manager
                let is_dir = item == 1; // 0 = New File, 1 = New Folder
                self.desk_prompt = DesktopNamePrompt {
                    active: true,
                    spawn_x: self.dctx.x,
                    spawn_y: self.dctx.y,
                    is_dir,
                    buf: [0u8; 32],
                    len: 0,
                };
                self.damage.add(self.desk_prompt.rect(self.sw, self.sh));
                return;
            }
            // Clicked outside the menu — just dismissed it, fall through.
        }

        if let Some(ch) = self.close_hover {
            if ch < self.windows.len() {
                let b = self.windows[ch].close_btn_rect();
                if mx as usize >= b.x && (mx as usize) < b.x + b.w
                    && my as usize >= b.y && (my as usize) < b.y + b.h {
                    self.close_window(ch); return;
                }
            }
        }

        // Power button click
        let pb = power_btn_rect(self.sw);
        if my >= 0 && (my as usize) < BAR_H
            && mx as usize >= pb.x && (mx as usize) < pb.x + pb.w
            && my as usize >= pb.y && (my as usize) < pb.y + pb.h
        {
            crate::arch::x86_64::power_off();
        }

        if let Some(tb) = self.taskbar_btn_at(mx, my) {
            match tb {
                0 => self.minimize_all(),
                1 => { self.launcher_open = !self.launcher_open; self.damage.mark_full(); }
                n => {
                    let mut wi = 0;
                    for i in 0..self.windows.len() {
                        if !self.windows[i].minimized {
                            if wi == n - FIXED_BTNS {
                                let b = self.windows[i].bounds();
                                self.damage.add(b);
                                self.raise_to_front(i);
                                self.focused = Some(self.windows.len() - 1);
                                self.damage.add(self.windows[self.windows.len()-1].bounds());
                                return;
                            }
                            wi += 1;
                        }
                    }
                }
            }
            self.damage.add(Rect { x: 0, y: 0, w: self.sw, h: BAR_H });
            return;
        }

        if self.launcher_open {
            if let Some(li) = self.launcher_item_at(mx, my) {
                self.launch_app(li);
                self.launcher_open = false;
                self.damage.mark_full();
                return;
            }
            if mx as usize >= LAUNCHER_W {
                self.launcher_open = false;
                self.damage.mark_full();
            }
        }

        if let Some(wi) = self.window_at(mx, my) {
            if self.focused != Some(wi) || wi != self.windows.len() - 1 {
                if let Some(fid) = self.focused {
                    if fid < self.windows.len() { self.damage.add(self.windows[fid].bounds()); }
                }
                self.damage.add(self.windows[wi].bounds());
                self.raise_to_front(wi);
                let new_idx = self.windows.len() - 1;
                self.focused = Some(new_idx);
                self.damage.add(self.windows[new_idx].bounds());
            }
            let tidx = self.windows.len() - 1;
            let win = &self.windows[tidx];
            if my >= win.y && my < win.y + WIN_BAR_H as i32 {
                let cb = win.close_btn_rect();
                if !(mx as usize >= cb.x && (mx as usize) < cb.x + cb.w
                    && my as usize >= cb.y && (my as usize) < cb.y + cb.h) {
                    let ox = mx - win.x; let oy = my - win.y;
                    self.drag_damage_accum = Some(self.windows[tidx].bounds());
                    self.last_drag_present_ms = uptime_ms();
                    self.drag = Some(DragState { win_idx: tidx, off_x: ox, off_y: oy });
                }
                return;
            }
            if let Some(zone) = hit_resize_zone(&self.windows[tidx], mx, my) {
                let win = &self.windows[tidx];
                self.resize = Some(ResizeState {
                    win_idx: tidx, zone,
                    start_mx: mx, start_my: my,
                    start_x: win.x, start_y: win.y,
                    start_w: win.w, start_h: win.h,
                });
                return;
            }
            let cr = self.windows[tidx].client_rect();
            let rx = mx - cr.x as i32; let ry = my - cr.y as i32;
            let act = self.windows[tidx].app.handle_mouse_click(rx, ry);
            self.handle_app_action(tidx, act);
            return;
        }

        if let Some(ii) = self.icon_at(mx, my) {
            let now = uptime_ms();
            let dbl = now.wrapping_sub(self.icons[ii].last_click_ms) < DBL_CLICK_MS;
            self.icons[ii].last_click_ms = now;
            for j in 0..NUM_ICONS { if j != ii { self.icons[j].selected = false; } }
            if dbl {
                self.icons[ii].selected = false;
                self.launch_app(ii);
            } else {
                self.icons[ii].selected = true;
                let r = icon_rect_of(&self.icons[ii]);
                let off_x = mx - r.x as i32;
                let off_y = my - r.y as i32;
                self.icon_drag = Some(IconDragState { idx: ii, off_x, off_y, moved: false });
            }
            for i in 0..NUM_ICONS { self.damage.add(icon_rect_of(&self.icons[i])); }
            return;
        }

        // ── Desktop items (user-created files/folders) ────────────────────
        if let Some(di) = self.desk_item_at(mx, my) {
            // Dismiss name prompt if open
            if self.desk_prompt.active {
                let pr = self.desk_prompt.rect(self.sw, self.sh);
                self.desk_prompt = DesktopNamePrompt::hidden();
                self.damage.add(pr);
            }
            let now = uptime_ms();
            let dbl = now.wrapping_sub(self.desk_items[di].last_click_ms) < DBL_CLICK_MS;
            self.desk_items[di].last_click_ms = now;
            for j in 0..self.desk_item_count { if j != di { self.desk_items[j].selected = false; } }
            if dbl {
                self.desk_items[di].selected = false;
                self.open_desk_item(di);
            } else {
                self.desk_items[di].selected = true;
                let r = self.desk_items[di].rect();
                self.desk_item_drag = Some(DeskItemDrag {
                    idx: di,
                    off_x: mx - r.x as i32,
                    off_y: my - r.y as i32,
                    moved: false,
                });
            }
            self.damage.mark_full();
            return;
        }

        // Click on empty desktop area — dismiss prompt, deselect items
        if self.desk_prompt.active {
            let pr = self.desk_prompt.rect(self.sw, self.sh);
            self.desk_prompt = DesktopNamePrompt::hidden();
            self.damage.add(pr);
        }
        // Mark currently-selected items dirty before clearing the flag.
        for i in 0..self.desk_item_count {
            if self.desk_items[i].selected { self.damage.add(self.desk_items[i].rect()); }
        }
        for i in 0..self.desk_item_count { self.desk_items[i].selected = false; }
    }

    fn on_right_button_press(&mut self, mx: i32, my: i32) {
        // If an existing desktop ctx menu is visible, dismiss it first.
        if self.dctx.visible {
            let old_r = self.dctx.rect(self.sw, self.sh);
            self.dctx.visible = false;
            self.damage.add(old_r);
        }

        // Route right-click to the focused window's client area first.
        if let Some(fi) = self.focused {
            if fi < self.windows.len() && !self.windows[fi].minimized {
                let cr = self.windows[fi].client_rect();
                if mx as usize >= cr.x && (mx as usize) < cr.x + cr.w
                    && my as usize >= cr.y && (my as usize) < cr.y + cr.h
                {
                    let rx = mx - cr.x as i32;
                    let ry = my - cr.y as i32;
                    let act = self.windows[fi].app.handle_mouse_right_click(rx, ry);
                    self.handle_app_action(fi, act);
                    return;
                }
            }
        }

        // Right-click on the bare desktop (not on taskbar, not on a window) —
        // open the desktop context menu.
        if (my as usize) < BAR_H { return; } // don't show over taskbar
        if self.launcher_open && (mx as usize) < LAUNCHER_W { return; } // not over launcher
        self.dctx = DesktopCtxMenu { visible: true, x: mx, y: my, hover: None };
        self.damage.add(self.dctx.rect(self.sw, self.sh));
    }

    // ── Desktop item helpers ──────────────────────────────────────────────

    fn desk_item_at(&self, mx: i32, my: i32) -> Option<usize> {
        for i in 0..self.desk_item_count {
            let r = self.desk_items[i].rect();
            if mx as usize >= r.x && (mx as usize) < r.x + r.w
                && my as usize >= r.y && (my as usize) < r.y + r.h
            {
                return Some(i);
            }
        }
        None
    }

    fn commit_desk_prompt(&mut self) {
        if self.desk_prompt.len == 0 || self.desk_item_count >= MAX_DESK_ITEMS {
            let pr = self.desk_prompt.rect(self.sw, self.sh);
            self.desk_prompt = DesktopNamePrompt::hidden();
            self.damage.add(Rect { x: pr.x, y: pr.y.saturating_sub(16), w: pr.w, h: pr.h + 30 });
            return;
        }
        let is_dir = self.desk_prompt.is_dir;
        let nlen   = self.desk_prompt.len;
        let mut name_bytes = [0u8; 32];
        name_bytes[..nlen].copy_from_slice(&self.desk_prompt.buf[..nlen]);
        // Create on FAT32 (inside the Desktop/ folder) and immediately look up
        // the cluster so double-click can open the folder directly.
        let fat32_cluster = if crate::fat32::is_mounted() {
            let desk_c = Self::desktop_dir_cluster();
            if desk_c == 0 { 0 } else {
                if is_dir {
                    crate::fat32::create_dir(desk_c, &name_bytes[..nlen]);
                } else {
                    crate::fs::fat32_create_and_open(desk_c, &name_bytes[..nlen]);
                }
                // find_in_dir right after creation — the entry is guaranteed to exist now.
                crate::fat32::find_in_dir(desk_c, &name_bytes[..nlen])
                    .map(|de| de.cluster)
                    .unwrap_or(0)
            }
        } else { 0 };
        // Place the desktop item near the spawn position
        let pr = self.desk_prompt.rect(self.sw, self.sh);
        let ix = (self.desk_prompt.spawn_x - (DI_W as i32 / 2))
            .max(0).min((self.sw.saturating_sub(DI_W)) as i32);
        let iy = (self.desk_prompt.spawn_y - DI_H as i32 - 8)
            .max(BAR_H as i32).min((self.sh.saturating_sub(DI_H)) as i32);
        let mut item = DesktopItem::blank();
        item.x = ix;
        item.y = iy;
        item.nlen = nlen;
        item.name = name_bytes;
        item.is_dir = is_dir;
        item.fat32_cluster = fat32_cluster;
        let item_rect = item.rect();
        self.desk_items[self.desk_item_count] = item;
        self.desk_item_count += 1;
        self.desk_prompt = DesktopNamePrompt::hidden();
        self.damage.add(Rect { x: pr.x, y: pr.y.saturating_sub(16), w: pr.w, h: pr.h + 30 });
        self.damage.add(item_rect);
        self.save_desktop_state();
    }

    fn open_desk_item(&mut self, idx: usize) {
        if idx >= self.desk_item_count { return; }
        let item = self.desk_items[idx];
        if item.is_dir {
            // Use the cluster stored at creation time. Fall back to a fresh
            // find_in_dir only for items loaded from an older DESKSTAT that
            // didn't record the cluster.
            let dir_cluster = if item.fat32_cluster != 0 {
                item.fat32_cluster
            } else if crate::fat32::is_mounted() {
                let desk_c = Self::desktop_dir_cluster();
                if desk_c != 0 {
                    crate::fat32::find_in_dir(desk_c, &item.name[..item.nlen])
                        .map(|de| de.cluster)
                        .unwrap_or(0)
                } else { 0 }
            } else { 0 };
            let app = Box::new(crate::filemanager::FileManagerApp::open_dir(
                dir_cluster, &item.name[..item.nlen]));
            self.open_window(app);
        } else {
            // Open the file in the editor — find it without overwriting its content.
            let desk_c = Self::desktop_dir_cluster();
            if desk_c != 0 {
            if let Some(fid) = crate::fs::fat32_find_and_open(desk_c, &item.name[..item.nlen]) {
                // Build /fat32/<hex-id> path
                let mut buf = [0u8; 32];
                let prefix = b"/fat32/";
                buf[..prefix.len()].copy_from_slice(prefix);
                let mut hi = prefix.len();
                let mut v = fid as u32;
                let mut tmp = [0u8; 8];
                let mut tlen = 0usize;
                loop {
                    let n = (v & 0xF) as u8;
                    tmp[tlen] = if n < 10 { b'0' + n } else { b'a' + n - 10 };
                    tlen += 1;
                    v >>= 4;
                    if v == 0 { break; }
                }
                tmp[..tlen].reverse();
                buf[hi..hi + tlen].copy_from_slice(&tmp[..tlen]);
                hi += tlen;
                if let Some(path) = core::str::from_utf8(&buf[..hi]).ok() {
                    let ed = Box::new(crate::editor::EditorApp::open(path));
                    self.open_window(ed);
                }
            }
            } // if desk_c != 0
        }
        self.damage.mark_full();
    }

    // ── Desktop directory helper ──────────────────────────────────────────────
    // All desktop items and DESKSTAT live inside a "Desktop" folder on FAT32.
    // This helper ensures the folder exists and returns its cluster.
    fn desktop_dir_cluster() -> u32 {
        if !crate::fat32::is_mounted() { return 0; }
        let root_c = crate::fat32::root_cluster();
        if root_c == 0 { return 0; }
        // Return existing Desktop folder cluster, creating it if absent.
        if let Some(de) = crate::fat32::find_in_dir(root_c, b"Desktop") {
            if de.cluster >= 2 { return de.cluster; }
        }
        crate::fat32::create_dir(root_c, b"Desktop");
        crate::fat32::find_in_dir(root_c, b"Desktop")
            .map(|de| de.cluster)
            .unwrap_or(0)
    }

    // ── Desktop state persistence ──────────────────────────────────────────
    // Binary file "DESKSTAT" in the FAT32 Desktop/ folder.
    // Format (v2, magic b"DSK2"):
    //   [0..4]  magic b"DSK2"
    //   [4]     app_count (= NUM_APPS)
    //   [5]     desk_item_count
    //   [6..8]  pad
    //   per app icon  (app_count * 8 bytes): x:i32 LE, y:i32 LE
    //   per desk item (item_count * 48 bytes):
    //     x:i32 LE, y:i32 LE, nlen:u8, is_dir:u8, pad:2,
    //     cluster:u32 LE, name:[u8;32]

    fn save_desktop_state(&self) {
        if !crate::fat32::is_mounted() { return; }
        const SZ: usize = 8 + NUM_APPS * 8 + MAX_DESK_ITEMS * 48;
        let mut buf = [0u8; SZ];
        let mut p = 0usize;
        buf[p..p + 4].copy_from_slice(b"DSK2"); p += 4;
        buf[p]     = NUM_APPS as u8;
        buf[p + 1] = self.desk_item_count as u8;
        p += 4; // [4]=app_count [5]=item_count [6..8]=pad
        for i in 0..NUM_APPS {
            buf[p..p + 4].copy_from_slice(&self.icons[i].x.to_le_bytes()); p += 4;
            buf[p..p + 4].copy_from_slice(&self.icons[i].y.to_le_bytes()); p += 4;
        }
        for i in 0..self.desk_item_count {
            let it = &self.desk_items[i];
            buf[p..p + 4].copy_from_slice(&it.x.to_le_bytes());           p += 4;
            buf[p..p + 4].copy_from_slice(&it.y.to_le_bytes());           p += 4;
            buf[p]     = it.nlen as u8;
            buf[p + 1] = it.is_dir as u8;
            // p+2, p+3 = pad (zeroed)
            buf[p + 4..p + 8].copy_from_slice(&it.fat32_cluster.to_le_bytes());
            buf[p + 8..p + 40].copy_from_slice(&it.name);
            p += 40; // 1+1+2+4+32
        }
        let desk_c = Self::desktop_dir_cluster();
        if desk_c != 0 {
            crate::fat32::write_file(desk_c, b"DESKSTAT", &buf[..p]);
        }
    }

    fn load_desktop_state(&mut self) {
        if !crate::fat32::is_mounted() { return; }
        const SZ: usize = 8 + NUM_APPS * 8 + MAX_DESK_ITEMS * 48;
        let desk_c = Self::desktop_dir_cluster();
        if desk_c == 0 { return; }
        let de = match crate::fat32::find_in_dir(desk_c, b"DESKSTAT") {
            Some(d) => d,
            None    => return,
        };
        if de.size < 8 { return; }
        let mut buf = [0u8; SZ];
        let nread = crate::fat32::read_file(de.cluster, de.size, &mut buf);
        if nread < 8 || &buf[0..4] != b"DSK2" { return; }
        let app_count  = buf[4] as usize;
        let item_count = buf[5] as usize;
        let mut p = 8usize;
        // App icon positions
        for i in 0..app_count.min(NUM_APPS) {
            if p + 8 > nread { return; }
            self.icons[i].x = i32::from_le_bytes([buf[p], buf[p+1], buf[p+2], buf[p+3]]);
            self.icons[i].y = i32::from_le_bytes([buf[p+4], buf[p+5], buf[p+6], buf[p+7]]);
            p += 8;
        }
        if app_count > NUM_APPS { p += (app_count - NUM_APPS) * 8; }
        // Desk items
        let n = item_count.min(MAX_DESK_ITEMS);
        self.desk_item_count = 0;
        for i in 0..n {
            if p + 48 > nread { break; }
            let x           = i32::from_le_bytes([buf[p],    buf[p+1],  buf[p+2],  buf[p+3]]);
            let y           = i32::from_le_bytes([buf[p+4],  buf[p+5],  buf[p+6],  buf[p+7]]);
            let nlen        = (buf[p+8] as usize).min(32);
            let is_dir      = buf[p+9] != 0;
            let fat32_cluster = u32::from_le_bytes([buf[p+12], buf[p+13], buf[p+14], buf[p+15]]);
            let mut name    = [0u8; 32];
            name.copy_from_slice(&buf[p+16..p+48]);
            let mut item = DesktopItem::blank();
            item.x = x; item.y = y; item.nlen = nlen; item.is_dir = is_dir;
            item.fat32_cluster = fat32_cluster; item.name = name;
            self.desk_items[i] = item;
            self.desk_item_count += 1;
            p += 48;
        }
    }

    fn snap_icon(&mut self, idx: usize) {
        let gx = ICON_GRID_X as i32;
        let gy = (BAR_H + ICON_GRID_Y) as i32;
        let sx = ICON_SNAP_STEP_X as i32;
        let sy = ICON_SNAP_STEP_Y as i32;
        let icon = &mut self.icons[idx];
        let col = ((icon.x - gx + sx / 2) / sx).max(0);
        let row = ((icon.y - gy + sy / 2) / sy).max(0);
        icon.x = (gx + col * sx).min((self.sw.saturating_sub(ICON_CELL_W)) as i32);
        icon.y = (gy + row * sy).min((self.sh.saturating_sub(ICON_CELL_H)) as i32);
        self.save_desktop_state();
    }

    fn on_button_release(&mut self) {
        // Flush any drag frames that were skipped by the rate-limiter so the
        // window snaps to its final position with no ghost artifact.
        if let Some(ref ds) = self.drag {
            let idx = ds.win_idx;
            if idx < self.windows.len() {
                let final_b = self.windows[idx].bounds();
                let flush = match self.drag_damage_accum {
                    Some(prev) => prev.union(&final_b),
                    None       => final_b,
                };
                self.damage.add(flush);
            }
        }
        self.drag_damage_accum = None;
        self.drag = None;
        self.resize = None;
        if let Some(ref ids) = self.icon_drag {
            let ii = ids.idx;
            if ids.moved {
                // Snap to nearest grid cell and mark both old and new positions dirty.
                let pre_snap = icon_rect_of(&self.icons[ii]);
                self.damage.add(pre_snap);
                self.snap_icon(ii);
                self.damage.add(icon_rect_of(&self.icons[ii]));
            } else {
                // Plain click: toggle selection (was set true on press).
                self.icons[ii].selected = !self.icons[ii].selected;
                self.damage.add(icon_rect_of(&self.icons[ii]));
            }
        }
        self.icon_drag = None;
        // Release desk item drag — only save if the item actually moved
        let should_save = self.desk_item_drag.as_ref().map_or(false, |d| d.moved);
        if let Some(ref ddi) = self.desk_item_drag {
            if ddi.idx < self.desk_item_count {
                self.damage.add(self.desk_items[ddi.idx].rect());
            }
        }
        self.desk_item_drag = None;
        if should_save { self.save_desktop_state(); }
        self.update_cursor_shape();
    }

    fn on_mouse_scroll(&mut self, delta: i32) {
        if let Some(fidx) = self.focused {
            if fidx < self.windows.len() && !self.windows[fidx].minimized {
                let act = self.windows[fidx].app.handle_mouse_scroll(delta);
                self.handle_app_action(fidx, act);
            }
        }
    }

    fn on_key(&mut self, key: Key) {
        // ── Desktop name-entry prompt ─────────────────────────────────────
        if self.desk_prompt.active {
            let pr = self.desk_prompt.rect(self.sw, self.sh);
            match key {
                Key::Escape => {
                    self.desk_prompt = DesktopNamePrompt::hidden();
                    self.damage.add(pr);
                    self.damage.add(Rect { x: pr.x, y: pr.y.saturating_sub(16), w: pr.w, h: pr.h + 30 });
                }
                Key::Enter => { self.commit_desk_prompt(); }
                Key::Backspace => {
                    if self.desk_prompt.len > 0 {
                        self.desk_prompt.len -= 1;
                        self.desk_prompt.buf[self.desk_prompt.len] = 0;
                        self.damage.add(Rect { x: pr.x, y: pr.y.saturating_sub(16), w: pr.w, h: pr.h + 30 });
                    }
                }
                Key::Char(c) => {
                    let invalid_char = matches!(c, b'/' | b'\\' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|');
                    if c >= 0x20 && c < 0x7F && !invalid_char && self.desk_prompt.len < 28 {
                        self.desk_prompt.buf[self.desk_prompt.len] = c;
                        self.desk_prompt.len += 1;
                        self.damage.add(Rect { x: pr.x, y: pr.y.saturating_sub(16), w: pr.w, h: pr.h + 30 });
                    }
                }
                _ => {}
            }
            return;
        }

        if let Some(fidx) = self.focused {
            if fidx < self.windows.len() && !self.windows[fidx].minimized {
                let act = self.windows[fidx].app.handle_key(key);
                // Only close on Escape if the app returned Close (not handled by app itself)
                if key == Key::Escape && matches!(act, AppAction::Nothing) {
                    self.close_window(fidx);
                    return;
                }
                self.handle_app_action(fidx, act);
            }
        }
    }

    fn handle_app_action(&mut self, win_idx: usize, action: AppAction) {
        match action {
            AppAction::Nothing => {}
            AppAction::Close => { self.close_window(win_idx); }
            AppAction::RedrawAll => {
                if win_idx < self.windows.len() {
                    self.windows[win_idx].surface_valid = false;
                    let b = self.windows[win_idx].client_rect();
                    self.damage.add(b);
                }
            }
            AppAction::RedrawArea(rx, ry, rw, rh) => {
                if win_idx < self.windows.len() {
                    self.windows[win_idx].surface_valid = false;
                    let cr = self.windows[win_idx].client_rect();
                    let rx = rx.min(cr.w);
                    let ry = ry.min(cr.h);
                    let rw = rw.min(cr.w.saturating_sub(rx));
                    let rh = rh.min(cr.h.saturating_sub(ry));
                    if rw != 0 && rh != 0 {
                        self.damage.add(Rect { x: cr.x + rx, y: cr.y + ry, w: rw, h: rh });
                    }
                }
            }
            AppAction::RedrawInput => {
                if win_idx < self.windows.len() {
                    self.windows[win_idx].surface_valid = false;
                    let cr = self.windows[win_idx].client_rect();
                    if let Some(ih) = self.windows[win_idx].app.input_region_height() {
                        let iy = cr.y + cr.h.saturating_sub(ih);
                        self.damage.add(Rect { x: cr.x, y: iy, w: cr.w, h: ih });
                    } else {
                        self.damage.add(cr);
                    }
                }
            }
            AppAction::OpenFile(path_bytes, path_len) => {
                let path = core::str::from_utf8(&path_bytes[..path_len]).unwrap_or("");
                // Route .ppm files to the image viewer; everything else to the editor.
                let lower_path = path;
                if lower_path.ends_with(".ppm") || lower_path.ends_with(".PPM") {
                    let viewer = Box::new(ImageViewerApp::open(path));
                    self.open_window(viewer);
                } else {
                    let editor = Box::new(EditorApp::open(path));
                    self.open_window(editor);
                }
            }
        }
    }

    fn launch_app(&mut self, idx: usize) {
        if idx >= NUM_APPS { return; }
        // Factory is defined in APP_REGISTRY — adding a new app there is the
        // only change needed; this function never needs to be touched again.
        let app = (APP_REGISTRY[idx].make)();
        self.open_window(app);
    }
}

// ── Numeric formatting ────────────────────────────────────────────────────────

fn fmt_hms(buf: &mut [u8; 24], h: u64, m: u64, s: u64) -> usize {
    let mut i = 0usize;
    fn pu(buf: &mut [u8; 24], i: &mut usize, n: u64) {
        if n >= 10 { buf[*i] = b'0' + (n / 10) as u8; *i += 1; }
        buf[*i] = b'0' + (n % 10) as u8; *i += 1;
    }
    pu(buf, &mut i, h); buf[i] = b':'; i += 1;
    if m < 10 { buf[i] = b'0'; i += 1; } pu(buf, &mut i, m); buf[i] = b':'; i += 1;
    if s < 10 { buf[i] = b'0'; i += 1; } pu(buf, &mut i, s);
    i
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run() -> ! {
    // Ensure the VFS is mounted before any app tries to use fs::lookup / fs::open.
    // The boot phases that call probe_vfs() may be skipped, so we guarantee it here.
    let _ = crate::fs::mount_root();

    let (sw, sh) = framebuffer::dimensions().unwrap_or((1280, 800));
    let mut desktop = Desktop::new(sw, sh);
    desktop.load_desktop_state();
    desktop.damage.mark_full();

    let mut cursor_moved = false;
    let mut ev_buf = [Event::KeyPress(Key::Unknown(0)); 16];

    loop {
        let count = input::poll_events(&mut ev_buf);

        // ── Coalesce mouse moves ───────────────────────────────────────────
        // Only the last MouseMove position in the batch matters; intermediate
        // positions just cause redundant hover recalculation and extra damage.
        // Find the index of the last MouseMove (if any).
        let last_mm: Option<usize> = (0..count).rev()
            .find(|&i| matches!(ev_buf[i], Event::MouseMove(..)));

        for i in 0..count {
            match ev_buf[i] {
                Event::MouseMove(mx, my) => {
                    if Some(i) == last_mm {
                        // Only process the final move in the batch.
                        desktop.on_mouse_move(mx, my);
                        cursor_moved = true;
                    } else {
                        // Earlier move: update position only (no hover, no damage).
                        desktop.cursor_x = mx;
                        desktop.cursor_y = my;
                        cursor_moved = true;
                    }
                }
                Event::MouseButton(buttons) => {
                    let prev = desktop.prev_btn_state;
                    desktop.prev_btn_state = buttons;
                    let pressed  =  buttons & !prev;
                    let released = !buttons &  prev;
                    let (mx, my) = (desktop.cursor_x, desktop.cursor_y);
                    if pressed & 1 != 0 { desktop.on_button_press(mx, my); }
                    if pressed & 2 != 0 { desktop.on_right_button_press(mx, my); }
                    if released & 1 != 0 { desktop.on_button_release(); }
                }
                Event::MouseScroll(delta) => { desktop.on_mouse_scroll(delta); }
                Event::KeyPress(key) => { desktop.on_key(key); }
            }
        }

        let now = uptime_ms();

        // Poll virtio-net for incoming frames and dispatch them to the network stack
        crate::net::poll_and_dispatch();

        desktop.tick_live_windows(now);

        // Update window snapshot for SysMonitor
        {
            let mut tbl = WIN_TABLE.lock();
            tbl.count = 0;
            for win in &desktop.windows {
                if tbl.count >= WIN_SNAP_MAX { break; }
                let mut snap = WinSnap::empty();
                let t = win.app.title().as_bytes();
                let tl = t.len().min(32);
                snap.title[..tl].copy_from_slice(&t[..tl]);
                snap.title_len = tl;
                let id = win.app.app_id().as_bytes();
                let il = id.len().min(16);
                snap.app_id[..il].copy_from_slice(&id[..il]);
                snap.id_len = il;
                snap.minimized = win.minimized;
                let idx = tbl.count;
                tbl.snaps[idx] = snap;
                tbl.count += 1;
            }
        }

        if !desktop.damage.is_empty() {
            desktop.present_damage();
            cursor_moved = false;
        } else if cursor_moved {
            desktop.cursor_move_fast();
            cursor_moved = false;
        } else {
            // Sleep until the next periodic frame is due, but wake immediately
            // on any input so hover/keypress latency stays near-zero.
            // Strategy: HLT once (yields until next interrupt — PIT at 100Hz,
            // PS/2 mouse IRQ, PS/2 keyboard IRQ), then poll the PS/2 FIFO.
            // If input is pending, break out and process it; otherwise HLT again
            // until the deadline or more input arrives.
            let wakeup_ms = desktop.next_wakeup_ms(now);
            let deadline_ticks = if wakeup_ms < u64::MAX {
                let hz = timer_hz() as u64;
                let sleep_ms = wakeup_ms.saturating_sub(now);
                timer_ticks().saturating_add(((sleep_ms * hz + 999) / 1000).max(1))
            } else {
                u64::MAX
            };
            loop {
                idle_once(); // HLT — wakes on any IRQ (PIT, PS/2 kbd, PS/2 mouse)
                // Drain PS/2 FIFO immediately after waking — before tick check.
                // This picks up any mouse/kbd data that arrived since last poll.
                crate::drivers::mouse::poll_aux_bytes();
                if crate::drivers::keyboard::scancode_count() > 0
                    || crate::drivers::mouse::has_pending_packets()
                    || timer_ticks() >= deadline_ticks
                {
                    break;
                }
            }
        }
    }
}
