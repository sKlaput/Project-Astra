use super::*;

// --- condvar notify_one probe support ---
static CV_ONE_DATA: AtomicU64 = AtomicU64::new(0);
static CV_ONE_WAKE: AtomicU64 = AtomicU64::new(0);
static CV_ONE_DONE: AtomicU64 = AtomicU64::new(0);
static PROBE_CV_ONE_MTX: sync::KMutex = sync::KMutex::new();
static PROBE_CV_ONE: sync::KCondVar = sync::KCondVar::new();

fn task_cv_one_waiter() {
    PROBE_CV_ONE_MTX.lock();
    while CV_ONE_DATA.load(Ordering::Relaxed) == 0 {
        PROBE_CV_ONE.wait(&PROBE_CV_ONE_MTX);
    }
    CV_ONE_WAKE.store(CV_ONE_DATA.load(Ordering::Relaxed), Ordering::Relaxed);
    PROBE_CV_ONE_MTX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_cv_one_signaler() {
    scheduler::sleep_current_for_ticks(3);
    PROBE_CV_ONE_MTX.lock();
    CV_ONE_DATA.store(42, Ordering::Relaxed);
    PROBE_CV_ONE.notify_one();
    PROBE_CV_ONE_MTX.unlock();
    CV_ONE_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_condvar_notify_one() {
    CV_ONE_DATA.store(0, Ordering::Relaxed);
    CV_ONE_WAKE.store(0, Ordering::Relaxed);
    CV_ONE_DONE.store(0, Ordering::Relaxed);

    scheduler::spawn_task_with_fn_prio(task_cv_one_waiter, 10);
    scheduler::spawn_task_with_fn_prio(task_cv_one_signaler, 20);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 80
        && (CV_ONE_WAKE.load(Ordering::Relaxed) == 0 || CV_ONE_DONE.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}

    let wake_val = CV_ONE_WAKE.load(Ordering::Relaxed);
    let done = CV_ONE_DONE.load(Ordering::Relaxed);

    serial::write_str("scheduler: condvar-one wake=");
    serial::write_u64(wake_val);
    serial::write_str(" done=");
    serial::write_u64(done);
    serial::write_line("");

    let pass = wake_val == 42 && done == 1;
    serial::write_line(if pass {
        "scheduler: condvar-one PASS"
    } else {
        "scheduler: condvar-one FAIL"
    });
}
