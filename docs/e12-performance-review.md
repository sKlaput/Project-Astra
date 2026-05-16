# E12 Performance and Gaming Review

Date: 2026-04-05

## Known Latency Sources

Primary latency sources identified in current kernel baseline:

1. Timer granularity and sleep jitter.
2. Scheduler dispatch and requeue overhead under mixed runnable/idle windows.
3. Rendering path contention between GUI operations and other kernel work.
4. I/O path contention (serial/logging and future block/network traffic).

## Background Work Minimization Rules

Rules for E12 onward:

1. Bound background work per tick.
2. Prefer short, incremental state-machine steps over long loops.
3. Keep diagnostic logging deterministic and bounded.
4. Avoid unbounded polling in hot paths.
5. Preserve explicit marker-based observability for every optimization.

## Graphics and Scheduler Review Against Gaming Goals

- Target frame pacing direction: 60 FPS baseline (16 ms frame budget).
- Scheduler should avoid long contiguous background bursts during interactive windows.
- GUI and compositor operations should remain bounded and preempt-friendly.
- Any aggressive optimization must keep diagnostics available and deterministic.

## First Game Mode Concept (v0)

Game Mode v0 policy concept:

- Frame budget target: 16 ms.
- Prefer foreground interactive tasks over background service tasks.
- Cap background dispatch pressure in a frame window.
- Defer heavy maintenance work unless spare budget is available.
- Keep explicit telemetry for frame-budget misses and background load.

## Correctness and Diagnosability Guardrail

Performance changes must not reduce fault visibility or weaken deterministic PASS/FAIL marker evidence.

## Action Items

1. Add bounded frame-window scheduler telemetry for foreground/background split. Status: closed in Slice 2.
2. Add lightweight GUI pacing telemetry markers. Status: closed in Slice 3.
3. Add net/IO background throttling hooks for Game Mode policy. Status: closed in Slice 4.
4. Evaluate timer tick configuration against frame pacing target. Status: closed in Slice 5.
5. Keep strict all-lane regression gate green for all E12 slices. Status: ongoing and currently green through Slice 5.
