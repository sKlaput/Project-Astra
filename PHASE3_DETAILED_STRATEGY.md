# Phase 3: Detailed Implementation Strategy

## Current Dispatcher Analysis

### RING_BUF Structure (Global)
```rust
static RING_BUF: [AtomicU64; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
static RING_HEAD: AtomicUsize = 0;  // Dequeue pointer
static RING_TAIL: AtomicUsize = 0;  // Enqueue pointer
```

### Key Algorithm: dequeue_next_inner()
1. Scan entire ring from HEAD to TAIL
2. Find task with lowest effective priority
3. Consider aging: boost = (now - enqueue_time) / aging_interval
4. Shift ring entries to compact after removal
5. Return highest-priority task

### Key Algorithm: enqueue_task_inner()
1. Check if queue full (TAIL - HEAD >= RING_CAP)
2. Add task ID to RING_BUF[TAIL % RING_CAP]
3. Record enqueue_tick for aging calculation
4. Increment TAIL with Release ordering

## Phase 3 Design: Per-Core Queues

### New Data Structure: Move Queue to PerCpuData

Extend `kernel/src/arch/x86_64/percpu.rs`:
```rust
pub struct PerCpuData {
    // ... existing fields (cpu_id, self_ptr, etc) ...
    
    // Phase 3: Per-core queue state
    pub queue_head: AtomicUsize,
    pub queue_tail: AtomicUsize,
    pub queue_buf: [AtomicU64; QUEUE_SIZE],  // 8 elements per CPU
}
```

### Why PerCpuData?
- Already per-CPU via GSBASE
- Accessible via `unsafe { percpu::this_cpu().queue_head }`
- No new global static needed
- Per-CPU isolation built-in

## Phase 3 Implementation: Step by Step

### Step 1: Extend PerCpuData (percpu.rs)
Add queue fields to PerCpuData struct:
```rust
pub queue_head: AtomicUsize,       // Dequeue pointer for this CPU
pub queue_tail: AtomicUsize,       // Enqueue pointer for this CPU
pub queue_buf: [AtomicU64; 8],     // Task IDs for this CPU's queue
pub queue_lock: AtomicBool,        // Lock for queue modifications
```

Initialize in PerCpuData::new():
```rust
queue_head: AtomicUsize::new(0),
queue_tail: AtomicUsize::new(0),
queue_buf: [...],
queue_lock: AtomicBool::new(false),
```

### Step 2: Create Per-Core Dispatch API (dispatch.rs)

```rust
// Per-core enqueue (called when task becomes ready)
pub fn enqueue_task_to_cpu(task_id: TaskId, cpu_id: u32) -> bool {
    // Get PerCpuData for target CPU
    // Acquire lock on that CPU's queue
    // Add task to queue_buf
    // Release lock
}

// Per-core dequeue with work-stealing
pub fn dequeue_next_for_cpu(cpu_id: u32) -> Option<TaskId> {
    // Try local queue first
    let local = try_dequeue_local(cpu_id);
    if local.is_some() {
        return local;
    }
    
    // Local queue empty, try stealing from others
    for other_cpu in (0..CPU_COUNT) {
        if other_cpu == cpu_id { continue; }
        if let Some(task) = try_steal_from(other_cpu) {
            return Some(task);
        }
    }
    
    None  // All queues empty
}

// Helper: try to dequeue from specific CPU's local queue
fn try_dequeue_local(cpu_id: u32) -> Option<TaskId> {
    // Get PerCpuData for cpu_id
    // Scan that CPU's queue for highest-priority task
    // Use same priority + aging logic as before
    // Compact and return
}

// Helper: try to steal one task from another CPU
fn try_steal_from(cpu_id: u32) -> Option<TaskId> {
    // Try to acquire lock on target CPU's queue
    // If locked, return None (don't wait - try another)
    // If obtained, take highest-priority task
    // Release lock and return
}
```

### Step 3: Modify Scheduler::run() (scheduler/mod.rs)

Current (Phase 2.3):
```rust
pub fn run() -> ! {
    let cpu_id = unsafe { percpu::cpu_id() };
    loop {
        if !dispatch_once() {  // Uses global dequeue_next_inner()
            idle::idle_until(...);
        }
    }
}
```

