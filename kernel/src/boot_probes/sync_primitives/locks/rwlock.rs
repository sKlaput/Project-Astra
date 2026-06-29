use super::*;

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
