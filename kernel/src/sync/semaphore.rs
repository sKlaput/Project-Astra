// ---------------------------------------------------------------------------
// Counting semaphore.
// ---------------------------------------------------------------------------

/// A cooperative counting semaphore.
///
/// `down()` (P / wait / acquire) decrements the count.  If the count would go
/// negative the calling task parks until another task calls `up()`.
///
/// `up()` (V / signal / release) increments the count.  If tasks are waiting,
/// the oldest one is unparked instead so the count stays balanced.
///
/// Not safe to call from interrupt context.
pub struct KSemaphore {
    // Current count.  Never goes below 0; a separate wait queue handles the
    // blocking so we don't need a negative sentinel.
    count: AtomicU64,
    wait_head: AtomicU64,
    wait_tail: AtomicU64,
    wait_buf: [AtomicU64; WAIT_CAP],
}

impl KSemaphore {
    pub const fn new(initial: u64) -> Self {
        KSemaphore {
            count: AtomicU64::new(initial),
            wait_head: AtomicU64::new(0),
            wait_tail: AtomicU64::new(0),
            wait_buf: [
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

    /// Decrement the semaphore.  Blocks if the count is already zero.
    pub fn down(&self) {
        loop {
            // Try to decrement without going negative (CAS loop).
            let cur = self.count.load(Ordering::Acquire);
            if cur > 0 {
                if self
                    .count
                    .compare_exchange(cur, cur - 1, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    return;
                }
                // CAS lost the race — retry without parking.
                continue;
            }

            // Count is 0: enqueue and park.
            let id = match scheduler::current_task() {
                Some(id) => id,
                None => {
                    // Called outside task context — spin.
                    for _ in 0..100 {
                        core::hint::spin_loop();
                    }
                    continue;
                }
            };
            self.enqueue_waiter_once(id);
            scheduler::park_current_task();
            // Woken by an `up()` caller which already decremented on our behalf.
            return;
        }
    }

    /// Increment the semaphore.  Wakes a waiting task if any, otherwise raises
    /// the count.
    pub fn up(&self) {
        // If there is a waiter, hand the resource directly to it — count stays
        // 0 and the waiter is unparked (it already got its decrement via the
        // `up()` path, so it should NOT decrement again when it resumes).
        if !self.wake_next_waiter() {
            self.count.fetch_add(1, Ordering::Release);
        }
    }

    /// Current count (snapshot, for diagnostics only).
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Try to decrement the semaphore without blocking.
    /// Returns `true` if decremented, `false` if the count was already zero.
    pub fn try_down(&self) -> bool {
        loop {
            let cur = self.count.load(Ordering::Acquire);
            if cur == 0 {
                return false;
            }
            if self
                .count
                .compare_exchange(cur, cur - 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }

    /// Decrement the semaphore, giving up if `deadline_tick` passes first.
    ///
    /// This is a deadline-bounded retry loop, not a queue-integrated timed wait.
    /// It sleeps for short tick-sized intervals between `try_down()` attempts.
    ///
    /// Returns `true` if decremented, `false` if the deadline expired.
    pub fn down_by_deadline_poll(&self, deadline_tick: u64) -> bool {
        loop {
            if self.try_down() {
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

    /// Backward-compatible alias for `down_by_deadline_poll`.
    pub fn down_until_tick(&self, deadline_tick: u64) -> bool {
        self.down_by_deadline_poll(deadline_tick)
    }

    fn enqueue_waiter_once(&self, id: TaskId) {
        if self.waiters_contain(id) {
            return;
        }
        let tail = self.wait_tail.fetch_add(1, Ordering::Relaxed);
        self.wait_buf[(tail as usize) % WAIT_CAP].store(id.0, Ordering::Relaxed);
    }

    fn dequeue_waiter_valid(&self) -> Option<TaskId> {
        loop {
            let head = self.wait_head.load(Ordering::Relaxed);
            let tail = self.wait_tail.load(Ordering::Acquire);
            if head == tail {
                return None;
            }
            let id = self.wait_buf[(head as usize) % WAIT_CAP].load(Ordering::Relaxed);
            self.wait_head.fetch_add(1, Ordering::Relaxed);
            let task = TaskId(id);
            if scheduler::task_state(task) == scheduler::TaskState::Sleeping {
                return Some(task);
            }
        }
    }

    fn wake_next_waiter(&self) -> bool {
        while let Some(waiter) = self.dequeue_waiter_valid() {
            if scheduler::unpark_task(waiter) {
                return true;
            }
        }
        false
    }

    fn waiters_contain(&self, id: TaskId) -> bool {
        let head = self.wait_head.load(Ordering::Relaxed);
        let tail = self.wait_tail.load(Ordering::Acquire);

        let mut idx = head;
        while idx != tail {
            if self.wait_buf[(idx as usize) % WAIT_CAP].load(Ordering::Relaxed) == id.0 {
                return true;
            }
            idx += 1;
        }
        false
    }
}
