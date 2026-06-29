use super::*;

static SIGNAL_TEL_WAIT_ID: AtomicU64 = AtomicU64::new(0);
static SIGNAL_TEL_WAIT_DONE: AtomicU64 = AtomicU64::new(0);
static SIGNAL_TEL_SET_OK: AtomicU64 = AtomicU64::new(0);

fn task_signal_tel_waiter() {
    let self_id = scheduler::current_task().unwrap();
    SIGNAL_TEL_WAIT_ID.store(self_id.0, Ordering::Relaxed);
    let deadline = scheduler::ticks().saturating_add(40);
    if scheduler::task_wait_signal_until_tick(self_id, 8, deadline) {
        SIGNAL_TEL_WAIT_DONE.store(1, Ordering::Relaxed);
        scheduler::task_clear_signals(self_id, 8);
    }
    scheduler::exit_task(self_id);
}

fn task_signal_tel_signaler() {
    scheduler::sleep_current_for_ticks(2);
    let id = SIGNAL_TEL_WAIT_ID.load(Ordering::Relaxed);
    if id != 0 && scheduler::task_signal(scheduler::TaskId(id), 8) {
        SIGNAL_TEL_SET_OK.store(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_task_signal_telemetry() {
    SIGNAL_TEL_WAIT_ID.store(0, Ordering::Relaxed);
    SIGNAL_TEL_WAIT_DONE.store(0, Ordering::Relaxed);
    SIGNAL_TEL_SET_OK.store(0, Ordering::Relaxed);

    let set_before = scheduler::stat_signal_set_count();
    let wake_before = scheduler::stat_signal_wake_count();
    let wake_fail_before = scheduler::stat_signal_wake_fail_count();

    scheduler::spawn_task_with_fn_prio(task_signal_tel_waiter, 20);
    scheduler::spawn_task_with_fn_prio(task_signal_tel_signaler, 30);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 80
        && (SIGNAL_TEL_WAIT_DONE.load(Ordering::Relaxed) == 0
            || SIGNAL_TEL_SET_OK.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let set_delta = scheduler::stat_signal_set_count().saturating_sub(set_before);
    let wake_delta = scheduler::stat_signal_wake_count().saturating_sub(wake_before);
    let wake_fail_delta = scheduler::stat_signal_wake_fail_count().saturating_sub(wake_fail_before);
    let wait_done = SIGNAL_TEL_WAIT_DONE.load(Ordering::Relaxed);
    let set_ok = SIGNAL_TEL_SET_OK.load(Ordering::Relaxed);

    serial::write_str("scheduler: signal-telemetry set=");
    serial::write_u64(set_delta);
    serial::write_str(" wake=");
    serial::write_u64(wake_delta);
    serial::write_str(" wake_fail=");
    serial::write_u64(wake_fail_delta);
    serial::write_str(" done=");
    serial::write_u64(wait_done);
    serial::write_str(" set_ok=");
    serial::write_u64(set_ok);
    serial::write_line("");

    let pass =
        set_delta == 1 && wake_delta == 1 && wake_fail_delta == 0 && wait_done == 1 && set_ok == 1;
    serial::write_line(if pass {
        "scheduler: signal-telemetry PASS"
    } else {
        "scheduler: signal-telemetry FAIL"
    });
}
