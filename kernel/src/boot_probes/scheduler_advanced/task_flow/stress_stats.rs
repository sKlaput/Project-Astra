use super::*;

static PROBE_STRESS_SEED: AtomicU64 = AtomicU64::new(0);
static PROBE_STRESS_A: AtomicU64 = AtomicU64::new(0);
static PROBE_STRESS_B: AtomicU64 = AtomicU64::new(0);
static PROBE_STRESS_C: AtomicU64 = AtomicU64::new(0);

fn next_stress_delay() -> u64 {
    let prev = PROBE_STRESS_SEED
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| {
            Some(
                x.wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407),
            )
        })
        .unwrap_or(0);
    let mixed = prev ^ (prev >> 17);
    (mixed % 3).saturating_add(1)
}

fn task_stress_a() {
    for i in 0..5u64 {
        PROBE_STRESS_A.fetch_add(1, Ordering::Relaxed);
        if i < 4 {
            scheduler::sleep_current_for_ticks(next_stress_delay());
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_stress_b() {
    for i in 0..5u64 {
        PROBE_STRESS_B.fetch_add(1, Ordering::Relaxed);
        if i < 4 {
            scheduler::sleep_current_for_ticks(next_stress_delay());
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_stress_c() {
    for i in 0..5u64 {
        PROBE_STRESS_C.fetch_add(1, Ordering::Relaxed);
        if i < 4 {
            scheduler::sleep_current_for_ticks(next_stress_delay());
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_scheduler_invariants() {
    let flags = scheduler::debug_invariant_flags();

    serial::write_str("scheduler: invariants flags=");
    serial::write_u64(flags);
    serial::write_line("");
}

pub(crate) fn probe_task_stress_sleep_mix() {
    PROBE_STRESS_SEED.store(0xC0FFEE1234ABCDEF, Ordering::Relaxed);
    PROBE_STRESS_A.store(0, Ordering::Relaxed);
    PROBE_STRESS_B.store(0, Ordering::Relaxed);
    PROBE_STRESS_C.store(0, Ordering::Relaxed);

    let ta = scheduler::spawn_task_with_fn(task_stress_a);
    let tb = scheduler::spawn_task_with_fn(task_stress_b);
    let tc = scheduler::spawn_task_with_fn(task_stress_c);

    for _ in 0..96 {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }

        let all_empty = [ta, tb, tc].iter().all(|t| {
            if let Some(task) = t {
                scheduler::task_state(*task) == scheduler::TaskState::Empty
            } else {
                true
            }
        });
        if all_empty {
            break;
        }
    }

    let a = PROBE_STRESS_A.load(Ordering::Relaxed);
    let b = PROBE_STRESS_B.load(Ordering::Relaxed);
    let c = PROBE_STRESS_C.load(Ordering::Relaxed);
    let flags = scheduler::debug_invariant_flags();

    let mut empty_after: u64 = 0;
    for t in [ta, tb, tc] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Empty {
                empty_after += 1;
            }
        }
    }

    while scheduler::dequeue_next().is_some() {}

    serial::write_str("scheduler: stress-sleep runs=");
    serial::write_u64(a);
    serial::write_str(",");
    serial::write_u64(b);
    serial::write_str(",");
    serial::write_u64(c);
    serial::write_str(" flags=");
    serial::write_u64(flags);
    serial::write_str(" empty=");
    serial::write_u64(empty_after);
    serial::write_line("/3");
}

pub(crate) fn probe_scheduler_stats() {
    let stats = scheduler::debug_stats_snapshot();

    serial::write_str("scheduler: stats dispatch=");
    serial::write_u64(stats.dispatches);
    serial::write_str(" sleep=");
    serial::write_u64(stats.sleeps);
    serial::write_str(" wake=");
    serial::write_u64(stats.wakes);
    serial::write_str(" exit=");
    serial::write_u64(stats.exits);
    serial::write_str(" requeue=");
    serial::write_u64(stats.requeues);
    serial::write_line("");
}

pub(crate) fn probe_scheduler_stats_guard() {
    // Baseline captured from cooperative stack-switch dispatch model.
    const EXPECT_DISPATCH: u64 = 34;
    const EXPECT_SLEEP: u64 = 20;
    const EXPECT_WAKE: u64 = 20;
    const EXPECT_EXIT: u64 = 17;
    const EXPECT_REQUEUE: u64 = 0;

    let stats = scheduler::debug_stats_snapshot();
    let mut mismatch_mask: u64 = 0;

    if stats.dispatches != EXPECT_DISPATCH {
        mismatch_mask |= 1 << 0;
    }
    if stats.sleeps != EXPECT_SLEEP {
        mismatch_mask |= 1 << 1;
    }
    if stats.wakes != EXPECT_WAKE {
        mismatch_mask |= 1 << 2;
    }
    if stats.exits != EXPECT_EXIT {
        mismatch_mask |= 1 << 3;
    }
    if stats.requeues != EXPECT_REQUEUE {
        mismatch_mask |= 1 << 4;
    }

    serial::write_str("scheduler: stats-guard mask=");
    serial::write_u64(mismatch_mask);
    serial::write_str(" expect=");
    serial::write_u64(EXPECT_DISPATCH);
    serial::write_str(",");
    serial::write_u64(EXPECT_SLEEP);
    serial::write_str(",");
    serial::write_u64(EXPECT_WAKE);
    serial::write_str(",");
    serial::write_u64(EXPECT_EXIT);
    serial::write_str(",");
    serial::write_u64(EXPECT_REQUEUE);
    serial::write_str(" got=");
    serial::write_u64(stats.dispatches);
    serial::write_str(",");
    serial::write_u64(stats.sleeps);
    serial::write_str(",");
    serial::write_u64(stats.wakes);
    serial::write_str(",");
    serial::write_u64(stats.exits);
    serial::write_str(",");
    serial::write_u64(stats.requeues);
    serial::write_line("");

    if mismatch_mask != 0 {
        serial::write_str("scheduler: stats-guard reason=");
        if (mismatch_mask & (1 << 0)) != 0 {
            serial::write_str("dispatch,");
        }
        if (mismatch_mask & (1 << 1)) != 0 {
            serial::write_str("sleep,");
        }
        if (mismatch_mask & (1 << 2)) != 0 {
            serial::write_str("wake,");
        }
        if (mismatch_mask & (1 << 3)) != 0 {
            serial::write_str("exit,");
        }
        if (mismatch_mask & (1 << 4)) != 0 {
            serial::write_str("requeue,");
        }
        serial::write_line("");
        serial::write_line("scheduler: stats-guard FAIL");
    } else {
        serial::write_line("scheduler: stats-guard reason=none");
        serial::write_line("scheduler: stats-guard PASS");
    }
}
