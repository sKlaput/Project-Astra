use super::*;

// --- park/unpark telemetry probe support ---
static PARKTEL_DONE: AtomicU64 = AtomicU64::new(0);
static PARKTEL_MUTEX_WAIT: AtomicU64 = AtomicU64::new(0);
static PROBE_PARKTEL_MUTEX: sync::KMutex = sync::KMutex::new();
static PROBE_PARKTEL_SEM: sync::KSemaphore = sync::KSemaphore::new(0);

fn task_parktel_sem_waiter() {
    PROBE_PARKTEL_SEM.down(); // parks until signaler calls up()
    PARKTEL_DONE.fetch_add(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_parktel_sem_signaler() {
    scheduler::sleep_current_for_ticks(2);
    PROBE_PARKTEL_SEM.up();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_parktel_mutex_holder() {
    PROBE_PARKTEL_MUTEX.lock();
    scheduler::sleep_current_for_ticks(3);
    PROBE_PARKTEL_MUTEX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_parktel_mutex_waiter() {
    if PROBE_PARKTEL_MUTEX.is_locked() {
        PARKTEL_MUTEX_WAIT.store(1, Ordering::Relaxed);
    }
    PROBE_PARKTEL_MUTEX.lock(); // parks while holder has lock
    PARKTEL_DONE.fetch_add(1, Ordering::Relaxed);
    PROBE_PARKTEL_MUTEX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_park_unpark_telemetry() {
    let parks_before = scheduler::stat_park_count();
    let unparks_before = scheduler::stat_unpark_count();
    let fails_before = scheduler::stat_unpark_fail_count();

    PARKTEL_DONE.store(0, Ordering::Relaxed);
    PARKTEL_MUTEX_WAIT.store(0, Ordering::Relaxed);

    // Deliberate failed wake to verify fail-path telemetry increments.
    let forced_fail = !scheduler::unpark_task(scheduler::TaskId(0xFFFF_FFFF_FFFF_FF00));

    scheduler::spawn_task_with_fn_prio(task_parktel_mutex_holder, 10);
    scheduler::spawn_task_with_fn_prio(task_parktel_mutex_waiter, 20);
    scheduler::spawn_task_with_fn_prio(task_parktel_sem_waiter, 30);
    scheduler::spawn_task_with_fn_prio(task_parktel_sem_signaler, 40);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 120 && PARKTEL_DONE.load(Ordering::Relaxed) < 2
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let parks_delta = scheduler::stat_park_count().saturating_sub(parks_before);
    let unparks_delta = scheduler::stat_unpark_count().saturating_sub(unparks_before);
    let fails_delta = scheduler::stat_unpark_fail_count().saturating_sub(fails_before);
    let done = PARKTEL_DONE.load(Ordering::Relaxed);
    let mutex_wait = PARKTEL_MUTEX_WAIT.load(Ordering::Relaxed);

    serial::write_str("scheduler: park-unpark parks=");
    serial::write_u64(parks_delta);
    serial::write_str(" unparks=");
    serial::write_u64(unparks_delta);
    serial::write_str(" fails=");
    serial::write_u64(fails_delta);
    serial::write_str(" done=");
    serial::write_u64(done);
    serial::write_str(" mutex_wait=");
    serial::write_u64(mutex_wait);
    serial::write_line("");

    let pass = forced_fail
        && done == 2
        && mutex_wait == 1
        && parks_delta >= 2
        && unparks_delta >= 2
        && fails_delta >= 1;
    serial::write_line(if pass {
        "scheduler: park-unpark PASS"
    } else {
        "scheduler: park-unpark FAIL"
    });
}
