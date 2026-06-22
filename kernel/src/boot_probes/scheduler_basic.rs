use core::sync::atomic::{AtomicU64, Ordering};

use crate::{arch, idle, scheduler, serial};

pub(crate) fn probe_timer_interrupts() {
    let before_ms = arch::x86_64::interrupts::uptime_ms();
    let before = arch::x86_64::interrupts::timer_ticks();

    for _ in 0..2_000_000 {
        core::hint::spin_loop();
    }

    let after = arch::x86_64::interrupts::timer_ticks();
    let delta = after.saturating_sub(before);

    serial::write_str("interrupts: timer tick delta=");
    serial::write_u64(delta);
    serial::write_line("");

    let after_ms = arch::x86_64::interrupts::uptime_ms();
    serial::write_str("interrupts: uptime-ms before=");
    serial::write_u64(before_ms);
    serial::write_str(" after=");
    serial::write_u64(after_ms);
    serial::write_line("");
}

pub(crate) fn probe_sleep_ticks() {
    let hz = idle::hz() as u64;
    let duration_ticks = (hz * 120) / 1000;
    let before_ticks = idle::now_ticks();
    idle::sleep_for_ticks(duration_ticks);
    let after_ticks = idle::now_ticks();

    serial::write_str("interrupts: sleep-ticks before=");
    serial::write_u64(before_ticks);
    serial::write_str(" after=");
    serial::write_u64(after_ticks);
    serial::write_str(" delta=");
    serial::write_u64(after_ticks.saturating_sub(before_ticks));
    serial::write_line("");
}

pub(crate) fn probe_scheduler_ticks() {
    let before = scheduler::ticks();

    for _ in 0..2_000_000 {
        core::hint::spin_loop();
    }

    let after = scheduler::ticks();
    let delta = after.saturating_sub(before);

    serial::write_str("scheduler: tick delta=");
    serial::write_u64(delta);
    serial::write_line("");
}

pub(crate) fn probe_scheduler_idle_decision() {
    if scheduler::take_idle_decision_event() {
        serial::write_line("scheduler: no runnable tasks, idling");
    }
}

pub(crate) fn probe_scheduler_queue_api() {
    let spawned_a = scheduler::spawn_task();
    let spawned_b = scheduler::spawn_task();
    let spawned_c = scheduler::spawn_task();
    let popped_a = scheduler::dequeue_next();
    let popped_b = scheduler::dequeue_next();
    let popped_c = scheduler::dequeue_next();

    serial::write_str("scheduler: queue-api spawned=");
    serial::write_u64(spawned_a.map(|t| t.0).unwrap_or(u64::MAX));
    serial::write_str(",");
    serial::write_u64(spawned_b.map(|t| t.0).unwrap_or(u64::MAX));
    serial::write_str(",");
    serial::write_u64(spawned_c.map(|t| t.0).unwrap_or(u64::MAX));
    serial::write_str(" popped=");
    serial::write_u64(popped_a.map(|t| t.0).unwrap_or(u64::MAX));
    serial::write_str(",");
    serial::write_u64(popped_b.map(|t| t.0).unwrap_or(u64::MAX));
    serial::write_str(",");
    serial::write_u64(popped_c.map(|t| t.0).unwrap_or(u64::MAX));
    serial::write_str(" runnable=");
    serial::write_u64(scheduler::runnable_count() as u64);
    serial::write_line("");
}

// --- priority probe support ---
static PRIO_SEQ: AtomicU64 = AtomicU64::new(0);
static PRIO_ORDER_HIGH: AtomicU64 = AtomicU64::new(0);
static PRIO_ORDER_MID: AtomicU64 = AtomicU64::new(0);
static PRIO_ORDER_LOW: AtomicU64 = AtomicU64::new(0);

fn task_prio_high() {
    let pos = PRIO_SEQ.fetch_add(1, Ordering::Relaxed).saturating_add(1);
    PRIO_ORDER_HIGH.store(pos, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}
fn task_prio_mid() {
    let pos = PRIO_SEQ.fetch_add(1, Ordering::Relaxed).saturating_add(1);
    PRIO_ORDER_MID.store(pos, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}
fn task_prio_low() {
    let pos = PRIO_SEQ.fetch_add(1, Ordering::Relaxed).saturating_add(1);
    PRIO_ORDER_LOW.store(pos, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_priority_order() {
    PRIO_SEQ.store(0, Ordering::Relaxed);
    PRIO_ORDER_HIGH.store(0, Ordering::Relaxed);
    PRIO_ORDER_MID.store(0, Ordering::Relaxed);
    PRIO_ORDER_LOW.store(0, Ordering::Relaxed);

    // Spawn in low→mid→high order; scheduler should still run high first.
    scheduler::spawn_task_with_fn_prio(task_prio_low, 200);
    scheduler::spawn_task_with_fn_prio(task_prio_mid, 128);
    scheduler::spawn_task_with_fn_prio(task_prio_high, 10);

    scheduler::dispatch_once(); // highest priority (10) runs
    scheduler::dispatch_once(); // next (128) runs
    scheduler::dispatch_once(); // last (200) runs

    while scheduler::dequeue_next().is_some() {}

    let h = PRIO_ORDER_HIGH.load(Ordering::Relaxed);
    let m = PRIO_ORDER_MID.load(Ordering::Relaxed);
    let l = PRIO_ORDER_LOW.load(Ordering::Relaxed);

    serial::write_str("scheduler: priority order high=");
    serial::write_u64(h);
    serial::write_str(" mid=");
    serial::write_u64(m);
    serial::write_str(" low=");
    serial::write_u64(l);
    serial::write_line("");

    let pass = h == 1 && m == 2 && l == 3;
    serial::write_line(if pass {
        "scheduler: priority PASS"
    } else {
        "scheduler: priority FAIL"
    });
}

pub(crate) fn probe_priority_slices() {
    // Temporarily set distinct per-class quanta and verify mapping.
    scheduler::configure_slice_classes(2, 5, 9);

    let high = scheduler::debug_slice_for_priority(10);
    let normal = scheduler::debug_slice_for_priority(128);
    let low = scheduler::debug_slice_for_priority(220);

    serial::write_str("scheduler: priority-slices high=");
    serial::write_u64(high as u64);
    serial::write_str(" normal=");
    serial::write_u64(normal as u64);
    serial::write_str(" low=");
    serial::write_u64(low as u64);
    serial::write_line("");

    let pass = high == 2 && normal == 5 && low == 9;
    serial::write_line(if pass {
        "scheduler: priority-slices PASS"
    } else {
        "scheduler: priority-slices FAIL"
    });

    // Restore baseline policy so existing probes keep their prior behavior.
    scheduler::configure_slice_classes(5, 5, 5);
}
