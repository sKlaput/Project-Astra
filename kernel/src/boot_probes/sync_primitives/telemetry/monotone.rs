use super::*;

// --- telemetry monotonicity guard probe ---
// No statics or tasks needed: drive the fail counter with deliberate bad
// unparks and assert all three counters are non-decreasing between two
// consecutive snapshots.
pub(crate) fn probe_telemetry_monotone() {
    let p0 = scheduler::stat_park_count();
    let u0 = scheduler::stat_unpark_count();
    let f0 = scheduler::stat_unpark_fail_count();

    // Exactly two invalid unparks drive the fail counter by a known delta.
    scheduler::unpark_task(scheduler::TaskId(0xDEAD_DEAD_DEAD_0001));
    scheduler::unpark_task(scheduler::TaskId(0xDEAD_DEAD_DEAD_0002));

    let p1 = scheduler::stat_park_count();
    let u1 = scheduler::stat_unpark_count();
    let f1 = scheduler::stat_unpark_fail_count();

    let fail_delta = f1.saturating_sub(f0);
    serial::write_str("scheduler: telemetry-mono parks=");
    serial::write_u64(p1);
    serial::write_str(" unparks=");
    serial::write_u64(u1);
    serial::write_str(" fail_delta=");
    serial::write_u64(fail_delta);
    serial::write_line("");

    let pass = p1 >= p0 && u1 >= u0 && fail_delta == 2;
    serial::write_line(if pass {
        "scheduler: telemetry-mono PASS"
    } else {
        "scheduler: telemetry-mono FAIL"
    });
}
