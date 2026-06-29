use super::*;

// --- priority-inheritance probe support ---
// Low-priority task holds a mutex while a high-priority waiter blocks on it.
// Medium-priority task competes for CPU. With inheritance enabled, low should
// be boosted to high priority while the high waiter is blocked.
static PI_HIGH_WAITING: AtomicU64 = AtomicU64::new(0);
static PI_HIGH_BLOCK_OBS: AtomicU64 = AtomicU64::new(0);
static PI_HIGH_DONE: AtomicU64 = AtomicU64::new(0);
static PI_LOW_DONE: AtomicU64 = AtomicU64::new(0);
static PI_MEDIUM_BEFORE_HIGH: AtomicU64 = AtomicU64::new(0);
static PROBE_PI_MUTEX: sync::KMutex = sync::KMutex::new();

fn task_pi_low_holder() {
    PROBE_PI_MUTEX.lock();
    // Busy section under lock; preemption can interrupt this section.
    for _ in 0..9_000_000 {
        core::hint::spin_loop();
    }
    PROBE_PI_MUTEX.unlock();
    PI_LOW_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_pi_high_waiter() {
    PI_HIGH_WAITING.store(1, Ordering::Relaxed);
    if PROBE_PI_MUTEX.is_locked() {
        PI_HIGH_BLOCK_OBS.store(1, Ordering::Relaxed);
    }
    PROBE_PI_MUTEX.lock();
    PI_HIGH_DONE.store(1, Ordering::Relaxed);
    PROBE_PI_MUTEX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_pi_medium_competitor() {
    for _ in 0..80 {
        if PI_HIGH_WAITING.load(Ordering::Relaxed) == 1
            && PI_HIGH_BLOCK_OBS.load(Ordering::Relaxed) == 1
            && PI_HIGH_DONE.load(Ordering::Relaxed) == 0
        {
            PI_MEDIUM_BEFORE_HIGH.fetch_add(1, Ordering::Relaxed);
        }
        if PI_HIGH_DONE.load(Ordering::Relaxed) == 1 {
            break;
        }
        scheduler::sleep_current_for_ticks(1);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_priority_inheritance() {
    PI_HIGH_WAITING.store(0, Ordering::Relaxed);
    PI_HIGH_BLOCK_OBS.store(0, Ordering::Relaxed);
    PI_HIGH_DONE.store(0, Ordering::Relaxed);
    PI_LOW_DONE.store(0, Ordering::Relaxed);
    PI_MEDIUM_BEFORE_HIGH.store(0, Ordering::Relaxed);

    let low_id = scheduler::spawn_task_with_fn_prio(task_pi_low_holder, 200).unwrap();
    // Give low task a head start so it acquires the mutex before high arrives.
    scheduler::dispatch_once();

    scheduler::spawn_task_with_fn_prio(task_pi_medium_competitor, 100);
    scheduler::spawn_task_with_fn_prio(task_pi_high_waiter, 10);

    let mut boost_seen = false;
    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 220
        && PI_HIGH_DONE.load(Ordering::Relaxed) == 0
    {
        if scheduler::task_priority(low_id) == 10 {
            boost_seen = true;
        }
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let high_waiting = PI_HIGH_WAITING.load(Ordering::Relaxed);
    let high_blocked = PI_HIGH_BLOCK_OBS.load(Ordering::Relaxed);
    let high_done = PI_HIGH_DONE.load(Ordering::Relaxed);
    let low_done = PI_LOW_DONE.load(Ordering::Relaxed);
    let medium_before = PI_MEDIUM_BEFORE_HIGH.load(Ordering::Relaxed);

    serial::write_str("scheduler: priority-inherit waiting=");
    serial::write_u64(high_waiting);
    serial::write_str(" blocked=");
    serial::write_u64(high_blocked);
    serial::write_str(" boost=");
    serial::write_u64(boost_seen as u64);
    serial::write_str(" medium_before=");
    serial::write_u64(medium_before);
    serial::write_str(" done=");
    serial::write_u64(low_done);
    serial::write_str(",");
    serial::write_u64(high_done);
    serial::write_line("");

    let pass = high_waiting == 1
        && high_blocked == 1
        && boost_seen
        && medium_before <= 2
        && low_done == 1
        && high_done == 1;
    serial::write_line(if pass {
        "scheduler: priority-inherit PASS"
    } else {
        "scheduler: priority-inherit FAIL"
    });
}
