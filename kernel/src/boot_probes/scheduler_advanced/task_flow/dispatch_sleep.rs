use super::*;

static PROBE_DISPATCH_A: AtomicU64 = AtomicU64::new(0);
static PROBE_DISPATCH_B: AtomicU64 = AtomicU64::new(0);
static PROBE_SLEEP_A: AtomicU64 = AtomicU64::new(0);
static PROBE_SLEEP_B: AtomicU64 = AtomicU64::new(0);

fn task_dispatch_a() {
    for _ in 0..2 {
        PROBE_DISPATCH_A.fetch_add(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}
fn task_dispatch_b() {
    for _ in 0..2 {
        PROBE_DISPATCH_B.fetch_add(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_sleep_a() {
    PROBE_SLEEP_A.fetch_add(1, Ordering::Relaxed);
    scheduler::sleep_current_for_ticks(3);
    PROBE_SLEEP_A.fetch_add(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_sleep_b() {
    PROBE_SLEEP_B.fetch_add(1, Ordering::Relaxed);
    scheduler::sleep_current_for_ticks(3);
    PROBE_SLEEP_B.fetch_add(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_task_dispatch() {
    PROBE_DISPATCH_A.store(0, Ordering::Relaxed);
    PROBE_DISPATCH_B.store(0, Ordering::Relaxed);

    scheduler::spawn_task_with_fn(task_dispatch_a);
    scheduler::spawn_task_with_fn(task_dispatch_b);

    // Each task runs its full loop in one dispatch and exits; 2 calls do work,
    // the remaining 2 return false (empty ring).
    for _ in 0..4 {
        scheduler::dispatch_once();
    }

    // Drain the re-queued tasks so the idle loop starts clean.
    while scheduler::dequeue_next().is_some() {}

    let a = PROBE_DISPATCH_A.load(Ordering::Relaxed);
    let b = PROBE_DISPATCH_B.load(Ordering::Relaxed);

    serial::write_str("scheduler: dispatch task_a=");
    serial::write_u64(a);
    serial::write_str(" task_b=");
    serial::write_u64(b);
    serial::write_line("");
}

pub(crate) fn probe_task_sleep_queue() {
    PROBE_SLEEP_A.store(0, Ordering::Relaxed);
    PROBE_SLEEP_B.store(0, Ordering::Relaxed);

    let ta = scheduler::spawn_task_with_fn(task_sleep_a);
    let tb = scheduler::spawn_task_with_fn(task_sleep_b);

    // First dispatch round: both tasks run once and transition to Sleeping.
    scheduler::dispatch_once();
    scheduler::dispatch_once();

    let mut sleeping_before: u64 = 0;
    for t in [ta, tb] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Sleeping {
                sleeping_before += 1;
            }
        }
    }

    // Advance time so tick() wakes both tasks back to Ready.
    idle::sleep_for_ticks(4);

    // Second dispatch round: each task runs again and exits itself.
    scheduler::dispatch_once();
    scheduler::dispatch_once();

    let a_runs = PROBE_SLEEP_A.load(Ordering::Relaxed);
    let b_runs = PROBE_SLEEP_B.load(Ordering::Relaxed);

    let mut empty_after: u64 = 0;
    for t in [ta, tb] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Empty {
                empty_after += 1;
            }
        }
    }

    while scheduler::dequeue_next().is_some() {}

    serial::write_str("scheduler: sleep-queue sleeping=");
    serial::write_u64(sleeping_before);
    serial::write_str("/2 runs_a=");
    serial::write_u64(a_runs);
    serial::write_str(" runs_b=");
    serial::write_u64(b_runs);
    serial::write_str(" empty=");
    serial::write_u64(empty_after);
    serial::write_line("/2");
}
