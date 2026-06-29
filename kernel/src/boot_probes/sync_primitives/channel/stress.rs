use super::*;

static CHAN_STRESS_COUNT: AtomicU64 = AtomicU64::new(0);
static CHAN_STRESS_SUM: AtomicU64 = AtomicU64::new(0);
static CHAN_STRESS_CONS_A: AtomicU64 = AtomicU64::new(0);
static CHAN_STRESS_CONS_B: AtomicU64 = AtomicU64::new(0);
static PROBE_CHAN_STRESS: sync::KChannel = sync::KChannel::new();

fn task_chan_stress_prod_a() {
    let mut value = 1u64;
    for _ in 0..8 {
        PROBE_CHAN_STRESS.send(value);
        value += 2;
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_chan_stress_prod_b() {
    let mut value = 2u64;
    for _ in 0..8 {
        PROBE_CHAN_STRESS.send(value);
        value += 2;
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_chan_stress_cons_a() {
    for i in 0..8 {
        let value = PROBE_CHAN_STRESS.recv();
        CHAN_STRESS_COUNT.fetch_add(1, Ordering::Relaxed);
        CHAN_STRESS_SUM.fetch_add(value, Ordering::Relaxed);
        CHAN_STRESS_CONS_A.fetch_add(1, Ordering::Relaxed);
        if i < 7 {
            scheduler::sleep_current_for_ticks(1);
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_chan_stress_cons_b() {
    for i in 0..8 {
        let value = PROBE_CHAN_STRESS.recv();
        CHAN_STRESS_COUNT.fetch_add(1, Ordering::Relaxed);
        CHAN_STRESS_SUM.fetch_add(value, Ordering::Relaxed);
        CHAN_STRESS_CONS_B.fetch_add(1, Ordering::Relaxed);
        if i < 7 {
            scheduler::sleep_current_for_ticks(1);
        }
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_channel_stress() {
    CHAN_STRESS_COUNT.store(0, Ordering::Relaxed);
    CHAN_STRESS_SUM.store(0, Ordering::Relaxed);
    CHAN_STRESS_CONS_A.store(0, Ordering::Relaxed);
    CHAN_STRESS_CONS_B.store(0, Ordering::Relaxed);

    // Producers outrank consumers so the 2-slot buffer repeatedly fills and
    // forces send-side blocking. The consumer sleeps re-open the empty/full
    // window many times across a bounded run.
    scheduler::spawn_task_with_fn_prio(task_chan_stress_prod_a, 10);
    scheduler::spawn_task_with_fn_prio(task_chan_stress_prod_b, 20);
    scheduler::spawn_task_with_fn_prio(task_chan_stress_cons_a, 80);
    scheduler::spawn_task_with_fn_prio(task_chan_stress_cons_b, 90);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 200
        && CHAN_STRESS_COUNT.load(Ordering::Relaxed) < 16
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let count = CHAN_STRESS_COUNT.load(Ordering::Relaxed);
    let sum = CHAN_STRESS_SUM.load(Ordering::Relaxed);
    let cons_a = CHAN_STRESS_CONS_A.load(Ordering::Relaxed);
    let cons_b = CHAN_STRESS_CONS_B.load(Ordering::Relaxed);
    let remaining = PROBE_CHAN_STRESS.len();

    serial::write_str("scheduler: channel-stress count=");
    serial::write_u64(count);
    serial::write_str(" sum=");
    serial::write_u64(sum);
    serial::write_str(" cons=");
    serial::write_u64(cons_a);
    serial::write_str(",");
    serial::write_u64(cons_b);
    serial::write_str(" remaining=");
    serial::write_u64(remaining);
    serial::write_line("");

    let pass = count == 16 && sum == 136 && cons_a == 8 && cons_b == 8 && remaining == 0;
    serial::write_line(if pass {
        "scheduler: channel-stress PASS"
    } else {
        "scheduler: channel-stress FAIL"
    });
}
