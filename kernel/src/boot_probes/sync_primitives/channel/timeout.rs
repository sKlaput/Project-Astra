use super::*;

static CHAN_TIMEOUT_RECV_TIMEDOUT: AtomicU64 = AtomicU64::new(0);
static CHAN_TIMEOUT_RECV_VALUE: AtomicU64 = AtomicU64::new(0);
static CHAN_TIMEOUT_SEND_TIMEDOUT: AtomicU64 = AtomicU64::new(0);
static CHAN_TIMEOUT_SEND_OK: AtomicU64 = AtomicU64::new(0);
static CHAN_TIMEOUT_DRAINED: AtomicU64 = AtomicU64::new(0);
static PROBE_CHAN_TIMEOUT: sync::KChannel = sync::KChannel::new();

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
