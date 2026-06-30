// ---------------------------------------------------------------------------
// Astra OS — Tetris  (built-in game, Phase 11 gaming milestone)
//
// Classic Tetris with all 7 standard tetrominoes, hard/soft drop, hold,
// next-piece preview, level scaling, and line-clear scoring.
//
// Controls:
//   Left / Right    — move piece
//   Up / Z          — rotate clockwise / counter-clockwise
//   Down            — soft drop (1 row, +1 pt/row)
//   Space           — hard drop (instant lock, +2 pts/row)
//   C               — hold piece
//   R               — restart
//   P / Space(game over) — pause
//
// Interior mutability pattern: `render(&self)` drives time-based gravity
// via a raw-pointer cast — safe in Astra's single-threaded kernel context.
// ---------------------------------------------------------------------------

use crate::app::{App, AppAction};
use crate::arch::x86_64::interrupts::uptime_ms;
use crate::framebuffer;
use crate::input::Key;
use core::cell::UnsafeCell;

// ── Board dimensions ──────────────────────────────────────────────────────────

const COLS: usize = 10;
const ROWS: usize = 20;
const CELL: usize = 18; // px per grid cell

// ── Layout ────────────────────────────────────────────────────────────────────

const BOARD_X: usize = 60; // grid left edge in client coords
const BOARD_Y: usize = 30; // grid top  edge in client coords
const BOARD_W: usize = COLS * CELL;
const BOARD_H: usize = ROWS * CELL;

// Right panel (next + hold + score) starts here
const PANEL_X: usize = BOARD_X + BOARD_W + 16;
const PANEL_W: usize = 5 * CELL;

// Preferred window size
const WIN_W: usize = PANEL_X + PANEL_W + 16;
const WIN_H: usize = BOARD_Y + BOARD_H + 30;

// ── Timing ────────────────────────────────────────────────────────────────────

// Gravity interval in ms per level (index = level, capped at 20).
const GRAVITY_MS: [u64; 21] = [
    800, 716, 633, 550, 466, 383, 300, 216, 133, 100, 83, 83, 83, 66, 66, 66, 50, 50, 50, 33, 33,
];
fn gravity_ms(level: usize) -> u64 {
    GRAVITY_MS[level.min(20)]
}

// ── Colours ───────────────────────────────────────────────────────────────────

const BG: u32 = 0x080C10;
const BOARD_BG: u32 = 0x0A0F14;
const GRID_LINE: u32 = 0x111820;
const BORDER: u32 = 0x1E3050;
const PANEL_BG: u32 = 0x090D12;
const LABEL_COL: u32 = 0x3A5878;
const VALUE_COL: u32 = 0x70A0CC;
const GHOST_COL: u32 = 0x1C2C3C; // ghost piece tint

// Tetromino colours: I O T S Z J L (indices 1-7, 0 = empty)
const PIECE_COLS: [u32; 8] = [
    0x000000, // 0 = empty
    0x00C8C8, // I — cyan
    0xC8C800, // O — yellow
    0xA000C8, // T — purple
    0x00C800, // S — green
    0xC80000, // Z — red
    0x0000C8, // J — blue
    0xC86400, // L — orange
];
// Darker shade for cell interior
const PIECE_DARK: [u32; 8] = [
    0x000000, 0x007878, 0x787800, 0x600078, 0x007800, 0x780000, 0x000078, 0x783C00,
];

const OVER_BG: u32 = 0x0C1018;
const OVER_TITLE: u32 = 0xFF5050;
const OVER_TEXT: u32 = 0x90A8C0;
const PAUSE_COL: u32 = 0xE0D040;

// ── Tetrominoes ───────────────────────────────────────────────────────────────
//
// Each piece has 4 rotations, each rotation is 4 (col, row) offsets from pivot.

type Offsets = [(i8, i8); 4];

// [piece_type 1..=7][rotation 0..4]
const PIECES: [[Offsets; 4]; 8] = [
    // 0: unused
    [[(0, 0), (0, 0), (0, 0), (0, 0)]; 4],
    // 1: I  (flat / vertical / flat / vertical)
    [
        [(-1, 0), (0, 0), (1, 0), (2, 0)], // 0: ────
        [(1, -1), (1, 0), (1, 1), (1, 2)], // 1: │
        [(-1, 1), (0, 1), (1, 1), (2, 1)], // 2: ────
        [(0, -1), (0, 0), (0, 1), (0, 2)], // 3: │
    ],
    // 2: O  (same all rotations)
    [[(0, 0), (1, 0), (0, 1), (1, 1)]; 4],
    // 3: T
    [
        [(0, 0), (1, 0), (2, 0), (1, 1)], // 0: ▲
        [(1, 0), (1, 1), (1, 2), (0, 1)], // 1: ◄
        [(1, 0), (0, 1), (1, 1), (2, 1)], // 2: ▼
        [(0, 0), (0, 1), (0, 2), (1, 1)], // 3: ►
    ],
    // 4: S
    [
        [(1, 0), (2, 0), (0, 1), (1, 1)],
        [(0, 0), (0, 1), (1, 1), (1, 2)],
        [(1, 0), (2, 0), (0, 1), (1, 1)],
        [(0, 0), (0, 1), (1, 1), (1, 2)],
    ],
    // 5: Z
    [
        [(0, 0), (1, 0), (1, 1), (2, 1)],
        [(1, 0), (0, 1), (1, 1), (0, 2)],
        [(0, 0), (1, 0), (1, 1), (2, 1)],
        [(1, 0), (0, 1), (1, 1), (0, 2)],
    ],
    // 6: J
    [
        [(0, 0), (0, 1), (1, 1), (2, 1)],
        [(0, 0), (1, 0), (0, 1), (0, 2)],
        [(0, 0), (1, 0), (2, 0), (2, 1)],
        [(1, 0), (1, 1), (0, 2), (1, 2)],
    ],
    // 7: L
    [
        [(2, 0), (0, 1), (1, 1), (2, 1)],
        [(0, 0), (0, 1), (0, 2), (1, 2)],
        [(0, 0), (1, 0), (2, 0), (0, 1)],
        [(0, 0), (1, 0), (1, 1), (1, 2)],
    ],
];

// Spawn column / row for each piece so they appear centred at the top
const SPAWN_COL: [i8; 8] = [0, 3, 4, 3, 3, 3, 3, 3];
const SPAWN_ROW: [i8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];

// ── Board ─────────────────────────────────────────────────────────────────────

type Board = [[u8; COLS]; ROWS];

// ── Game state ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Playing,
    Paused,
    GameOver,
}

struct TetrisState {
    board: Board,
    phase: Phase,

    // Current piece
    piece: u8,  // 1..=7
    rot: usize, // 0..4
    pc: i8,     // pivot column
    pr: i8,     // pivot row

    // Hold
    hold: u8,        // 0 = empty
    hold_used: bool, // can only hold once per piece

    // Next piece bag (7-bag randomiser)
    bag: [u8; 7],
    bag_pos: usize,

    // Stats
    score: u32,
    lines: u32,
    level: usize,

    // Timing
    last_drop_ms: u64,

    // Lock delay: give a short window to slide after landing
    lock_ms: u64, // 0 = not in lock-delay
}

include!("tetris/state.rs");
include!("tetris/app_wrapper.rs");
include!("tetris/render.rs");
include!("tetris/app_impl.rs");
