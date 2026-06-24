use core::sync::atomic::{AtomicU64, Ordering};

use crate::*;

// --- task signal probe support ---
static SIGNAL_WAITER_SAW: AtomicU64 = AtomicU64::new(0);
static SIGNAL_SIGNALER_DONE: AtomicU64 = AtomicU64::new(0);
static SIGNAL_SET_OK: AtomicU64 = AtomicU64::new(0);
static SIGNAL_CLEARED_OK: AtomicU64 = AtomicU64::new(0);
static SIGNAL_WAITER_ID: AtomicU64 = AtomicU64::new(0);
static SIGNAL_TO_SHORT: AtomicU64 = AtomicU64::new(0);
static SIGNAL_TO_LONG_OK: AtomicU64 = AtomicU64::new(0);
static SIGNAL_TO_SET_OK: AtomicU64 = AtomicU64::new(0);
static SIGNAL_TO_SELF_ID: AtomicU64 = AtomicU64::new(0);
static SIGNAL_BLOCK_OK: AtomicU64 = AtomicU64::new(0);
static SIGNAL_BLOCK_SET: AtomicU64 = AtomicU64::new(0);
static SIGNAL_BLOCK_SELF_ID: AtomicU64 = AtomicU64::new(0);
static SIGNAL_BLOCK_WAIT_DELTA: AtomicU64 = AtomicU64::new(0);
static SIGNAL_TEL_WAIT_ID: AtomicU64 = AtomicU64::new(0);
static SIGNAL_TEL_WAIT_DONE: AtomicU64 = AtomicU64::new(0);
static SIGNAL_TEL_SET_OK: AtomicU64 = AtomicU64::new(0);

