use super::*;

pub(crate) fn probe_aging_toggle() {
    let prev_enabled = scheduler::debug_aging_enabled();
    let prev_ticks = scheduler::debug_aging_ticks_per_level();

    // Phase A: aging disabled — lower-priority task is expected to starve in this window.
    scheduler::configure_aging(false, prev_ticks);
    AGING_STOP.store(0, Ordering::Relaxed);
    AGING_LOW_RAN.store(0, Ordering::Relaxed);
    AGING_HIGH_ITERS.store(0, Ordering::Relaxed);
    scheduler::spawn_task_with_fn_prio(task_aging_high_hog, 10);
    scheduler::spawn_task_with_fn_prio(task_aging_low_once, 120);

    let start_a = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_a) < 160
        && AGING_LOW_RAN.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    let low_disabled = AGING_LOW_RAN.load(Ordering::Relaxed);

    AGING_STOP.store(1, Ordering::Relaxed);
    let drain_a = scheduler::ticks();
    while scheduler::dispatch_once() {
        if scheduler::ticks().saturating_sub(drain_a) > 60 {
            break;
        }
    }
    while scheduler::dequeue_next().is_some() {}

    // Phase B: aging enabled — lower-priority task should run.
    scheduler::configure_aging(true, 2);
    AGING_STOP.store(0, Ordering::Relaxed);
    AGING_LOW_RAN.store(0, Ordering::Relaxed);
    AGING_HIGH_ITERS.store(0, Ordering::Relaxed);
    scheduler::spawn_task_with_fn_prio(task_aging_high_hog, 10);
    scheduler::spawn_task_with_fn_prio(task_aging_low_once, 120);

    let start_b = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_b) < 280
        && AGING_LOW_RAN.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    let low_enabled = AGING_LOW_RAN.load(Ordering::Relaxed);

    AGING_STOP.store(1, Ordering::Relaxed);
    let drain_b = scheduler::ticks();
    while scheduler::dispatch_once() {
        if scheduler::ticks().saturating_sub(drain_b) > 60 {
            break;
        }
    }
    while scheduler::dequeue_next().is_some() {}

    scheduler::configure_aging(prev_enabled, prev_ticks);

    serial::write_str("scheduler: aging-toggle disabled_low=");
    serial::write_u64(low_disabled);
    serial::write_str(" enabled_low=");
    serial::write_u64(low_enabled);
    serial::write_line("");

    let pass = low_disabled == 0 && low_enabled == 1;
    serial::write_line(if pass {
        "scheduler: aging-toggle PASS"
    } else {
        "scheduler: aging-toggle FAIL"
    });
}
