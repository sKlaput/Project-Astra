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


// ============================================================================
// Phase 3: Per-Core Task Queue with Work-Stealing
// ============================================================================

/// Enqueue task to a specific CPU's queue
pub fn enqueue_task_to_cpu(id: TaskId, cpu_id: u32) -> bool {
    unsafe {
        // Get target CPU's per-core data via LAPIC ID
        // Note: In Phase 3, we'll use a CPU ID mapping array
        // For now, use GSBASE if same CPU, else would need global access
        
        // Simplified: for same CPU enqueue
        let percpu = crate::arch::x86_64::percpu::this_cpu();
        if percpu.cpu_id != cpu_id {
            // Different CPU - would need per-CPU struct array
            // Deferred to Phase 3 refinement
            return false;
        }
        
        let tail = percpu.queue_tail.load(Ordering::Relaxed);
        let head = percpu.queue_head.load(Ordering::Relaxed);
        let cap = crate::arch::x86_64::percpu::queue_capacity() as u32;
        
        // Check if queue full
        if tail.wrapping_sub(head) >= cap {
            return false;
        }
        
        // Add to queue
        let slot = (tail % cap) as usize;
        crate::arch::x86_64::percpu::queue_set(slot, id.0);
        
        // Record enqueue tick for aging
        table::TASK_TABLE[table::table_slot(id.0)]
            .enqueue_tick
            .store(super::SCHED_TICKS.load(Ordering::Relaxed), Ordering::Relaxed);
        
        // Update tail with Release ordering
        percpu.queue_tail.store(tail.wrapping_add(1), Ordering::Release);
        super::IDLE_DECISION_SEEN.store(false, Ordering::Relaxed);
        
        true
    }
}

/// Dequeue highest-priority task from current CPU's queue (per-core)
pub fn dequeue_next_per_cpu() -> Option<TaskId> {
    unsafe {
        let percpu = crate::arch::x86_64::percpu::this_cpu();

        // Try local queue first
        if let Some(task) = try_dequeue_local(percpu) {
            return Some(task);
        }

        let cpu_id = unsafe { crate::arch::x86_64::percpu::cpu_id() };
        if let Some(task) = unsafe { try_work_steal(cpu_id) } {
            return Some(task);
        }

        // Fall back to the global ring buffer.  spawn_task* always enqueues
        // there, so without this fallback nothing ever dispatches.
        dequeue_next_inner()
    }
}

/// Try to dequeue from local CPU's queue
unsafe fn try_dequeue_local(percpu: &mut crate::arch::x86_64::percpu::PerCpuData) -> Option<TaskId> {
    let head = percpu.queue_head.load(Ordering::Relaxed);
    let tail = percpu.queue_tail.load(Ordering::Acquire);
    let now = super::SCHED_TICKS.load(Ordering::Relaxed);
    let cap = crate::arch::x86_64::percpu::queue_capacity() as u32;
    
    if head == tail {
        return None;  // Queue empty
    }
    
    // Find highest-priority task (same logic as global queue)
    let mut best_idx = head;
    let mut best_prio = 255u8;
    
    let mut i = head;
    while i != tail {
        let slot_idx = (i % cap) as usize;
        let task_id = percpu.queue_buf[slot_idx].load(Ordering::Relaxed);
        
        let task_slot = table::table_slot(task_id);
        let base = table::TASK_TABLE[task_slot].priority.load(Ordering::Relaxed);
        
        let effective = if AGING_ENABLED.load(Ordering::Relaxed) {
            let enq = table::TASK_TABLE[task_slot].enqueue_tick.load(Ordering::Relaxed);
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
    
    // Get the best task
    let best_slot = (best_idx % cap) as usize;
    let best_id = percpu.queue_buf[best_slot].load(Ordering::Relaxed);
    
    // Compact: shift entries after best_idx one slot toward head
    let new_tail = tail.wrapping_sub(1);
    let mut j = best_idx;
    while j != new_tail {
        let next_slot = ((j.wrapping_add(1)) % cap) as usize;
        let current_slot = (j % cap) as usize;
        let next_val = percpu.queue_buf[next_slot].load(Ordering::Relaxed);
        percpu.queue_buf[current_slot].store(next_val, Ordering::Relaxed);
        j = j.wrapping_add(1);
    }
    
    percpu.queue_tail.store(new_tail, Ordering::Release);
    
    Some(TaskId(best_id))
}

/// Try to steal a task from another CPU's queue (Phase 3.4: Work-Stealing)
unsafe fn try_work_steal(current_cpu: u32) -> Option<TaskId> {
    let max_cpus = 256;
    let now = super::SCHED_TICKS.load(Ordering::Relaxed);
    
    // Try CPUs in round-robin order
    for offset in 1..max_cpus {
        let target_cpu = ((current_cpu as usize + offset) % max_cpus) as u32;
        
        // Try to get target CPU's data
        let target_percpu = unsafe { crate::arch::x86_64::percpu::get_percpu_data(target_cpu) };
        let Some(target_percpu) = target_percpu else {
            continue;
        };
        
        // Non-blocking lock acquire
        let acquired = target_percpu.queue_lock.compare_exchange_weak(
            0, 1, Ordering::Acquire, Ordering::Relaxed
        ).is_ok();
        
        if !acquired {
            continue;  // Lock held, skip
        }
        
        let head = target_percpu.queue_head.load(Ordering::Relaxed);
        let tail = target_percpu.queue_tail.load(Ordering::Acquire);
        let cap = crate::arch::x86_64::percpu::queue_capacity() as u32;
        
        if head == tail {
            target_percpu.queue_lock.store(0, Ordering::Release);
            continue;  // Queue empty
        }
        
        // Find best task
        let mut best_idx = head;
        let mut best_prio = 255u8;
        
        let mut i = head;
        while i != tail {
            let slot_idx = (i % cap) as usize;
            let task_id = target_percpu.queue_buf[slot_idx].load(Ordering::Relaxed);
            
            let task_slot = table::table_slot(task_id);
            let base = table::TASK_TABLE[task_slot].priority.load(Ordering::Relaxed);
            
            let effective = if AGING_ENABLED.load(Ordering::Relaxed) {
                let enq = table::TASK_TABLE[task_slot].enqueue_tick.load(Ordering::Relaxed);
                let waited = now.saturating_sub(enq);
                let interval = AGING_TICKS_PER_LEVEL.load(Ordering::Relaxed).max(1);
                let boost = (waited / interval).min(255) as u8;
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
        
        // Steal the task
        let best_slot = (best_idx % cap) as usize;
        let best_id = target_percpu.queue_buf[best_slot].load(Ordering::Relaxed);
        
        // Compact queue
        let new_tail = tail.wrapping_sub(1);
        let mut j = best_idx;
        while j != new_tail {
            let next_slot = ((j.wrapping_add(1)) % cap) as usize;
            let current_slot = (j % cap) as usize;
            let next_val = target_percpu.queue_buf[next_slot].load(Ordering::Relaxed);
            target_percpu.queue_buf[current_slot].store(next_val, Ordering::Relaxed);
            j = j.wrapping_add(1);
        }
        
        target_percpu.queue_tail.store(new_tail, Ordering::Release);
        target_percpu.queue_lock.store(0, Ordering::Release);
        
        return Some(TaskId(best_id));
    }
    
    None
}
