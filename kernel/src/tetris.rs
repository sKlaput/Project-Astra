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

impl TetrisState {
    fn new() -> Self {
        let mut s = TetrisState {
            board: [[0u8; COLS]; ROWS],
            phase: Phase::Playing,
            piece: 0,
            rot: 0,
            pc: 0,
            pr: 0,
            hold: 0,
            hold_used: false,
            bag: [1, 2, 3, 4, 5, 6, 7],
            bag_pos: 7, // trigger refill immediately
            score: 0,
            lines: 0,
            level: 0,
            last_drop_ms: uptime_ms(),
            lock_ms: 0,
        };
        s.shuffle_bag();
        s.bag_pos = 0;
        s.spawn_next();
        s
    }

    fn reset(&mut self) {
        *self = TetrisState::new();
    }

    // ── Bag randomiser (simple LCG seeded from uptime) ────────────────────

    fn shuffle_bag(&mut self) {
        // LCG: just use uptime as entropy source
        let mut seed = uptime_ms() ^ 0xDEAD_BEEF_1234_5678;
        for i in (1..7usize).rev() {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = (seed >> 33) as usize % (i + 1);
            self.bag.swap(i, j);
        }
        self.bag_pos = 0;
    }

    fn next_piece(&mut self) -> u8 {
        if self.bag_pos >= 7 {
            self.shuffle_bag();
        }
        let p = self.bag[self.bag_pos];
        self.bag_pos += 1;
        p
    }

    fn peek_next(&self) -> u8 {
        if self.bag_pos < 7 {
            self.bag[self.bag_pos]
        } else {
            self.bag[0]
        }
    }

    // ── Piece movement / rotation ─────────────────────────────────────────

    fn cells(&self, p: u8, r: usize, col: i8, row: i8) -> [(i8, i8); 4] {
        let offs = PIECES[p as usize][r];
        [
            (col + offs[0].0, row + offs[0].1),
            (col + offs[1].0, row + offs[1].1),
            (col + offs[2].0, row + offs[2].1),
            (col + offs[3].0, row + offs[3].1),
        ]
    }

    fn current_cells(&self) -> [(i8, i8); 4] {
        self.cells(self.piece, self.rot, self.pc, self.pr)
    }

    fn valid(&self, p: u8, r: usize, col: i8, row: i8) -> bool {
        for (c, rr) in self.cells(p, r, col, row) {
            if c < 0 || c >= COLS as i8 {
                return false;
            }
            if rr >= ROWS as i8 {
                return false;
            }
            if rr >= 0 && self.board[rr as usize][c as usize] != 0 {
                return false;
            }
        }
        true
    }

    fn ghost_row(&self) -> i8 {
        let mut r = self.pr;
        while self.valid(self.piece, self.rot, self.pc, r + 1) {
            r += 1;
        }
        r
    }

    fn spawn_next(&mut self) {
        let p = self.next_piece();
        let sc = SPAWN_COL[p as usize];
        let sr = SPAWN_ROW[p as usize];
        if !self.valid(p, 0, sc, sr) {
            self.phase = Phase::GameOver;
        } else {
            self.piece = p;
            self.rot = 0;
            self.pc = sc;
            self.pr = sr;
            self.hold_used = false;
            self.lock_ms = 0;
        }
    }

    fn try_move(&mut self, dc: i8, dr: i8) -> bool {
        if self.valid(self.piece, self.rot, self.pc + dc, self.pr + dr) {
            self.pc += dc;
            self.pr += dr;
            true
        } else {
            false
        }
    }

    fn try_rotate(&mut self, cw: bool) {
        let new_r = if cw {
            (self.rot + 1) % 4
        } else {
            (self.rot + 3) % 4
        };
        // Try rotation, then wall-kick offsets ±1 col, then ±2 col
        let kicks: &[(i8, i8)] = &[(0, 0), (1, 0), (-1, 0), (2, 0), (-2, 0), (0, -1), (0, 1)];
        for &(kc, kr) in kicks {
            if self.valid(self.piece, new_r, self.pc + kc, self.pr + kr) {
                self.rot = new_r;
                self.pc += kc;
                self.pr += kr;
                self.lock_ms = 0; // reset lock delay on rotate
                return;
            }
        }
    }

    fn hard_drop(&mut self) {
        let gr = self.ghost_row();
        let dist = (gr - self.pr) as u32;
        self.pr = gr;
        self.score += dist * 2;
        self.lock_piece();
    }

