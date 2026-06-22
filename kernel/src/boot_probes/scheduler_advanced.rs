use core::sync::atomic::{AtomicU64, Ordering};

use crate::{idle, scheduler, serial, sync};

// --- rwlock probe support ---
// --- preemption probe support ---
// Spawn a CPU-bound task that never yields.  With a time slice of DEFAULT_SLICE
// ticks the timer ISR will preempt it mid-loop.  We verify STAT_PREEMPT_COUNT
// increases and the task eventually completes (not stuck forever).
static BUSY_SUM: AtomicU64 = AtomicU64::new(0);
static AGING_LOW_RAN: AtomicU64 = AtomicU64::new(0);
static AGING_HIGH_ITERS: AtomicU64 = AtomicU64::new(0);
static AGING_STOP: AtomicU64 = AtomicU64::new(0);

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

fn task_aging_high_hog() {
    while AGING_LOW_RAN.load(Ordering::Relaxed) == 0 && AGING_STOP.load(Ordering::Relaxed) == 0 {
        AGING_HIGH_ITERS.fetch_add(1, Ordering::Relaxed);
        core::hint::spin_loop();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_aging_low_once() {
    AGING_LOW_RAN.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

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

// --- task-names probe support ---
static NAME_A_MATCH: AtomicU64 = AtomicU64::new(0);
static NAME_B_MATCH: AtomicU64 = AtomicU64::new(0);
static NAME_C_MATCH: AtomicU64 = AtomicU64::new(0);

fn task_name_a() {
    // Verify that the name is visible from inside the task via current_task().
    if let Some(id) = scheduler::current_task() {
        if scheduler::task_name(id) == "alpha" {
            NAME_A_MATCH.store(1, Ordering::Relaxed);
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}
fn task_name_b() {
    if let Some(id) = scheduler::current_task() {
        if scheduler::task_name(id) == "beta" {
            NAME_B_MATCH.store(1, Ordering::Relaxed);
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}
fn task_name_c() {
    if let Some(id) = scheduler::current_task() {
        if scheduler::task_name(id) == "gamma" {
            NAME_C_MATCH.store(1, Ordering::Relaxed);
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_task_names() {
    NAME_A_MATCH.store(0, Ordering::Relaxed);
    NAME_B_MATCH.store(0, Ordering::Relaxed);
    NAME_C_MATCH.store(0, Ordering::Relaxed);

    // Spawn three named tasks; verify name is retrievable before dispatch.
    let id_a = scheduler::spawn_task_with_fn_prio_name(task_name_a, 128, "alpha").unwrap();
    let id_b = scheduler::spawn_task_with_fn_prio_name(task_name_b, 128, "beta").unwrap();
    let id_c = scheduler::spawn_task_with_fn_prio_name(task_name_c, 128, "gamma").unwrap();

    let pre_a = scheduler::task_name(id_a) == "alpha";
    let pre_b = scheduler::task_name(id_b) == "beta";
    let pre_c = scheduler::task_name(id_c) == "gamma";

    // Drain all three tasks.
    let deadline = scheduler::ticks() + 80;
    while scheduler::ticks() < deadline {
        if !scheduler::dispatch_once() {
            break;
        }
    }
    while scheduler::dispatch_once() {}

    let post_a = NAME_A_MATCH.load(Ordering::Relaxed);
    let post_b = NAME_B_MATCH.load(Ordering::Relaxed);
    let post_c = NAME_C_MATCH.load(Ordering::Relaxed);

    serial::write_str("scheduler: task-names pre=");
    serial::write_u64(pre_a as u64);
    serial::write_u64(pre_b as u64);
    serial::write_u64(pre_c as u64);
    serial::write_str(" in-task=");
    serial::write_u64(post_a);
    serial::write_u64(post_b);
    serial::write_u64(post_c);
    serial::write_line("");

    let pass = pre_a && pre_b && pre_c && post_a == 1 && post_b == 1 && post_c == 1;
    serial::write_line(if pass {
        "scheduler: task-names PASS"
    } else {
        "scheduler: task-names FAIL"
    });
}

// --- priority-mutation probe support ---
// Three tasks at mid priority (128). After all three are enqueued, we bump
// task C to priority 0 (highest urgency). The probe then dequeues one task
// and verifies it is C, proving the mutation won the next dequeue.
static PMUT_ORDER: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
static PMUT_SEQ: AtomicU64 = AtomicU64::new(0);

fn task_pmut_record() {
    let pos = PMUT_SEQ.fetch_add(1, Ordering::Relaxed) as usize;
    if pos < 3 {
        // Record which task_id ran at this position.
        if let Some(id) = scheduler::current_task() {
            PMUT_ORDER[pos].store(id.0, Ordering::Relaxed);
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_priority_mutation() {
    PMUT_SEQ.store(0, Ordering::Relaxed);
    PMUT_ORDER[0].store(0, Ordering::Relaxed);
    PMUT_ORDER[1].store(0, Ordering::Relaxed);
    PMUT_ORDER[2].store(0, Ordering::Relaxed);

    // Spawn A, B, C all at mid priority 128 — they enter the ring in FIFO order.
    let id_a = scheduler::spawn_task_with_fn_prio_name(task_pmut_record, 128, "pmut-A").unwrap();
    let id_b = scheduler::spawn_task_with_fn_prio_name(task_pmut_record, 128, "pmut-B").unwrap();
    let id_c = scheduler::spawn_task_with_fn_prio_name(task_pmut_record, 128, "pmut-C").unwrap();

    // Verify initial priorities are all 128.
    let prio_before_a = scheduler::task_priority(id_a);
    let prio_before_c = scheduler::task_priority(id_c);

    // Bump C to highest urgency — must beat A and B on the next dequeue.
    let bump_ok = scheduler::set_task_priority(id_c, 0);
    let prio_after_c = scheduler::task_priority(id_c);

    // Dispatch once: should pick C (priority 0).
    scheduler::dispatch_once();
    // Drain A and B.
    scheduler::dispatch_once();
    scheduler::dispatch_once();

    let first_ran = PMUT_ORDER[0].load(Ordering::Relaxed);
    let c_ran_first = first_ran == id_c.0;

    serial::write_str("scheduler: priority-mutation prio_before=");
    serial::write_u64(prio_before_a as u64);
    serial::write_str(",");
    serial::write_u64(prio_before_c as u64);
    serial::write_str(" prio_after_c=");
    serial::write_u64(prio_after_c as u64);
    serial::write_str(" bump_ok=");
    serial::write_u64(bump_ok as u64);
    serial::write_str(" c_first=");
    serial::write_u64(c_ran_first as u64);
    serial::write_line("");

    let pass =
        prio_before_a == 128 && prio_before_c == 128 && bump_ok && prio_after_c == 0 && c_ran_first;
    serial::write_line(if pass {
        "scheduler: priority-mutation PASS"
    } else {
        "scheduler: priority-mutation FAIL"
    });
}

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

// --- dispatch probe support ---
static PROBE_DISPATCH_A: AtomicU64 = AtomicU64::new(0);
static PROBE_DISPATCH_B: AtomicU64 = AtomicU64::new(0);
static PROBE_SLEEP_A: AtomicU64 = AtomicU64::new(0);
static PROBE_SLEEP_B: AtomicU64 = AtomicU64::new(0);
static PROBE_WAKE_BASE: AtomicU64 = AtomicU64::new(0);
static PROBE_WAKE_RUN_A: AtomicU64 = AtomicU64::new(0);
static PROBE_WAKE_RUN_B: AtomicU64 = AtomicU64::new(0);
static PROBE_WAKE_RUN_C: AtomicU64 = AtomicU64::new(0);
static PROBE_WAKE_SEQ: AtomicU64 = AtomicU64::new(0);
static PROBE_WAKE_ORDER1: AtomicU64 = AtomicU64::new(0);
static PROBE_WAKE_ORDER2: AtomicU64 = AtomicU64::new(0);
static PROBE_WAKE_ORDER3: AtomicU64 = AtomicU64::new(0);
static PROBE_MIX_A: AtomicU64 = AtomicU64::new(0);
static PROBE_MIX_B: AtomicU64 = AtomicU64::new(0);
static PROBE_MIX_C: AtomicU64 = AtomicU64::new(0);
static PROBE_STRESS_SEED: AtomicU64 = AtomicU64::new(0);
static PROBE_STRESS_A: AtomicU64 = AtomicU64::new(0);
static PROBE_STRESS_B: AtomicU64 = AtomicU64::new(0);
static PROBE_STRESS_C: AtomicU64 = AtomicU64::new(0);

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

fn record_wake_position(label: u64) {
    let pos = PROBE_WAKE_SEQ
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    if pos == 1 {
        PROBE_WAKE_ORDER1.store(label, Ordering::Relaxed);
    } else if pos == 2 {
        PROBE_WAKE_ORDER2.store(label, Ordering::Relaxed);
    } else if pos == 3 {
        PROBE_WAKE_ORDER3.store(label, Ordering::Relaxed);
    }
}

fn task_wake_a() {
    let base = PROBE_WAKE_BASE.load(Ordering::Relaxed);
    scheduler::sleep_current_until_tick(base.saturating_add(3));
    record_wake_position(1);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_wake_b() {
    let base = PROBE_WAKE_BASE.load(Ordering::Relaxed);
    scheduler::sleep_current_until_tick(base.saturating_add(1));
    record_wake_position(2);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_wake_c() {
    let base = PROBE_WAKE_BASE.load(Ordering::Relaxed);
    scheduler::sleep_current_until_tick(base.saturating_add(2));
    record_wake_position(3);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_mix_a() {
    PROBE_MIX_A.fetch_add(1, Ordering::Relaxed);
    scheduler::sleep_current_for_ticks(2);
    PROBE_MIX_A.fetch_add(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_mix_b() {
    PROBE_MIX_B.fetch_add(1, Ordering::Relaxed);
    scheduler::sleep_current_for_ticks(1);
    PROBE_MIX_B.fetch_add(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_mix_c() {
    for _ in 0..3 {
        PROBE_MIX_C.fetch_add(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

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

pub(crate) fn probe_task_wake_order() {
    PROBE_WAKE_RUN_A.store(0, Ordering::Relaxed);
    PROBE_WAKE_RUN_B.store(0, Ordering::Relaxed);
    PROBE_WAKE_RUN_C.store(0, Ordering::Relaxed);
    PROBE_WAKE_SEQ.store(0, Ordering::Relaxed);
    PROBE_WAKE_ORDER1.store(0, Ordering::Relaxed);
    PROBE_WAKE_ORDER2.store(0, Ordering::Relaxed);
    PROBE_WAKE_ORDER3.store(0, Ordering::Relaxed);

    let base = scheduler::ticks();
    PROBE_WAKE_BASE.store(base, Ordering::Relaxed);

    let ta = scheduler::spawn_task_with_fn(task_wake_a);
    let tb = scheduler::spawn_task_with_fn(task_wake_b);
    let tc = scheduler::spawn_task_with_fn(task_wake_c);

    // First pass: all tasks move to Sleeping with staggered deadlines.
    scheduler::dispatch_once();
    scheduler::dispatch_once();
    scheduler::dispatch_once();

    // Wait enough ticks for all three deadlines to pass.
    idle::sleep_for_ticks(5);

    // Second pass: tasks wake and run in deadline order (B, C, A).
    scheduler::dispatch_once();
    scheduler::dispatch_once();
    scheduler::dispatch_once();

    let o1 = PROBE_WAKE_ORDER1.load(Ordering::Relaxed);
    let o2 = PROBE_WAKE_ORDER2.load(Ordering::Relaxed);
    let o3 = PROBE_WAKE_ORDER3.load(Ordering::Relaxed);

    let mut empty_after: u64 = 0;
    for t in [ta, tb, tc] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Empty {
                empty_after += 1;
            }
        }
    }

    while scheduler::dequeue_next().is_some() {}

    serial::write_str("scheduler: wake-order order=");
    serial::write_u64(o1);
    serial::write_str(",");
    serial::write_u64(o2);
    serial::write_str(",");
    serial::write_u64(o3);
    serial::write_str(" empty=");
    serial::write_u64(empty_after);
    serial::write_line("/3");
}

pub(crate) fn probe_task_mixed_fairness() {
    PROBE_MIX_A.store(0, Ordering::Relaxed);
    PROBE_MIX_B.store(0, Ordering::Relaxed);
    PROBE_MIX_C.store(0, Ordering::Relaxed);

    let ta = scheduler::spawn_task_with_fn(task_mix_a);
    let tb = scheduler::spawn_task_with_fn(task_mix_b);
    let tc = scheduler::spawn_task_with_fn(task_mix_c);

    // Phase 1: A/B go sleeping, C stays runnable and consumes remaining slices.
    scheduler::dispatch_once();
    scheduler::dispatch_once();
    scheduler::dispatch_once();
    scheduler::dispatch_once();
    scheduler::dispatch_once();

    // Phase 2: wake sleepers and let each run once more and exit.
    idle::sleep_for_ticks(4);
    scheduler::dispatch_once();
    scheduler::dispatch_once();

    let a = PROBE_MIX_A.load(Ordering::Relaxed);
    let b = PROBE_MIX_B.load(Ordering::Relaxed);
    let c = PROBE_MIX_C.load(Ordering::Relaxed);

    let mut empty_after: u64 = 0;
    for t in [ta, tb, tc] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Empty {
                empty_after += 1;
            }
        }
    }

    while scheduler::dequeue_next().is_some() {}

    serial::write_str("scheduler: mixed-fairness a=");
    serial::write_u64(a);
    serial::write_str(" b=");
    serial::write_u64(b);
    serial::write_str(" c=");
    serial::write_u64(c);
    serial::write_str(" empty=");
    serial::write_u64(empty_after);
    serial::write_line("/3");
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

pub(crate) fn probe_scheduler_task_state() {
    let id = scheduler::spawn_task();

    if let Some(task) = id {
        let state = scheduler::task_state(task);
        serial::write_str("scheduler: task-state task_id=");
        serial::write_u64(task.0);
        serial::write_str(" state=");
        serial::write_str(match state {
            scheduler::TaskState::Ready => "Ready",
            scheduler::TaskState::Running => "Running",
            scheduler::TaskState::Sleeping => "Sleeping",
            scheduler::TaskState::Empty => "Empty",
        });
        serial::write_line("");
        // Drain again to keep the queue empty for the idle loop.
        scheduler::dequeue_next();
    }
}

pub(crate) fn probe_task_lifecycle() {
    // Spawn 3 tasks — ring is empty at this point so all must succeed.
    let ta = scheduler::spawn_task();
    let tb = scheduler::spawn_task();
    let tc = scheduler::spawn_task();

    // Verify all are Ready immediately after spawn.
    let mut ready_count: u64 = 0;
    for t in [ta, tb, tc] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Ready {
                ready_count += 1;
            }
        }
    }

    // Simulate being scheduled: dequeue each from the ring.
    scheduler::dequeue_next();
    scheduler::dequeue_next();
    scheduler::dequeue_next();

    // Simulate task completion: exit clears the metadata table entry.
    for t in [ta, tb, tc] {
        if let Some(task) = t {
            scheduler::exit_task(task);
        }
    }

    // Verify all are now Empty.
    let mut empty_count: u64 = 0;
    for t in [ta, tb, tc] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Empty {
                empty_count += 1;
            }
        }
    }

    serial::write_str("scheduler: task-lifecycle ready=");
    serial::write_u64(ready_count);
    serial::write_str("/3 empty=");
    serial::write_u64(empty_count);
    serial::write_str("/3");
    serial::write_line("");
}

pub(crate) fn probe_scheduler_ring_overflow() {
    // Fill the ring to capacity, then attempt one extra spawn — it must return None.
    let cap = scheduler::ring_capacity();
    let mut spawned: usize = 0;
    let mut dropped: usize = 0;

    for _ in 0..=cap {
        match scheduler::spawn_task() {
            Some(_) => spawned += 1,
            None => dropped += 1,
        }
    }

    // Drain the ring so the idle-decision logic stays clean.
    let mut drained: usize = 0;
    while scheduler::dequeue_next().is_some() {
        drained += 1;
    }

    serial::write_str("scheduler: ring-overflow spawned=");
    serial::write_u64(spawned as u64);
    serial::write_str(" dropped=");
    serial::write_u64(dropped as u64);
    serial::write_str(" drained=");
    serial::write_u64(drained as u64);
    serial::write_line("");
}
