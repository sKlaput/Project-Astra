use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};

use super::table::{self, TaskId, TaskState};

pub const RING_CAP: usize = 8;

pub static RING_HEAD: AtomicUsize = AtomicUsize::new(0);
pub static RING_TAIL: AtomicUsize = AtomicUsize::new(0);

static RING_BUF: [AtomicU64; RING_CAP] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

static AGING_ENABLED: AtomicBool = AtomicBool::new(true);
static AGING_TICKS_PER_LEVEL: AtomicU64 = AtomicU64::new(2);
static SLICE_CLASS_HIGH: AtomicU8 = AtomicU8::new(5);
static SLICE_CLASS_NORMAL: AtomicU8 = AtomicU8::new(5);
static SLICE_CLASS_LOW: AtomicU8 = AtomicU8::new(5);

fn nonzero_slice(v: u8) -> u8 {
    if v == 0 { 1 } else { v }
}

pub fn slice_for_priority(priority: u8) -> u8 {
    if priority <= 63 {
        nonzero_slice(SLICE_CLASS_HIGH.load(Ordering::Relaxed))
    } else if priority <= 191 {
        nonzero_slice(SLICE_CLASS_NORMAL.load(Ordering::Relaxed))
    } else {
        nonzero_slice(SLICE_CLASS_LOW.load(Ordering::Relaxed))
    }
}

/// Configure preemption slice lengths by priority class (0..=63 high, 64..=191 normal, 192..=255 low).
pub fn configure_slice_classes(high: u8, normal: u8, low: u8) {
    SLICE_CLASS_HIGH.store(nonzero_slice(high), Ordering::Relaxed);
    SLICE_CLASS_NORMAL.store(nonzero_slice(normal), Ordering::Relaxed);
    SLICE_CLASS_LOW.store(nonzero_slice(low), Ordering::Relaxed);
}

pub fn debug_slice_for_priority(priority: u8) -> u8 {
    slice_for_priority(priority)
}

pub fn configure_aging(enabled: bool, ticks_per_level: u64) {
    AGING_ENABLED.store(enabled, Ordering::Relaxed);
    AGING_TICKS_PER_LEVEL.store(ticks_per_level.max(1), Ordering::Relaxed);
}

pub fn debug_aging_enabled() -> bool {
    AGING_ENABLED.load(Ordering::Relaxed)
}

pub fn debug_aging_ticks_per_level() -> u64 {
    AGING_TICKS_PER_LEVEL.load(Ordering::Relaxed)
}

// ---- ring buffer operations -------------------------------------------------

pub fn enqueue_task_inner(id: TaskId) -> bool {
    let tail = RING_TAIL.load(Ordering::Relaxed);
    let head = RING_HEAD.load(Ordering::Relaxed);

    if tail.wrapping_sub(head) >= RING_CAP {
        return false;
    }

    RING_BUF[tail % RING_CAP].store(id.0, Ordering::Relaxed);
    table::TASK_TABLE[table::table_slot(id.0)]
        .enqueue_tick
        .store(super::SCHED_TICKS.load(Ordering::Relaxed), Ordering::Relaxed);
    RING_TAIL.store(tail.wrapping_add(1), Ordering::Release);
    super::IDLE_DECISION_SEEN.store(false, Ordering::Relaxed);
    true
}

