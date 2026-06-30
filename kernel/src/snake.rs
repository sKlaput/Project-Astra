// ---------------------------------------------------------------------------
// Astra OS — Snake  (built-in game, Phase 11 early gaming milestone)
//
// Classic snake game rendered entirely via the kernel framebuffer.
// Controls: Arrow keys / WASD to steer, Space to pause, R to restart.
// Speed increases every 5 points.  High score persists to FAT32.
//
// Interior mutability pattern: `render(&self)` advances the tick via a
// raw-pointer cast — safe because Astra runs single-threaded on one core.
// ---------------------------------------------------------------------------

use crate::app::{App, AppAction};
use crate::arch::x86_64::interrupts::uptime_ms;
use crate::framebuffer;
use crate::input::Key;
use core::cell::UnsafeCell;

// ── Grid / layout constants ───────────────────────────────────────────────────

const COLS: usize = 24;
const ROWS: usize = 18;
const CELL: usize = 20; // px per grid cell
const X_OFF: usize = 40; // px from window left to grid left
const Y_OFF: usize = 44; // px from window top  to grid top
const WIN_W: usize = COLS * CELL + X_OFF * 2; // 560
const WIN_H: usize = ROWS * CELL + Y_OFF + 20; // 424

const MAX_LEN: usize = COLS * ROWS; // 432 — max possible snake length

// ── Timing ────────────────────────────────────────────────────────────────────

const BASE_MS: u64 = 150; // ms between moves at speed 0
const SPEED_INC: u64 = 8; // ms shaved off per 5 pts (min 60ms)
const MIN_MS: u64 = 60;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG: u32 = 0x060B06;
const GRID_LINE: u32 = 0x0A120A;
const BORDER: u32 = 0x1A3A1A;
const SNAKE_HEAD: u32 = 0x50F870;
const SNAKE_BODY: u32 = 0x28A840;
const SNAKE_DIM: u32 = 0x1A6828; // tail end
const FOOD: u32 = 0xFF4444;
const FOOD_INNER: u32 = 0xFF8888;
const SCORE_COL: u32 = 0x70E070;
const HI_COL: u32 = 0xE0C040;
const TITLE_COL: u32 = 0x40C060;
const OVER_BG: u32 = 0x0C1A0C;
const OVER_TITLE: u32 = 0xFF6060;
const OVER_TEXT: u32 = 0xB0C8B0;
const PAUSE_COL: u32 = 0xE0D040;
const HINT_COL: u32 = 0x1E381E;

// ── Direction ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    fn opposite(self) -> Dir {
        match self {
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
            Dir::Left => Dir::Right,
            Dir::Right => Dir::Left,
        }
    }
}

// ── Game phase ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Playing,
    Paused,
    GameOver,
    Ready,
}

// ── Core game state (lives inside UnsafeCell) ─────────────────────────────────

struct Game {
    /// Circular buffer of body segments, oldest at `tail_idx`, newest at `head_idx`.
    body: [(u8, u8); MAX_LEN],
    head_idx: usize,
    tail_idx: usize,
    len: usize,

    dir: Dir,
    next_dir: Dir,
    food: (u8, u8),

    phase: Phase,
    score: u32,
    high_score: u32,

    last_move_ms: u64,
    rng: u64,
}

include!("snake/game.rs");
include!("snake/app_wrapper.rs");
include!("snake/render.rs");
include!("snake/app_impl.rs");
include!("snake/actions.rs");
include!("snake/formatting.rs");
