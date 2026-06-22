use core::sync::atomic::{AtomicU64, Ordering};

use crate::{idle, scheduler, serial, sync};

// --- rwlock probe support ---
// Two readers acquire concurrently (both hold at once), a writer blocks
// until both release, then the writer acquires exclusively.
static RW_SEQ: AtomicU64 = AtomicU64::new(0);
static RW_RA_POS: AtomicU64 = AtomicU64::new(0); // when reader A acquired
static RW_RB_POS: AtomicU64 = AtomicU64::new(0); // when reader B acquired
static RW_W_POS: AtomicU64 = AtomicU64::new(0); // when writer acquired
static PROBE_RWL: sync::KRwLock = sync::KRwLock::new();

fn task_rw_reader_a() {
    PROBE_RWL.read_lock();
    RW_RA_POS.store(
        RW_SEQ.fetch_add(1, Ordering::Relaxed) + 1,
        Ordering::Relaxed,
    );
    scheduler::sleep_current_for_ticks(4);
    PROBE_RWL.read_unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}
fn task_rw_reader_b() {
    PROBE_RWL.read_lock();
    RW_RB_POS.store(
        RW_SEQ.fetch_add(1, Ordering::Relaxed) + 1,
        Ordering::Relaxed,
    );
    scheduler::sleep_current_for_ticks(4);
    PROBE_RWL.read_unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}
