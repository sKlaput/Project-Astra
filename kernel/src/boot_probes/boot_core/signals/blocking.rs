use super::*;

static SIGNAL_BLOCK_OK: AtomicU64 = AtomicU64::new(0);
static SIGNAL_BLOCK_SET: AtomicU64 = AtomicU64::new(0);
static SIGNAL_BLOCK_SELF_ID: AtomicU64 = AtomicU64::new(0);
static SIGNAL_BLOCK_WAIT_DELTA: AtomicU64 = AtomicU64::new(0);

fn task_signal_block_waiter() {
    let self_id = scheduler::current_task().unwrap();
    SIGNAL_BLOCK_SELF_ID.store(self_id.0, Ordering::Relaxed);
    let start = scheduler::ticks();
    let deadline = start.saturating_add(60);
    let ok = scheduler::task_wait_signal_until_tick(self_id, 2, deadline);
    let end = scheduler::ticks();
    SIGNAL_BLOCK_WAIT_DELTA.store(end.saturating_sub(start), Ordering::Relaxed);
    if ok {
        SIGNAL_BLOCK_OK.store(1, Ordering::Relaxed);
        scheduler::task_clear_signals(self_id, 2);
    }
    scheduler::exit_task(self_id);
}

fn task_signal_block_signaler() {
    scheduler::sleep_current_for_ticks(3);
    let id = SIGNAL_BLOCK_SELF_ID.load(Ordering::Relaxed);
    if id != 0 && scheduler::task_signal(scheduler::TaskId(id), 2) {
        SIGNAL_BLOCK_SET.store(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_task_signal_blocking() {
    SIGNAL_BLOCK_OK.store(0, Ordering::Relaxed);
    SIGNAL_BLOCK_SET.store(0, Ordering::Relaxed);
    SIGNAL_BLOCK_SELF_ID.store(0, Ordering::Relaxed);
    SIGNAL_BLOCK_WAIT_DELTA.store(0, Ordering::Relaxed);

    scheduler::spawn_task_with_fn_prio(task_signal_block_waiter, 20);
    scheduler::spawn_task_with_fn_prio(task_signal_block_signaler, 30);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 80
        && (SIGNAL_BLOCK_OK.load(Ordering::Relaxed) == 0
            || SIGNAL_BLOCK_SET.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let ok = SIGNAL_BLOCK_OK.load(Ordering::Relaxed);
    let set = SIGNAL_BLOCK_SET.load(Ordering::Relaxed);
    let wait_delta = SIGNAL_BLOCK_WAIT_DELTA.load(Ordering::Relaxed);

    serial::write_str("scheduler: signal-blocking ok=");
    serial::write_u64(ok);
    serial::write_str(" set=");
    serial::write_u64(set);
    serial::write_str(" delta=");
    serial::write_u64(wait_delta);
    serial::write_line("");

    let pass = ok == 1 && set == 1 && wait_delta >= 2 && wait_delta <= 20;
    serial::write_line(if pass {
        "scheduler: signal-blocking PASS"
    } else {
        "scheduler: signal-blocking FAIL"
    });
}
