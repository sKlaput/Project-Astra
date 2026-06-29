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

fn fixed_from_str(s: &[u8]) -> Option<Fixed> {
    // Accept optional leading '-', digits, optional '.', digits.
    if s.is_empty() {
        return None;
    }
    let (neg, s) = if s[0] == b'-' {
        (true, &s[1..])
    } else {
        (false, s)
    };
    let dot = s.iter().position(|&b| b == b'.');
    let int_part = if let Some(d) = dot { &s[..d] } else { s };
    let frac_part = if let Some(d) = dot {
        &s[d + 1..]
    } else {
        b"" as &[u8]
    };

    let mut v: i64 = 0;
    for &b in int_part {
        if b < b'0' || b > b'9' {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as i64)?;
    }
    v = v.checked_mul(SCALE)?;

    let mut frac_scale = SCALE / 10;
    for &b in frac_part {
        if b < b'0' || b > b'9' {
            return None;
        }
        if frac_scale > 0 {
            v = v.checked_add((b - b'0') as i64 * frac_scale)?;
            frac_scale /= 10;
        }
    }

    Some(if neg { -v } else { v })
}

fn fixed_to_str(buf: &mut [u8; 32], v: Fixed) -> usize {
    if v == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut pos = 0usize;
    let neg = v < 0;
    let mut abs = if neg { v.wrapping_neg() } else { v };
    // Clamp to avoid UB if i64::MIN
    if abs < 0 {
        abs = i64::MAX;
    }

    let int_part = abs / SCALE;
    let frac_part = abs % SCALE;

    if neg {
        buf[pos] = b'-';
        pos += 1;
    }

    // Integer digits
    let int_start = pos;
    let mut tmp = int_part;
    if tmp == 0 {
        buf[pos] = b'0';
        pos += 1;
    } else {
        let digit_start = pos;
        while tmp > 0 {
            buf[pos] = b'0' + (tmp % 10) as u8;
            pos += 1;
            tmp /= 10;
        }
        // Reverse the integer digits
        buf[int_start..pos].reverse();
        let _ = digit_start;
    }

    // Fractional part — trim trailing zeros, up to 6 places
    if frac_part != 0 {
        buf[pos] = b'.';
        pos += 1;
        let mut fp = frac_part;
        let mut frac_digits = [0u8; 6];
        for i in (0..6).rev() {
            frac_digits[i] = (fp % 10) as u8;
            fp /= 10;
        }
        // Trim trailing zeros
        let mut end = 6;
        while end > 0 && frac_digits[end - 1] == 0 {
            end -= 1;
        }
        for &d in &frac_digits[..end] {
            if pos < 32 {
                buf[pos] = b'0' + d;
                pos += 1;
            }
        }
    }
    pos
}

fn fixed_div(a: Fixed, b: Fixed) -> Option<Fixed> {
    if b == 0 {
        return None;
    }
    // a/b as fixed = (a * SCALE) / b — but watch for overflow
    // Use i128 intermediate
    let result = (a as i128 * SCALE as i128) / b as i128;
    if result > i64::MAX as i128 || result < i64::MIN as i128 {
        None
    } else {
        Some(result as i64)
    }
}

fn fixed_mul(a: Fixed, b: Fixed) -> Option<Fixed> {
    let result = (a as i128 * b as i128) / SCALE as i128;
    if result > i64::MAX as i128 || result < i64::MIN as i128 {
        None
    } else {
        Some(result as i64)
    }
}

pub struct CalculatorApp {
    /// Current display string (digits as typed).
    input: [u8; 24],
    input_len: usize,
    /// Accumulated left-hand operand.
    accum: Fixed,
    /// Pending operator, if any.
    pending_op: Option<char>,
    /// True after = pressed — next digit starts a fresh entry.
    just_eq: bool,
    /// Error state (division by zero etc.)
    error: bool,
    /// Hover button (row, col) or None.
    hovered: Option<(usize, usize)>,
}

impl CalculatorApp {
    pub fn new() -> Self {
        CalculatorApp {
            input: [0u8; 24],
            input_len: 0,
            accum: 0,
            pending_op: None,
            just_eq: false,
            error: false,
            hovered: None,
        }
    }

    // ── Helpers ────────────────────────────────────────────────────────────

