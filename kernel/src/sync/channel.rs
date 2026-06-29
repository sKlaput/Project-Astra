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
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
            rx_head: AtomicU64::new(0),
            rx_tail: AtomicU64::new(0),
            rx_buf: [
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
                    for _ in 0..100 {
                        core::hint::spin_loop();
                    }
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
                for _ in 0..100 {
                    core::hint::spin_loop();
                }
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
                    for _ in 0..100 {
                        core::hint::spin_loop();
                    }
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
                for _ in 0..100 {
                    core::hint::spin_loop();
                }
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
