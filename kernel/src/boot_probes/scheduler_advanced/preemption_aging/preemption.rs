use super::*;

// --- preemption probe support ---
// Spawn a CPU-bound task that never yields.  With a time slice of DEFAULT_SLICE
// ticks the timer ISR will preempt it mid-loop.  We verify STAT_PREEMPT_COUNT
// increases and the task eventually completes (not stuck forever).
static BUSY_SUM: AtomicU64 = AtomicU64::new(0);
fn task_busy_work() {
    // ~10 million wrapping adds in debug mode ≈ 100-200 ms >> one 10 ms tick.
    let mut acc: u64 = 0;
    for i in 0..10_000_000u64 {
        acc = acc.wrapping_add(i);
    }
    BUSY_SUM.store(acc, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_preemption() {
    BUSY_SUM.store(0, Ordering::Relaxed);
    let before = scheduler::stat_preempt_count();

    scheduler::spawn_task_with_fn(task_busy_work);

    // Keep dispatching until the task exits.  dispatch_once returns false
    // only when the ring is empty (task exited or never spawned).
    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let after = scheduler::stat_preempt_count();
    let preempted = after - before;
    let sum = BUSY_SUM.load(Ordering::Relaxed);

    serial::write_str("scheduler: preemption count=");
    serial::write_u64(preempted);
    serial::write_str(" sum=");
    serial::write_u64(sum);
    serial::write_line("");

    // Task must have been preempted at least once and must have completed.
    let pass = preempted >= 1 && sum != 0;
    serial::write_line(if pass {
        "scheduler: preemption PASS"
    } else {
        "scheduler: preemption FAIL"
    });
}
