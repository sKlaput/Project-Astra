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