    fn lock_piece(&mut self) {
        let cells = self.current_cells();
        for (c, r) in cells {
            if r >= 0 && (r as usize) < ROWS && (c as usize) < COLS {
                self.board[r as usize][c as usize] = self.piece;
            }
        }
        let cleared = self.clear_lines();
        self.score += match cleared {
            1 => 100 * (self.level as u32 + 1),
            2 => 300 * (self.level as u32 + 1),
            3 => 500 * (self.level as u32 + 1),
            4 => 800 * (self.level as u32 + 1),
            _ => 0,
        };
        self.lines += cleared;
        self.level = (self.lines / 10) as usize;
        self.last_drop_ms = uptime_ms();
        self.lock_ms = 0;
        self.spawn_next();
    }

    fn clear_lines(&mut self) -> u32 {
        let mut cleared = 0u32;
        let mut write = ROWS;
        for read in (0..ROWS).rev() {
            if self.board[read].iter().all(|&c| c != 0) {
                cleared += 1;
            } else {
                write -= 1;
                self.board[write] = self.board[read];
            }
        }
        for r in 0..write {
            self.board[r] = [0u8; COLS];
        }
        cleared
    }

    fn do_hold(&mut self) {
        if self.hold_used {
            return;
        }
        self.hold_used = true;
        if self.hold == 0 {
            self.hold = self.piece;
            self.spawn_next();
        } else {
            let tmp = self.hold;
            self.hold = self.piece;
            let sc = SPAWN_COL[tmp as usize];
            let sr = SPAWN_ROW[tmp as usize];
            if self.valid(tmp, 0, sc, sr) {
                self.piece = tmp;
                self.rot = 0;
                self.pc = sc;
                self.pr = sr;
                self.lock_ms = 0;
            } else {
                self.phase = Phase::GameOver;
            }
        }
    }

    // ── Per-frame gravity tick ────────────────────────────────────────────

    fn tick(&mut self) {
        if self.phase != Phase::Playing {
            return;
        }
        let now = uptime_ms();
        let gms = gravity_ms(self.level);

        if now.wrapping_sub(self.last_drop_ms) >= gms {
            self.last_drop_ms = now;
            if !self.try_move(0, 1) {
                // Can't drop — enter lock delay
                if self.lock_ms == 0 {
                    self.lock_ms = now;
                } else if now.wrapping_sub(self.lock_ms) >= 500 {
                    self.lock_piece();
                }
            } else {
                self.lock_ms = 0;
            }
        }
    }
}

// ── App wrapper ───────────────────────────────────────────────────────────────

pub struct TetrisApp {
    inner: UnsafeCell<TetrisState>,
}

impl TetrisApp {
    pub fn new() -> Self {
        TetrisApp {
            inner: UnsafeCell::new(TetrisState::new()),
        }
    }

    fn state_mut(&mut self) -> &mut TetrisState {
        self.inner.get_mut()
    }
}

// ── Rendering helpers ─────────────────────────────────────────────────────────

fn draw_cell(cx: usize, cy: usize, col_idx: u8, ghost: bool) {
    if col_idx == 0 {
        return;
    }
    let bg = if ghost {
        GHOST_COL
    } else {
        PIECE_COLS[col_idx as usize]
    };
    let dk = if ghost {
        GHOST_COL
    } else {
        PIECE_DARK[col_idx as usize]
    };
    // Fill with bright color, then slightly darker inset
    framebuffer::fill_rect(cx, cy, CELL, CELL, bg);
    framebuffer::fill_rect(cx + 2, cy + 2, CELL - 4, CELL - 4, dk);
}

fn draw_mini_piece(piece: u8, px: usize, py: usize) {
    if piece == 0 {
        return;
    }
    const S: usize = 7; // mini-cell size
    let offs = PIECES[piece as usize][0];
    // Find bounding box
    let min_c = offs.iter().map(|&(c, _)| c).min().unwrap_or(0);
    let min_r = offs.iter().map(|&(_, r)| r).min().unwrap_or(0);
    let col = PIECE_COLS[piece as usize];
    let dk = PIECE_DARK[piece as usize];
    for (c, r) in offs {
        let x = px + (c - min_c) as usize * S;
        let y = py + (r - min_r) as usize * S;
        framebuffer::fill_rect(x, y, S, S, col);
        framebuffer::fill_rect(x + 1, y + 1, S - 2, S - 2, dk);
    }
}