    fn input_fixed(&self) -> Fixed {
        if self.input_len == 0 {
            return 0;
        }
        fixed_from_str(&self.input[..self.input_len]).unwrap_or(0)
    }

    fn push_char(&mut self, ch: u8) {
        if self.input_len < 22 {
            self.input[self.input_len] = ch;
            self.input_len += 1;
        }
    }

    fn clear_input(&mut self) {
        self.input_len = 0;
    }

    fn set_result(&mut self, v: Fixed) {
        let mut buf = [0u8; 32];
        let n = fixed_to_str(&mut buf, v);
        let n = n.min(23);
        self.input[..n].copy_from_slice(&buf[..n]);
        self.input_len = n;
    }

    fn press(&mut self, kind: BtnKind) -> bool {
        if self.error && kind != BtnKind::Clear {
            return false;
        }
        match kind {
            BtnKind::Clear => {
                self.clear_input();
                self.accum = 0;
                self.pending_op = None;
                self.just_eq = false;
                self.error = false;
            }

            BtnKind::Backspace => {
                if self.input_len > 0 {
                    self.input_len -= 1;
                }
            }

            BtnKind::Digit(d) => {
                if self.just_eq {
                    self.clear_input();
                    self.just_eq = false;
                }
                // Don't allow leading zeros unless followed by decimal
                if self.input_len == 1 && self.input[0] == b'0' && d != 0 {
                    self.input_len = 0; // overwrite lone zero
                }
                if self.input_len == 0 && d == 0 {
                    // Display "0" as placeholder but don't push multiple
                    self.push_char(b'0');
                } else {
                    if !(self.input_len == 1 && self.input[0] == b'0') {
                        self.push_char(b'0' + d);
                    }
                }
            }

            BtnKind::Dot => {
                if self.just_eq {
                    self.clear_input();
                    self.just_eq = false;
                }
                let has_dot = self.input[..self.input_len].contains(&b'.');
                if !has_dot {
                    if self.input_len == 0 {
                        self.push_char(b'0');
                    }
                    self.push_char(b'.');
                }
            }

            BtnKind::Negate => {
                if self.input_len == 0 {
                    return false;
                }
                if self.input[0] == b'-' {
                    // Remove leading minus
                    let new_len = self.input_len - 1;
                    for i in 0..new_len {
                        self.input[i] = self.input[i + 1];
                    }
                    self.input_len = new_len;
                } else {
                    // Insert leading minus — shift right
                    if self.input_len < 23 {
                        for i in (0..self.input_len).rev() {
                            self.input[i + 1] = self.input[i];
                        }
                        self.input[0] = b'-';
                        self.input_len += 1;
                    }
                }
            }

            BtnKind::Percent => {
                let v = self.input_fixed();
                // x% = x / 100
                match fixed_div(v, 100 * SCALE) {
                    Some(r) => self.set_result(r),
                    None => {
                        self.error = true;
                        return true;
                    }
                }
                self.just_eq = true;
            }

            BtnKind::Op(op) => {
                // If there's already a pending op and the user just typed a
                // number, resolve first.
                if self.pending_op.is_some() && !self.just_eq && self.input_len > 0 {
                    let rhs = self.input_fixed();
                    let result = apply_op(self.accum, self.pending_op.unwrap(), rhs);
                    match result {
                        Some(r) => {
                            self.accum = r;
                            self.set_result(r);
                        }
                        None => {
                            self.error = true;
                            return true;
                        }
                    }
                } else {
                    self.accum = self.input_fixed();
                }
                self.pending_op = Some(op);
                self.just_eq = true; // next digit starts fresh entry
            }

            BtnKind::Eq => {
                if self.pending_op.is_none() {
                    return false;
                }
                let rhs = self.input_fixed();
                let result = apply_op(self.accum, self.pending_op.unwrap(), rhs);
                match result {
                    Some(r) => {
                        self.set_result(r);
                        self.accum = r;
                    }
                    None => {
                        self.error = true;
                        return true;
                    }
                }
                self.pending_op = None;
                self.just_eq = true;
            }
        }
        true
    }

    // ── Layout helpers ─────────────────────────────────────────────────────

