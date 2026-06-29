/// A simple spinlock for short critical sections.
///
/// Busy-waits until the lock is acquired; does not park or yield.
/// Use for very short sections only (e.g., interrupt-safe state updates).
pub struct KSpinlock {
    locked: AtomicU64,
}

impl KSpinlock {
    pub const fn new() -> Self {
        KSpinlock {
            locked: AtomicU64::new(0),
        }
    }

    /// Busy-wait until the lock is acquired.
    pub fn lock(&self) {
        while self
            .locked
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while self.locked.load(Ordering::Relaxed) != 0 {
                core::hint::spin_loop();
            }
        }
    }

    /// Try to acquire the lock without waiting.  Returns `true` if acquired.
    pub fn try_lock(&self) -> bool {
        self.locked
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    /// Release the lock.
    pub fn unlock(&self) {
        self.locked.store(0, Ordering::Release);
    }

    /// Returns true if the lock is currently held.
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed) != 0
    }
}

/// A simple cooperative blocking mutex.
///
/// `lock()` suspends the calling task via `park_current_task()` if the mutex
/// is already held.  `unlock()` transfers ownership to the first waiting task
/// (FIFO) or releases the lock entirely if no one is waiting.
///
/// This mutex is NOT safe to call from interrupt context.
pub struct KMutex {
    // 0 = unlocked; nonzero = TaskId of the current holder.
    owner: AtomicU64,
    // Base priority captured when the current owner acquired the lock.
    owner_base_prio: AtomicU64,
    // FIFO wait queue: ring of task IDs.  Head/tail are raw monotonic indices.
    wait_head: AtomicU64,
    wait_tail: AtomicU64,
    wait_buf: [AtomicU64; WAIT_CAP],
}

impl KMutex {
    pub const fn new() -> Self {
        KMutex {
            owner: AtomicU64::new(0),
            owner_base_prio: AtomicU64::new(128),
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

    /// Acquire the mutex.  Blocks the calling task until the lock is free.
    pub fn lock(&self) {
        loop {
            let id = match scheduler::current_task() {
                Some(id) => id,
                // Called outside task context – spin until acquired (should not
                // happen in normal usage, but fail safe rather than hang).
                None => {
                    if self.try_lock_raw(0) {
                        return;
                    }
                    for _ in 0..100 {
                        core::hint::spin_loop();
                    }
                    continue;
                }
            };

            // Try to acquire atomically.
            if self.try_lock_raw(id.0) {
                self.owner_base_prio
                    .store(scheduler::task_priority(id) as u64, Ordering::Relaxed);
                return;
            }

            // Priority inheritance: if a higher-priority waiter blocks on this
            // lock, boost the owner so it can run and release sooner.
            let owner_raw = self.owner.load(Ordering::Acquire);
            if owner_raw != 0 {
                let owner_id = TaskId(owner_raw);
                let waiter_prio = scheduler::task_priority(id);
                let owner_prio = scheduler::task_priority(owner_id);
                if waiter_prio < owner_prio {
                    scheduler::set_task_priority_any(owner_id, waiter_prio);
                }
            }

            // Lock is contended: enqueue self in the wait ring, then park.
            self.enqueue_waiter_once(id);
            scheduler::park_current_task();
            // Woken by the releaser: loop back and retry acquire.
        }
    }

    /// Release the mutex.  Wakes the first waiting task if any.
    pub fn unlock(&self) {
        let owner_raw = self.owner.load(Ordering::Acquire);
        if owner_raw != 0 {
            let owner_id = TaskId(owner_raw);
            let base = self.owner_base_prio.load(Ordering::Relaxed) as u8;
            scheduler::set_task_priority_any(owner_id, base);
        }
        // Clear owner first; then wake the next waiter so it can re-acquire
        // via the normal CAS path.  This is safe on a single-core cooperative
        // scheduler: no other task runs between the store and unpark_task because
        // the current task is still executing and has not yielded yet.
        self.owner.store(0, Ordering::Release);
        self.wake_next_waiter();
    }

    /// Returns true if the mutex is currently held.
    pub fn is_locked(&self) -> bool {
        self.owner.load(Ordering::Acquire) != 0
    }

    /// Try to acquire the mutex without blocking.  Returns `true` if acquired.
    pub fn try_lock(&self) -> bool {
        let id = match scheduler::current_task() {
            Some(id) => id,
            None => return self.try_lock_raw(0),
        };
        if self.try_lock_raw(id.0) {
            self.owner_base_prio
                .store(scheduler::task_priority(id) as u64, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Acquire the mutex, giving up if `deadline_tick` passes first.
    ///
    /// This is a deadline-bounded retry loop, not a queue-integrated timed wait.
    /// It sleeps for short tick-sized intervals between `try_lock()` attempts.
    ///
    /// Returns `true` if the lock was acquired, `false` if the deadline expired.
    pub fn lock_by_deadline_poll(&self, deadline_tick: u64) -> bool {
        loop {
            if self.try_lock() {
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

    /// Backward-compatible alias for `lock_by_deadline_poll`.
    pub fn lock_until_tick(&self, deadline_tick: u64) -> bool {
        self.lock_by_deadline_poll(deadline_tick)
    }

    // --- internals ---

    fn try_lock_raw(&self, expected_owner: u64) -> bool {
        self.owner
            .compare_exchange(0, expected_owner, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
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

    fn wake_next_waiter(&self) {
        while let Some(waiter) = self.dequeue_waiter_valid() {
            if scheduler::unpark_task(waiter) {
                break;
            }
        }
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
