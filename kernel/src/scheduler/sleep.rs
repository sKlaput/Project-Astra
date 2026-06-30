//! Sleep, wake, and park operations for tasks.

use core::sync::atomic::Ordering;
use crate::scheduler::{TaskId, TaskState};
use super::{table, context, stats, dispatch};

pub fn sleep_current_for_ticks(ticks: u64) -> bool {
    let wake_at = crate::scheduler::ticks()
        .saturating_add(ticks.max(1));
    sleep_current_until_tick(wake_at)
}

pub fn sleep_current_until_tick(deadline_tick: u64) -> bool {
    let id = match crate::scheduler::current_task() {
        Some(id) => id,
        None => return false,
    };

    let now = crate::scheduler::ticks();
    let wake_at = deadline_tick.max(now.saturating_add(1));
    let slot = table::table_slot(id.0);

    table::TASK_TABLE[slot].wake_tick.store(wake_at, Ordering::Release);
    super::set_task_state(id, TaskState::Sleeping);
    stats::record_sleep();
    super::CURRENT_TASK.store(0, Ordering::Release);

    let sched_rsp = super::SCHEDULER_CONTEXT_RSP.load(Ordering::Acquire);
    unsafe {
        context::context_switch(table::TASK_TABLE[slot].context_rsp.as_ptr(), sched_rsp);
    }
    true
}

pub fn park_current_task() -> bool {
    let id = match crate::scheduler::current_task() {
        Some(id) => id,
        None => return false,
    };

    let slot = table::table_slot(id.0);
    table::TASK_TABLE[slot].wake_tick.store(u64::MAX, Ordering::Release);
    super::set_task_state(id, TaskState::Sleeping);
    stats::record_park();
    stats::record_sleep();
    super::CURRENT_TASK.store(0, Ordering::Release);

    let sched_rsp = super::SCHEDULER_CONTEXT_RSP.load(Ordering::Acquire);
    unsafe {
        context::context_switch(table::TASK_TABLE[slot].context_rsp.as_ptr(), sched_rsp);
    }
    true
}

pub fn unpark_task(id: TaskId) -> bool {
    let slot = table::table_slot(id.0);
    if table::TASK_TABLE[slot].id.load(Ordering::Relaxed) != id.0 {
        stats::record_unpark_fail();
        return false;
    }
    let state = TaskState::from_u8(
        table::TASK_TABLE[slot].state.load(Ordering::Acquire),
    );
    if state != TaskState::Sleeping {
        stats::record_unpark_fail();
        return false;
    }
    if dispatch::enqueue_task_inner(id) {
        table::TASK_TABLE[slot].wake_tick.store(0, Ordering::Relaxed);
        super::set_task_state(id, TaskState::Ready);
        stats::record_wake();
        stats::record_unpark();
        true
    } else {
        stats::record_unpark_fail();
        false
    }
}

