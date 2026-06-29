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
use crate::arch::x86_64::interrupts::uptime_ms;
use crate::framebuffer;
use crate::fs;
use crate::input::Key;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG: u32 = 0x080C12;
const HEADER_BG: u32 = 0x0C1420;
const COLHDR_BG: u32 = 0x0A1018;
const COLHDR_COL: u32 = 0x4A6880;
const SEL_BG: u32 = 0x1C3F62; // clearly selected
const SEL_BORDER: u32 = 0x3870B0; // bright accent bar on left edge
const SEL_COL: u32 = 0xF0F8FF;
const DIR_COL: u32 = 0x5CB8FF;
const FILE_COL: u32 = 0xB8D0E4;
const SIZE_COL: u32 = 0x4A6880;
const SIZE_SEL: u32 = 0x8AB0CC;
const BORDER_COL: u32 = 0x182840;
const PATH_BG: u32 = 0x0A1828; // slightly distinct background for path row
const PATH_COL: u32 = 0x88BEDD; // bright enough to read the path clearly
const PATH_LBL: u32 = 0x3A5870; // dimmer "Location:" label
const COUNT_COL: u32 = 0x5A7C98; // item count, readable
const HINT_COL: u32 = 0x38566A;
const HINT_KEY: u32 = 0x607890;
const EMPTY_COL: u32 = 0x2A4058;
const ERR_COL: u32 = 0xC04040;
const EVEN_BG: u32 = 0x0A0F16;
const HOVER_BG: u32 = 0x10202E; // mouse-hover row (visibly brighter than even rows)
const SCROLL_BG: u32 = 0x0C141C;
const SCROLL_FG: u32 = 0x2A4060;

// Breadcrumb colours
const CRUMB_COL: u32 = 0x70B0D0; // clickable segment
const CRUMB_HOV: u32 = 0xC0E4FC; // hovered clickable segment
const CRUMB_CUR: u32 = 0x486880; // current (last) segment — dimmer
const CRUMB_SEP: u32 = 0x2A4860; // " > " separator

const MAX_CRUMBS: usize = 8;

// Context menu
const CTX_BG: u32 = 0x111E2E;
const CTX_BORDER: u32 = 0x2A4870;
const CTX_SEL_BG: u32 = 0x1E3F62;
const CTX_COL: u32 = 0xC8E0F4;
const CTX_DIS: u32 = 0x3A5870; // disabled item
const CTX_ITEM_H: usize = 16;
const CTX_PAD_X: usize = 10;
const CTX_MIN_W: usize = 130;

// ── Layout ────────────────────────────────────────────────────────────────────

const PAD_X: usize = 12;
const ROW_H: usize = 18;
const HEADER_H: usize = 26;
const COL_HDR_H: usize = 16;
const HINT_H: usize = 20;
const CHAR_W: usize = 6;
const SCROLL_W: usize = 6;
const SIZE_COL_W: usize = 64;
const PREFIX_W: usize = 4 * CHAR_W;

const MAX_ENTRIES: usize = 32;
const DBL_CLICK_MS: u64 = 450;

include!("filemanager/model.rs");
include!("filemanager/interaction.rs");
include!("filemanager/core.rs");
include!("filemanager/app.rs");
include!("filemanager/this_pc.rs");
include!("filemanager/files.rs");
include!("filemanager/context_actions.rs");
include!("filemanager/helpers.rs");