    fn btn_rect(
        &self,
        row: usize,
        col: usize,
        cx: usize,
        cy: usize,
    ) -> (usize, usize, usize, usize) {
        let x = cx + PAD + col * (BTN_W + BTN_GAP);
        let y = cy + PAD + DISP_H + PAD + row * (BTN_H + BTN_GAP);
        (x, y, BTN_W, BTN_H)
    }

    fn hover_damage_rect(
        &self,
        prev: Option<(usize, usize)>,
        next: Option<(usize, usize)>,
    ) -> Option<(usize, usize, usize, usize)> {
        let mut area: Option<(usize, usize, usize, usize)> = None;
        for hov in [prev, next] {
            if let Some((row, col)) = hov {
                let (x, y, w, h) = self.btn_rect(row, col, 0, 0);
                area = Some(match area {
                    Some((ax, ay, aw, ah)) => {
                        let x0 = ax.min(x);
                        let y0 = ay.min(y);
                        let x1 = (ax + aw).max(x + w);
                        let y1 = (ay + ah).max(y + h);
                        (x0, y0, x1 - x0, y1 - y0)
                    }
                    None => (x, y, w, h),
                });
            }
        }
        area
    }

    fn hit_test(&self, rel_x: i32, rel_y: i32) -> Option<(usize, usize)> {
        for row in 0..ROWS {
            for col in 0..COLS {
                let (x, y, w, h) = self.btn_rect(row, col, 0, 0);
                if rel_x >= x as i32
                    && rel_x < (x + w) as i32
                    && rel_y >= y as i32
                    && rel_y < (y + h) as i32
                {
                    return Some((row, col));
                }
            }
        }
        None
    }
}

fn apply_op(a: Fixed, op: char, b: Fixed) -> Option<Fixed> {
    match op {
        '+' => a.checked_add(b),
        '-' => a.checked_sub(b),
        '*' => fixed_mul(a, b),
        '/' => fixed_div(a, b),
        _ => None,
    }
}

