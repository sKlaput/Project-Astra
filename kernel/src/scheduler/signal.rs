//! Task signal handling.

use core::sync::atomic::Ordering;
use crate::scheduler::TaskId;
use super::{stats, table};

pub fn task_signal(id: TaskId, bits: u64) -> bool {
    let slot = table::table_slot(id.0);
    if table::TASK_TABLE[slot].id.load(Ordering::Relaxed) != id.0 {
        return false;
    }
    table::TASK_TABLE[slot].signals.fetch_or(bits, Ordering::Relaxed);
    stats::record_signal_set();
    // Best-effort wake for sleeping tasks that now have unblocked pending signals.
    let state = table::TaskState::from_u8(table::TASK_TABLE[slot].state.load(Ordering::Acquire));
    if state == table::TaskState::Sleeping && task_pending_unblocked_signals(id) != 0 {
        if super::sleep::unpark_task(id) {
            stats::record_signal_wake();
        } else {
            stats::record_signal_wake_fail();
        }
    }
    true
}

pub fn task_pending_signals(id: TaskId) -> u64 {
    let slot = table::table_slot(id.0);
    if table::TASK_TABLE[slot].id.load(Ordering::Relaxed) == id.0 {
        table::TASK_TABLE[slot].signals.load(Ordering::Relaxed)
    } else {
        0
    }
}

pub fn task_clear_signals(id: TaskId, bits: u64) -> u64 {
    let slot = table::table_slot(id.0);
    if table::TASK_TABLE[slot].id.load(Ordering::Relaxed) == id.0 {
        table::TASK_TABLE[slot].signals.fetch_and(!bits, Ordering::Relaxed)
    } else {
        0
    }
}

pub fn task_signal_mask(id: TaskId) -> u64 {
    let slot = table::table_slot(id.0);
    if table::TASK_TABLE[slot].id.load(Ordering::Relaxed) == id.0 {
        table::TASK_TABLE[slot].signal_mask.load(Ordering::Relaxed)
    } else {
        0
    }
}

pub fn task_block_signals(id: TaskId, bits: u64) -> u64 {
    let slot = table::table_slot(id.0);
    if table::TASK_TABLE[slot].id.load(Ordering::Relaxed) == id.0 {
        table::TASK_TABLE[slot].signal_mask.fetch_or(bits, Ordering::Relaxed)
    } else {
        0
    }
}

pub fn task_unblock_signals(id: TaskId, bits: u64) -> u64 {
    let slot = table::table_slot(id.0);
    if table::TASK_TABLE[slot].id.load(Ordering::Relaxed) == id.0 {
        table::TASK_TABLE[slot].signal_mask.fetch_and(!bits, Ordering::Relaxed)
    } else {
        0
    }
}

pub fn task_pending_unblocked_signals(id: TaskId) -> u64 {
    task_pending_signals(id) & !task_signal_mask(id)
}

pub fn task_take_unblocked_signals(id: TaskId, bits: u64) -> u64 {
    let slot = table::table_slot(id.0);
    if table::TASK_TABLE[slot].id.load(Ordering::Relaxed) != id.0 {
        return 0;
    }
    loop {
        let pending = table::TASK_TABLE[slot].signals.load(Ordering::Acquire);
        let mask = table::TASK_TABLE[slot].signal_mask.load(Ordering::Acquire);
        let matched = pending & !mask & bits;
        if matched == 0 {
            return 0;
        }
        if table::TASK_TABLE[slot]
            .signals
            .compare_exchange(pending, pending & !matched, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            return matched;
        }
    }
}

