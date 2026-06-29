// ---------------------------------------------------------------------------
// Condition variable.
// ---------------------------------------------------------------------------

/// A cooperative condition variable for use with [`KMutex`].
///
/// `wait(mutex)` atomically releases the mutex and parks the calling task.
/// On wake the mutex is re-acquired before returning to the caller, so the
/// standard "loop + predicate check" idiom works correctly:
///
/// ```
/// mutex.lock();
/// while !condition() { cv.wait(&mutex); }
/// // ... critical section ...
/// mutex.unlock();
/// ```
///
/// `notify_one()` wakes exactly one waiting task (FIFO order).
/// `notify_all()` wakes every waiting task.
///
/// Not safe to call from interrupt context.
pub struct KCondVar {
    notify_seq: AtomicU64,
    wait_head: AtomicU64,
    wait_tail: AtomicU64,
    wait_buf: [AtomicU64; WAIT_CAP],
}

impl KCondVar {
    pub const fn new() -> Self {
        KCondVar {
            notify_seq: AtomicU64::new(0),
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

    /// Release `mutex`, sleep until notified, then re-acquire `mutex`.
    ///
    /// The mutex must be held by the calling task before entering `wait`.
    /// On return the mutex is held again.
    pub fn wait(&self, mutex: &KMutex) {
        let id = match scheduler::current_task() {
            Some(id) => id,
            // Outside task context: just spin-wait and hope the condition
            // becomes true externally; this path should never be hit in
            // normal kernel code.
            None => return,
        };
        self.enqueue_waiter_once(id);
        mutex.unlock();
        scheduler::park_current_task();
        mutex.lock();
    }

    /// Release `mutex`, sleep until notified or deadline, then re-acquire `mutex`.
    ///
    /// This is a deadline-bounded polling wait over `notify_seq`, not a timed
    /// waiter queue that is removed by targeted notify operations.
    ///
    /// Returns `true` if woken by `notify_one()` / `notify_all()` before the
    /// deadline, `false` if the timeout expired first.
    pub fn wait_by_deadline_poll(&self, mutex: &KMutex, deadline_tick: u64) -> bool {
        let id = match scheduler::current_task() {
            Some(id) => id,
            None => return false,
        };

        let observed_seq = self.notify_seq.load(Ordering::Acquire);

        self.enqueue_waiter_once(id);
        mutex.unlock();

        loop {
            if self.notify_seq.load(Ordering::Acquire) != observed_seq {
                mutex.lock();
                return true;
            }

            let now = scheduler::ticks();
            if now >= deadline_tick {
                mutex.lock();
                return false;
            }

            let wake_at = (now + 1).min(deadline_tick);
            scheduler::sleep_current_until_tick(wake_at);
        }
    }

    /// Backward-compatible alias for `wait_by_deadline_poll`.
    pub fn wait_until_tick(&self, mutex: &KMutex, deadline_tick: u64) -> bool {
        self.wait_by_deadline_poll(mutex, deadline_tick)
    }

    /// Wake the oldest waiting task (FIFO).
    pub fn notify_one(&self) {
        self.notify_seq.fetch_add(1, Ordering::Release);
        self.wake_next_waiter();
    }

    /// Wake all currently waiting tasks.
    pub fn notify_all(&self) {
        self.notify_seq.fetch_add(1, Ordering::Release);
        while self.wake_next_waiter() {}
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
