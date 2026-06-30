impl Game {
    fn new() -> Self {
        let mut g = Game {
            body: [(0, 0); MAX_LEN],
            head_idx: 0,
            tail_idx: 0,
            len: 0,
            dir: Dir::Right,
            next_dir: Dir::Right,
            food: (0, 0),
            phase: Phase::Ready,
            score: 0,
            high_score: 0,
            last_move_ms: 0,
            rng: 0x53_4173_7472_614F,
        };
        g.reset();
        g
    }

    fn reset(&mut self) {
        self.head_idx = 2;
        self.tail_idx = 0;
        self.len = 3;
        self.body[0] = (10, 9);
        self.body[1] = (11, 9);
        self.body[2] = (12, 9);
        self.dir = Dir::Right;
        self.next_dir = Dir::Right;
        self.score = 0;
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
            let fx = (idx % COLS) as u8;
            let fy = (idx / COLS) as u8;
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
            if self.body[idx] == (x, y) {
                return true;
            }
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
        if self.phase != Phase::Playing {
            return false;
        }
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
            Dir::Up => (hx, hy.wrapping_sub(1)),
            Dir::Down => (hx, hy + 1),
            Dir::Left => (hx.wrapping_sub(1), hy),
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
            if self.score > self.high_score {
                self.high_score = self.score;
            }
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