pub fn task_wait_signal_until_tick(id: TaskId, bits: u64, deadline_tick: u64) -> bool {
    loop {
        if task_pending_unblocked_signals(id) & bits != 0 {
            return true;
        }
        let now = crate::scheduler::ticks();
        if now >= deadline_tick {
            return false;
        }
        if crate::scheduler::current_task() == Some(id) {
            if task_pending_unblocked_signals(id) & bits != 0 {
                return true;
            }
            crate::scheduler::sleep_current_until_tick(deadline_tick);
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

pub fn task_wait_all_signals_until_tick(id: TaskId, bits: u64, deadline_tick: u64) -> bool {
    loop {
        if task_pending_unblocked_signals(id) & bits == bits {
            return true;
        }
        let now = crate::scheduler::ticks();
        if now >= deadline_tick {
            return false;
        }
        if crate::scheduler::current_task() == Some(id) {
            if task_pending_unblocked_signals(id) & bits == bits {
                return true;
            }
            crate::scheduler::sleep_current_until_tick(deadline_tick);
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

pub fn task_wait_consume_signal_until_tick(id: TaskId, bits: u64, deadline_tick: u64) -> u64 {
    loop {
        let matched = task_take_unblocked_signals(id, bits);
        if matched != 0 {
            return matched;
        }
        let now = crate::scheduler::ticks();
        if now >= deadline_tick {
            return 0;
        }
        if crate::scheduler::current_task() == Some(id) {
            if task_pending_unblocked_signals(id) & bits != 0 {
                continue;
            }
            crate::scheduler::sleep_current_until_tick(deadline_tick);
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

pub fn task_wait_all_consume_signals_until_tick(id: TaskId, bits: u64, deadline_tick: u64) -> u64 {
    let slot = table::table_slot(id.0);
    if table::TASK_TABLE[slot].id.load(Ordering::Relaxed) != id.0 {
        return 0;
    }
    loop {
        loop {
            let pending = table::TASK_TABLE[slot].signals.load(Ordering::Acquire);
            let mask = table::TASK_TABLE[slot].signal_mask.load(Ordering::Acquire);
            let unblocked = pending & !mask;
            if unblocked & bits != bits {
                break;
            }
            if table::TASK_TABLE[slot]
                .signals
                .compare_exchange(pending, pending & !bits, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return bits;
            }
        }
        let now = crate::scheduler::ticks();
        if now >= deadline_tick {
            return 0;
        }
        if crate::scheduler::current_task() == Some(id) {
            if task_pending_unblocked_signals(id) & bits == bits {
                continue;
            }
            crate::scheduler::sleep_current_until_tick(deadline_tick);
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

pub fn task_wait_signal(id: TaskId, bits: u64) -> bool {
    loop {
        if task_pending_unblocked_signals(id) & bits != 0 {
            return true;
        }
        if crate::scheduler::current_task() == Some(id) {
            if task_pending_unblocked_signals(id) & bits != 0 {
                return true;
            }
            super::sleep::park_current_task();
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

pub fn task_wait_consume_signal(id: TaskId, bits: u64) -> u64 {
    loop {
        let matched = task_take_unblocked_signals(id, bits);
        if matched != 0 {
            return matched;
        }
        if crate::scheduler::current_task() == Some(id) {
            if task_pending_unblocked_signals(id) & bits != 0 {
                continue;
            }
            super::sleep::park_current_task();
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}

pub fn task_wait_all_consume_signals(id: TaskId, bits: u64) -> u64 {
    let slot = table::table_slot(id.0);
    if table::TASK_TABLE[slot].id.load(Ordering::Relaxed) != id.0 {
        return 0;
    }
    loop {
        loop {
            let pending = table::TASK_TABLE[slot].signals.load(Ordering::Acquire);
            let mask = table::TASK_TABLE[slot].signal_mask.load(Ordering::Acquire);
            let unblocked = pending & !mask;
            if unblocked & bits != bits {
                break;
            }
            if table::TASK_TABLE[slot]
                .signals
                .compare_exchange(pending, pending & !bits, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return bits;
            }
        }
        if crate::scheduler::current_task() == Some(id) {
            if task_pending_unblocked_signals(id) & bits == bits {
                continue;
            }
            super::sleep::park_current_task();
        } else {
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
    }
}
