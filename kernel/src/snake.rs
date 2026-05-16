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
use crate::framebuffer;
use crate::input::Key;
use crate::arch::x86_64::interrupts::uptime_ms;
use core::cell::UnsafeCell;

// ── Grid / layout constants ───────────────────────────────────────────────────

const COLS:    usize = 24;
const ROWS:    usize = 18;
const CELL:    usize = 20;      // px per grid cell
const X_OFF:   usize = 40;      // px from window left to grid left
const Y_OFF:   usize = 44;      // px from window top  to grid top
const WIN_W:   usize = COLS * CELL + X_OFF * 2;  // 560
const WIN_H:   usize = ROWS * CELL + Y_OFF + 20; // 424

const MAX_LEN: usize = COLS * ROWS;   // 432 — max possible snake length

// ── Timing ────────────────────────────────────────────────────────────────────

const BASE_MS:   u64 = 150;   // ms between moves at speed 0
const SPEED_INC: u64 = 8;     // ms shaved off per 5 pts (min 60ms)
const MIN_MS:    u64 = 60;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG:          u32 = 0x060B06;
const GRID_LINE:   u32 = 0x0A120A;
const BORDER:      u32 = 0x1A3A1A;
const SNAKE_HEAD:  u32 = 0x50F870;
const SNAKE_BODY:  u32 = 0x28A840;
const SNAKE_DIM:   u32 = 0x1A6828;   // tail end
const FOOD:        u32 = 0xFF4444;
const FOOD_INNER:  u32 = 0xFF8888;
const SCORE_COL:   u32 = 0x70E070;
const HI_COL:      u32 = 0xE0C040;
const TITLE_COL:   u32 = 0x40C060;
const DIM_COL:     u32 = 0x204020;
const OVER_BG:     u32 = 0x0C1A0C;
const OVER_TITLE:  u32 = 0xFF6060;
const OVER_TEXT:   u32 = 0xB0C8B0;
const PAUSE_COL:   u32 = 0xE0D040;
const HINT_COL:    u32 = 0x1E381E;

// ── Direction ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir { Up, Down, Left, Right }

impl Dir {
    fn opposite(self) -> Dir {
        match self { Dir::Up => Dir::Down, Dir::Down => Dir::Up,
                     Dir::Left => Dir::Right, Dir::Right => Dir::Left }
    }
}

// ── Game phase ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase { Playing, Paused, GameOver, Ready }

// ── Core game state (lives inside UnsafeCell) ─────────────────────────────────

struct Game {
    /// Circular buffer of body segments, oldest at `tail_idx`, newest at `head_idx`.
    body:      [(u8, u8); MAX_LEN],
    head_idx:  usize,
    tail_idx:  usize,
    len:       usize,

    dir:       Dir,
    next_dir:  Dir,
    food:      (u8, u8),

    phase:     Phase,
    score:     u32,
    high_score: u32,

    last_move_ms: u64,
    rng:          u64,
}

impl Game {
    fn new() -> Self {
        let mut g = Game {
            body:        [(0, 0); MAX_LEN],
            head_idx:    0,
            tail_idx:    0,
            len:         0,
            dir:         Dir::Right,
            next_dir:    Dir::Right,
            food:        (0, 0),
            phase:       Phase::Ready,
            score:       0,
            high_score:  0,
            last_move_ms: 0,
            rng:         0x53_4173_7472_614F,
        };
        g.reset();
        g
    }

    fn reset(&mut self) {
        self.head_idx  = 2;
        self.tail_idx  = 0;
        self.len       = 3;
        self.body[0]   = (10, 9);
        self.body[1]   = (11, 9);
        self.body[2]   = (12, 9);
        self.dir       = Dir::Right;
        self.next_dir  = Dir::Right;
        self.score     = 0;
        self.last_move_ms = uptime_ms();
        self.rng ^= self.last_move_ms;
        self.place_food();
        self.phase = Phase::Playing;
    }

