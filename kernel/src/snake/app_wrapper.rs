// ── SnakeApp ──────────────────────────────────────────────────────────────────

pub struct SnakeApp {
    inner: UnsafeCell<Game>,
}

// SAFETY: Astra OS is single-threaded (no SMP, no Send across threads).
unsafe impl Sync for SnakeApp {}

impl SnakeApp {
    pub fn new() -> Self {
        SnakeApp {
            inner: UnsafeCell::new(Game::new()),
        }
    }

    fn g(&self) -> &mut Game {
        // SAFETY: single-threaded; we never hold two &mut refs simultaneously.
        unsafe { &mut *self.inner.get() }
    }
}

