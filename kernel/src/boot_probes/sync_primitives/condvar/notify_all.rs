use super::*;

// --- condvar notify_all probe support ---
static CV_ALL_DATA: AtomicU64 = AtomicU64::new(0);
static CV_ALL_WAKE_A: AtomicU64 = AtomicU64::new(0);
static CV_ALL_WAKE_B: AtomicU64 = AtomicU64::new(0);
static CV_ALL_SIG_DONE: AtomicU64 = AtomicU64::new(0);
static PROBE_CV_ALL_MTX: sync::KMutex = sync::KMutex::new();
static PROBE_CV_ALL: sync::KCondVar = sync::KCondVar::new();

fn task_cv_all_waiter_a() {
    PROBE_CV_ALL_MTX.lock();
    while CV_ALL_DATA.load(Ordering::Relaxed) == 0 {
        PROBE_CV_ALL.wait(&PROBE_CV_ALL_MTX);
    }
    CV_ALL_WAKE_A.store(CV_ALL_DATA.load(Ordering::Relaxed), Ordering::Relaxed);
    PROBE_CV_ALL_MTX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_cv_all_waiter_b() {
    PROBE_CV_ALL_MTX.lock();
    while CV_ALL_DATA.load(Ordering::Relaxed) == 0 {
        PROBE_CV_ALL.wait(&PROBE_CV_ALL_MTX);
    }
    CV_ALL_WAKE_B.store(CV_ALL_DATA.load(Ordering::Relaxed), Ordering::Relaxed);
    PROBE_CV_ALL_MTX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_cv_all_signaler() {
    scheduler::sleep_current_for_ticks(3);
    PROBE_CV_ALL_MTX.lock();
    CV_ALL_DATA.store(99, Ordering::Relaxed);
    PROBE_CV_ALL.notify_all();
    PROBE_CV_ALL_MTX.unlock();
    CV_ALL_SIG_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_condvar_notify_all() {
    CV_ALL_DATA.store(0, Ordering::Relaxed);
    CV_ALL_WAKE_A.store(0, Ordering::Relaxed);
    CV_ALL_WAKE_B.store(0, Ordering::Relaxed);
    CV_ALL_SIG_DONE.store(0, Ordering::Relaxed);

    scheduler::spawn_task_with_fn_prio(task_cv_all_waiter_a, 10);
    scheduler::spawn_task_with_fn_prio(task_cv_all_waiter_b, 20);
    scheduler::spawn_task_with_fn_prio(task_cv_all_signaler, 30);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 80
        && (CV_ALL_WAKE_A.load(Ordering::Relaxed) == 0
            || CV_ALL_WAKE_B.load(Ordering::Relaxed) == 0
            || CV_ALL_SIG_DONE.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}

    let wake_a = CV_ALL_WAKE_A.load(Ordering::Relaxed);
    let wake_b = CV_ALL_WAKE_B.load(Ordering::Relaxed);
    let sig_done = CV_ALL_SIG_DONE.load(Ordering::Relaxed);

    serial::write_str("scheduler: condvar-all wake_a=");
    serial::write_u64(wake_a);
    serial::write_str(" wake_b=");
    serial::write_u64(wake_b);
    serial::write_str(" sig_done=");
    serial::write_u64(sig_done);
    serial::write_line("");

    let pass = wake_a == 99 && wake_b == 99 && sig_done == 1;
    serial::write_line(if pass {
        "scheduler: condvar-all PASS"
    } else {
        "scheduler: condvar-all FAIL"
    });
}
