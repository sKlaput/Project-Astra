use super::*;

static SIGNAL_TO_SHORT: AtomicU64 = AtomicU64::new(0);
static SIGNAL_TO_LONG_OK: AtomicU64 = AtomicU64::new(0);
static SIGNAL_TO_SET_OK: AtomicU64 = AtomicU64::new(0);
static SIGNAL_TO_SELF_ID: AtomicU64 = AtomicU64::new(0);

fn task_signal_timeout_waiter_short() {
    let self_id = scheduler::current_task().unwrap();
    let deadline = scheduler::ticks().saturating_add(4);
    let ok = scheduler::task_wait_signal_until_tick(self_id, 1, deadline);
    if !ok {
        SIGNAL_TO_SHORT.store(1, Ordering::Relaxed);
    }
    scheduler::exit_task(self_id);
}

fn task_signal_timeout_waiter_long() {
    let self_id = scheduler::current_task().unwrap();
    SIGNAL_TO_SELF_ID.store(self_id.0, Ordering::Relaxed);
    let deadline = scheduler::ticks().saturating_add(30);
    let ok = scheduler::task_wait_signal_until_tick(self_id, 1, deadline);
    if ok {
        SIGNAL_TO_LONG_OK.store(1, Ordering::Relaxed);
        let before = scheduler::task_clear_signals(self_id, 1);
        if before & 1 != 0 {
            SIGNAL_TO_SET_OK.store(1, Ordering::Relaxed);
        }
    }
    scheduler::exit_task(self_id);
}

fn task_signal_timeout_signaler() {
    for _ in 0..40 {
        let id = SIGNAL_TO_SELF_ID.load(Ordering::Relaxed);
        if id != 0 {
            scheduler::task_signal(scheduler::TaskId(id), 1);
            break;
        }
        scheduler::sleep_current_for_ticks(1);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_task_signal_timeout() {
    SIGNAL_TO_SHORT.store(0, Ordering::Relaxed);
    SIGNAL_TO_LONG_OK.store(0, Ordering::Relaxed);
    SIGNAL_TO_SET_OK.store(0, Ordering::Relaxed);
    SIGNAL_TO_SELF_ID.store(0, Ordering::Relaxed);

    // Phase A: no sender; short wait should time out.
    scheduler::spawn_task_with_fn_prio(task_signal_timeout_waiter_short, 20);
    let start_a = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_a) < 20
        && SIGNAL_TO_SHORT.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    // Phase B: long waiter should wake by incoming signal before deadline.
    scheduler::spawn_task_with_fn_prio(task_signal_timeout_waiter_long, 30);
    scheduler::spawn_task_with_fn_prio(task_signal_timeout_signaler, 40);
    let start_b = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_b) < 80
        && (SIGNAL_TO_LONG_OK.load(Ordering::Relaxed) == 0
            || SIGNAL_TO_SET_OK.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let short_to = SIGNAL_TO_SHORT.load(Ordering::Relaxed);
    let long_ok = SIGNAL_TO_LONG_OK.load(Ordering::Relaxed);
    let set_ok = SIGNAL_TO_SET_OK.load(Ordering::Relaxed);

    serial::write_str("scheduler: signal-timeout short_to=");
    serial::write_u64(short_to);
    serial::write_str(" long_ok=");
    serial::write_u64(long_ok);
    serial::write_str(" set_ok=");
    serial::write_u64(set_ok);
    serial::write_line("");

    let pass = short_to == 1 && long_ok == 1 && set_ok == 1;
    serial::write_line(if pass {
        "scheduler: signal-timeout PASS"
    } else {
        "scheduler: signal-timeout FAIL"
    });
}
