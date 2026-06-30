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

