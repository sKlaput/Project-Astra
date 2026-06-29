use super::*;

pub(crate) fn probe_priority_aging() {
    AGING_STOP.store(0, Ordering::Relaxed);
    AGING_LOW_RAN.store(0, Ordering::Relaxed);
    AGING_HIGH_ITERS.store(0, Ordering::Relaxed);

    // Big base-priority gap with aging enabled: low-priority task should run eventually.
    scheduler::spawn_task_with_fn_prio(task_aging_high_hog, 10);
    scheduler::spawn_task_with_fn_prio(task_aging_low_once, 120);

    let start = scheduler::ticks();
    let max_wait_ticks = 450u64;
    while scheduler::ticks().saturating_sub(start) < max_wait_ticks
        && AGING_LOW_RAN.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    AGING_STOP.store(1, Ordering::Relaxed);
    let drain_start = scheduler::ticks();
    while scheduler::dispatch_once() {
        if scheduler::ticks().saturating_sub(drain_start) > 80 {
            break;
        }
    }
    while scheduler::dequeue_next().is_some() {}

    let low = AGING_LOW_RAN.load(Ordering::Relaxed);
    let iters = AGING_HIGH_ITERS.load(Ordering::Relaxed);
    let waited = scheduler::ticks().saturating_sub(start);

    serial::write_str("scheduler: aging low_ran=");
    serial::write_u64(low);
    serial::write_str(" waited_ticks=");
    serial::write_u64(waited);
    serial::write_str(" high_iters=");
    serial::write_u64(iters);
    serial::write_line("");

    let pass = low == 1;
    serial::write_line(if pass {
        "scheduler: aging PASS"
    } else {
        "scheduler: aging FAIL"
    });
}
