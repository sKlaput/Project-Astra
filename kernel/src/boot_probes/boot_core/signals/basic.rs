use super::*;

// --- task signal probe support ---
static SIGNAL_WAITER_SAW: AtomicU64 = AtomicU64::new(0);
static SIGNAL_SIGNALER_DONE: AtomicU64 = AtomicU64::new(0);
static SIGNAL_SET_OK: AtomicU64 = AtomicU64::new(0);
static SIGNAL_CLEARED_OK: AtomicU64 = AtomicU64::new(0);
static SIGNAL_WAITER_ID: AtomicU64 = AtomicU64::new(0);

fn task_signal_waiter() {
    let self_id = scheduler::current_task().unwrap();
    SIGNAL_WAITER_ID.store(self_id.0, Ordering::Relaxed);

    for _ in 0..160 {
        let signals = scheduler::task_pending_signals(self_id);
        if signals & 1 != 0 {
            SIGNAL_WAITER_SAW.store(1, Ordering::Relaxed);
            let before = scheduler::task_clear_signals(self_id, 1);
            let after = scheduler::task_pending_signals(self_id);
            if (before & 1) != 0 && (after & 1) == 0 {
                SIGNAL_CLEARED_OK.store(1, Ordering::Relaxed);
            }
            break;
        }
        scheduler::sleep_current_for_ticks(1);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_signal_signaler() {
    for _ in 0..40 {
        let waiter_id = SIGNAL_WAITER_ID.load(Ordering::Relaxed);
        if waiter_id != 0 {
            if scheduler::task_signal(scheduler::TaskId(waiter_id), 1) {
                SIGNAL_SET_OK.store(1, Ordering::Relaxed);
            }
            break;
        }
        scheduler::sleep_current_for_ticks(1);
    }
    SIGNAL_SIGNALER_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_task_signal() {
    SIGNAL_WAITER_SAW.store(0, Ordering::Relaxed);
    SIGNAL_SIGNALER_DONE.store(0, Ordering::Relaxed);
    SIGNAL_SET_OK.store(0, Ordering::Relaxed);
    SIGNAL_CLEARED_OK.store(0, Ordering::Relaxed);
    SIGNAL_WAITER_ID.store(0, Ordering::Relaxed);

    let waiter = scheduler::spawn_task_with_fn_prio(task_signal_waiter, 40);
    let signaler = scheduler::spawn_task_with_fn_prio(task_signal_signaler, 50);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 100 {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
        let waiter_saw = SIGNAL_WAITER_SAW.load(Ordering::Relaxed);
        let sig_done = SIGNAL_SIGNALER_DONE.load(Ordering::Relaxed);
        let set_ok = SIGNAL_SET_OK.load(Ordering::Relaxed);
        let cleared_ok = SIGNAL_CLEARED_OK.load(Ordering::Relaxed);
        if waiter_saw == 1 && sig_done == 1 && set_ok == 1 && cleared_ok == 1 {
            break;
        }
    }
    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let mut empty_after = 0u64;
    for t in [waiter, signaler] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Empty {
                empty_after += 1;
            }
        }
    }

    let waiter_saw = SIGNAL_WAITER_SAW.load(Ordering::Relaxed);
    let sig_done = SIGNAL_SIGNALER_DONE.load(Ordering::Relaxed);
    let set_ok = SIGNAL_SET_OK.load(Ordering::Relaxed);
    let cleared_ok = SIGNAL_CLEARED_OK.load(Ordering::Relaxed);

    serial::write_str("scheduler: task-signal waiter_saw=");
    serial::write_u64(waiter_saw);
    serial::write_str(" signaler_done=");
    serial::write_u64(sig_done);
    serial::write_str(" set_ok=");
    serial::write_u64(set_ok);
    serial::write_str(" cleared_ok=");
    serial::write_u64(cleared_ok);
    serial::write_str(" empty=");
    serial::write_u64(empty_after);
    serial::write_str("/2");
    serial::write_line("");

    let pass =
        waiter_saw == 1 && sig_done == 1 && set_ok == 1 && cleared_ok == 1 && empty_after == 2;
    serial::write_line(if pass {
        "scheduler: task-signal PASS"
    } else {
        "scheduler: task-signal FAIL"
    });
}
