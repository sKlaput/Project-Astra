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

