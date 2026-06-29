use super::*;

// --- spinlock probe support ---
static SPIN_COUNTER: AtomicU64 = AtomicU64::new(0);
static PROBE_SPIN: sync::KSpinlock = sync::KSpinlock::new();

fn task_spin_a() {
    for _ in 0..2 {
        PROBE_SPIN.lock();
        SPIN_COUNTER.fetch_add(1, Ordering::Relaxed);
        PROBE_SPIN.unlock();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_spin_b() {
    for _ in 0..3 {
        PROBE_SPIN.lock();
        SPIN_COUNTER.fetch_add(1, Ordering::Relaxed);
        PROBE_SPIN.unlock();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_spinlock() {
    SPIN_COUNTER.store(0, Ordering::Relaxed);

    let ta = scheduler::spawn_task_with_fn(task_spin_a);
    let tb = scheduler::spawn_task_with_fn(task_spin_b);

    for _ in 0..100 {
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

    let counter = SPIN_COUNTER.load(Ordering::Relaxed);

    serial::write_str("scheduler: spinlock counter=");
    serial::write_u64(counter);
    serial::write_line("");

    let pass = counter == 5;
    serial::write_line(if pass {
        "scheduler: spinlock PASS"
    } else {
        "scheduler: spinlock FAIL"
    });
}