    fn rand(&mut self) -> u64 {
        // xorshift64
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    fn place_food(&mut self) {
        // Find a cell not occupied by the snake body.
        let total = COLS * ROWS;
        let start = (self.rand() as usize) % total;
        for i in 0..total {
            let idx = (start + i) % total;
            let fx  = (idx % COLS) as u8;
            let fy  = (idx / COLS) as u8;
            if !self.body_contains(fx, fy) {
                self.food = (fx, fy);
                return;
            }
        }
        // Grid full — game won (edge case)
        self.food = (0, 0);
    }

    fn body_contains(&self, x: u8, y: u8) -> bool {
        for i in 0..self.len {
            let idx = (self.tail_idx + i) % MAX_LEN;
            if self.body[idx] == (x, y) { return true; }
        }
        false
    }

    fn head(&self) -> (u8, u8) {
        self.body[self.head_idx]
    }

    fn move_speed_ms(&self) -> u64 {
        let level = (self.score / 5) as u64;
        BASE_MS.saturating_sub(level * SPEED_INC).max(MIN_MS)
    }

    /// Advance the game by one tick.  Returns true if state changed.
    fn tick(&mut self) -> bool {
        if self.phase != Phase::Playing { return false; }
        let now = uptime_ms();
        if now.saturating_sub(self.last_move_ms) < self.move_speed_ms() {
            return false;
        }
        self.last_move_ms = now;

        // Apply queued direction (can't reverse)
        if self.next_dir != self.dir.opposite() {
            self.dir = self.next_dir;
        }

        let (hx, hy) = self.head();
        let (nx, ny) = match self.dir {
            Dir::Up    => (hx, hy.wrapping_sub(1)),
            Dir::Down  => (hx, hy + 1),
            Dir::Left  => (hx.wrapping_sub(1), hy),
            Dir::Right => (hx + 1, hy),
        };

        // Wall collision
        if nx >= COLS as u8 || ny >= ROWS as u8 {
            self.game_over();
            return true;
        }

        // Self collision (check body except the tail tip that will be removed)
        let ate_food = (nx, ny) == self.food;
        // If not eating, the tail will move so skip tail in collision check
        let check_len = if ate_food { self.len } else { self.len - 1 };
        for i in 0..check_len {
            let idx = (self.tail_idx + i) % MAX_LEN;
            if self.body[idx] == (nx, ny) {
                self.game_over();
                return true;
            }
        }

        // Advance head
        self.head_idx = (self.head_idx + 1) % MAX_LEN;
        self.body[self.head_idx] = (nx, ny);
        self.len += 1;

        if ate_food {
            self.score += 1;
            if self.score > self.high_score { self.high_score = self.score; }
            self.place_food();
        } else {
            // Advance tail
            self.tail_idx = (self.tail_idx + 1) % MAX_LEN;
            self.len -= 1;
        }

        true
    }

    fn game_over(&mut self) {
        self.phase = Phase::GameOver;
    }
}

// ── SnakeApp ──────────────────────────────────────────────────────────────────

pub struct SnakeApp {
    inner: UnsafeCell<Game>,
}

// SAFETY: Astra OS is single-threaded (no SMP, no Send across threads).
unsafe impl Sync for SnakeApp {}

impl SnakeApp {
    pub fn new() -> Self {
        SnakeApp { inner: UnsafeCell::new(Game::new()) }
    }

