use super::*;

pub(crate) fn probe_aging_telemetry() {
    // Snapshot global counters before this probe's scenario.
    let boosts_before = scheduler::stat_aging_boosts();
    let max_wait_before = scheduler::stat_max_wait_ticks();

    // Run a fresh aging scenario so we can verify the counters increment.
    // Aging enabled at ticks_per_level=2 means any task waiting ≥2 ticks gets a boost.
    scheduler::configure_aging(true, 2);
    AGING_STOP.store(0, Ordering::Relaxed);
    AGING_LOW_RAN.store(0, Ordering::Relaxed);
    AGING_HIGH_ITERS.store(0, Ordering::Relaxed);
    scheduler::spawn_task_with_fn_prio(task_aging_high_hog, 10);
    scheduler::spawn_task_with_fn_prio(task_aging_low_once, 120);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 300
        && AGING_LOW_RAN.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    AGING_STOP.store(1, Ordering::Relaxed);
    let drain = scheduler::ticks();
    while scheduler::dispatch_once() {
        if scheduler::ticks().saturating_sub(drain) > 60 {
            break;
        }
    }
    while scheduler::dequeue_next().is_some() {}

    let boosts_after = scheduler::stat_aging_boosts();
    let max_wait_global = scheduler::stat_max_wait_ticks();
    let boost_delta = boosts_after.saturating_sub(boosts_before);
    let max_advanced = max_wait_global >= max_wait_before;

    serial::write_str("scheduler: aging-telemetry boosts=");
    serial::write_u64(boost_delta);
    serial::write_str(" max_wait=");
    serial::write_u64(max_wait_global);
    serial::write_line("");

    let pass = boost_delta > 0 && max_wait_global > 0 && max_advanced;
    serial::write_line(if pass {
        "scheduler: aging-telemetry PASS"
    } else {
        "scheduler: aging-telemetry FAIL"
    });
}
