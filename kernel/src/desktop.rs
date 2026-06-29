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

use crate::about::AboutApp;
use crate::app::{App, AppAction};
use crate::arch::x86_64::halt::idle_once;
use crate::arch::x86_64::interrupts::{timer_hz, timer_ticks, uptime_ms};
use crate::calculator::CalculatorApp;
use crate::editor::EditorApp;
use crate::filemanager::FileManagerApp;
use crate::framebuffer;
use crate::imageviewer::ImageViewerApp;
use crate::input::{self, Event, Key};
use crate::logviewer::LogViewerApp;
use crate::notes::NotesApp;
use crate::settings::SettingsApp;
use crate::snake::SnakeApp;
use crate::sysmonitor::SysMonitorApp;
use crate::terminal::TerminalApp;
use crate::tetris::TetrisApp;

use core::sync::atomic::{AtomicU32, Ordering as AO};
use spin::Mutex;

// Runtime-mutable desktop background colour (written by Settings app).
pub static DESKTOP_BG_COLOR: AtomicU32 = AtomicU32::new(0x0D1117);

// ── Window snapshot table (read by SysMonitor) ────────────────────────────────

pub const WIN_SNAP_MAX: usize = 20;

#[derive(Clone, Copy)]
pub struct WinSnap {
    pub title: [u8; 32],
    pub title_len: usize,
    pub app_id: [u8; 16],
    pub id_len: usize,
    pub minimized: bool,
}

impl WinSnap {
    const fn empty() -> Self {
        WinSnap {
            title: [0u8; 32],
            title_len: 0,
            app_id: [0u8; 16],
            id_len: 0,
            minimized: false,
        }
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

#[inline(always)]
fn desktop_bg() -> u32 {
    DESKTOP_BG_COLOR.load(AO::Relaxed)
}
const DESKTOP_BG: u32 = 0x0D1117; // kept for other callers
const BAR_BG: u32 = 0x0A0E14;
const BAR_BORDER: u32 = 0x1E3A5F;
const BAR_TEXT: u32 = 0xD0E8FF;
const BAR_BTN_BG: u32 = 0x1A2A3A;
const BAR_BTN_HOV: u32 = 0x253848;
const BAR_BTN_ACT: u32 = 0x1E3A5F;
const BAR_BTN_TEXT: u32 = 0xC8E0F8;
const BAR_UPTIME: u32 = 0x3A6080;

const WIN_SHADOW: u32 = 0x06090E;
const WIN_BORDER: u32 = 0x253848;
const WIN_BORDER_FOC: u32 = 0x2E5888;
const WIN_BG: u32 = 0x0A0E14;
const WIN_BAR_BG: u32 = 0x0C1320;
const WIN_BAR_FOC: u32 = 0x0F1E36;
const WIN_BAR_BORDER: u32 = 0x1E3A5F;
const WIN_TITLE_COL: u32 = 0xD8EEFF;
const WIN_HINT_COL: u32 = 0x3A5878;
const WIN_CLOSE_HOV: u32 = 0x7A1E1E;

const ICON_BG: u32 = 0x111820;
const ICON_SEL: u32 = 0x1A3050;
const ICON_BORDER: u32 = 0x1E3A5F;
const ICON_TEXT: u32 = 0x90B8D8;
const ICON_TEXT_SEL: u32 = 0xD8EEFF;
const ICON_ACCENT: u32 = 0x2E5888;

const LAUNCHER_BG: u32 = 0x0A0F18;
const LAUNCHER_BORD: u32 = 0x1A2F48;
const LAUNCHER_HEAD: u32 = 0x0C1830;
const LAUNCHER_TEXT: u32 = 0xD8EEFF;
const LAUNCHER_SUB: u32 = 0x4A7090;
const LAUNCHER_HOV: u32 = 0x162840;
const LAUNCHER_SEP: u32 = 0x1A2F48;

const CURSOR_WHITE: u32 = 0xFFFFFF;
const CURSOR_BLACK: u32 = 0x000000;

// ── Layout ────────────────────────────────────────────────────────────────────

const BAR_H: usize = 30;
const WIN_BAR_H: usize = 28;
const WIN_SHADOW_OFS: usize = 4;
const WIN_MIN_W: usize = 240;
const WIN_MIN_H: usize = 180;
const WIN_PAD_X: usize = 8;
const RESIZE_ZONE: usize = 6;

const ICON_CELL_W: usize = 88;
const ICON_CELL_H: usize = 78;
const ICON_GRID_X: usize = 8;
const ICON_GRID_Y: usize = 16;
const DBL_CLICK_MS: u64 = 450;

const LAUNCHER_W: usize = 220;
const LAUNCHER_HEAD_H: usize = 48;
const LAUNCHER_ITEM_H: usize = 40;
const LAUNCHER_PAD_X: usize = 14;

const ICON_SNAP_STEP_X: usize = ICON_CELL_W + 8; // = 96  (column stride)
const ICON_SNAP_STEP_Y: usize = ICON_CELL_H + 6; // = 84  (row stride, matches original spacing)

// All four apps share a single source-of-truth descriptor table.  Every
// launch path (desktop icon, launcher panel, taskbar) pulls from here so
// label, identity, and subtitle are never out of sync.
const NUM_APPS: usize = 11;
const NUM_ICONS: usize = NUM_APPS;
const NUM_LAUNCHER: usize = NUM_APPS;
const MAX_WINDOWS: usize = 12;
const MAX_DAMAGE: usize = 16;

const CURSOR_W: usize = 10;
const CURSOR_H: usize = 16;

include!("desktop/context_menu.rs");
include!("desktop/geometry.rs");
include!("desktop/cursor.rs");
include!("desktop/apps.rs");
include!("desktop/window.rs");
include!("desktop/state.rs");
include!("desktop/core.rs");
include!("desktop/rendering.rs");
include!("desktop/input.rs");
include!("desktop/desktop_items.rs");
include!("desktop/persistence.rs");
include!("desktop/events.rs");
include!("desktop/entry.rs");