    fn g(&self) -> &mut Game {
        // SAFETY: single-threaded; we never hold two &mut refs simultaneously.
        unsafe { &mut *self.inner.get() }
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn draw_cell(gx: usize, gy: usize, cx: usize, cy: usize, color: u32) {
    let px = cx + X_OFF + gx * CELL;
    let py = cy + Y_OFF + gy * CELL;
    framebuffer::fill_rect(px + 1, py + 1, CELL - 2, CELL - 2, color);
}

impl App for SnakeApp {
    fn title(&self) -> &str { "Snake" }
    fn app_id(&self) -> &'static str { "snake" }
    fn preferred_size(&self) -> (usize, usize) { (WIN_W, WIN_H) }
    fn allow_multiple_instances(&self) -> bool { false }
    fn refresh_interval_ms(&self) -> Option<u64> { Some(50) }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        let g = self.g();
        // Advance tick
        g.tick();

        // ── Background ────────────────────────────────────────────────────
        framebuffer::fill_rect(cx, cy, cw, ch, BG);

        // ── Grid lines ────────────────────────────────────────────────────
        let gx0 = cx + X_OFF;
        let gy0 = cy + Y_OFF;
        let gw  = COLS * CELL;
        let gh  = ROWS * CELL;

        // Vertical lines
        for col in 0..=COLS {
            framebuffer::fill_rect(gx0 + col * CELL, gy0, 1, gh, GRID_LINE);
        }
        // Horizontal lines
        for row in 0..=ROWS {
            framebuffer::fill_rect(gx0, gy0 + row * CELL, gw, 1, GRID_LINE);
        }
        // Border
        framebuffer::fill_rect(gx0 - 2, gy0 - 2, gw + 4, 2, BORDER);
        framebuffer::fill_rect(gx0 - 2, gy0 + gh, gw + 4, 2, BORDER);
        framebuffer::fill_rect(gx0 - 2, gy0 - 2, 2, gh + 4, BORDER);
        framebuffer::fill_rect(gx0 + gw, gy0 - 2, 2, gh + 4, BORDER);

        // ── Snake body ────────────────────────────────────────────────────
        for i in 0..g.len {
            let idx = (g.tail_idx + i) % MAX_LEN;
            let (bx, by) = g.body[idx];
            let frac = i * 3 / g.len.max(1);   // 0,1,2 for dim/mid/bright
            let col = if i + 1 == g.len {
                SNAKE_HEAD
            } else if frac < 1 {
                SNAKE_DIM
            } else {
                SNAKE_BODY
            };
            draw_cell(bx as usize, by as usize, cx, cy, col);
        }

        // ── Food ──────────────────────────────────────────────────────────
        {
            let (fx, fy) = g.food;
            let px = cx + X_OFF + fx as usize * CELL;
            let py = cy + Y_OFF + fy as usize * CELL;
            framebuffer::fill_rect(px + 2, py + 2, CELL - 4, CELL - 4, FOOD);
            framebuffer::fill_rect(px + 5, py + 5, CELL - 10, CELL - 10, FOOD_INNER);
        }

        // ── Score bar (top) ───────────────────────────────────────────────
        {
            let mut buf = [0u8; 32];
            let len = fmt_score(&mut buf, b"SCORE ", g.score);
            let s   = core::str::from_utf8(&buf[..len]).unwrap_or("");
            framebuffer::draw_text_at(cx + X_OFF, cy + 6, s, SCORE_COL);

            let mut hbuf = [0u8; 32];
            let hlen = fmt_score(&mut hbuf, b"BEST  ", g.high_score);
            let hs   = core::str::from_utf8(&hbuf[..hlen]).unwrap_or("");
            framebuffer::draw_text_at(cx + X_OFF + 140, cy + 6, hs, HI_COL);

            let lvl   = (g.score / 5) + 1;
            let mut lbuf = [0u8; 32];
            let llen = fmt_score(&mut lbuf, b"LVL ", lvl);
            let ls   = core::str::from_utf8(&lbuf[..llen]).unwrap_or("");
            framebuffer::draw_text_at(cx + X_OFF + 280, cy + 6, ls, TITLE_COL);
        }

        // ── Hint bar (bottom) ─────────────────────────────────────────────
        framebuffer::draw_text_at(
            cx + X_OFF, cy + Y_OFF + gh + 5,
            "ARROWS/WASD: steer   SPACE: pause   R: restart",
            HINT_COL,
        );

        // ── Overlay: Ready / Paused / Game Over ───────────────────────────
        match g.phase {
            Phase::Ready => {
                draw_overlay(cx, cy, cw, ch,
                    "SNAKE",         TITLE_COL,
                    "Press any direction to start",   OVER_TEXT);
            }
            Phase::Paused => {
                draw_overlay(cx, cy, cw, ch,
                    "PAUSED",        PAUSE_COL,
                    "Press SPACE to resume",           OVER_TEXT);
            }
            Phase::GameOver => {
                draw_overlay(cx, cy, cw, ch,
                    "GAME OVER",     OVER_TITLE,
                    "Press R to play again",           OVER_TEXT);
            }
            Phase::Playing => {}
        }
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        let g = self.g();
        match key {
            Key::Char(b'r') | Key::Char(b'R') => {
                let hi = g.high_score;
                g.reset();
                g.high_score = hi;
                AppAction::RedrawAll
            }
            Key::Char(b' ') => {
                match g.phase {
                    Phase::Playing  => { g.phase = Phase::Paused; AppAction::RedrawAll }
                    Phase::Paused   => { g.phase = Phase::Playing; g.last_move_ms = uptime_ms(); AppAction::RedrawAll }
                    Phase::GameOver => { AppAction::Nothing }
                    Phase::Ready    => { AppAction::Nothing }
                }
            }
            Key::ArrowUp    | Key::Char(b'w') | Key::Char(b'W') => steer(g, Dir::Up),
            Key::ArrowDown  | Key::Char(b's') | Key::Char(b'S') => steer(g, Dir::Down),
            Key::ArrowLeft  | Key::Char(b'a') | Key::Char(b'A') => steer(g, Dir::Left),
            Key::ArrowRight | Key::Char(b'd') | Key::Char(b'D') => steer(g, Dir::Right),
            _ => AppAction::Nothing,
        }
    }
}

fn steer(g: &mut Game, d: Dir) -> AppAction {
    // Start game on first move input
    if g.phase == Phase::Ready {
        g.phase = Phase::Playing;
        g.last_move_ms = uptime_ms();
    }
    if g.phase == Phase::Playing && d != g.dir.opposite() {
        g.next_dir = d;
    }
    AppAction::Nothing
}

fn draw_overlay(cx: usize, cy: usize, cw: usize, ch: usize,
                heading: &str, hcol: u32, body: &str, bcol: u32)
{
    let ow = 280usize;
    let oh = 80usize;
    let ox = cx + (cw.saturating_sub(ow)) / 2;
    let oy = cy + (ch.saturating_sub(oh)) / 2;
    framebuffer::fill_rect(ox, oy, ow, oh, OVER_BG);
    framebuffer::fill_rect(ox, oy, ow, 2, BORDER);
    framebuffer::fill_rect(ox, oy + oh - 2, ow, 2, BORDER);
    framebuffer::fill_rect(ox, oy, 2, oh, BORDER);
    framebuffer::fill_rect(ox + ow - 2, oy, 2, oh, BORDER);

    let text_w = heading.len() * 12;
    let tx = ox + (ow.saturating_sub(text_w)) / 2;
    framebuffer::draw_text_scaled(tx, oy + 16, heading, hcol, 2);

    let bw = body.len() * 6;
    let bx = ox + (ow.saturating_sub(bw)) / 2;
    framebuffer::draw_text_at(bx, oy + 52, body, bcol);
}

// ── Number formatting helpers ─────────────────────────────────────────────────

fn fmt_score(buf: &mut [u8; 32], prefix: &[u8], n: u32) -> usize {
    let mut i = 0usize;
    for &b in prefix { buf[i] = b; i += 1; }
    let s = n.to_str_buf(buf, &mut i);
    let _ = s;
    i
}

trait ToStrBuf {
    fn to_str_buf(self, buf: &mut [u8; 32], i: &mut usize) -> usize;
}

impl ToStrBuf for u32 {
    fn to_str_buf(self, buf: &mut [u8; 32], i: &mut usize) -> usize {
        if self == 0 {
            buf[*i] = b'0'; *i += 1; return *i;
        }
        let mut tmp = [0u8; 10];
        let mut ti = 0usize;
        let mut n = self;
        while n > 0 { tmp[ti] = b'0' + (n % 10) as u8; ti += 1; n /= 10; }
        for j in (0..ti).rev() { buf[*i] = tmp[j]; *i += 1; }
        *i
    }
}