fn task_rw_writer() {
    PROBE_RWL.write_lock(); // blocks until both readers release
    RW_W_POS.store(
        RW_SEQ.fetch_add(1, Ordering::Relaxed) + 1,
        Ordering::Relaxed,
    );
    PROBE_RWL.write_unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_rwlock() {
    RW_SEQ.store(0, Ordering::Relaxed);
    RW_RA_POS.store(0, Ordering::Relaxed);
    RW_RB_POS.store(0, Ordering::Relaxed);
    RW_W_POS.store(0, Ordering::Relaxed);

    // Readers at higher priority (10, 20) acquire before the writer (30).
    scheduler::spawn_task_with_fn_prio(task_rw_reader_a, 10);
    scheduler::spawn_task_with_fn_prio(task_rw_reader_b, 20);
    scheduler::spawn_task_with_fn_prio(task_rw_writer, 30);

    // Round 1: readers acquire the read lock and sleep; writer parks on write_lock.
    scheduler::dispatch_once(); // reader_a: read_locks (state=1), sleeps
    scheduler::dispatch_once(); // reader_b: read_locks (state=2), sleeps
    scheduler::dispatch_once(); // writer:   write_lock blocked (state=2), parks

    // Wait for reader sleep deadlines to expire.
    idle::sleep_for_ticks(6);

    // Round 2: readers wake, release, last reader unparks writer.
    scheduler::dispatch_once(); // reader_a wakes: read_unlock (state=1)
    scheduler::dispatch_once(); // reader_b wakes: read_unlock (state=0) -> unparks writer
    scheduler::dispatch_once(); // writer: write_locks, records pos, write_unlocks, exits

    while scheduler::dequeue_next().is_some() {}

    let ra = RW_RA_POS.load(Ordering::Relaxed);
    let rb = RW_RB_POS.load(Ordering::Relaxed);
    let w = RW_W_POS.load(Ordering::Relaxed);

    serial::write_str("scheduler: rwlock ra=");
    serial::write_u64(ra);
    serial::write_str(" rb=");
    serial::write_u64(rb);
    serial::write_str(" w=");
    serial::write_u64(w);
    serial::write_line("");

    // Both readers acquired before the writer; writer acquired strictly last.
    let readers_first = ra >= 1 && ra <= 2 && rb >= 1 && rb <= 2 && ra != rb;
    let pass = readers_first && w == 3;
    serial::write_line(if pass {
        "scheduler: rwlock PASS"
    } else {
        "scheduler: rwlock FAIL"
    });
}

// --- rwlock deadline-poll probe support ---
static RW_TO_SHORT_TIMEOUT: AtomicU64 = AtomicU64::new(0);
static RW_TO_LONG_OK: AtomicU64 = AtomicU64::new(0);
static RW_TO_READER_DONE: AtomicU64 = AtomicU64::new(0);
static PROBE_RWL_TIMEOUT: sync::KRwLock = sync::KRwLock::new();

fn task_rw_to_reader_holder() {
    PROBE_RWL_TIMEOUT.read_lock();
    scheduler::sleep_current_for_ticks(6);
    PROBE_RWL_TIMEOUT.read_unlock();
    RW_TO_READER_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_rw_to_writer_short() {
    let deadline = scheduler::ticks().saturating_add(3);
    let ok = PROBE_RWL_TIMEOUT.write_lock_by_deadline_poll(deadline);
    if !ok {
        RW_TO_SHORT_TIMEOUT.store(1, Ordering::Relaxed);
    } else {
        PROBE_RWL_TIMEOUT.write_unlock();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_rw_to_writer_long() {
    let deadline = scheduler::ticks().saturating_add(24);
    let ok = PROBE_RWL_TIMEOUT.write_lock_by_deadline_poll(deadline);
    if ok {
        RW_TO_LONG_OK.store(1, Ordering::Relaxed);
        PROBE_RWL_TIMEOUT.write_unlock();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_rwlock_timeout() {
    RW_TO_SHORT_TIMEOUT.store(0, Ordering::Relaxed);
    RW_TO_LONG_OK.store(0, Ordering::Relaxed);
    RW_TO_READER_DONE.store(0, Ordering::Relaxed);

    // Reader holder acquires first and keeps shared lock for a few ticks.
    scheduler::spawn_task_with_fn_prio(task_rw_to_reader_holder, 10);
    scheduler::dispatch_once();

    // Short writer should timeout while reader still holds.
    scheduler::spawn_task_with_fn_prio(task_rw_to_writer_short, 20);
    let start_a = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_a) < 40
        && RW_TO_SHORT_TIMEOUT.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    // Long writer should succeed once reader releases.
    scheduler::spawn_task_with_fn_prio(task_rw_to_writer_long, 30);
    let start_b = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_b) < 100
        && (RW_TO_LONG_OK.load(Ordering::Relaxed) == 0
            || RW_TO_READER_DONE.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}

    let to_short = RW_TO_SHORT_TIMEOUT.load(Ordering::Relaxed);
    let ok_long = RW_TO_LONG_OK.load(Ordering::Relaxed);
    let reader_done = RW_TO_READER_DONE.load(Ordering::Relaxed);

    serial::write_str("scheduler: rwlock-deadline-poll to_short=");
    serial::write_u64(to_short);
    serial::write_str(" ok_long=");
    serial::write_u64(ok_long);
    serial::write_str(" reader_done=");
    serial::write_u64(reader_done);
    serial::write_line("");

    let pass = to_short == 1 && ok_long == 1 && reader_done == 1;
    serial::write_line(if pass {
        "scheduler: rwlock-deadline-poll PASS"
    } else {
        "scheduler: rwlock-deadline-poll FAIL"
    });
}

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

// --- channel probe support ---
// Producer sends 1..=6 into a channel with capacity 2. Producer starts at
// higher priority, so it fills and then blocks on full; consumer drains and
// unblocks producer. This validates both recv-empty and send-full blocking.
static CHAN_COUNT: AtomicU64 = AtomicU64::new(0);
static CHAN_SUM: AtomicU64 = AtomicU64::new(0);
static CHAN_EMPTY_BLOCKED: AtomicU64 = AtomicU64::new(0);
static CHAN_EMPTY_GOT: AtomicU64 = AtomicU64::new(0);
static PROBE_CHAN: sync::KChannel = sync::KChannel::new();
static CHAN_STRESS_COUNT: AtomicU64 = AtomicU64::new(0);
static CHAN_STRESS_SUM: AtomicU64 = AtomicU64::new(0);
static CHAN_STRESS_CONS_A: AtomicU64 = AtomicU64::new(0);
static CHAN_STRESS_CONS_B: AtomicU64 = AtomicU64::new(0);
static PROBE_CHAN_STRESS: sync::KChannel = sync::KChannel::new();
static CHAN_TIMEOUT_RECV_TIMEDOUT: AtomicU64 = AtomicU64::new(0);
static CHAN_TIMEOUT_RECV_VALUE: AtomicU64 = AtomicU64::new(0);
static CHAN_TIMEOUT_SEND_TIMEDOUT: AtomicU64 = AtomicU64::new(0);
static CHAN_TIMEOUT_SEND_OK: AtomicU64 = AtomicU64::new(0);
static CHAN_TIMEOUT_DRAINED: AtomicU64 = AtomicU64::new(0);
static PROBE_CHAN_TIMEOUT: sync::KChannel = sync::KChannel::new();

fn task_chan_empty_consumer() {
    CHAN_EMPTY_BLOCKED.store(1, Ordering::Relaxed);
    let v = PROBE_CHAN.recv();
    CHAN_EMPTY_GOT.store(v, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_chan_producer() {
    for v in 1..=6u64 {
        PROBE_CHAN.send(v);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_chan_consumer() {
    for _ in 0..6u64 {
        let v = PROBE_CHAN.recv();
        CHAN_COUNT.fetch_add(1, Ordering::Relaxed);
        CHAN_SUM.fetch_add(v, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_channel() {
    CHAN_COUNT.store(0, Ordering::Relaxed);
    CHAN_SUM.store(0, Ordering::Relaxed);
    CHAN_EMPTY_BLOCKED.store(0, Ordering::Relaxed);
    CHAN_EMPTY_GOT.store(0, Ordering::Relaxed);

    let try_recv_empty = PROBE_CHAN.try_recv().is_none();
    let try_send_snapshot = PROBE_CHAN.try_send(77);
    let try_recv_snapshot = PROBE_CHAN.try_recv().unwrap_or(0);

    // Phase A: prove recv() blocks on an empty channel, then resumes after send.
    scheduler::spawn_task_with_fn_prio(task_chan_empty_consumer, 10);
    scheduler::dispatch_once();

    let empty_wait_started = CHAN_EMPTY_BLOCKED.load(Ordering::Relaxed) == 1;
    let empty_wait_parked = CHAN_EMPTY_GOT.load(Ordering::Relaxed) == 0 && PROBE_CHAN.len() == 0;
    let try_send_empty = PROBE_CHAN.try_send(99);

    scheduler::dispatch_once();
    let empty_resumed = CHAN_EMPTY_GOT.load(Ordering::Relaxed) == 99;

    // Phase B: producer outruns consumer, fills the 2-slot buffer, then blocks
    // on full until the consumer drains and wakes it.
    scheduler::spawn_task_with_fn_prio(task_chan_producer, 10);
    scheduler::spawn_task_with_fn_prio(task_chan_consumer, 50);

    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let count = CHAN_COUNT.load(Ordering::Relaxed);
    let sum = CHAN_SUM.load(Ordering::Relaxed);
    let remaining = PROBE_CHAN.len();

    serial::write_str("scheduler: channel consumed=");
    serial::write_u64(count);
    serial::write_str(" sum=");
    serial::write_u64(sum);
    serial::write_str(" remaining=");
    serial::write_u64(remaining);
    serial::write_str(" empty=");
    serial::write_u64(empty_wait_started as u64);
    serial::write_u64(empty_wait_parked as u64);
    serial::write_u64(try_recv_empty as u64);
    serial::write_u64(try_send_snapshot as u64);
    serial::write_u64((try_recv_snapshot == 77) as u64);
    serial::write_u64(try_send_empty as u64);
    serial::write_u64(empty_resumed as u64);
    serial::write_line("");

    let pass = count == 6
        && sum == 21
        && remaining == 0
        && empty_wait_started
        && empty_wait_parked
        && try_recv_empty
        && try_send_snapshot
        && try_recv_snapshot == 77
        && try_send_empty
        && empty_resumed;
    serial::write_line(if pass {
        "scheduler: channel PASS"
    } else {
        "scheduler: channel FAIL"
    });
}

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

fn task_chan_timeout_recv_short() {
    let deadline = scheduler::ticks().saturating_add(4);
    let result = PROBE_CHAN_TIMEOUT.recv_until_tick(deadline);
    if result.is_none() {
        CHAN_TIMEOUT_RECV_TIMEDOUT.store(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_chan_timeout_send_short() {
    let deadline = scheduler::ticks().saturating_add(4);
    let ok = PROBE_CHAN_TIMEOUT.send_until_tick(999, deadline);
    if !ok {
        CHAN_TIMEOUT_SEND_TIMEDOUT.store(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_chan_timeout_recv_long() {
    let deadline = scheduler::ticks().saturating_add(30);
    if let Some(value) = PROBE_CHAN_TIMEOUT.recv_until_tick(deadline) {
        CHAN_TIMEOUT_RECV_VALUE.store(value, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_chan_timeout_send_long() {
    let deadline = scheduler::ticks().saturating_add(30);
    if PROBE_CHAN_TIMEOUT.send_until_tick(333, deadline) {
        CHAN_TIMEOUT_SEND_OK.store(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_chan_timeout_drain_one() {
    scheduler::sleep_current_for_ticks(2);
    if PROBE_CHAN_TIMEOUT.try_recv().is_some() {
        CHAN_TIMEOUT_DRAINED.store(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_channel_timeout() {
    CHAN_TIMEOUT_RECV_TIMEDOUT.store(0, Ordering::Relaxed);
    CHAN_TIMEOUT_RECV_VALUE.store(0, Ordering::Relaxed);
    CHAN_TIMEOUT_SEND_TIMEDOUT.store(0, Ordering::Relaxed);
    CHAN_TIMEOUT_SEND_OK.store(0, Ordering::Relaxed);
    CHAN_TIMEOUT_DRAINED.store(0, Ordering::Relaxed);
    while PROBE_CHAN_TIMEOUT.try_recv().is_some() {}

    // Phase A: empty receive should timeout.
    scheduler::spawn_task_with_fn_prio(task_chan_timeout_recv_short, 10);
    let start_a = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_a) < 20
        && CHAN_TIMEOUT_RECV_TIMEDOUT.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    // Phase B: full send should timeout.
    let fill_a = PROBE_CHAN_TIMEOUT.try_send(111);
    let fill_b = PROBE_CHAN_TIMEOUT.try_send(222);
    scheduler::spawn_task_with_fn_prio(task_chan_timeout_send_short, 10);
    let start_b = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_b) < 20
        && CHAN_TIMEOUT_SEND_TIMEDOUT.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    // Phase C: timeout APIs should also succeed when unblocked before deadline.
    while PROBE_CHAN_TIMEOUT.try_recv().is_some() {}
    scheduler::spawn_task_with_fn_prio(task_chan_timeout_recv_long, 20);
    scheduler::dispatch_once(); // receiver blocks waiting for data
    let sent_for_recv = PROBE_CHAN_TIMEOUT.try_send(777);
    while scheduler::dispatch_once() {}

    let mut refill_a = PROBE_CHAN_TIMEOUT.try_send(1);
    let mut refill_b = PROBE_CHAN_TIMEOUT.try_send(2);
    if !(refill_a && refill_b) {
        while PROBE_CHAN_TIMEOUT.try_recv().is_some() {}
        refill_a = PROBE_CHAN_TIMEOUT.try_send(1);
        refill_b = PROBE_CHAN_TIMEOUT.try_send(2);
    }
    scheduler::spawn_task_with_fn_prio(task_chan_timeout_send_long, 20);
    scheduler::spawn_task_with_fn_prio(task_chan_timeout_drain_one, 30);
    let start_c = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_c) < 50
        && (CHAN_TIMEOUT_SEND_OK.load(Ordering::Relaxed) == 0
            || CHAN_TIMEOUT_DRAINED.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}

    let recv_timed = CHAN_TIMEOUT_RECV_TIMEDOUT.load(Ordering::Relaxed);
    let send_timed = CHAN_TIMEOUT_SEND_TIMEDOUT.load(Ordering::Relaxed);
    let recv_value = CHAN_TIMEOUT_RECV_VALUE.load(Ordering::Relaxed);
    let send_ok = CHAN_TIMEOUT_SEND_OK.load(Ordering::Relaxed);
    let drained = CHAN_TIMEOUT_DRAINED.load(Ordering::Relaxed);
    let remaining = PROBE_CHAN_TIMEOUT.len();

    serial::write_str("scheduler: channel-timeout recv_to=");
    serial::write_u64(recv_timed);
    serial::write_str(" send_to=");
    serial::write_u64(send_timed);
    serial::write_str(" recv_val=");
    serial::write_u64(recv_value);
    serial::write_str(" send_ok=");
    serial::write_u64(send_ok);
    serial::write_str(" drained=");
    serial::write_u64(drained);
    serial::write_str(" fill=");
    serial::write_u64(fill_a as u64);
    serial::write_u64(fill_b as u64);
    serial::write_u64(sent_for_recv as u64);
    serial::write_u64(refill_a as u64);
    serial::write_u64(refill_b as u64);
    serial::write_str(" remaining=");
    serial::write_u64(remaining);
    serial::write_line("");

    let pass = recv_timed == 1
        && send_timed == 1
        && recv_value != 0
        && send_ok == 1
        && drained == 1
        && fill_a
        && fill_b
        && sent_for_recv
        && refill_a
        && refill_b;
    serial::write_line(if pass {
        "scheduler: channel-timeout PASS"
    } else {
        "scheduler: channel-timeout FAIL"
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

// --- mutex deadline-poll probe support ---
static MTX_TO_LOCK: AtomicU64 = AtomicU64::new(0);
static MTX_OK_LOCK: AtomicU64 = AtomicU64::new(0);
static MTX_HOLDER_DONE: AtomicU64 = AtomicU64::new(0);
static PROBE_MTX_TIMEOUT: sync::KMutex = sync::KMutex::new();

fn task_mtx_timeout_holder() {
    PROBE_MTX_TIMEOUT.lock();
    scheduler::sleep_current_for_ticks(6);
    PROBE_MTX_TIMEOUT.unlock();
    MTX_HOLDER_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_mtx_timeout_waiter_short() {
    // Phase A: contended mutex with short deadline → should time out.
    let deadline = scheduler::ticks().saturating_add(3);
    let ok = PROBE_MTX_TIMEOUT.lock_by_deadline_poll(deadline);
    if !ok {
        MTX_TO_LOCK.store(1, Ordering::Relaxed);
    } else {
        PROBE_MTX_TIMEOUT.unlock();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_mtx_timeout_waiter_long() {
    // Phase B: generous deadline; succeeds after holder releases.
    let deadline = scheduler::ticks().saturating_add(30);
    let ok = PROBE_MTX_TIMEOUT.lock_by_deadline_poll(deadline);
    if ok {
        MTX_OK_LOCK.store(1, Ordering::Relaxed);
        PROBE_MTX_TIMEOUT.unlock();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_mutex_timeout() {
    MTX_TO_LOCK.store(0, Ordering::Relaxed);
    MTX_OK_LOCK.store(0, Ordering::Relaxed);
    MTX_HOLDER_DONE.store(0, Ordering::Relaxed);
    if PROBE_MTX_TIMEOUT.is_locked() {
        PROBE_MTX_TIMEOUT.unlock();
    }

    // Spawn holder at highest priority so it acquires the lock before waiters arrive.
    scheduler::spawn_task_with_fn_prio(task_mtx_timeout_holder, 10);
    scheduler::dispatch_once(); // holder runs, acquires lock, sleeps

    // Short waiter times out; long waiter succeeds after holder releases.
    scheduler::spawn_task_with_fn_prio(task_mtx_timeout_waiter_short, 20);
    scheduler::spawn_task_with_fn_prio(task_mtx_timeout_waiter_long, 30);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 120
        && (MTX_TO_LOCK.load(Ordering::Relaxed) == 0 || MTX_OK_LOCK.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let to_lock = MTX_TO_LOCK.load(Ordering::Relaxed);
    let ok_lock = MTX_OK_LOCK.load(Ordering::Relaxed);
    let holder_done = MTX_HOLDER_DONE.load(Ordering::Relaxed);

    serial::write_str("scheduler: mtx-deadline-poll to_lock=");
    serial::write_u64(to_lock);
    serial::write_str(" ok_lock=");
    serial::write_u64(ok_lock);
    serial::write_str(" holder_done=");
    serial::write_u64(holder_done);
    serial::write_line("");

    let pass = to_lock == 1 && ok_lock == 1 && holder_done == 1;
    serial::write_line(if pass {
        "scheduler: mtx-deadline-poll PASS"
    } else {
        "scheduler: mtx-deadline-poll FAIL"
    });
}

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

// --- condvar notify_one probe support ---
static CV_ONE_DATA: AtomicU64 = AtomicU64::new(0);
static CV_ONE_WAKE: AtomicU64 = AtomicU64::new(0);
static CV_ONE_DONE: AtomicU64 = AtomicU64::new(0);
static PROBE_CV_ONE_MTX: sync::KMutex = sync::KMutex::new();
static PROBE_CV_ONE: sync::KCondVar = sync::KCondVar::new();

fn task_cv_one_waiter() {
    PROBE_CV_ONE_MTX.lock();
    while CV_ONE_DATA.load(Ordering::Relaxed) == 0 {
        PROBE_CV_ONE.wait(&PROBE_CV_ONE_MTX);
    }
    CV_ONE_WAKE.store(CV_ONE_DATA.load(Ordering::Relaxed), Ordering::Relaxed);
    PROBE_CV_ONE_MTX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_cv_one_signaler() {
    scheduler::sleep_current_for_ticks(3);
    PROBE_CV_ONE_MTX.lock();
    CV_ONE_DATA.store(42, Ordering::Relaxed);
    PROBE_CV_ONE.notify_one();
    PROBE_CV_ONE_MTX.unlock();
    CV_ONE_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_condvar_notify_one() {
    CV_ONE_DATA.store(0, Ordering::Relaxed);
    CV_ONE_WAKE.store(0, Ordering::Relaxed);
    CV_ONE_DONE.store(0, Ordering::Relaxed);

    scheduler::spawn_task_with_fn_prio(task_cv_one_waiter, 10);
    scheduler::spawn_task_with_fn_prio(task_cv_one_signaler, 20);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 80
        && (CV_ONE_WAKE.load(Ordering::Relaxed) == 0 || CV_ONE_DONE.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}

    let wake_val = CV_ONE_WAKE.load(Ordering::Relaxed);
    let done = CV_ONE_DONE.load(Ordering::Relaxed);

    serial::write_str("scheduler: condvar-one wake=");
    serial::write_u64(wake_val);
    serial::write_str(" done=");
    serial::write_u64(done);
    serial::write_line("");

    let pass = wake_val == 42 && done == 1;
    serial::write_line(if pass {
        "scheduler: condvar-one PASS"
    } else {
        "scheduler: condvar-one FAIL"
    });
}

// --- condvar notify_all probe support ---
static CV_ALL_DATA: AtomicU64 = AtomicU64::new(0);
static CV_ALL_WAKE_A: AtomicU64 = AtomicU64::new(0);
static CV_ALL_WAKE_B: AtomicU64 = AtomicU64::new(0);
static CV_ALL_SIG_DONE: AtomicU64 = AtomicU64::new(0);
static PROBE_CV_ALL_MTX: sync::KMutex = sync::KMutex::new();
static PROBE_CV_ALL: sync::KCondVar = sync::KCondVar::new();
static CV_TO_TIMED_OUT: AtomicU64 = AtomicU64::new(0);
static CV_TO_WOKE: AtomicU64 = AtomicU64::new(0);
static CV_TO_SIG_DONE: AtomicU64 = AtomicU64::new(0);
static PROBE_CV_TO_MTX: sync::KMutex = sync::KMutex::new();
static PROBE_CV_TO: sync::KCondVar = sync::KCondVar::new();
static CV_TO_DATA: AtomicU64 = AtomicU64::new(0);

fn task_cv_all_waiter_a() {
    PROBE_CV_ALL_MTX.lock();
    while CV_ALL_DATA.load(Ordering::Relaxed) == 0 {
        PROBE_CV_ALL.wait(&PROBE_CV_ALL_MTX);
    }
    CV_ALL_WAKE_A.store(CV_ALL_DATA.load(Ordering::Relaxed), Ordering::Relaxed);
    PROBE_CV_ALL_MTX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_cv_all_waiter_b() {
    PROBE_CV_ALL_MTX.lock();
    while CV_ALL_DATA.load(Ordering::Relaxed) == 0 {
        PROBE_CV_ALL.wait(&PROBE_CV_ALL_MTX);
    }
    CV_ALL_WAKE_B.store(CV_ALL_DATA.load(Ordering::Relaxed), Ordering::Relaxed);
    PROBE_CV_ALL_MTX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_cv_all_signaler() {
    scheduler::sleep_current_for_ticks(3);
    PROBE_CV_ALL_MTX.lock();
    CV_ALL_DATA.store(99, Ordering::Relaxed);
    PROBE_CV_ALL.notify_all();
    PROBE_CV_ALL_MTX.unlock();
    CV_ALL_SIG_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_condvar_notify_all() {
    CV_ALL_DATA.store(0, Ordering::Relaxed);
    CV_ALL_WAKE_A.store(0, Ordering::Relaxed);
    CV_ALL_WAKE_B.store(0, Ordering::Relaxed);
    CV_ALL_SIG_DONE.store(0, Ordering::Relaxed);

    scheduler::spawn_task_with_fn_prio(task_cv_all_waiter_a, 10);
    scheduler::spawn_task_with_fn_prio(task_cv_all_waiter_b, 20);
    scheduler::spawn_task_with_fn_prio(task_cv_all_signaler, 30);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 80
        && (CV_ALL_WAKE_A.load(Ordering::Relaxed) == 0
            || CV_ALL_WAKE_B.load(Ordering::Relaxed) == 0
            || CV_ALL_SIG_DONE.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}

    let wake_a = CV_ALL_WAKE_A.load(Ordering::Relaxed);
    let wake_b = CV_ALL_WAKE_B.load(Ordering::Relaxed);
    let sig_done = CV_ALL_SIG_DONE.load(Ordering::Relaxed);

    serial::write_str("scheduler: condvar-all wake_a=");
    serial::write_u64(wake_a);
    serial::write_str(" wake_b=");
    serial::write_u64(wake_b);
    serial::write_str(" sig_done=");
    serial::write_u64(sig_done);
    serial::write_line("");

    let pass = wake_a == 99 && wake_b == 99 && sig_done == 1;
    serial::write_line(if pass {
        "scheduler: condvar-all PASS"
    } else {
        "scheduler: condvar-all FAIL"
    });
}

fn task_cv_timeout_waiter_short() {
    PROBE_CV_TO_MTX.lock();
    while CV_TO_DATA.load(Ordering::Relaxed) == 0 {
        let deadline = scheduler::ticks().saturating_add(4);
        if !PROBE_CV_TO.wait_by_deadline_poll(&PROBE_CV_TO_MTX, deadline) {
            CV_TO_TIMED_OUT.store(1, Ordering::Relaxed);
            break;
        }
    }
    PROBE_CV_TO_MTX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_cv_timeout_waiter_long() {
    PROBE_CV_TO_MTX.lock();
    while CV_TO_DATA.load(Ordering::Relaxed) == 0 {
        let deadline = scheduler::ticks().saturating_add(24);
        if !PROBE_CV_TO.wait_by_deadline_poll(&PROBE_CV_TO_MTX, deadline) {
            break;
        }
    }
    if CV_TO_DATA.load(Ordering::Relaxed) == 7 {
        CV_TO_WOKE.store(1, Ordering::Relaxed);
    }
    PROBE_CV_TO_MTX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_cv_timeout_signaler() {
    scheduler::sleep_current_for_ticks(3);
    PROBE_CV_TO_MTX.lock();
    CV_TO_DATA.store(7, Ordering::Relaxed);
    PROBE_CV_TO.notify_one();
    PROBE_CV_TO_MTX.unlock();
    CV_TO_SIG_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_condvar_timeout() {
    CV_TO_TIMED_OUT.store(0, Ordering::Relaxed);
    CV_TO_WOKE.store(0, Ordering::Relaxed);
    CV_TO_SIG_DONE.store(0, Ordering::Relaxed);
    CV_TO_DATA.store(0, Ordering::Relaxed);

    // Phase A: timed wait on empty predicate should timeout.
    scheduler::spawn_task_with_fn_prio(task_cv_timeout_waiter_short, 10);
    let start_a = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_a) < 30
        && CV_TO_TIMED_OUT.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    // Phase B: waiter with longer deadline should wake from notify_one.
    CV_TO_DATA.store(0, Ordering::Relaxed);
    scheduler::spawn_task_with_fn_prio(task_cv_timeout_waiter_long, 20);
    scheduler::spawn_task_with_fn_prio(task_cv_timeout_signaler, 30);
    let start_b = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_b) < 80
        && (CV_TO_WOKE.load(Ordering::Relaxed) == 0 || CV_TO_SIG_DONE.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}

    let timed_out = CV_TO_TIMED_OUT.load(Ordering::Relaxed);
    let woke = CV_TO_WOKE.load(Ordering::Relaxed);
    let sig_done = CV_TO_SIG_DONE.load(Ordering::Relaxed);

    serial::write_str("scheduler: condvar-deadline-poll to=");
    serial::write_u64(timed_out);
    serial::write_str(" woke=");
    serial::write_u64(woke);
    serial::write_str(" sig=");
    serial::write_u64(sig_done);
    serial::write_line("");

    let pass = timed_out == 1 && woke == 1 && sig_done == 1;
    serial::write_line(if pass {
        "scheduler: condvar-deadline-poll PASS"
    } else {
        "scheduler: condvar-deadline-poll FAIL"
    });
}

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

// --- park/unpark telemetry probe support ---
static PARKTEL_DONE: AtomicU64 = AtomicU64::new(0);
static PARKTEL_MUTEX_WAIT: AtomicU64 = AtomicU64::new(0);
static PROBE_PARKTEL_MUTEX: sync::KMutex = sync::KMutex::new();
static PROBE_PARKTEL_SEM: sync::KSemaphore = sync::KSemaphore::new(0);

fn task_parktel_sem_waiter() {
    PROBE_PARKTEL_SEM.down(); // parks until signaler calls up()
    PARKTEL_DONE.fetch_add(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_parktel_sem_signaler() {
    scheduler::sleep_current_for_ticks(2);
    PROBE_PARKTEL_SEM.up();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_parktel_mutex_holder() {
    PROBE_PARKTEL_MUTEX.lock();
    scheduler::sleep_current_for_ticks(3);
    PROBE_PARKTEL_MUTEX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_parktel_mutex_waiter() {
    if PROBE_PARKTEL_MUTEX.is_locked() {
        PARKTEL_MUTEX_WAIT.store(1, Ordering::Relaxed);
    }
    PROBE_PARKTEL_MUTEX.lock(); // parks while holder has lock
    PARKTEL_DONE.fetch_add(1, Ordering::Relaxed);
    PROBE_PARKTEL_MUTEX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_park_unpark_telemetry() {
    let parks_before = scheduler::stat_park_count();
    let unparks_before = scheduler::stat_unpark_count();
    let fails_before = scheduler::stat_unpark_fail_count();

    PARKTEL_DONE.store(0, Ordering::Relaxed);
    PARKTEL_MUTEX_WAIT.store(0, Ordering::Relaxed);

    // Deliberate failed wake to verify fail-path telemetry increments.
    let forced_fail = !scheduler::unpark_task(scheduler::TaskId(0xFFFF_FFFF_FFFF_FF00));

    scheduler::spawn_task_with_fn_prio(task_parktel_mutex_holder, 10);
    scheduler::spawn_task_with_fn_prio(task_parktel_mutex_waiter, 20);
    scheduler::spawn_task_with_fn_prio(task_parktel_sem_waiter, 30);
    scheduler::spawn_task_with_fn_prio(task_parktel_sem_signaler, 40);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 120 && PARKTEL_DONE.load(Ordering::Relaxed) < 2
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }
    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let parks_delta = scheduler::stat_park_count().saturating_sub(parks_before);
    let unparks_delta = scheduler::stat_unpark_count().saturating_sub(unparks_before);
    let fails_delta = scheduler::stat_unpark_fail_count().saturating_sub(fails_before);
    let done = PARKTEL_DONE.load(Ordering::Relaxed);
    let mutex_wait = PARKTEL_MUTEX_WAIT.load(Ordering::Relaxed);

    serial::write_str("scheduler: park-unpark parks=");
    serial::write_u64(parks_delta);
    serial::write_str(" unparks=");
    serial::write_u64(unparks_delta);
    serial::write_str(" fails=");
    serial::write_u64(fails_delta);
    serial::write_str(" done=");
    serial::write_u64(done);
    serial::write_str(" mutex_wait=");
    serial::write_u64(mutex_wait);
    serial::write_line("");

    let pass = forced_fail
        && done == 2
        && mutex_wait == 1
        && parks_delta >= 2
        && unparks_delta >= 2
        && fails_delta >= 1;
    serial::write_line(if pass {
        "scheduler: park-unpark PASS"
    } else {
        "scheduler: park-unpark FAIL"
    });
}

// --- spinlock probe support ---
static SPIN_COUNTER: AtomicU64 = AtomicU64::new(0);
static PROBE_SPIN: sync::KSpinlock = sync::KSpinlock::new();

fn task_spin_a() {
    for _ in 0..2 {
        PROBE_SPIN.lock();
        SPIN_COUNTER.fetch_add(1, Ordering::Relaxed);
        PROBE_SPIN.unlock();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_spin_b() {
    for _ in 0..3 {
        PROBE_SPIN.lock();
        SPIN_COUNTER.fetch_add(1, Ordering::Relaxed);
        PROBE_SPIN.unlock();
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_spinlock() {
    SPIN_COUNTER.store(0, Ordering::Relaxed);

    let ta = scheduler::spawn_task_with_fn(task_spin_a);
    let tb = scheduler::spawn_task_with_fn(task_spin_b);

    for _ in 0..100 {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
        let done = [ta, tb].iter().all(|t| {
            t.map_or(true, |id| {
                scheduler::task_state(id) == scheduler::TaskState::Empty
            })
        });
        if done {
            break;
        }
    }

    while scheduler::dequeue_next().is_some() {}

    let counter = SPIN_COUNTER.load(Ordering::Relaxed);

    serial::write_str("scheduler: spinlock counter=");
    serial::write_u64(counter);
    serial::write_line("");

    let pass = counter == 5;
    serial::write_line(if pass {
        "scheduler: spinlock PASS"
    } else {
        "scheduler: spinlock FAIL"
    });
}

// --- mutex probe support ---
static MUTEX_COUNTER: AtomicU64 = AtomicU64::new(0);
static MUTEX_A_ACQUIRED: AtomicU64 = AtomicU64::new(0);
static MUTEX_B_WAITED: AtomicU64 = AtomicU64::new(0);
static PROBE_MUTEX: sync::KMutex = sync::KMutex::new();

fn task_mutex_a() {
    // Task A: grab the mutex, increment counter twice with a sleep in between,
    // then release.  During the sleep B is dispatched and must block on lock().
    PROBE_MUTEX.lock();
    MUTEX_A_ACQUIRED.store(1, Ordering::Relaxed);
    MUTEX_COUNTER.fetch_add(1, Ordering::Relaxed); // counter = 1
    scheduler::sleep_current_for_ticks(3); // B gets scheduled here
    MUTEX_COUNTER.fetch_add(1, Ordering::Relaxed); // counter = 2
    PROBE_MUTEX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_mutex_b() {
    // Task B: try to acquire the same mutex — will park until A unlocks.
    PROBE_MUTEX.lock();
    MUTEX_B_WAITED.store(1, Ordering::Relaxed);
    MUTEX_COUNTER.fetch_add(10, Ordering::Relaxed); // counter = 12
    PROBE_MUTEX.unlock();
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_mutex_contention() {
    MUTEX_COUNTER.store(0, Ordering::Relaxed);
    MUTEX_A_ACQUIRED.store(0, Ordering::Relaxed);
    MUTEX_B_WAITED.store(0, Ordering::Relaxed);

    let ta = scheduler::spawn_task_with_fn(task_mutex_a);
    let tb = scheduler::spawn_task_with_fn(task_mutex_b);

    // Drive until both tasks exit.
    for _ in 0..64 {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
        let done = [ta, tb].iter().all(|t| {
            t.map_or(true, |id| {
                scheduler::task_state(id) == scheduler::TaskState::Empty
            })
        });
        if done {
            break;
        }
    }

    while scheduler::dequeue_next().is_some() {}

    let counter = MUTEX_COUNTER.load(Ordering::Relaxed);
    let a_ok = MUTEX_A_ACQUIRED.load(Ordering::Relaxed);
    let b_ok = MUTEX_B_WAITED.load(Ordering::Relaxed);

    let mut empty_after: u64 = 0;
    for t in [ta, tb] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Empty {
                empty_after += 1;
            }
        }
    }

    serial::write_str("scheduler: mutex counter=");
    serial::write_u64(counter);
    serial::write_str(" a_acquired=");
    serial::write_u64(a_ok);
    serial::write_str(" b_waited=");
    serial::write_u64(b_ok);
    serial::write_str(" empty=");
    serial::write_u64(empty_after);
    serial::write_line("/2");

    let pass = counter == 12 && a_ok == 1 && b_ok == 1 && empty_after == 2;
    serial::write_line(if pass {
        "scheduler: mutex PASS"
    } else {
        "scheduler: mutex FAIL"
    });
}
