# Phase 3: Multicore Scheduler - Architecture & Planning

## Objective
Transform the kernel from single shared ready queue to per-core task queues with load balancing.
Enable optimal CPU utilization through intelligent task distribution.

## Current State (After Phase 2.3)
✓ All CPUs execute scheduler loop
✓ Single shared ready queue (8-task ring buffer)
✓ Each CPU independently selects next task from shared queue
✓ No per-CPU scheduling policies
✓ No load balancing

## Phase 3 Goals
1. Per-core task queues (instead of global)
2. Load balancing across cores
3. Work-stealing for idle CPUs
4. Priority-aware scheduling per core
5. Task affinity and pinning
6. Fair CPU distribution

## Time Estimate: 7 hours
- Design & architecture: 1 hour
- Per-core queue implementation: 2 hours
- Load balancing logic: 2 hours
- Work-stealing: 1 hour
- Testing & debugging: 1 hour

## Key Concepts

### Per-Core Task Queue
- Each CPU has its own ready queue (ring buffer)
- Reduces contention compared to global queue
- Tasks belong to a specific core (initial assignment)
- Can be moved between cores (load balancing)

### Load Balancing
- When CPU A's queue is empty, it can steal from CPU B
- Prevents one CPU overloaded while another idles
- Work-stealing: idle CPU scans others for work
- Fairness: distribute high-priority tasks evenly

### Task Affinity
- Pin task to CPU (optional, Phase 3)
- Or allow migration for load balancing
- Trade-off: cache locality vs utilization

## Current Scheduler Architecture (to understand first)

From scheduler/mod.rs:
- CURRENT_TASK: Global atomic tracking current task
- RING_BUF: 8-task ring buffer (shared)
- RING_HEAD/RING_TAIL: Queue pointers
- dispatch_once(): Pick next task from queue
- Context switching: save/restore task state

## Phase 3 Implementation Strategy

### Step 1: Per-Core Queue Data Structure
- Extend PerCpuData in percpu.rs with per-core queue state
- Queue pointers (head/tail) per CPU
- Task array per CPU (or shared with CPU affinity)

### Step 2: Extend Dispatch Module
- Change RING_BUF from global to per-core
- Modify enqueue/dequeue for per-CPU
- Backward compat with existing API

### Step 3: Load Balancing
- Idle detection: when dequeue returns None
- Work-stealing: scan other CPUs' queues
- Fairness algorithm: round-robin or priority-based

### Step 4: Integration
- Scheduler::run() uses per-core queue
- Work-stealing happens transparently
- Affinity optional (Phase 3 later task)

### Step 5: Testing
- Single CPU (should work same as before)
- Dual CPU load balancing
- Asymmetric loads (one CPU heavy, one light)
- Task migration tracking

## File Changes Required

### Primary
1. kernel/src/scheduler/dispatch.rs - Per-core queues
2. kernel/src/arch/x86_64/percpu.rs - Queue state in PerCpuData
3. kernel/src/scheduler/mod.rs - Integration with per-core queues

### Secondary
1. kernel/src/scheduler/table.rs - CPU affinity field
2. kernel/src/scheduler/context.rs - CPU ID awareness

## Data Structure Changes

### Current (Phase 2.3)
```rust
static RING_BUF: [AtomicU64; 8] = [...];  // Global queue
static RING_HEAD: AtomicUsize = 0;
static RING_TAIL: AtomicUsize = 0;
```

### New (Phase 3)
```rust
// Option 1: Per-CPU arrays
static RING_BUF_CPU0: [AtomicU64; 8] = [...];
static RING_BUF_CPU1: [AtomicU64; 8] = [...];
// ... for each CPU

// Option 2: Centralized with CPU affinity
static RING_BUF: [TaskEntry; MAX_TASKS] = [...];
// Each entry has: task_id, cpu_affinity
```

Better approach: Store queue state in PerCpuData
```rust
pub struct PerCpuData {
    // ... existing fields ...
    pub queue_head: AtomicUsize,
    pub queue_tail: AtomicUsize,
    pub queue_buf: [AtomicU64; QUEUE_SIZE],
}
```

## Work-Stealing Algorithm

```
When CPU A needs a task:
1. Try dequeue from A's queue
2. If empty, search other CPUs in order:
   - CPU B's queue
   - CPU C's queue
   - ... etc
3. If task found, steal it (move to A's queue)
4. If all empty, idle until timer or interrupt
```

## Testing Strategy

### Test 1: Single CPU (Backward compat)
- Boot with -smp 1
- Should work identically to Phase 2.3

### Test 2: Dual CPU, balanced load
- Boot with -smp 2
- Run ~4 equal-priority tasks
- Both CPUs should get tasks

### Test 3: Unbalanced load
- Boot with -smp 2
- Spawn 3 tasks on CPU 0
- Spawn 1 task on CPU 1
- Work-stealing should balance them

### Test 4: Task completion
- Tasks finishing should trigger rebalancing
- New tasks should be distributed fairly

### Test 5: Priority handling
- High-priority task should migrate to any CPU
- Not stuck in one CPU's queue

## Success Criteria
✓ Compiles with 0 errors
✓ Single-core still works
✓ Dual-core tasks distributed fairly
✓ Work-stealing detects and steals idle tasks
✓ No deadlocks or crashes
✓ Load balancing visible in timing

## Next Steps
1. Deep-dive into current scheduler code
2. Design per-core queue data structure
3. Implement per-core dispatch
4. Add work-stealing
5. Test with various configurations

Estimated: 7 hours to complete

Ready to begin?
