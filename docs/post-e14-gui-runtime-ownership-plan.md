# Post-E14 GUI Runtime Ownership Plan

Date: 2026-04-06
Scope: Post-E14 Slice 6

## Goal

Move E9/E10 GUI probes toward explicit runtime ownership contracts while preserving regression stability.

## Current Baseline (Verified)

- GUI demo and window-manager probe paths execute and pass under strict gate.
- Framebuffer mapping remains policy-scoped (user context only for real mapping path).
- Deep probe modes remain available but controlled by feature toggles.

## Ownership Guardrails

1. Keep framebuffer mapping policy explicit: user-task mapping path only.
2. Preserve clean deny behavior for invalid or kernel-context map requests.
3. Keep GUI surface syscalls deterministic under invalid input checks.
4. Extend ownership contracts before adding broader GUI state complexity.

## Staged Follow-On

1. Ownership baseline (this slice)
- Validate invalid-argument behavior for GUI syscall surface.
- Validate framebuffer map ownership boundaries.

2. App-runtime ownership stage
- Define foreground/background ownership contract for GUI app probes.
- Add deterministic marker checks for app lifecycle authority boundaries.

3. Runtime composition stage
- Add clearer ownership contract for window-manager state transitions.
- Keep marker-based regression evidence in focused validator.

## Slice 6 Marker Contract

Required markers:

- `gui-own: baseline PASS`
- `gui-own: surface PASS`
- `gui-own: ownership PASS`
- `gui-own: poste14-contract PASS`

## Exit Condition

Slice 6 is complete when focused GUI ownership validator and strict all-lane gate both PASS.
