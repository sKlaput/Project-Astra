use super::*;

static CV_TO_TIMED_OUT: AtomicU64 = AtomicU64::new(0);
static CV_TO_WOKE: AtomicU64 = AtomicU64::new(0);
static CV_TO_SIG_DONE: AtomicU64 = AtomicU64::new(0);
static PROBE_CV_TO_MTX: sync::KMutex = sync::KMutex::new();
static PROBE_CV_TO: sync::KCondVar = sync::KCondVar::new();
static CV_TO_DATA: AtomicU64 = AtomicU64::new(0);

fn task_cv_timeout_waiter_short() {
    PROBE_CV_TO_MTX.lock();
    while CV_TO_DATA.load(Ordering::Relaxed) == 0 {
        let deadline = scheduler::ticks().saturating_add(4);
        if !PROBE_CV_TO.wait_by_deadline_poll(&PROBE_CV_TO_MTX, deadline) {
            CV_TO_TIMED_OUT.store(1, Ordering::Relaxed);
            break;
        }
    }
    PROBE_CV_TO_MTX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_cv_timeout_waiter_long() {
    PROBE_CV_TO_MTX.lock();
    while CV_TO_DATA.load(Ordering::Relaxed) == 0 {
        let deadline = scheduler::ticks().saturating_add(24);
        if !PROBE_CV_TO.wait_by_deadline_poll(&PROBE_CV_TO_MTX, deadline) {
            break;
        }
    }
    if CV_TO_DATA.load(Ordering::Relaxed) == 7 {
        CV_TO_WOKE.store(1, Ordering::Relaxed);
    }
    PROBE_CV_TO_MTX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_cv_timeout_signaler() {
    scheduler::sleep_current_for_ticks(3);
    PROBE_CV_TO_MTX.lock();
    CV_TO_DATA.store(7, Ordering::Relaxed);
    PROBE_CV_TO.notify_one();
    PROBE_CV_TO_MTX.unlock();
    CV_TO_SIG_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_condvar_timeout() {
    CV_TO_TIMED_OUT.store(0, Ordering::Relaxed);
    CV_TO_WOKE.store(0, Ordering::Relaxed);
    CV_TO_SIG_DONE.store(0, Ordering::Relaxed);
    CV_TO_DATA.store(0, Ordering::Relaxed);

    // Phase A: timed wait on empty predicate should timeout.
    scheduler::spawn_task_with_fn_prio(task_cv_timeout_waiter_short, 10);
    let start_a = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_a) < 30
        && CV_TO_TIMED_OUT.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    // Phase B: waiter with longer deadline should wake from notify_one.
    CV_TO_DATA.store(0, Ordering::Relaxed);
    scheduler::spawn_task_with_fn_prio(task_cv_timeout_waiter_long, 20);
    scheduler::spawn_task_with_fn_prio(task_cv_timeout_signaler, 30);
    let start_b = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_b) < 80
        && (CV_TO_WOKE.load(Ordering::Relaxed) == 0 || CV_TO_SIG_DONE.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}

    let timed_out = CV_TO_TIMED_OUT.load(Ordering::Relaxed);
    let woke = CV_TO_WOKE.load(Ordering::Relaxed);
    let sig_done = CV_TO_SIG_DONE.load(Ordering::Relaxed);

    serial::write_str("scheduler: condvar-deadline-poll to=");
    serial::write_u64(timed_out);
    serial::write_str(" woke=");
    serial::write_u64(woke);
    serial::write_str(" sig=");
    serial::write_u64(sig_done);
    serial::write_line("");

    let pass = timed_out == 1 && woke == 1 && sig_done == 1;
    serial::write_line(if pass {
        "scheduler: condvar-deadline-poll PASS"
    } else {
        "scheduler: condvar-deadline-poll FAIL"
    });
}