New (Phase 3):
```rust
pub fn run() -> ! {
    let cpu_id = unsafe { percpu::cpu_id() };
    loop {
        if !dispatch_once_per_cpu(cpu_id) {  // Per-core with work-stealing
            idle::idle_until(...);
        }
    }
}

fn dispatch_once_per_cpu(cpu_id: u32) -> bool {
    // Use per-core dequeue with work-stealing
    match dispatch::dequeue_next_for_cpu(cpu_id) {
        Some(task_id) => {
            // Execute task
            true
        }
        None => {
            false  // No tasks, will idle
        }
    }
}
```

### Step 4: Task Spawning (scheduler/mod.rs)

Current:
```rust
pub fn spawn(...) -> TaskId {
    let id = get_next_task_id();
    // Add to global queue
    enqueue_task_inner(id);
    id
}
```

New (Phase 3):
```rust
pub fn spawn(...) -> TaskId {
    let id = get_next_task_id();
    let cpu_id = unsafe { percpu::cpu_id() };
    // Add to spawning CPU's queue
    enqueue_task_to_cpu(id, cpu_id);
    id
}
```

## Phase 3 Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│ scheduler::run() - Per-core scheduler loop              │
│   (Each CPU runs independently)                         │
└─────────────────────────────────────────────────────────┘
              ↓
┌─────────────────────────────────────────────────────────┐
│ dispatch::dequeue_next_for_cpu(cpu_id)                 │
│   1. Try local queue                                    │
│   2. If empty, try work-stealing from other CPUs       │
└─────────────────────────────────────────────────────────┘
              ↓
┌─────────────────────────────────────────────────────────┐
│ PerCpuData.queue_buf[0..8]                             │
│   (8 task slots per CPU, FIFO with priority sorting)   │
└─────────────────────────────────────────────────────────┘
```

## Ordering & Atomicity

### Lock-Free Per-CPU Queues
- Each CPU owns its queue (GSBASE isolation)
- No locks needed for single-CPU access
- Head/tail are atomics for ordering

### Work-Stealing Locking
- Use simple spinlock (`AtomicBool`) on target queue
- Stealing CPU tries acquire, gives up if locked
- Prevents deadlock: never wait for lock

### Memory Ordering
- Enqueue: Release ordering on TAIL (flushes entries)
- Dequeue: Acquire on HEAD (sees enqueued entries)
- Steal: Relaxed for lock (not ordered w.r.t. queue data)

## Expected Behavior: Work-Stealing Example

CPU 0: [Task A, Task B, Task C]
CPU 1: [Task D]
CPU 2: []  (idle)

```
Time 1: CPU 0 executes A
        CPU 1 executes D
        CPU 2 dequeues from local (empty)
                → tries steal from CPU 0 (success! gets B)

Time 2: CPU 0 executes next task (C from its queue)
        CPU 1 executes next task (D finishes)
        CPU 2 executes stolen task B

Time 3: CPU 1 dequeues from local (empty)
                → tries steal from CPU 0 (success! gets C)
        etc...
```

Result: Automatic load balancing without migration overhead!

## Testing Strategy

### Test 1: Basic Per-Core Queue
- Single CPU case (should work like before)
- Verify enqueue/dequeue on same CPU

### Test 2: Work-Stealing
- CPU 0: 3 tasks, CPU 1: 0 tasks
- CPU 1 should steal from CPU 0 immediately

### Test 3: Load Balancing
- CPU 0: high-priority task
- CPU 1: low-priority task
- After steal, both CPUs work on good mix

### Test 4: Priority Still Works
- Multiple CPUs with mixed priorities
- Highest-priority task should run first (from any queue)

## Estimated Implementation Time
- Modify PerCpuData: 30 min
- Implement per-core dispatch: 1.5 hours
- Work-stealing: 1 hour
- Integration & testing: 1.5 hours
- Debugging: 1.5 hours
**Total: ~7 hours**

## Summary

Phase 3 transforms the scheduler from:
- **Global queue** (single contention point, simple)
→ **Per-core queues** (low contention, work-stealing)

This is **modern multicore OS design**:
- Each CPU has local ready queue (cache-friendly)
- Idle CPUs steal from busy CPUs (load balancing)
- No complex centralized scheduler (scalable)
- Priority still respected (correctness)

Ready to implement?
