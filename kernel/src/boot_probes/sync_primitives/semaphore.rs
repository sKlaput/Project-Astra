use super::*;

// --- semaphore probe support ---
// Producer calls up() N times; consumer calls down() N times.
// We verify that the consumer ran exactly N times and the semaphore ends at 0.
static SEM_CONSUME_COUNT: AtomicU64 = AtomicU64::new(0);
static PROBE_SEM: sync::KSemaphore = sync::KSemaphore::new(0);

fn task_sem_producer() {
    // Signal 4 items; the outer dispatch_once loop interleaves with consumer.
    for _ in 0..4u64 {
        PROBE_SEM.up();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_sem_consumer() {
    // Consume 4 items; each down() blocks until the producer signals.
    for _ in 0..4u64 {
        PROBE_SEM.down();
        SEM_CONSUME_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_semaphore() {
    SEM_CONSUME_COUNT.store(0, Ordering::Relaxed);

    scheduler::spawn_task_with_fn(task_sem_consumer); // consumer spawned first
    scheduler::spawn_task_with_fn(task_sem_producer); // producer spawned second

    // Run until both tasks have exited.  The ring holds 8 tasks;
    // keep dispatching until nothing is left.
    while scheduler::dispatch_once() {}

    while scheduler::dequeue_next().is_some() {}

    let consumed = SEM_CONSUME_COUNT.load(Ordering::Relaxed);
    let remaining = PROBE_SEM.count();

    serial::write_str("scheduler: semaphore consumed=");
    serial::write_u64(consumed);
    serial::write_str(" remaining=");
    serial::write_u64(remaining);
    serial::write_line("");

    let pass = consumed == 4 && remaining == 0;
    serial::write_line(if pass {
        "scheduler: semaphore PASS"
    } else {
        "scheduler: semaphore FAIL"
    });
}

// --- semaphore deadline-poll probe support ---
static SEM_TO_DOWN: AtomicU64 = AtomicU64::new(0);
static SEM_OK_DOWN: AtomicU64 = AtomicU64::new(0);
static SEM_RELEASER_DONE: AtomicU64 = AtomicU64::new(0);
static PROBE_SEM_TIMEOUT: sync::KSemaphore = sync::KSemaphore::new(0);

fn task_sem_timeout_waiter_short() {
    // Phase A: down on empty semaphore with a short deadline → should time out.
    let deadline = scheduler::ticks().saturating_add(4);
    let ok = PROBE_SEM_TIMEOUT.down_by_deadline_poll(deadline);
    if !ok {
        SEM_TO_DOWN.store(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_sem_timeout_releaser() {
    // Phase B releaser: sleep briefly then signal the semaphore.
    scheduler::sleep_current_for_ticks(3);
    PROBE_SEM_TIMEOUT.up();
    SEM_RELEASER_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_sem_timeout_waiter_long() {
    // Phase B waiter: down with generous deadline → should succeed after releaser fires.
    let deadline = scheduler::ticks().saturating_add(20);
    let ok = PROBE_SEM_TIMEOUT.down_by_deadline_poll(deadline);
    if ok {
        SEM_OK_DOWN.store(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_semaphore_timeout() {
    SEM_TO_DOWN.store(0, Ordering::Relaxed);
    SEM_OK_DOWN.store(0, Ordering::Relaxed);
    SEM_RELEASER_DONE.store(0, Ordering::Relaxed);

    // Phase A: down on empty semaphore with short deadline → timeout.
    scheduler::spawn_task_with_fn_prio(task_sem_timeout_waiter_short, 10);
    let start_a = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_a) < 20
        && SEM_TO_DOWN.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    // Phase B: releaser sleeps then calls up(); waiter uses generous deadline → success.
    scheduler::spawn_task_with_fn_prio(task_sem_timeout_releaser, 20);
    scheduler::spawn_task_with_fn_prio(task_sem_timeout_waiter_long, 30);
    let start_b = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_b) < 80
        && (SEM_OK_DOWN.load(Ordering::Relaxed) == 0
            || SEM_RELEASER_DONE.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}

    let to_down = SEM_TO_DOWN.load(Ordering::Relaxed);
    let ok_down = SEM_OK_DOWN.load(Ordering::Relaxed);
    let rel_done = SEM_RELEASER_DONE.load(Ordering::Relaxed);
    let remaining = PROBE_SEM_TIMEOUT.count();

    serial::write_str("scheduler: sem-deadline-poll to_down=");
    serial::write_u64(to_down);
    serial::write_str(" ok_down=");
    serial::write_u64(ok_down);
    serial::write_str(" rel_done=");
    serial::write_u64(rel_done);
    serial::write_str(" remaining=");
    serial::write_u64(remaining);
    serial::write_line("");

    let pass = to_down == 1 && ok_down == 1 && rel_done == 1;
    serial::write_line(if pass {
        "scheduler: sem-deadline-poll PASS"
    } else {
        "scheduler: sem-deadline-poll FAIL"
    });
}
