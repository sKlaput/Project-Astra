use super::*;

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
