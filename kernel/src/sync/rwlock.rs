// ---------------------------------------------------------------------------
// Reader-writer lock.
// ---------------------------------------------------------------------------

/// `state` encoding: 0 = unlocked, 1..N = N readers holding, WRITE_LOCKED = writer holding.
const WRITE_LOCKED: u64 = u64::MAX;

/// A cooperative reader-writer lock.
///
/// Multiple readers may hold the lock concurrently.  A single writer gets
/// exclusive access.  Waiting writers block new readers to prevent starvation.
///
/// Not safe to call from interrupt context.
pub struct KRwLock {
    state: AtomicU64,
    /// Count of writers currently waiting; nonzero causes new readers to queue
    /// rather than acquire immediately, preventing writer starvation.
    write_want: AtomicU64,
    wq_head: AtomicU64,
    wq_tail: AtomicU64,
    wq_buf: [AtomicU64; WAIT_CAP],
    rq_head: AtomicU64,
    rq_tail: AtomicU64,
    rq_buf: [AtomicU64; WAIT_CAP],
}

impl KRwLock {
    pub const fn new() -> Self {
        KRwLock {
            state: AtomicU64::new(0),
            write_want: AtomicU64::new(0),
            wq_head: AtomicU64::new(0),
            wq_tail: AtomicU64::new(0),
            wq_buf: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
            rq_head: AtomicU64::new(0),
            rq_tail: AtomicU64::new(0),
            rq_buf: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
        }
    }