impl App for CalculatorApp {
    fn title(&self) -> &str {
        "Calculator"
    }
    fn preferred_size(&self) -> (usize, usize) {
        let w = PAD * 2 + COLS * BTN_W + (COLS - 1) * BTN_GAP;
        let h = PAD * 3 + DISP_H + ROWS * BTN_H + (ROWS - 1) * BTN_GAP + PAD;
        (w, h)
    }
    fn app_id(&self) -> &'static str {
        "calculator"
    }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        // Background
        framebuffer::fill_rect(cx, cy, cw, ch, BG);

        // ── Display area ──────────────────────────────────────────────────
        let dx = cx + PAD;
        let dy = cy + PAD;
        let dw = COLS * BTN_W + (COLS - 1) * BTN_GAP;
        let dh = DISP_H;
        framebuffer::fill_rect(dx, dy, dw, dh, DISPLAY_BG);
        // Thin top border
        framebuffer::fill_rect(dx, dy, dw, 2, 0x2A3F5F);

        // Pending operator in top-right of display
        if let Some(op) = self.pending_op {
            let op_ch = [op as u8];
            let op_str = unsafe { core::str::from_utf8_unchecked(&op_ch) };
            framebuffer::draw_text_at(dx + dw.saturating_sub(14), dy + 4, op_str, DISPLAY_OP);
        }

        // Main display value (scale-2 text, right-aligned)
        let display_text: &str = if self.error {
            "Error"
        } else if self.input_len == 0 {
            "0"
        } else {
            unsafe { core::str::from_utf8_unchecked(&self.input[..self.input_len]) }
        };

        let text_col = if self.error { ERR_COL } else { DISPLAY_TXT };
        // Scale-2 rendering: draw each character at 2× by drawing 4 fill_rect quads
        let char_w = 6usize;
        let char_h = 7usize;
        let scale = 2usize;
        let sw = char_w * scale;
        let sh = char_h * scale;
        let text_bytes = display_text.as_bytes();
        let text_pixel_w = text_bytes.len() * sw;
        let text_x = if text_pixel_w < dw.saturating_sub(8) {
            dx + dw.saturating_sub(text_pixel_w + 8)
        } else {
            dx + 4
        };
        let text_y = dy + (dh - sh) / 2;
        framebuffer::draw_text_at(text_x, text_y, display_text, text_col);

        // ── Buttons ───────────────────────────────────────────────────────
        for row in 0..ROWS {
            for col in 0..COLS {
                let btn = &BTNS[row][col];
                let (bx, by, bw, bh) = self.btn_rect(row, col, cx, cy);
                let hov = self.hovered == Some((row, col));

                let bg = match btn.kind {
                    BtnKind::Clear => {
                        if hov {
                            BTN_CLR_HOV
                        } else {
                            BTN_CLR_BG
                        }
                    }
                    BtnKind::Eq => {
                        if hov {
                            BTN_EQ_HOV
                        } else {
                            BTN_EQ_BG
                        }
                    }
                    BtnKind::Op(_) => {
                        if hov {
                            BTN_OP_HOV
                        } else {
                            BTN_OP_BG
                        }
                    }
                    _ => {
                        if hov {
                            BTN_HOV
                        } else {
                            BTN_BG
                        }
                    }
                };

                framebuffer::fill_rect(bx, by, bw, bh, bg);
                // Border
                framebuffer::fill_rect(bx, by, bw, 1, BTN_BORDER);
                framebuffer::fill_rect(bx, by, 1, bh, BTN_BORDER);
                framebuffer::fill_rect(bx, by + bh - 1, bw, 1, BTN_BORDER);
                framebuffer::fill_rect(bx + bw - 1, by, 1, bh, BTN_BORDER);

                // Label — centered
                let label = btn.label;
                let lw = label.len() * 6;
                let lx = bx + (bw.saturating_sub(lw)) / 2;
                let ly = by + (bh.saturating_sub(7)) / 2;
                framebuffer::draw_text_at(lx, ly, label, BTN_TXT);
            }
        }
    }

    fn handle_key(&mut self, key: Key) -> AppAction {
        let kind = match key {
            Key::Char(b'0') | Key::Char(b')') => Some(BtnKind::Digit(0)),
            Key::Char(b'1') | Key::Char(b'!') => Some(BtnKind::Digit(1)),
            Key::Char(b'2') | Key::Char(b'@') => Some(BtnKind::Digit(2)),
            Key::Char(b'3') | Key::Char(b'#') => Some(BtnKind::Digit(3)),
            Key::Char(b'4') | Key::Char(b'$') => Some(BtnKind::Digit(4)),
            Key::Char(b'5') => Some(BtnKind::Digit(5)),
            Key::Char(b'6') => Some(BtnKind::Digit(6)),
            Key::Char(b'7') => Some(BtnKind::Digit(7)),
            Key::Char(b'8') => Some(BtnKind::Digit(8)),
            Key::Char(b'9') => Some(BtnKind::Digit(9)),
            Key::Char(b'.') | Key::Char(b',') => Some(BtnKind::Dot),
            Key::Char(b'+') => Some(BtnKind::Op('+')),
            Key::Char(b'-') => Some(BtnKind::Op('-')),
            Key::Char(b'*') => Some(BtnKind::Op('*')),
            Key::Char(b'/') => Some(BtnKind::Op('/')),
            Key::Char(b'=') | Key::Enter => Some(BtnKind::Eq),
            Key::Backspace => Some(BtnKind::Backspace),
            Key::Escape => Some(BtnKind::Clear),
            Key::Char(b'%') => Some(BtnKind::Percent),
            _ => None,
        };

        if let Some(k) = kind {
            if self.press(k) {
                return AppAction::RedrawAll;
            }
        }
        AppAction::Nothing
    }

    fn handle_mouse_click(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        if let Some((row, col)) = self.hit_test(rel_x, rel_y) {
            let kind = BTNS[row][col].kind;
            self.press(kind);
            AppAction::RedrawAll
        } else {
            AppAction::Nothing
        }
    }

    fn handle_mouse_move(&mut self, rel_x: i32, rel_y: i32) -> AppAction {
        let new_hov = self.hit_test(rel_x, rel_y);
        if new_hov != self.hovered {
            let old_hov = self.hovered;
            self.hovered = new_hov;
            if let Some((x, y, w, h)) = self.hover_damage_rect(old_hov, new_hov) {
                AppAction::RedrawArea(x, y, w, h)
            } else {
                AppAction::Nothing
            }
        } else {
            AppAction::Nothing
        }
    }
}
