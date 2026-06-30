# Phase 3 Step 5: Testing & Load Balancing Verification

## Overview
Phase 3.5 validates the multicore per-core queue scheduler with work-stealing load balancing
across 2-core and 4-core configurations.

## Test Scenarios

### 1. Two-Core Testing (QEMU -smp 2)

#### Setup
`ash
# Build and run with 2 CPUs
cargo build --release
qemu-system-x86_64 -smp 2 \
  -kernel target/x86_64-unknown-none/release/kernel \
  -m 256M -enable-kvm -serial stdio
`

#### Expected Behavior
- **BSP startup**: CPU 0 initializes global state, loads AP startup code
- **AP startup**: CPU 1 executes ap_entry(), initializes GDT/TSS/GSBASE/IDT/scheduler
- **Scheduler activation**: Both CPUs enter independent scheduler::run() loops
- **Queue management**: Each CPU maintains its own 8-task queue
- **Work-stealing**: When CPU 0's queue empties, it steals from CPU 1; vice versa

#### Validation Checks
1. Both CPUs print startup messages (gdt, percpu initialization)
2. No deadlocks or hangs (scheduler loop runs continuously)
3. Tasks execute on assigned CPUs (check task state transitions)
4. Work-stealing activates when local queue empties

### 2. Four-Core Testing (QEMU -smp 4)

#### Setup
`ash
qemu-system-x86_64 -smp 4 \
  -kernel target/x86_64-unknown-none/release/kernel \
  -m 256M -enable-kvm -serial stdio
`

#### Expected Behavior
- **Startup sequence**: BSP initializes, then APs 1-3 execute ap_entry()
- **Per-core isolation**: Each CPU has independent queue, GSBASE, and scheduler state
- **Load distribution**: 4 tasks × 4 CPUs = better parallelism opportunity
- **Work-stealing chain**: CPU 0→1→2→3→0 (round-robin stealing pattern)

#### Validation Checks
1. All 4 CPUs initialize successfully (4 GDT messages)
2. Task distribution across queues (verify via debug output)
3. No race conditions in queue operations
4. Work-stealing doesn't create deadlocks (spinlock timeout-free)

## Debugging Tools

### Serial Output Analysis
- Look for "gdt: per-core AP GDT loaded lapic=" messages (one per AP)
- Look for "percpu: CPU X per-core data initialized" messages
- Check for error messages (usually indicate synchronization issues)

### QEMU Monitor Commands
`
# View CPU register state
info registers

# View interrupt status
info pic
info ioapic

# Single-step execution (if needed)
stepi
`

### Kernel Debug Output
Add temporary printfs in:
- dequeue_next_per_cpu(): Log when stealing occurs
- 	ry_work_steal(): Log which CPU steals from which
- nqueue_task_to_cpu(): Log task enqueue operations

Example instrumentation:
`ust
// In dispatch.rs try_work_steal()
if best_id != 0 {
    crate::serial::write_str("cpu");
    crate::serial::write_u32(current_cpu);
    crate::serial::write_str(": stealing task ");
    crate::serial::write_u64(best_id);
    crate::serial::write_str(" from cpu");
    crate::serial::write_u32(target_cpu);
    crate::serial::write_line("");
}
`

## Performance Metrics

### Key Metrics to Observe
1. **Dispatch rate**: Tasks/second dispatched from all CPUs
2. **Work-steal frequency**: % of dequeues that result in work-steal vs local dequeue
3. **Scheduler efficiency**: Time in scheduler loop vs time in task execution
4. **Queue utilization**: Average queue depth across CPUs

### Measurement Points
Add statistics collection in:
- dequeue_next_per_cpu(): Count local vs stolen dequeues
- 	ry_work_steal(): Count successful steals per CPU
- 	ry_dequeue_local(): Count queue empty scenarios

## Known Limitations & Future Work

### Current Phase 3 Limitations
1. **No NUMA awareness**: Work-stealing is blind to cache locality
2. **Fixed round-robin**: Stealing order is deterministic, could be optimized
3. **No backpressure**: Stealing CPU doesn't signal source, may cause thrashing
4. **Limited queue size**: 8 tasks/CPU limits effective parallelism to ~32 tasks max

### Phase 4 (USB HID) Dependency
Task dispatch testing will be more robust once USB HID is implemented:
- Can use keyboard interrupt to trigger task creation
- Better integration testing with real I/O patterns

### Future Optimizations (v0.5+)
1. **NUMA-aware stealing**: Steal from nearby NUMA nodes first
2. **Adaptive stealing**: Backoff when target queue is contested
3. **Queue resizing**: Dynamic queue capacity based on workload
4. **Stealing prioritization**: Steal highest-priority tasks first

## Test Cases Summary

| Scenario | CPUs | Expected | Status |
|----------|------|----------|--------|
| Single CPU | 1 | All tasks on CPU 0 | Not tested (Phase 3 requires 2+) |
| Dual core | 2 | Tasks split across 2 queues | **TODO: Test** |
| Quad core | 4 | Tasks split across 4 queues | **TODO: Test** |
| Load imbalance | 2 | Work-stealing balances load | **TODO: Test** |
| Queue full | 2 | Queue full scenario (8+ tasks) | **TODO: Test** |

## Next Steps

1. ✓ Phase 3.1: PerCpuData architecture (COMPLETE)
2. ✓ Phase 3.2: Per-core queue enqueue/dequeue (COMPLETE)
3. ✓ Phase 3.3: Scheduler integration (COMPLETE)
4. ✓ Phase 3.4: Work-stealing (COMPLETE)
5. **→ Phase 3.5: Testing (IN PROGRESS)**
6. Phase 4: USB HID Input (8 hours)
7. Phase 5: Real Hardware Testing (6 hours)