fn draw_label(x: usize, y: usize, s: &str) {
    framebuffer::draw_text_at(x, y, s, LABEL_COL);
}

fn draw_value_u32(x: usize, y: usize, v: u32) {
    let mut buf = [0u8; 12];
    let mut i = 12usize;
    let mut n = v;
    if n == 0 {
        buf[11] = b'0';
        i = 11;
    }
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if let Ok(s) = core::str::from_utf8(&buf[i..]) {
        framebuffer::draw_text_at(x, y, s, VALUE_COL);
    }
}

// ── App trait ─────────────────────────────────────────────────────────────────

impl App for TetrisApp {
    fn title(&self) -> &str {
        "Tetris"
    }
    fn preferred_size(&self) -> (usize, usize) {
        (WIN_W, WIN_H)
    }
    fn app_id(&self) -> &'static str {
        "tetris"
    }
    fn allow_multiple_instances(&self) -> bool {
        false
    }

    fn refresh_interval_ms(&self) -> Option<u64> {
        Some(50) // 20 fps max
    }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        // Advance game logic via interior-mutability cast (single-threaded kernel).
        let s = unsafe { &mut *self.inner.get() };
        s.tick();

        framebuffer::fill_rect(cx, cy, cw, ch, BG);

        // ── Board background ──────────────────────────────────────────────
        let bx = cx + BOARD_X;
        let by = cy + BOARD_Y;
        framebuffer::fill_rect(bx, by, BOARD_W, BOARD_H, BOARD_BG);
        // Grid lines
        for c in 1..COLS {
            framebuffer::fill_rect(bx + c * CELL, by, 1, BOARD_H, GRID_LINE);
        }
        for r in 1..ROWS {
            framebuffer::fill_rect(bx, by + r * CELL, BOARD_W, 1, GRID_LINE);
        }
        // Board border
        framebuffer::fill_rect(
            bx.saturating_sub(2),
            by.saturating_sub(2),
            BOARD_W + 4,
            2,
            BORDER,
        );
        framebuffer::fill_rect(bx.saturating_sub(2), by + BOARD_H, BOARD_W + 4, 2, BORDER);
        framebuffer::fill_rect(
            bx.saturating_sub(2),
            by.saturating_sub(2),
            2,
            BOARD_H + 4,
            BORDER,
        );
        framebuffer::fill_rect(bx + BOARD_W, by.saturating_sub(2), 2, BOARD_H + 4, BORDER);

        // ── Locked cells ──────────────────────────────────────────────────
        for r in 0..ROWS {
            for c in 0..COLS {
                let v = s.board[r][c];
                if v != 0 {
                    draw_cell(bx + c * CELL, by + r * CELL, v, false);
                }
            }
        }

        // ── Ghost piece ───────────────────────────────────────────────────
        if s.phase == Phase::Playing {
            let gr = s.ghost_row();
            if gr != s.pr {
                for (c, _r) in s.cells(s.piece, s.rot, s.pc, gr) {
                    if c >= 0 && (c as usize) < COLS {
                        let cc = c as usize;
                        let rr = gr.max(0) as usize;
                        if rr < ROWS {
                            draw_cell(bx + cc * CELL, by + rr * CELL, s.piece, true);
                        }
                    }
                }
            }
        }

        // ── Active piece ──────────────────────────────────────────────────
        if s.phase == Phase::Playing || s.phase == Phase::Paused {
            for (c, r) in s.current_cells() {
                if c >= 0 && r >= 0 && (c as usize) < COLS && (r as usize) < ROWS {
                    draw_cell(
                        bx + c as usize * CELL,
                        by + r as usize * CELL,
                        s.piece,
                        false,
                    );
                }
            }
        }

        // ── Right panel ───────────────────────────────────────────────────
        let px = cx + PANEL_X;
        framebuffer::fill_rect(px, cy + BOARD_Y, PANEL_W, BOARD_H, PANEL_BG);

        let mut py = cy + BOARD_Y + 4;

        // Next piece
        draw_label(px + 2, py, "NEXT");
        py += 12;
        framebuffer::fill_rect(px, py, PANEL_W, 36, BOARD_BG);
        draw_mini_piece(s.peek_next(), px + 4, py + 4);
        py += 44;

        // Hold piece
        draw_label(px + 2, py, "HOLD");
        py += 12;
        framebuffer::fill_rect(px, py, PANEL_W, 36, BOARD_BG);
        if s.hold != 0 {
            draw_mini_piece(s.hold, px + 4, py + 4);
            if s.hold_used {
                // Dim the hold box to show it can't be used again this piece
                framebuffer::fill_rect(px, py, PANEL_W, 36, 0x090D12_u32.wrapping_add(0x101010));
            }
        }
        py += 48;

        // Score
        draw_label(px + 2, py, "SCORE");
        py += 12;
        draw_value_u32(px + 2, py, s.score);
        py += 18;

        // Lines
        draw_label(px + 2, py, "LINES");
        py += 12;
        draw_value_u32(px + 2, py, s.lines);
        py += 18;

        // Level
        draw_label(px + 2, py, "LEVEL");
        py += 12;
        draw_value_u32(px + 2, py, s.level as u32);
        py += 24;

        // Controls hint
        let hints: &[&str] = &[
            "\u{2190}\u{2192}=move",
            "\u{2191}=rotate",
            "\u{2193}=soft drop",
            "SPC=hard drop",
            "C=hold",
            "P=pause",
            "R=restart",
        ];
        for h in hints {
            framebuffer::draw_text_at(px + 2, py, h, LABEL_COL);
            py += 11;
        }

        // ── Overlays ──────────────────────────────────────────────────────
        let ov_x = bx + BOARD_W / 4;
        let ov_w = BOARD_W / 2;

        if s.phase == Phase::GameOver {
            framebuffer::fill_rect(ov_x, by + BOARD_H / 3 - 10, ov_w, 54, OVER_BG);
            framebuffer::fill_rect(ov_x, by + BOARD_H / 3 - 10, ov_w, 1, BORDER);
            framebuffer::fill_rect(ov_x, by + BOARD_H / 3 + 44, ov_w, 1, BORDER);
            let ty = by + BOARD_H / 3;
            framebuffer::draw_text_at(ov_x + 4, ty, "GAME OVER", OVER_TITLE);
            framebuffer::draw_text_at(ov_x + 4, ty + 14, "R=restart", OVER_TEXT);
        } else if s.phase == Phase::Paused {
            framebuffer::fill_rect(ov_x, by + BOARD_H / 3 - 10, ov_w, 38, OVER_BG);
            framebuffer::fill_rect(ov_x, by + BOARD_H / 3 - 10, ov_w, 1, BORDER);
            framebuffer::fill_rect(ov_x, by + BOARD_H / 3 + 28, ov_w, 1, BORDER);
            let ty = by + BOARD_H / 3;
            framebuffer::draw_text_at(ov_x + 12, ty, "PAUSED", PAUSE_COL);
            framebuffer::draw_text_at(ov_x + 4, ty + 14, "P=resume", OVER_TEXT);
        }
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        let s = self.state_mut();

        // R always restarts
        if key == Key::Char(b'r') || key == Key::Char(b'R') {
            s.reset();
            return AppAction::RedrawAll;
        }

        match s.phase {
            Phase::GameOver => {
                // Any key other than R (handled above) → nothing
                return AppAction::Nothing;
            }
            Phase::Paused => {
                if key == Key::Char(b'p') || key == Key::Char(b'P') {
                    s.phase = Phase::Playing;
                    s.last_drop_ms = uptime_ms();
                    return AppAction::RedrawAll;
                }
                return AppAction::Nothing;
            }
            Phase::Playing => {}
        }

        match key {
            Key::ArrowLeft => {
                s.try_move(-1, 0);
            }
            Key::ArrowRight => {
                s.try_move(1, 0);
            }
            Key::ArrowDown => {
                if s.try_move(0, 1) {
                    s.score += 1;
                    s.last_drop_ms = uptime_ms();
                }
            }
            Key::ArrowUp => {
                s.try_rotate(true);
            }
            Key::Char(b'z') | Key::Char(b'Z') => {
                s.try_rotate(false);
            }
            Key::Char(b' ') => {
                s.hard_drop();
            }
            Key::Char(b'c') | Key::Char(b'C') => {
                s.do_hold();
            }
            Key::Char(b'p') | Key::Char(b'P') => {
                s.phase = Phase::Paused;
            }
            _ => return AppAction::Nothing,
        }

        AppAction::RedrawAll
    }
}
