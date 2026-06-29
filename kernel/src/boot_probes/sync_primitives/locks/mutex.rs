use super::*;

// --- mutex deadline-poll probe support ---
static MTX_TO_LOCK: AtomicU64 = AtomicU64::new(0);
static MTX_OK_LOCK: AtomicU64 = AtomicU64::new(0);
static MTX_HOLDER_DONE: AtomicU64 = AtomicU64::new(0);
static PROBE_MTX_TIMEOUT: sync::KMutex = sync::KMutex::new();

fn task_mtx_timeout_holder() {
    PROBE_MTX_TIMEOUT.lock();
    scheduler::sleep_current_for_ticks(6);
    PROBE_MTX_TIMEOUT.unlock();
    MTX_HOLDER_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_mtx_timeout_waiter_short() {
    // Phase A: contended mutex with short deadline → should time out.
    let deadline = scheduler::ticks().saturating_add(3);
    let ok = PROBE_MTX_TIMEOUT.lock_by_deadline_poll(deadline);
    if !ok {
        MTX_TO_LOCK.store(1, Ordering::Relaxed);
    } else {
        PROBE_MTX_TIMEOUT.unlock();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_mtx_timeout_waiter_long() {
    // Phase B: generous deadline; succeeds after holder releases.
    let deadline = scheduler::ticks().saturating_add(30);
    let ok = PROBE_MTX_TIMEOUT.lock_by_deadline_poll(deadline);
    if ok {
        MTX_OK_LOCK.store(1, Ordering::Relaxed);
        PROBE_MTX_TIMEOUT.unlock();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_mutex_timeout() {
    MTX_TO_LOCK.store(0, Ordering::Relaxed);
    MTX_OK_LOCK.store(0, Ordering::Relaxed);
    MTX_HOLDER_DONE.store(0, Ordering::Relaxed);
    if PROBE_MTX_TIMEOUT.is_locked() {
        PROBE_MTX_TIMEOUT.unlock();
    }

    // Spawn holder at highest priority so it acquires the lock before waiters arrive.
    scheduler::spawn_task_with_fn_prio(task_mtx_timeout_holder, 10);
    scheduler::dispatch_once(); // holder runs, acquires lock, sleeps

    // Short waiter times out; long waiter succeeds after holder releases.
    scheduler::spawn_task_with_fn_prio(task_mtx_timeout_waiter_short, 20);
    scheduler::spawn_task_with_fn_prio(task_mtx_timeout_waiter_long, 30);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 120
        && (MTX_TO_LOCK.load(Ordering::Relaxed) == 0 || MTX_OK_LOCK.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let to_lock = MTX_TO_LOCK.load(Ordering::Relaxed);
    let ok_lock = MTX_OK_LOCK.load(Ordering::Relaxed);
    let holder_done = MTX_HOLDER_DONE.load(Ordering::Relaxed);

    serial::write_str("scheduler: mtx-deadline-poll to_lock=");
    serial::write_u64(to_lock);
    serial::write_str(" ok_lock=");
    serial::write_u64(ok_lock);
    serial::write_str(" holder_done=");
    serial::write_u64(holder_done);
    serial::write_line("");

    let pass = to_lock == 1 && ok_lock == 1 && holder_done == 1;
    serial::write_line(if pass {
        "scheduler: mtx-deadline-poll PASS"
    } else {
        "scheduler: mtx-deadline-poll FAIL"
    });
}

// --- mutex probe support ---
static MUTEX_COUNTER: AtomicU64 = AtomicU64::new(0);
static MUTEX_A_ACQUIRED: AtomicU64 = AtomicU64::new(0);
static MUTEX_B_WAITED: AtomicU64 = AtomicU64::new(0);
static PROBE_MUTEX: sync::KMutex = sync::KMutex::new();

fn task_mutex_a() {
    // Task A: grab the mutex, increment counter twice with a sleep in between,
    // then release.  During the sleep B is dispatched and must block on lock().
    PROBE_MUTEX.lock();
    MUTEX_A_ACQUIRED.store(1, Ordering::Relaxed);
    MUTEX_COUNTER.fetch_add(1, Ordering::Relaxed); // counter = 1
    scheduler::sleep_current_for_ticks(3); // B gets scheduled here
    MUTEX_COUNTER.fetch_add(1, Ordering::Relaxed); // counter = 2
    PROBE_MUTEX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_mutex_b() {
    // Task B: try to acquire the same mutex — will park until A unlocks.
    PROBE_MUTEX.lock();
    MUTEX_B_WAITED.store(1, Ordering::Relaxed);
    MUTEX_COUNTER.fetch_add(10, Ordering::Relaxed); // counter = 12
    PROBE_MUTEX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_mutex_contention() {
    MUTEX_COUNTER.store(0, Ordering::Relaxed);
    MUTEX_A_ACQUIRED.store(0, Ordering::Relaxed);
    MUTEX_B_WAITED.store(0, Ordering::Relaxed);

    let ta = scheduler::spawn_task_with_fn(task_mutex_a);
    let tb = scheduler::spawn_task_with_fn(task_mutex_b);

    // Drive until both tasks exit.
    for _ in 0..64 {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
        let done = [ta, tb].iter().all(|t| {
            t.map_or(true, |id| {
                scheduler::task_state(id) == scheduler::TaskState::Empty
            })
        });
        if done {
            break;
        }
    }

    while scheduler::dequeue_next().is_some() {}

    let counter = MUTEX_COUNTER.load(Ordering::Relaxed);
    let a_ok = MUTEX_A_ACQUIRED.load(Ordering::Relaxed);
    let b_ok = MUTEX_B_WAITED.load(Ordering::Relaxed);

    let mut empty_after: u64 = 0;
    for t in [ta, tb] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Empty {
                empty_after += 1;
            }
        }
    }

    serial::write_str("scheduler: mutex counter=");
    serial::write_u64(counter);
    serial::write_str(" a_acquired=");
    serial::write_u64(a_ok);
    serial::write_str(" b_waited=");
    serial::write_u64(b_ok);
    serial::write_str(" empty=");
    serial::write_u64(empty_after);
    serial::write_line("/2");

    let pass = counter == 12 && a_ok == 1 && b_ok == 1 && empty_after == 2;
    serial::write_line(if pass {
        "scheduler: mutex PASS"
    } else {
        "scheduler: mutex FAIL"
    });
}
