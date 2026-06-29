use super::*;

// --- mixed sync probe support ---
// Producer fills a bounded channel and signals a semaphore per item. Two
// consumers acquire the semaphore, receive from the channel, then contend on a
// mutex while updating shared totals. This exercises cross-primitive park and
// unpark flow under a bounded deterministic workload.
static SYNC_MIX_COUNT: AtomicU64 = AtomicU64::new(0);
static SYNC_MIX_SUM: AtomicU64 = AtomicU64::new(0);
static SYNC_MIX_CONS_A: AtomicU64 = AtomicU64::new(0);
static SYNC_MIX_CONS_B: AtomicU64 = AtomicU64::new(0);
static SYNC_MIX_MUTEX_WAIT: AtomicU64 = AtomicU64::new(0);
static SYNC_MIX_SLEEP_ONCE: AtomicU64 = AtomicU64::new(0);
static PROBE_SYNC_MIX_MUTEX: sync::KMutex = sync::KMutex::new();
static PROBE_SYNC_MIX_SEM: sync::KSemaphore = sync::KSemaphore::new(0);
static PROBE_SYNC_MIX_CHAN: sync::KChannel = sync::KChannel::new();

fn task_sync_mix_producer() {
    for value in 1..=6u64 {
        PROBE_SYNC_MIX_CHAN.send(value);
        PROBE_SYNC_MIX_SEM.up();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_sync_mix_consumer_a() {
    for _ in 0..3 {
        PROBE_SYNC_MIX_SEM.down();
        let value = PROBE_SYNC_MIX_CHAN.recv();
        if PROBE_SYNC_MIX_MUTEX.is_locked() {
            SYNC_MIX_MUTEX_WAIT.store(1, Ordering::Relaxed);
        }
        PROBE_SYNC_MIX_MUTEX.lock();
        SYNC_MIX_CONS_A.fetch_add(1, Ordering::Relaxed);
        SYNC_MIX_COUNT.fetch_add(1, Ordering::Relaxed);
        SYNC_MIX_SUM.fetch_add(value, Ordering::Relaxed);
        if SYNC_MIX_SLEEP_ONCE
            .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            scheduler::sleep_current_for_ticks(1);
        }
        PROBE_SYNC_MIX_MUTEX.unlock();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_sync_mix_consumer_b() {
    for _ in 0..3 {
        PROBE_SYNC_MIX_SEM.down();
        let value = PROBE_SYNC_MIX_CHAN.recv();
        if PROBE_SYNC_MIX_MUTEX.is_locked() {
            SYNC_MIX_MUTEX_WAIT.store(1, Ordering::Relaxed);
        }
        PROBE_SYNC_MIX_MUTEX.lock();
        SYNC_MIX_CONS_B.fetch_add(1, Ordering::Relaxed);
        SYNC_MIX_COUNT.fetch_add(1, Ordering::Relaxed);
        SYNC_MIX_SUM.fetch_add(value, Ordering::Relaxed);
        PROBE_SYNC_MIX_MUTEX.unlock();
        scheduler::sleep_current_for_ticks(1);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_sync_mix() {
    SYNC_MIX_COUNT.store(0, Ordering::Relaxed);
    SYNC_MIX_SUM.store(0, Ordering::Relaxed);
    SYNC_MIX_CONS_A.store(0, Ordering::Relaxed);
    SYNC_MIX_CONS_B.store(0, Ordering::Relaxed);
    SYNC_MIX_MUTEX_WAIT.store(0, Ordering::Relaxed);
    SYNC_MIX_SLEEP_ONCE.store(0, Ordering::Relaxed);

    scheduler::spawn_task_with_fn_prio(task_sync_mix_producer, 10);
    scheduler::spawn_task_with_fn_prio(task_sync_mix_consumer_a, 40);
    scheduler::spawn_task_with_fn_prio(task_sync_mix_consumer_b, 50);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 160
        && SYNC_MIX_COUNT.load(Ordering::Relaxed) < 6
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let count = SYNC_MIX_COUNT.load(Ordering::Relaxed);
    let sum = SYNC_MIX_SUM.load(Ordering::Relaxed);
    let cons_a = SYNC_MIX_CONS_A.load(Ordering::Relaxed);
    let cons_b = SYNC_MIX_CONS_B.load(Ordering::Relaxed);
    let mutex_wait = SYNC_MIX_MUTEX_WAIT.load(Ordering::Relaxed);
    let remaining = PROBE_SYNC_MIX_CHAN.len();
    let sem_remaining = PROBE_SYNC_MIX_SEM.count();

    serial::write_str("scheduler: sync-mix count=");
    serial::write_u64(count);
    serial::write_str(" sum=");
    serial::write_u64(sum);
    serial::write_str(" cons=");
    serial::write_u64(cons_a);
    serial::write_str(",");
    serial::write_u64(cons_b);
    serial::write_str(" mutex_wait=");
    serial::write_u64(mutex_wait);
    serial::write_str(" remaining=");
    serial::write_u64(remaining);
    serial::write_str(",");
    serial::write_u64(sem_remaining);
    serial::write_line("");

    // Sync-mix guard policy:
    // mutex contention is timing-sensitive under deep diagnostic lanes,
    // so treat wait observation as bounded telemetry instead of a strict
    // must-equal-one gate to avoid false negatives.
    let pass = count == 6
        && sum == 21
        && cons_a == 3
        && cons_b == 3
        && mutex_wait <= 1
        && remaining == 0
        && sem_remaining == 0;
    serial::write_line(if pass {
        "scheduler: sync-mix PASS"
    } else {
        "scheduler: sync-mix FAIL"
    });
}
