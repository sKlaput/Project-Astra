// ---------------------------------------------------------------------------
// Kernel synchronization primitives built on cooperative scheduling.
// ---------------------------------------------------------------------------
use core::sync::atomic::{AtomicU64, Ordering};
use crate::scheduler::{self, TaskId};

// Maximum number of tasks that can queue on a single mutex at once.
const WAIT_CAP: usize = 8;

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
        while self.locked.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() {
            while self.locked.load(Ordering::Relaxed) != 0 {
                core::hint::spin_loop();
            }
        }
    }

    /// Try to acquire the lock without waiting.  Returns `true` if acquired.
    pub fn try_lock(&self) -> bool {
        self.locked.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok()
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
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
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
                    if self.try_lock_raw(0) { return; }
                    for _ in 0..100 { core::hint::spin_loop(); }
                    continue;
                }
            };

            // Try to acquire atomically.
            if self.try_lock_raw(id.0) {
                self.owner_base_prio.store(scheduler::task_priority(id) as u64, Ordering::Relaxed);
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
            self.owner_base_prio.store(scheduler::task_priority(id) as u64, Ordering::Relaxed);
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
                for _ in 0..100 { core::hint::spin_loop(); }
            }
        }
    }

    /// Backward-compatible alias for `lock_by_deadline_poll`.
    pub fn lock_until_tick(&self, deadline_tick: u64) -> bool {
        self.lock_by_deadline_poll(deadline_tick)
    }

    // --- internals ---

    fn try_lock_raw(&self, expected_owner: u64) -> bool {
        self.owner.compare_exchange(
            0, expected_owner,
            Ordering::Acquire, Ordering::Relaxed,
        ).is_ok()
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
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
            ],
        }
    }

    /// Decrement the semaphore.  Blocks if the count is already zero.
    pub fn down(&self) {
        loop {
            // Try to decrement without going negative (CAS loop).
            let cur = self.count.load(Ordering::Acquire);
            if cur > 0 {
                if self.count.compare_exchange(
                    cur, cur - 1,
                    Ordering::Acquire, Ordering::Relaxed,
                ).is_ok() {
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
                    for _ in 0..100 { core::hint::spin_loop(); }
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
            if self.count.compare_exchange(
                cur, cur - 1,
                Ordering::Acquire, Ordering::Relaxed,
            ).is_ok() {
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
                for _ in 0..100 { core::hint::spin_loop(); }
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

// ---------------------------------------------------------------------------
// Bounded message channel (u64 payload).
// ---------------------------------------------------------------------------

/// Fixed-capacity cooperative channel for u64 messages.
///
/// `try_send()` fails immediately when the buffer is full.
/// `send()` blocks when the buffer is full.
/// `send_until_tick()` blocks until success or timeout.
/// `try_recv()` returns `None` immediately when the buffer is empty.
/// `recv()` blocks when the buffer is empty.
/// `recv_until_tick()` blocks until data arrives or timeout.
///
/// Not safe to call from interrupt context.
pub struct KChannel {
    // Ring of u64 messages.
    msg_head: AtomicU64,
    msg_tail: AtomicU64,
    msg_buf: [AtomicU64; CHANNEL_CAP],
    // Waiting senders (buffer full).
    tx_head: AtomicU64,
    tx_tail: AtomicU64,
    tx_buf: [AtomicU64; WAIT_CAP],
    // Waiting receivers (buffer empty).
    rx_head: AtomicU64,
    rx_tail: AtomicU64,
    rx_buf: [AtomicU64; WAIT_CAP],
}

const CHANNEL_CAP: usize = 2;

impl KChannel {
    pub const fn new() -> Self {
        KChannel {
            msg_head: AtomicU64::new(0),
            msg_tail: AtomicU64::new(0),
            msg_buf: [AtomicU64::new(0), AtomicU64::new(0)],
            tx_head: AtomicU64::new(0),
            tx_tail: AtomicU64::new(0),
            tx_buf: [
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
            ],
            rx_head: AtomicU64::new(0),
            rx_tail: AtomicU64::new(0),
            rx_buf: [
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
            ],
        }
    }

    /// Try to send a message without blocking.
    pub fn try_send(&self, value: u64) -> bool {
        let head = self.msg_head.load(Ordering::Relaxed);
        let tail = self.msg_tail.load(Ordering::Acquire);
        if (tail - head) >= CHANNEL_CAP as u64 {
            return false;
        }

        self.msg_buf[(tail as usize) % CHANNEL_CAP].store(value, Ordering::Relaxed);
        self.msg_tail.store(tail + 1, Ordering::Release);

        self.wake_next_rx_waiter();
        true
    }

    /// Send a message, blocking while the channel is full.
    pub fn send(&self, value: u64) {
        loop {
            if self.try_send(value) {
                return;
            }

            let id = match scheduler::current_task() {
                Some(id) => id,
                None => {
                    for _ in 0..100 { core::hint::spin_loop(); }
                    continue;
                }
            };
            self.enqueue_tx_waiter_once(id);
            scheduler::park_current_task();
        }
    }

    /// Send a message with a deadline in scheduler ticks.
    ///
    /// Returns `true` if the message was sent before or at `deadline_tick`,
    /// `false` if the timeout expired first.
    pub fn send_until_tick(&self, value: u64, deadline_tick: u64) -> bool {
        loop {
            if self.try_send(value) {
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
                for _ in 0..100 { core::hint::spin_loop(); }
            }
        }
    }

    /// Try to receive a message without blocking.
    pub fn try_recv(&self) -> Option<u64> {
        let head = self.msg_head.load(Ordering::Relaxed);
        let tail = self.msg_tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }

        let value = self.msg_buf[(head as usize) % CHANNEL_CAP].load(Ordering::Relaxed);
        self.msg_head.store(head + 1, Ordering::Release);

        self.wake_next_tx_waiter();
        Some(value)
    }

    /// Receive a message, blocking while the channel is empty.
    pub fn recv(&self) -> u64 {
        loop {
            if let Some(value) = self.try_recv() {
                return value;
            }

            let id = match scheduler::current_task() {
                Some(id) => id,
                None => {
                    for _ in 0..100 { core::hint::spin_loop(); }
                    continue;
                }
            };
            self.enqueue_rx_waiter_once(id);
            scheduler::park_current_task();
        }
    }

    /// Receive a message with a deadline in scheduler ticks.
    ///
    /// Returns `Some(value)` if a message was received by `deadline_tick`,
    /// or `None` when the timeout expires first.
    pub fn recv_until_tick(&self, deadline_tick: u64) -> Option<u64> {
        loop {
            if let Some(value) = self.try_recv() {
                return Some(value);
            }

            let now = scheduler::ticks();
            if now >= deadline_tick {
                return None;
            }

            if scheduler::current_task().is_some() {
                let wake_at = (now + 1).min(deadline_tick);
                scheduler::sleep_current_until_tick(wake_at);
            } else {
                for _ in 0..100 { core::hint::spin_loop(); }
            }
        }
    }

    /// Buffered item count snapshot, for diagnostics.
    pub fn len(&self) -> u64 {
        let head = self.msg_head.load(Ordering::Relaxed);
        let tail = self.msg_tail.load(Ordering::Relaxed);
        tail.saturating_sub(head)
    }

    fn enqueue_tx_waiter_once(&self, id: TaskId) {
        if self.tx_waiters_contain(id) {
            return;
        }
        let tail = self.tx_tail.fetch_add(1, Ordering::Relaxed);
        self.tx_buf[(tail as usize) % WAIT_CAP].store(id.0, Ordering::Relaxed);
    }

    fn dequeue_tx_waiter_valid(&self) -> Option<TaskId> {
        loop {
            let head = self.tx_head.load(Ordering::Relaxed);
            let tail = self.tx_tail.load(Ordering::Acquire);
            if head == tail {
                return None;
            }
            let id = self.tx_buf[(head as usize) % WAIT_CAP].load(Ordering::Relaxed);
            self.tx_head.fetch_add(1, Ordering::Relaxed);
            let task = TaskId(id);
            if scheduler::task_state(task) == scheduler::TaskState::Sleeping {
                return Some(task);
            }
        }
    }

    fn enqueue_rx_waiter_once(&self, id: TaskId) {
        if self.rx_waiters_contain(id) {
            return;
        }
        let tail = self.rx_tail.fetch_add(1, Ordering::Relaxed);
        self.rx_buf[(tail as usize) % WAIT_CAP].store(id.0, Ordering::Relaxed);
    }

    fn dequeue_rx_waiter_valid(&self) -> Option<TaskId> {
        loop {
            let head = self.rx_head.load(Ordering::Relaxed);
            let tail = self.rx_tail.load(Ordering::Acquire);
            if head == tail {
                return None;
            }
            let id = self.rx_buf[(head as usize) % WAIT_CAP].load(Ordering::Relaxed);
            self.rx_head.fetch_add(1, Ordering::Relaxed);
            let task = TaskId(id);
            if scheduler::task_state(task) == scheduler::TaskState::Sleeping {
                return Some(task);
            }
        }
    }

    fn wake_next_tx_waiter(&self) {
        while let Some(tx) = self.dequeue_tx_waiter_valid() {
            if scheduler::unpark_task(tx) {
                break;
            }
        }
    }

    fn wake_next_rx_waiter(&self) {
        while let Some(rx) = self.dequeue_rx_waiter_valid() {
            if scheduler::unpark_task(rx) {
                break;
            }
        }
    }

    fn tx_waiters_contain(&self, id: TaskId) -> bool {
        let head = self.tx_head.load(Ordering::Relaxed);
        let tail = self.tx_tail.load(Ordering::Acquire);

        let mut idx = head;
        while idx != tail {
            if self.tx_buf[(idx as usize) % WAIT_CAP].load(Ordering::Relaxed) == id.0 {
                return true;
            }
            idx += 1;
        }
        false
    }

    fn rx_waiters_contain(&self, id: TaskId) -> bool {
        let head = self.rx_head.load(Ordering::Relaxed);
        let tail = self.rx_tail.load(Ordering::Acquire);

        let mut idx = head;
        while idx != tail {
            if self.rx_buf[(idx as usize) % WAIT_CAP].load(Ordering::Relaxed) == id.0 {
                return true;
            }
            idx += 1;
        }
        false
    }
}

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
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
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
            wq_head: AtomicU64::new(0), wq_tail: AtomicU64::new(0),
            wq_buf: [
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
            ],
            rq_head: AtomicU64::new(0), rq_tail: AtomicU64::new(0),
            rq_buf: [
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
            ],
        }
    }

    /// Try to acquire a shared (read) lock without blocking.
    pub fn try_read_lock(&self) -> bool {
        let cur = self.state.load(Ordering::Acquire);
        if cur != WRITE_LOCKED && self.write_want.load(Ordering::Relaxed) == 0 {
            self.state.compare_exchange(
                cur, cur + 1,
                Ordering::Acquire, Ordering::Relaxed,
            ).is_ok()
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
                for _ in 0..100 { core::hint::spin_loop(); }
            }
        }
    }

    /// Backward-compatible alias for `read_lock_by_deadline_poll`.
    pub fn read_lock_until_tick(&self, deadline_tick: u64) -> bool {
        self.read_lock_by_deadline_poll(deadline_tick)
    }

    /// Try to acquire an exclusive (write) lock without blocking.
    pub fn try_write_lock(&self) -> bool {
        self.state.compare_exchange(
            0, WRITE_LOCKED,
            Ordering::Acquire, Ordering::Relaxed,
        ).is_ok()
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
                for _ in 0..100 { core::hint::spin_loop(); }
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
                if self.state.compare_exchange(
                    cur, cur + 1,
                    Ordering::Acquire, Ordering::Relaxed,
                ).is_ok() {
                    return;
                }
                continue;
            }
            let id = match scheduler::current_task() {
                Some(id) => id,
                None => { for _ in 0..100 { core::hint::spin_loop(); } continue; }
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
            if self.state.compare_exchange(
                0, WRITE_LOCKED,
                Ordering::Acquire, Ordering::Relaxed,
            ).is_ok() {
                return;
            }
            let id = match scheduler::current_task() {
                Some(id) => id,
                None => { for _ in 0..100 { core::hint::spin_loop(); } continue; }
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
                    Some(r) => { buf[n] = r; n += 1; }
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
        if h == t { return None; }
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
        if h == t { return None; }
        let id = self.rq_buf[(h as usize) % WAIT_CAP].load(Ordering::Relaxed);
        self.rq_head.fetch_add(1, Ordering::Relaxed);
        Some(TaskId(id))
    }
}
