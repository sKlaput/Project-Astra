use super::*;

// --- channel probe support ---
// Producer sends 1..=6 into a channel with capacity 2. Producer starts at
// higher priority, so it fills and then blocks on full; consumer drains and
// unblocks producer. This validates both recv-empty and send-full blocking.
static CHAN_COUNT: AtomicU64 = AtomicU64::new(0);
static CHAN_SUM: AtomicU64 = AtomicU64::new(0);
static CHAN_EMPTY_BLOCKED: AtomicU64 = AtomicU64::new(0);
static CHAN_EMPTY_GOT: AtomicU64 = AtomicU64::new(0);
static PROBE_CHAN: sync::KChannel = sync::KChannel::new();

fn task_chan_empty_consumer() {
    CHAN_EMPTY_BLOCKED.store(1, Ordering::Relaxed);
    let v = PROBE_CHAN.recv();
    CHAN_EMPTY_GOT.store(v, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_chan_producer() {
    for v in 1..=6u64 {
        PROBE_CHAN.send(v);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_chan_consumer() {
    for _ in 0..6u64 {
        let v = PROBE_CHAN.recv();
        CHAN_COUNT.fetch_add(1, Ordering::Relaxed);
        CHAN_SUM.fetch_add(v, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_channel() {
    CHAN_COUNT.store(0, Ordering::Relaxed);
    CHAN_SUM.store(0, Ordering::Relaxed);
    CHAN_EMPTY_BLOCKED.store(0, Ordering::Relaxed);
    CHAN_EMPTY_GOT.store(0, Ordering::Relaxed);

    let try_recv_empty = PROBE_CHAN.try_recv().is_none();
    let try_send_snapshot = PROBE_CHAN.try_send(77);
    let try_recv_snapshot = PROBE_CHAN.try_recv().unwrap_or(0);

    // Phase A: prove recv() blocks on an empty channel, then resumes after send.
    scheduler::spawn_task_with_fn_prio(task_chan_empty_consumer, 10);
    scheduler::dispatch_once();

    let empty_wait_started = CHAN_EMPTY_BLOCKED.load(Ordering::Relaxed) == 1;
    let empty_wait_parked = CHAN_EMPTY_GOT.load(Ordering::Relaxed) == 0 && PROBE_CHAN.len() == 0;
    let try_send_empty = PROBE_CHAN.try_send(99);

    scheduler::dispatch_once();
    let empty_resumed = CHAN_EMPTY_GOT.load(Ordering::Relaxed) == 99;

    // Phase B: producer outruns consumer, fills the 2-slot buffer, then blocks
    // on full until the consumer drains and wakes it.
    scheduler::spawn_task_with_fn_prio(task_chan_producer, 10);
    scheduler::spawn_task_with_fn_prio(task_chan_consumer, 50);

    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let count = CHAN_COUNT.load(Ordering::Relaxed);
    let sum = CHAN_SUM.load(Ordering::Relaxed);
    let remaining = PROBE_CHAN.len();

    serial::write_str("scheduler: channel consumed=");
    serial::write_u64(count);
    serial::write_str(" sum=");
    serial::write_u64(sum);
    serial::write_str(" remaining=");
    serial::write_u64(remaining);
    serial::write_str(" empty=");
    serial::write_u64(empty_wait_started as u64);
    serial::write_u64(empty_wait_parked as u64);
    serial::write_u64(try_recv_empty as u64);
    serial::write_u64(try_send_snapshot as u64);
    serial::write_u64((try_recv_snapshot == 77) as u64);
    serial::write_u64(try_send_empty as u64);
    serial::write_u64(empty_resumed as u64);
    serial::write_line("");

    let pass = count == 6
        && sum == 21
        && remaining == 0
        && empty_wait_started
        && empty_wait_parked
        && try_recv_empty
        && try_send_snapshot
        && try_recv_snapshot == 77
        && try_send_empty
        && empty_resumed;
    serial::write_line(if pass {
        "scheduler: channel PASS"
    } else {
        "scheduler: channel FAIL"
    });
}
