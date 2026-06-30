// ---------------------------------------------------------------------------
// Astra OS — Calculator app
//
// A simple integer/decimal four-function calculator.  Mouse clicks on the
// button grid and keyboard input (digits, operators, Enter/=, Backspace, Esc)
// are both supported.
// ---------------------------------------------------------------------------

use crate::app::{App, AppAction};
use crate::framebuffer;
use crate::input::Key;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG: u32 = 0x0D1117;
const DISPLAY_BG: u32 = 0x060B10;
const DISPLAY_TXT: u32 = 0xE8F4FD;
const DISPLAY_OP: u32 = 0x4A7090;
const BTN_BG: u32 = 0x1A2332;
const BTN_HOV: u32 = 0x243448;
const BTN_BORDER: u32 = 0x2A3F5F;
const BTN_OP_BG: u32 = 0x1A3A5F;
const BTN_OP_HOV: u32 = 0x245080;
const BTN_EQ_BG: u32 = 0x1A5F3F;
const BTN_EQ_HOV: u32 = 0x24805A;
const BTN_CLR_BG: u32 = 0x5F1A1A;
const BTN_CLR_HOV: u32 = 0x802424;
const BTN_TXT: u32 = 0xD8EEFF;
const ERR_COL: u32 = 0xFF4444;

// ── Layout ────────────────────────────────────────────────────────────────────

const PAD: usize = 10;
const DISP_H: usize = 64;
const BTN_W: usize = 60;
const BTN_H: usize = 44;
const BTN_GAP: usize = 6;
const COLS: usize = 4;
const ROWS: usize = 5;

// ── Button table ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum BtnKind {
    Digit(u8), // 0-9
    Dot,
    Op(char), // + - * /
    Eq,
    Clear,     // AC
    Backspace, // ←
    Negate,    // +/-
    Percent,   // %
}

struct Btn {
    label: &'static str,
    kind: BtnKind,
}

// 5 rows × 4 cols
const BTNS: [[Btn; COLS]; ROWS] = [
    [
        Btn {
            label: "AC",
            kind: BtnKind::Clear,
        },
        Btn {
            label: "+/-",
            kind: BtnKind::Negate,
        },
        Btn {
            label: "%",
            kind: BtnKind::Percent,
        },
        Btn {
            label: "÷",
            kind: BtnKind::Op('/'),
        },
    ],
    [
        Btn {
            label: "7",
            kind: BtnKind::Digit(7),
        },
        Btn {
            label: "8",
            kind: BtnKind::Digit(8),
        },
        Btn {
            label: "9",
            kind: BtnKind::Digit(9),
        },
        Btn {
            label: "×",
            kind: BtnKind::Op('*'),
        },
    ],
    [
        Btn {
            label: "4",
            kind: BtnKind::Digit(4),
        },
        Btn {
            label: "5",
            kind: BtnKind::Digit(5),
        },
        Btn {
            label: "6",
            kind: BtnKind::Digit(6),
        },
        Btn {
            label: "−",
            kind: BtnKind::Op('-'),
        },
    ],
    [
        Btn {
            label: "1",
            kind: BtnKind::Digit(1),
        },
        Btn {
            label: "2",
            kind: BtnKind::Digit(2),
        },
        Btn {
            label: "3",
            kind: BtnKind::Digit(3),
        },
        Btn {
            label: "+",
            kind: BtnKind::Op('+'),
        },
    ],
    [
        Btn {
            label: "←",
            kind: BtnKind::Backspace,
        },
        Btn {
            label: "0",
            kind: BtnKind::Digit(0),
        },
        Btn {
            label: ".",
            kind: BtnKind::Dot,
        },
        Btn {
            label: "=",
            kind: BtnKind::Eq,
        },
    ],
];

// ── State ─────────────────────────────────────────────────────────────────────

/// Fixed-point value: stored as i64 micro-units (×1_000_000).
/// Allows 6 decimal places without floating point.
type Fixed = i64;
const SCALE: i64 = 1_000_000;

include!("calculator/fixed.rs");
include!("calculator/core.rs");
include!("calculator/app_impl.rs");