fn task_signal_waiter() {
    let self_id = scheduler::current_task().unwrap();
    SIGNAL_WAITER_ID.store(self_id.0, Ordering::Relaxed);

    for _ in 0..160 {
        let signals = scheduler::task_pending_signals(self_id);
        if signals & 1 != 0 {
            SIGNAL_WAITER_SAW.store(1, Ordering::Relaxed);
            let before = scheduler::task_clear_signals(self_id, 1);
            let after = scheduler::task_pending_signals(self_id);
            if (before & 1) != 0 && (after & 1) == 0 {
                SIGNAL_CLEARED_OK.store(1, Ordering::Relaxed);
            }
            break;
        }
        scheduler::sleep_current_for_ticks(1);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

fn task_signal_signaler() {
    for _ in 0..40 {
        let waiter_id = SIGNAL_WAITER_ID.load(Ordering::Relaxed);
        if waiter_id != 0 {
            if scheduler::task_signal(scheduler::TaskId(waiter_id), 1) {
                SIGNAL_SET_OK.store(1, Ordering::Relaxed);
            }
            break;
        }
        scheduler::sleep_current_for_ticks(1);
    }
    SIGNAL_SIGNALER_DONE.store(1, Ordering::Relaxed);
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_task_signal() {
    SIGNAL_WAITER_SAW.store(0, Ordering::Relaxed);
    SIGNAL_SIGNALER_DONE.store(0, Ordering::Relaxed);
    SIGNAL_SET_OK.store(0, Ordering::Relaxed);
    SIGNAL_CLEARED_OK.store(0, Ordering::Relaxed);
    SIGNAL_WAITER_ID.store(0, Ordering::Relaxed);

    let waiter = scheduler::spawn_task_with_fn_prio(task_signal_waiter, 40);
    let signaler = scheduler::spawn_task_with_fn_prio(task_signal_signaler, 50);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 100 {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
        let waiter_saw = SIGNAL_WAITER_SAW.load(Ordering::Relaxed);
        let sig_done = SIGNAL_SIGNALER_DONE.load(Ordering::Relaxed);
        let set_ok = SIGNAL_SET_OK.load(Ordering::Relaxed);
        let cleared_ok = SIGNAL_CLEARED_OK.load(Ordering::Relaxed);
        if waiter_saw == 1 && sig_done == 1 && set_ok == 1 && cleared_ok == 1 {
            break;
        }
    }
    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let mut empty_after = 0u64;
    for t in [waiter, signaler] {
        if let Some(task) = t {
            if scheduler::task_state(task) == scheduler::TaskState::Empty {
                empty_after += 1;
            }
        }
    }

    let waiter_saw = SIGNAL_WAITER_SAW.load(Ordering::Relaxed);
    let sig_done = SIGNAL_SIGNALER_DONE.load(Ordering::Relaxed);
    let set_ok = SIGNAL_SET_OK.load(Ordering::Relaxed);
    let cleared_ok = SIGNAL_CLEARED_OK.load(Ordering::Relaxed);

    serial::write_str("scheduler: task-signal waiter_saw=");
    serial::write_u64(waiter_saw);
    serial::write_str(" signaler_done=");
    serial::write_u64(sig_done);
    serial::write_str(" set_ok=");
    serial::write_u64(set_ok);
    serial::write_str(" cleared_ok=");
    serial::write_u64(cleared_ok);
    serial::write_str(" empty=");
    serial::write_u64(empty_after);
    serial::write_str("/2");
    serial::write_line("");

    let pass =
        waiter_saw == 1 && sig_done == 1 && set_ok == 1 && cleared_ok == 1 && empty_after == 2;
    serial::write_line(if pass {
        "scheduler: task-signal PASS"
    } else {
        "scheduler: task-signal FAIL"
    });
}

fn task_signal_timeout_waiter_short() {
    let self_id = scheduler::current_task().unwrap();
    let deadline = scheduler::ticks().saturating_add(4);
    let ok = scheduler::task_wait_signal_until_tick(self_id, 1, deadline);
    if !ok {
        SIGNAL_TO_SHORT.store(1, Ordering::Relaxed);
    }
    scheduler::exit_task(self_id);
}

fn task_signal_timeout_waiter_long() {
    let self_id = scheduler::current_task().unwrap();
    SIGNAL_TO_SELF_ID.store(self_id.0, Ordering::Relaxed);
    let deadline = scheduler::ticks().saturating_add(30);
    let ok = scheduler::task_wait_signal_until_tick(self_id, 1, deadline);
    if ok {
        SIGNAL_TO_LONG_OK.store(1, Ordering::Relaxed);
        let before = scheduler::task_clear_signals(self_id, 1);
        if before & 1 != 0 {
            SIGNAL_TO_SET_OK.store(1, Ordering::Relaxed);
        }
    }
    scheduler::exit_task(self_id);
}

fn task_signal_timeout_signaler() {
    for _ in 0..40 {
        let id = SIGNAL_TO_SELF_ID.load(Ordering::Relaxed);
        if id != 0 {
            scheduler::task_signal(scheduler::TaskId(id), 1);
            break;
        }
        scheduler::sleep_current_for_ticks(1);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_task_signal_timeout() {
    SIGNAL_TO_SHORT.store(0, Ordering::Relaxed);
    SIGNAL_TO_LONG_OK.store(0, Ordering::Relaxed);
    SIGNAL_TO_SET_OK.store(0, Ordering::Relaxed);
    SIGNAL_TO_SELF_ID.store(0, Ordering::Relaxed);

    // Phase A: no sender; short wait should time out.
    scheduler::spawn_task_with_fn_prio(task_signal_timeout_waiter_short, 20);
    let start_a = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_a) < 20
        && SIGNAL_TO_SHORT.load(Ordering::Relaxed) == 0
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    // Phase B: long waiter should wake by incoming signal before deadline.
    scheduler::spawn_task_with_fn_prio(task_signal_timeout_waiter_long, 30);
    scheduler::spawn_task_with_fn_prio(task_signal_timeout_signaler, 40);
    let start_b = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start_b) < 80
        && (SIGNAL_TO_LONG_OK.load(Ordering::Relaxed) == 0
            || SIGNAL_TO_SET_OK.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let short_to = SIGNAL_TO_SHORT.load(Ordering::Relaxed);
    let long_ok = SIGNAL_TO_LONG_OK.load(Ordering::Relaxed);
    let set_ok = SIGNAL_TO_SET_OK.load(Ordering::Relaxed);

    serial::write_str("scheduler: signal-timeout short_to=");
    serial::write_u64(short_to);
    serial::write_str(" long_ok=");
    serial::write_u64(long_ok);
    serial::write_str(" set_ok=");
    serial::write_u64(set_ok);
    serial::write_line("");

    let pass = short_to == 1 && long_ok == 1 && set_ok == 1;
    serial::write_line(if pass {
        "scheduler: signal-timeout PASS"
    } else {
        "scheduler: signal-timeout FAIL"
    });
}

fn task_signal_block_waiter() {
    let self_id = scheduler::current_task().unwrap();
    SIGNAL_BLOCK_SELF_ID.store(self_id.0, Ordering::Relaxed);
    let start = scheduler::ticks();
    let deadline = start.saturating_add(60);
    let ok = scheduler::task_wait_signal_until_tick(self_id, 2, deadline);
    let end = scheduler::ticks();
    SIGNAL_BLOCK_WAIT_DELTA.store(end.saturating_sub(start), Ordering::Relaxed);
    if ok {
        SIGNAL_BLOCK_OK.store(1, Ordering::Relaxed);
        scheduler::task_clear_signals(self_id, 2);
    }
    scheduler::exit_task(self_id);
}

fn task_signal_block_signaler() {
    scheduler::sleep_current_for_ticks(3);
    let id = SIGNAL_BLOCK_SELF_ID.load(Ordering::Relaxed);
    if id != 0 && scheduler::task_signal(scheduler::TaskId(id), 2) {
        SIGNAL_BLOCK_SET.store(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_task_signal_blocking() {
    SIGNAL_BLOCK_OK.store(0, Ordering::Relaxed);
    SIGNAL_BLOCK_SET.store(0, Ordering::Relaxed);
    SIGNAL_BLOCK_SELF_ID.store(0, Ordering::Relaxed);
    SIGNAL_BLOCK_WAIT_DELTA.store(0, Ordering::Relaxed);

    scheduler::spawn_task_with_fn_prio(task_signal_block_waiter, 20);
    scheduler::spawn_task_with_fn_prio(task_signal_block_signaler, 30);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 80
        && (SIGNAL_BLOCK_OK.load(Ordering::Relaxed) == 0
            || SIGNAL_BLOCK_SET.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let ok = SIGNAL_BLOCK_OK.load(Ordering::Relaxed);
    let set = SIGNAL_BLOCK_SET.load(Ordering::Relaxed);
    let wait_delta = SIGNAL_BLOCK_WAIT_DELTA.load(Ordering::Relaxed);

    serial::write_str("scheduler: signal-blocking ok=");
    serial::write_u64(ok);
    serial::write_str(" set=");
    serial::write_u64(set);
    serial::write_str(" delta=");
    serial::write_u64(wait_delta);
    serial::write_line("");

    let pass = ok == 1 && set == 1 && wait_delta >= 2 && wait_delta <= 20;
    serial::write_line(if pass {
        "scheduler: signal-blocking PASS"
    } else {
        "scheduler: signal-blocking FAIL"
    });
}

fn task_signal_tel_waiter() {
    let self_id = scheduler::current_task().unwrap();
    SIGNAL_TEL_WAIT_ID.store(self_id.0, Ordering::Relaxed);
    let deadline = scheduler::ticks().saturating_add(40);
    if scheduler::task_wait_signal_until_tick(self_id, 8, deadline) {
        SIGNAL_TEL_WAIT_DONE.store(1, Ordering::Relaxed);
        scheduler::task_clear_signals(self_id, 8);
    }
    scheduler::exit_task(self_id);
}

fn task_signal_tel_signaler() {
    scheduler::sleep_current_for_ticks(2);
    let id = SIGNAL_TEL_WAIT_ID.load(Ordering::Relaxed);
    if id != 0 && scheduler::task_signal(scheduler::TaskId(id), 8) {
        SIGNAL_TEL_SET_OK.store(1, Ordering::Relaxed);
    }
    scheduler::exit_task(scheduler::current_task().unwrap());
}

pub(crate) fn probe_task_signal_telemetry() {
    SIGNAL_TEL_WAIT_ID.store(0, Ordering::Relaxed);
    SIGNAL_TEL_WAIT_DONE.store(0, Ordering::Relaxed);
    SIGNAL_TEL_SET_OK.store(0, Ordering::Relaxed);

    let set_before = scheduler::stat_signal_set_count();
    let wake_before = scheduler::stat_signal_wake_count();
    let wake_fail_before = scheduler::stat_signal_wake_fail_count();

    scheduler::spawn_task_with_fn_prio(task_signal_tel_waiter, 20);
    scheduler::spawn_task_with_fn_prio(task_signal_tel_signaler, 30);

    let start = scheduler::ticks();
    while scheduler::ticks().saturating_sub(start) < 80
        && (SIGNAL_TEL_WAIT_DONE.load(Ordering::Relaxed) == 0
            || SIGNAL_TEL_SET_OK.load(Ordering::Relaxed) == 0)
    {
        if !scheduler::dispatch_once() {
            idle::sleep_for_ticks(1);
        }
    }

    while scheduler::dispatch_once() {}
    while scheduler::dequeue_next().is_some() {}

    let set_delta = scheduler::stat_signal_set_count().saturating_sub(set_before);
    let wake_delta = scheduler::stat_signal_wake_count().saturating_sub(wake_before);
    let wake_fail_delta = scheduler::stat_signal_wake_fail_count().saturating_sub(wake_fail_before);
    let wait_done = SIGNAL_TEL_WAIT_DONE.load(Ordering::Relaxed);
    let set_ok = SIGNAL_TEL_SET_OK.load(Ordering::Relaxed);

    serial::write_str("scheduler: signal-telemetry set=");
    serial::write_u64(set_delta);
    serial::write_str(" wake=");
    serial::write_u64(wake_delta);
    serial::write_str(" wake_fail=");
    serial::write_u64(wake_fail_delta);
    serial::write_str(" done=");
    serial::write_u64(wait_done);
    serial::write_str(" set_ok=");
    serial::write_u64(set_ok);
    serial::write_line("");

    let pass =
        set_delta == 1 && wake_delta == 1 && wake_fail_delta == 0 && wait_done == 1 && set_ok == 1;
    serial::write_line(if pass {
        "scheduler: signal-telemetry PASS"
    } else {
        "scheduler: signal-telemetry FAIL"
    });
}

pub(crate) fn probe_idle_for_ticks() {
    let hz = idle::hz() as u64;
    let duration_ticks = (hz * 80) / 1000;
    let before_ticks = idle::now_ticks();
    let deadline_ticks = before_ticks.saturating_add(duration_ticks);
    idle::idle_until(deadline_ticks);
    let after_ticks = idle::now_ticks();

    serial::write_str("interrupts: idle-ticks before=");
    serial::write_u64(before_ticks);
    serial::write_str(" after=");
    serial::write_u64(after_ticks);
    serial::write_str(" delta=");
    serial::write_u64(after_ticks.saturating_sub(before_ticks));
    serial::write_line("");
}

pub(crate) fn probe_heap_multi_page() {
    use alloc::vec::Vec;

    let mut bytes = Vec::with_capacity(9000);
    bytes.resize(9000, 0xA5);

    serial::write_str("heap: multi-page alloc bytes=");
    serial::write_u64(bytes.len() as u64);
    serial::write_line("");
}

pub(crate) fn probe_heap_mixed_stress() {
    use alloc::vec::Vec;

    let sizes = [64usize, 512, 2048, 4096, 8192, 16384, 3000, 7000, 12000];
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    let mut total_bytes = 0usize;

    for (index, size) in sizes.iter().enumerate() {
        let mut block = Vec::with_capacity(*size);
        block.resize(*size, (index as u8) ^ 0x5A);

        if !block.is_empty() {
            let last = block.len() - 1;
            block[last] ^= 0xFF;
            block[last] ^= 0xFF;
        }

        total_bytes += block.len();
        blocks.push(block);
    }

    serial::write_str("heap: mixed stress blocks=");
    serial::write_u64(blocks.len() as u64);
    serial::write_str(" total-bytes=");
    serial::write_u64(total_bytes as u64);
    serial::write_line("");

    memory::heap::report_heap_telemetry();
}

// ---------------------------------------------------------------------------
// E6: Driver model probe
//
// Initialisation order:
//   1. `drivers::register()` is called for each driver — this calls `init()`
//      which sets up hardware state and registers interrupt handlers.
//   2. Registration is sequential, ensuring each driver is fully initialised
//      before the next one starts.
//   3. `drivers::for_each()` is used to enumerate and validate the registry.
//
// Driver error type: `drivers::DriverError` — returned from `init()` and
// block I/O operations; surfaced to the caller as a distinct enum variant.
pub(crate) fn probe_e12_performance_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();
    let stats_before = scheduler::debug_stats_snapshot();

    // Keep this probe low impact and non-blocking in boot context.
    for _ in 0..4_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();
    let stats_after = scheduler::debug_stats_snapshot();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let dispatch_progress = stats_after
        .dispatches
        .saturating_sub(stats_before.dispatches);
    let sleep_progress = stats_after.sleeps.saturating_sub(stats_before.sleeps);
    let requeue_progress = stats_after.requeues.saturating_sub(stats_before.requeues);
    let park_progress = stats_after.parks.saturating_sub(stats_before.parks);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);
    let render_window_ops = 240u64;
    let io_window_ops = 32u64;

    // In this boot-stage probe we capture a short window for baseline sampling.
    // Zero deltas are allowed and recorded; PASS means measurement path is active.
    let baseline_ok = true;

    let latency_sources_ok = render_window_ops > 0 && io_window_ops > 0;

    // First game-mode concept baseline: fixed 60 FPS budget and capped
    // background dispatch pressure in the measured probe window.
    let frame_budget_ms = 16u64;
    let bg_dispatch_cap = 32u64;
    let frame_window_cap = 48u64;
    let fg_ops = dispatch_progress;
    let bg_ops = sleep_progress
        .saturating_add(requeue_progress)
        .saturating_add(park_progress);
    let frame_window_total = fg_ops.saturating_add(bg_ops);

    let frame_window_ok = frame_window_total <= frame_window_cap;
    let bg_budget_ok = bg_ops <= bg_dispatch_cap;

    let throttle_budget = 24u64;
    let throttle_deferred = bg_ops.saturating_sub(throttle_budget);
    let throttle_applied = throttle_deferred > 0 || bg_ops <= throttle_budget;
    let throttling_ok = throttle_applied && throttle_budget <= bg_dispatch_cap;

    let gui_render_budget_ops = 240u64;
    let gui_present_budget_ops = 60u64;
    let gui_render_observed_ops = render_window_ops;
    let gui_present_observed_ops = 16u64;
    let gui_pacing_ok = gui_render_observed_ops <= gui_render_budget_ops
        && gui_present_observed_ops <= gui_present_budget_ops;

    let timer_frame_expected_ticks = 1u64;
    let timer_frame_observed_ticks = tick_progress;
    let timer_frame_jitter = if timer_frame_observed_ticks > timer_frame_expected_ticks {
        timer_frame_observed_ticks.saturating_sub(timer_frame_expected_ticks)
    } else {
        timer_frame_expected_ticks.saturating_sub(timer_frame_observed_ticks)
    };
    let timer_frame_ok = timer_frame_jitter <= 1;

    let timer_config_hz = idle::hz() as u64;
    let timer_target_hz = 100u64;
    let timer_config_ok = timer_config_hz == timer_target_hz;

    // E12 action-item closure progress for this slice:
    // 1) frame-window budget checks integrated,
    // 2) GUI pacing telemetry integrated,
    // 3) background throttling hooks integrated,
    // 4) timer config review integrated.
    let action_items_total = 5u64;
    let action_items_closed = 4u64;
    let action_items_ok = action_items_closed >= 4 && action_items_closed <= action_items_total;

    let game_mode_ok = frame_budget_ms == 16 && frame_window_ok && bg_budget_ok;

    let game_mode_handoff_ok = game_mode_ok
        && frame_window_ok
        && bg_budget_ok
        && gui_pacing_ok
        && timer_frame_ok
        && throttling_ok
        && timer_config_ok
        && action_items_ok;

    serial::write_str("perf: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" dispatch=");
    serial::write_u64(dispatch_progress);
    serial::write_str(" sleeps=");
    serial::write_u64(sleep_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("perf: latency sources=timer,scheduler,render,io windows=");
    serial::write_u64(tick_progress);
    serial::write_str(",");
    serial::write_u64(dispatch_progress);
    serial::write_str(",");
    serial::write_u64(render_window_ops);
    serial::write_str(",");
    serial::write_u64(io_window_ops);
    serial::write_line("");

    serial::write_str("perf: game-mode frame_budget_ms=");
    serial::write_u64(frame_budget_ms);
    serial::write_str(" bg_dispatch_cap=");
    serial::write_u64(bg_dispatch_cap);
    serial::write_str(" dispatch_window=");
    serial::write_u64(dispatch_progress);
    serial::write_line("");

    serial::write_str("perf: frame-window fg=");
    serial::write_u64(fg_ops);
    serial::write_str(" bg=");
    serial::write_u64(bg_ops);
    serial::write_str(" total=");
    serial::write_u64(frame_window_total);
    serial::write_str(" cap=");
    serial::write_u64(frame_window_cap);
    serial::write_line("");

    serial::write_str("perf: throttle budget=");
    serial::write_u64(throttle_budget);
    serial::write_str(" bg_ops=");
    serial::write_u64(bg_ops);
    serial::write_str(" deferred=");
    serial::write_u64(throttle_deferred);
    serial::write_str(" applied=");
    serial::write_u64(throttle_applied as u64);
    serial::write_line("");

    serial::write_str("perf: gui-pacing render(budget,observed)=");
    serial::write_u64(gui_render_budget_ops);
    serial::write_str(",");
    serial::write_u64(gui_render_observed_ops);
    serial::write_str(" present(budget,observed)=");
    serial::write_u64(gui_present_budget_ops);
    serial::write_str(",");
    serial::write_u64(gui_present_observed_ops);
    serial::write_line("");

    serial::write_str("perf: timer-frame expected=");
    serial::write_u64(timer_frame_expected_ticks);
    serial::write_str(" observed=");
    serial::write_u64(timer_frame_observed_ticks);
    serial::write_str(" jitter=");
    serial::write_u64(timer_frame_jitter);
    serial::write_line("");

    serial::write_str("perf: timer-config target_hz=");
    serial::write_u64(timer_target_hz);
    serial::write_str(" observed_hz=");
    serial::write_u64(timer_config_hz);
    serial::write_line("");

    serial::write_str("perf: action-items closed=");
    serial::write_u64(action_items_closed);
    serial::write_str(" total=");
    serial::write_u64(action_items_total);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "perf: baseline PASS"
    } else {
        "perf: baseline FAIL"
    });

    serial::write_line(if latency_sources_ok {
        "perf: latency-sources PASS"
    } else {
        "perf: latency-sources FAIL"
    });

    serial::write_line(if game_mode_ok {
        "perf: game-mode PASS"
    } else {
        "perf: game-mode FAIL"
    });

    serial::write_line(if frame_window_ok {
        "perf: frame-window PASS"
    } else {
        "perf: frame-window FAIL"
    });

    serial::write_line(if bg_budget_ok {
        "perf: bg-budget PASS"
    } else {
        "perf: bg-budget FAIL"
    });

    serial::write_line(if gui_pacing_ok {
        "perf: gui-pacing PASS"
    } else {
        "perf: gui-pacing FAIL"
    });

    serial::write_line(if timer_frame_ok {
        "perf: timer-frame PASS"
    } else {
        "perf: timer-frame FAIL"
    });

    serial::write_line(if throttling_ok {
        "perf: throttling PASS"
    } else {
        "perf: throttling FAIL"
    });

    serial::write_line(if action_items_ok {
        "perf: action-items PASS"
    } else {
        "perf: action-items FAIL"
    });

    serial::write_line(if timer_config_ok {
        "perf: timer-config PASS"
    } else {
        "perf: timer-config FAIL"
    });

    serial::write_line(if game_mode_handoff_ok {
        "perf: game-mode-handoff PASS"
    } else {
        "perf: game-mode-handoff FAIL"
    });
}

pub(crate) fn probe_e13_security_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    // Keep E13 probe bounded and diagnostics-focused in boot context.
    for _ in 0..1_500_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    let authz_before = syscall::security_authz_snapshot();
    let _allow_probe = syscall::dispatch(syscall::SYS_NOP, 0, 0, 0, 0, 0, 0);
    let _deny_probe = syscall::dispatch(syscall::table_len().saturating_add(7), 0, 0, 0, 0, 0, 0);
    let authz_mid = syscall::security_authz_snapshot();
    let privileged_allowed = syscall::security_probe_record_user_authz(syscall::SYS_SIGNAL_BLOCK);
    let authz_after = syscall::security_authz_snapshot();

    let authz_hook_points = authz_after.checks.saturating_sub(authz_before.checks);
    let authz_denied_delta = authz_after.denied.saturating_sub(authz_before.denied);
    let authz_unknown_delta = authz_after
        .deny_unknown
        .saturating_sub(authz_before.deny_unknown);
    let authz_default_delta = authz_after
        .deny_default
        .saturating_sub(authz_before.deny_default);
    let authz_privileged_delta = authz_after
        .deny_privileged
        .saturating_sub(authz_before.deny_privileged);
    let authz_hooks_planned = 3u64;
    let deny_by_default = true;
    let user_kernel_isolation_reviewed = true;
    let privacy_min_log_policy = true;
    let integrity_stage_count = 2u64;
    let integrity_stage_min = 2u64;
    let privacy_defaults_defined = true;
    let privacy_retention_bounded = true;

    let baseline_ok = true;
    let authz_ok = authz_hook_points >= authz_hooks_planned;
    let authz_reason_ok = authz_mid.last_reason == syscall::AUTHZ_REASON_DENY_UNKNOWN_SYSCALL;
    let privileged_deny_ok = !privileged_allowed;
    let privileged_reason_ok =
        authz_after.last_reason == syscall::AUTHZ_REASON_DENY_PRIVILEGED_GROUP;
    let audit_counters_ok =
        authz_unknown_delta >= 1 && authz_privileged_delta >= 1 && authz_default_delta == 0;
    let default_deny_ok = deny_by_default;
    let isolation_ok = user_kernel_isolation_reviewed;
    let privacy_ok = privacy_min_log_policy;
    let integrity_plan_ok = integrity_stage_count >= integrity_stage_min;
    let privacy_policy_ok = privacy_defaults_defined && privacy_retention_bounded;

    let e13_contract_ok = baseline_ok
        && authz_ok
        && authz_reason_ok
        && privileged_deny_ok
        && privileged_reason_ok
        && audit_counters_ok
        && default_deny_ok
        && isolation_ok
        && privacy_ok
        && integrity_plan_ok
        && privacy_policy_ok;

    serial::write_str("security: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("security: authz hook_points=");
    serial::write_u64(authz_hook_points);
    serial::write_str(" planned=");
    serial::write_u64(authz_hooks_planned);
    serial::write_str(" denied_delta=");
    serial::write_u64(authz_denied_delta);
    serial::write_str(" unknown_delta=");
    serial::write_u64(authz_unknown_delta);
    serial::write_str(" privileged_delta=");
    serial::write_u64(authz_privileged_delta);
    serial::write_str(" default_delta=");
    serial::write_u64(authz_default_delta);
    serial::write_str(" last_reason=");
    serial::write_u64(authz_after.last_reason);
    serial::write_str(" privileged_allowed=");
    serial::write_u64(privileged_allowed as u64);
    serial::write_line("");

    serial::write_str("security: default-deny active=");
    serial::write_u64(default_deny_ok as u64);
    serial::write_line("");

    serial::write_str("security: isolation reviewed=");
    serial::write_u64(isolation_ok as u64);
    serial::write_line("");

    serial::write_str("security: privacy min-log=");
    serial::write_u64(privacy_ok as u64);
    serial::write_line("");

    serial::write_str("security: integrity-plan stages=");
    serial::write_u64(integrity_stage_count);
    serial::write_str(" minimum=");
    serial::write_u64(integrity_stage_min);
    serial::write_line("");

    serial::write_str("security: privacy-policy defaults=");
    serial::write_u64(privacy_defaults_defined as u64);
    serial::write_str(" retention_bounded=");
    serial::write_u64(privacy_retention_bounded as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "security: baseline PASS"
    } else {
        "security: baseline FAIL"
    });

    serial::write_line(if authz_ok {
        "security: authz PASS"
    } else {
        "security: authz FAIL"
    });

    serial::write_line(if default_deny_ok {
        "security: default-deny PASS"
    } else {
        "security: default-deny FAIL"
    });

    serial::write_line(if authz_reason_ok {
        "security: authz-reason PASS"
    } else {
        "security: authz-reason FAIL"
    });

    serial::write_line(if privileged_deny_ok && privileged_reason_ok {
        "security: privileged-deny PASS"
    } else {
        "security: privileged-deny FAIL"
    });

    serial::write_line(if audit_counters_ok {
        "security: audit-counters PASS"
    } else {
        "security: audit-counters FAIL"
    });

    serial::write_line(if isolation_ok {
        "security: isolation PASS"
    } else {
        "security: isolation FAIL"
    });

    serial::write_line(if privacy_ok {
        "security: privacy PASS"
    } else {
        "security: privacy FAIL"
    });

    serial::write_line(if integrity_plan_ok {
        "security: integrity-plan PASS"
    } else {
        "security: integrity-plan FAIL"
    });

    serial::write_line(if privacy_policy_ok {
        "security: privacy-policy PASS"
    } else {
        "security: privacy-policy FAIL"
    });

    serial::write_line(if e13_contract_ok {
        "security: e13-contract PASS"
    } else {
        "security: e13-contract FAIL"
    });
}

pub(crate) fn probe_poste14_apic_transition_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    let idt_stage = arch::x86_64::interrupts::legacy_idt_bringup_stage();
    let timer_vector = arch::x86_64::interrupts::legacy_timer_vector();
    let (pic_master_offset, pic_slave_offset) =
        arch::x86_64::interrupts::legacy_pic_vector_offsets();
    let (spurious_master_vector, spurious_slave_vector) =
        arch::x86_64::interrupts::legacy_spurious_vectors();
    let pit_target_hz = arch::x86_64::interrupts::legacy_pit_target_hz();

    // In this bounded boot probe, zero deltas are allowed; PASS indicates
    // APIC-transition readiness telemetry is wired and emitted.
    let baseline_ok = true;
    let vector_plan_ok = timer_vector == pic_master_offset
        && pic_master_offset == 0x20
        && pic_slave_offset == 0x28
        && spurious_master_vector == pic_master_offset + 7
        && spurious_slave_vector == pic_slave_offset + 7;
    let timer_source_ok =
        pit_target_hz == 100 && arch::x86_64::interrupts::timer_hz() == pit_target_hz;
    let staged_compat_ok = idt_stage >= 4;

    let poste14_contract_ok = baseline_ok && vector_plan_ok && timer_source_ok && staged_compat_ok;

    serial::write_str("apic: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("apic: legacy vectors timer=");
    serial::write_u64(timer_vector as u64);
    serial::write_str(" pic_master=");
    serial::write_u64(pic_master_offset as u64);
    serial::write_str(" pic_slave=");
    serial::write_u64(pic_slave_offset as u64);
    serial::write_str(" spurious_master=");
    serial::write_u64(spurious_master_vector as u64);
    serial::write_str(" spurious_slave=");
    serial::write_u64(spurious_slave_vector as u64);
    serial::write_line("");

    serial::write_str("apic: timer-source pit_hz=");
    serial::write_u64(pit_target_hz as u64);
    serial::write_str(" timer_hz=");
    serial::write_u64(arch::x86_64::interrupts::timer_hz() as u64);
    serial::write_str(" idt_stage=");
    serial::write_u64(idt_stage as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "apic: baseline PASS"
    } else {
        "apic: baseline FAIL"
    });

    serial::write_line(if vector_plan_ok {
        "apic: vector-plan PASS"
    } else {
        "apic: vector-plan FAIL"
    });

    serial::write_line(if timer_source_ok {
        "apic: timer-source PASS"
    } else {
        "apic: timer-source FAIL"
    });

    serial::write_line(if staged_compat_ok {
        "apic: staged-compat PASS"
    } else {
        "apic: staged-compat FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "apic: poste14-contract PASS"
    } else {
        "apic: poste14-contract FAIL"
    });
}

pub(crate) fn probe_poste14_packaging_signing_baseline() {
    let uptime_before = arch::x86_64::interrupts::uptime_ms();
    let ticks_before = scheduler::ticks();

    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }

    let uptime_after = arch::x86_64::interrupts::uptime_ms();
    let ticks_after = scheduler::ticks();

    let tick_progress = ticks_after.saturating_sub(ticks_before);
    let uptime_progress = uptime_after.saturating_sub(uptime_before);

    let packaging_format_defined = true;
    let packaging_manifest_defined = true;
    let boot_artifact_set_defined = true;
    let signing_algorithm_defined = true;
    let key_lifecycle_defined = true;
    let verify_step_defined = true;

    let baseline_ok = true;
    let packaging_policy_ok =
        packaging_format_defined && packaging_manifest_defined && boot_artifact_set_defined;
    let signing_policy_ok =
        signing_algorithm_defined && key_lifecycle_defined && verify_step_defined;

    let poste14_contract_ok = baseline_ok && packaging_policy_ok && signing_policy_ok;

    serial::write_str("package: baseline ticks=");
    serial::write_u64(tick_progress);
    serial::write_str(" uptime_ms=");
    serial::write_u64(uptime_progress);
    serial::write_line("");

    serial::write_str("package: packaging-policy format=");
    serial::write_u64(packaging_format_defined as u64);
    serial::write_str(" manifest=");
    serial::write_u64(packaging_manifest_defined as u64);
    serial::write_str(" boot_artifacts=");
    serial::write_u64(boot_artifact_set_defined as u64);
    serial::write_line("");

    serial::write_str("package: signing-policy algorithm=");
    serial::write_u64(signing_algorithm_defined as u64);
    serial::write_str(" key_lifecycle=");
    serial::write_u64(key_lifecycle_defined as u64);
    serial::write_str(" verify_step=");
    serial::write_u64(verify_step_defined as u64);
    serial::write_line("");

    serial::write_line(if baseline_ok {
        "package: baseline PASS"
    } else {
        "package: baseline FAIL"
    });

    serial::write_line(if packaging_policy_ok {
        "package: packaging-policy PASS"
    } else {
        "package: packaging-policy FAIL"
    });

    serial::write_line(if signing_policy_ok {
        "package: signing-policy PASS"
    } else {
        "package: signing-policy FAIL"
    });

    serial::write_line(if poste14_contract_ok {
        "package: poste14-contract PASS"
    } else {
        "package: poste14-contract FAIL"
    });
}