    /// Try to acquire a shared (read) lock without blocking.
    pub fn try_read_lock(&self) -> bool {
        let cur = self.state.load(Ordering::Acquire);
        if cur != WRITE_LOCKED && self.write_want.load(Ordering::Relaxed) == 0 {
            self.state
                .compare_exchange(cur, cur + 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        } else {
            false
        }
    }

    /// Acquire a shared (read) lock until `deadline_tick`.
    ///
    /// This is a deadline-bounded retry loop over `try_read_lock()`.
    /// Returns `true` on success, `false` on timeout.
    pub fn read_lock_by_deadline_poll(&self, deadline_tick: u64) -> bool {
        loop {
            if self.try_read_lock() {
                return true;
            }
            let now = scheduler::ticks();
            if now >= deadline_tick {
                return false;
            }
            if scheduler::current_task().is_some() {
                let wake_at = (now + 1).min(deadline_tick);
                scheduler::sleep_current_until_tick(wake_at);
            } else {
                for _ in 0..100 {
                    core::hint::spin_loop();
                }
            }
        }
    }

    /// Backward-compatible alias for `read_lock_by_deadline_poll`.
    pub fn read_lock_until_tick(&self, deadline_tick: u64) -> bool {
        self.read_lock_by_deadline_poll(deadline_tick)
    }

    /// Try to acquire an exclusive (write) lock without blocking.
    pub fn try_write_lock(&self) -> bool {
        self.state
            .compare_exchange(0, WRITE_LOCKED, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    /// Acquire an exclusive (write) lock until `deadline_tick`.
    ///
    /// This is a deadline-bounded retry loop over `try_write_lock()`.
    /// Returns `true` on success, `false` on timeout.
    pub fn write_lock_by_deadline_poll(&self, deadline_tick: u64) -> bool {
        loop {
            if self.try_write_lock() {
                return true;
            }
            let now = scheduler::ticks();
            if now >= deadline_tick {
                return false;
            }
            if scheduler::current_task().is_some() {
                let wake_at = (now + 1).min(deadline_tick);
                scheduler::sleep_current_until_tick(wake_at);
            } else {
                for _ in 0..100 {
                    core::hint::spin_loop();
                }
            }
        }
    }

    /// Backward-compatible alias for `write_lock_by_deadline_poll`.
    pub fn write_lock_until_tick(&self, deadline_tick: u64) -> bool {
        self.write_lock_by_deadline_poll(deadline_tick)
    }

    /// Acquire a shared (read) lock.  Blocks if a writer holds the lock or is
    /// waiting (to prevent writer starvation).
    pub fn read_lock(&self) {
        loop {
            let cur = self.state.load(Ordering::Acquire);
            if cur != WRITE_LOCKED && self.write_want.load(Ordering::Relaxed) == 0 {
                if self
                    .state
                    .compare_exchange(cur, cur + 1, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    return;
                }
                continue;
            }
            let id = match scheduler::current_task() {
                Some(id) => id,
                None => {
                    for _ in 0..100 {
                        core::hint::spin_loop();
                    }
                    continue;
                }
            };
            self.enqueue_reader(id);
            scheduler::park_current_task();
            // Woken by write_unlock, which already incremented `state` on our behalf.
            return;
        }
    }

    /// Release a shared (read) lock.
    pub fn read_unlock(&self) {
        let prev = self.state.fetch_sub(1, Ordering::Release);
        if prev == 1 {
            // Last reader: hand off to a waiting writer if any.
            if let Some(w) = self.dequeue_writer() {
                self.state.store(WRITE_LOCKED, Ordering::Release);
                self.write_want.fetch_sub(1, Ordering::Relaxed);
                scheduler::unpark_task(w);
            }
        }
    }

    /// Acquire the exclusive (write) lock.  Blocks if any reader or writer holds it.
    pub fn write_lock(&self) {
        loop {
            if self
                .state
                .compare_exchange(0, WRITE_LOCKED, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
            let id = match scheduler::current_task() {
                Some(id) => id,
                None => {
                    for _ in 0..100 {
                        core::hint::spin_loop();
                    }
                    continue;
                }
            };
            // Increment write_want before enqueuing so concurrent read_lock() calls
            // see it and queue rather than acquire.  Single-core cooperative: no task
            // runs between these two stores, so there is no TOCTOU window.
            self.write_want.fetch_add(1, Ordering::Relaxed);
            self.enqueue_writer(id);
            scheduler::park_current_task();
            // Woken by read_unlock or write_unlock, which set state = WRITE_LOCKED.
            return;
        }
    }

    /// Release the exclusive (write) lock.
    pub fn write_unlock(&self) {
        if let Some(next_w) = self.dequeue_writer() {
            // Hand the lock directly to the next writer (state stays WRITE_LOCKED).
            self.write_want.fetch_sub(1, Ordering::Relaxed);
            scheduler::unpark_task(next_w);
        } else {
            // No writers waiting: collect pending readers, set state = count, unpark all.
            // Safe on single-core cooperative: nothing runs until we yield.
            let mut buf = [TaskId(0); WAIT_CAP];
            let mut n = 0usize;
            while n < WAIT_CAP {
                match self.dequeue_reader() {
                    Some(r) => {
                        buf[n] = r;
                        n += 1;
                    }
                    None => break,
                }
            }
            self.state.store(n as u64, Ordering::Release);
            for &r in &buf[..n] {
                scheduler::unpark_task(r);
            }
        }
    }

    fn enqueue_writer(&self, id: TaskId) {
        let t = self.wq_tail.fetch_add(1, Ordering::Relaxed);
        self.wq_buf[(t as usize) % WAIT_CAP].store(id.0, Ordering::Relaxed);
    }
    fn dequeue_writer(&self) -> Option<TaskId> {
        let h = self.wq_head.load(Ordering::Relaxed);
        let t = self.wq_tail.load(Ordering::Acquire);
        if h == t {
            return None;
        }
        let id = self.wq_buf[(h as usize) % WAIT_CAP].load(Ordering::Relaxed);
        self.wq_head.fetch_add(1, Ordering::Relaxed);
        Some(TaskId(id))
    }
    fn enqueue_reader(&self, id: TaskId) {
        let t = self.rq_tail.fetch_add(1, Ordering::Relaxed);
        self.rq_buf[(t as usize) % WAIT_CAP].store(id.0, Ordering::Relaxed);
    }
    fn dequeue_reader(&self) -> Option<TaskId> {
        let h = self.rq_head.load(Ordering::Relaxed);
        let t = self.rq_tail.load(Ordering::Acquire);
        if h == t {
            return None;
        }
        let id = self.rq_buf[(h as usize) % WAIT_CAP].load(Ordering::Relaxed);
        self.rq_head.fetch_add(1, Ordering::Relaxed);
        Some(TaskId(id))
    }
}
