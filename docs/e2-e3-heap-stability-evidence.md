# E2/E3 Heap Stability Evidence

## Scope

This note captures post-E1 bring-up validation for:
- E2 frame allocator + paging infrastructure (paging activation deferred)
- E3 heap allocator integration with deterministic runtime checks

## Current Configuration

- Paging activation: deferred (no CR3/CR0 paging enable step in boot path)
- HHDM source: runtime Limine HHDM response (not hardcoded)
- Frame allocator policy: skips frames below `1 MiB`
- QEMU run mode: `-no-reboot` enabled for deterministic fault isolation
- Heap ladder gate: currently `HEAP_TEST_HALT_AFTER_STEP = None` (full run)

## Deterministic Heap Ladder

The kernel emits strict markers in order:

1. `[HEAP-1] raw alloc OK`
2. `[HEAP-2] Box OK`
3. `[HEAP-3] Vec OK`
4. `[HEAP-4] String OK`

Final status line after full ladder:

- `heap: allocated 196 bytes`

## Evidence Logs

Three consecutive full boots (with full ladder, no halt gate) produced identical key markers:

- `build/heap-full-runA.log`
- `build/heap-full-runB.log`
- `build/heap-full-runC.log`

Shared key lines observed in all three logs:

- `paging: hhdm offset=18446603336221196288`
- `frame_allocator: initialized with 116716 frames (455 MB)`
- `[HEAP-1] raw alloc OK`
- `[HEAP-2] Box OK`
- `[HEAP-3] Vec OK`
- `[HEAP-4] String OK`
- `heap: allocated 196 bytes`

## Isolated Ladder Runs (debug checkpoints)

- Step-2 isolate log: `build/heap-ladder-step2.log`
- Step-3 isolate log: `build/heap-ladder-step3.log`
- Step-4 isolate log: `build/heap-ladder-step4.log`

Each isolate reached its target marker and halted intentionally.

## Interpretation

- Heap allocator path is currently stable for raw alloc + `Box` + `Vec` + `String` on repeated boots.
- No intermittent divergence observed in the collected full-run logs.
- Paging structures compile and initialize, but paging activation remains intentionally out of scope for this checkpoint.