/// Dequeue the highest-priority (lowest value) ready task. Equal-priority tasks are FIFO.
pub fn dequeue_next_inner() -> Option<TaskId> {
    let head = RING_HEAD.load(Ordering::Relaxed);
    let tail = RING_TAIL.load(Ordering::Acquire);
    let now = super::SCHED_TICKS.load(Ordering::Relaxed);

    if head == tail {
        return None;
    }

    let mut best_idx = head;
    let mut best_prio = 255u8;

    let mut i = head;
    while i != tail {
        let task_id = RING_BUF[i % RING_CAP].load(Ordering::Relaxed);
        let slot = table::table_slot(task_id);
        let base = table::TASK_TABLE[slot].priority.load(Ordering::Relaxed);
        let effective = if AGING_ENABLED.load(Ordering::Relaxed) {
            let enq = table::TASK_TABLE[slot].enqueue_tick.load(Ordering::Relaxed);
            let waited = now.saturating_sub(enq);
            let interval = AGING_TICKS_PER_LEVEL.load(Ordering::Relaxed).max(1);
            let boost = (waited / interval).min(255) as u8;
            if boost > 0 {
                super::stats::record_aging_boost(waited);
            }
            base.saturating_sub(boost)
        } else {
            base
        };
        if effective < best_prio {
            best_prio = effective;
            best_idx = i;
        }
        i = i.wrapping_add(1);
    }

    let best_id = RING_BUF[best_idx % RING_CAP].load(Ordering::Relaxed);

    // Compact ring: shift entries after best_idx one slot toward head.
    let new_tail = tail.wrapping_sub(1);
    let mut j = best_idx;
    while j != new_tail {
        let next_val = RING_BUF[j.wrapping_add(1) % RING_CAP].load(Ordering::Relaxed);
        RING_BUF[j % RING_CAP].store(next_val, Ordering::Relaxed);
        j = j.wrapping_add(1);
    }
    RING_TAIL.store(new_tail, Ordering::Release);

    Some(TaskId(best_id))
}

pub fn ring_contains_task_inner(id: TaskId) -> bool {
    let head = RING_HEAD.load(Ordering::Relaxed);
    let tail = RING_TAIL.load(Ordering::Acquire);

    let mut idx = head;
    while idx != tail {
        if RING_BUF[idx % RING_CAP].load(Ordering::Relaxed) == id.0 {
            return true;
        }
        idx = idx.wrapping_add(1);
    }
    false
}

pub fn runnable_count() -> usize {
    let head = RING_HEAD.load(Ordering::Relaxed);
    let tail = RING_TAIL.load(Ordering::Relaxed);
    tail.wrapping_sub(head)
}

// ---- priority management ----------------------------------------------------

pub fn set_task_priority(id: TaskId, new_prio: u8) -> bool {
    super::with_interrupts_masked(|| {
        let slot = table::table_slot(id.0);
        let t = &table::TASK_TABLE[slot];
        if t.id.load(Ordering::Relaxed) != id.0 {
            return false;
        }
        if TaskState::from_u8(t.state.load(Ordering::Acquire)) != TaskState::Ready {
            return false;
        }
        t.priority.store(new_prio, Ordering::Relaxed);
        t.enqueue_tick.store(super::SCHED_TICKS.load(Ordering::Relaxed), Ordering::Relaxed);
        t.slice.store(slice_for_priority(new_prio), Ordering::Relaxed);
        true
    })
}

pub fn set_task_priority_any(id: TaskId, new_prio: u8) -> bool {
    super::with_interrupts_masked(|| {
        let slot = table::table_slot(id.0);
        let t = &table::TASK_TABLE[slot];
        if t.id.load(Ordering::Relaxed) != id.0 {
            return false;
        }
        let state = TaskState::from_u8(t.state.load(Ordering::Acquire));
        if state == TaskState::Empty {
            return false;
        }
        t.priority.store(new_prio, Ordering::Relaxed);
        t.slice.store(slice_for_priority(new_prio), Ordering::Relaxed);
        if state == TaskState::Ready {
            t.enqueue_tick.store(super::SCHED_TICKS.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        true
    })
}

pub fn task_priority(id: TaskId) -> u8 {
    let slot = table::table_slot(id.0);
    if table::TASK_TABLE[slot].id.load(Ordering::Relaxed) == id.0 {
        table::TASK_TABLE[slot].priority.load(Ordering::Relaxed)
    } else {
        128
    }
}

pub fn aging_enabled() -> bool {
    AGING_ENABLED.load(Ordering::Relaxed)
}

pub fn aging_ticks_per_level() -> u64 {
    AGING_TICKS_PER_LEVEL.load(Ordering::Relaxed)
}
