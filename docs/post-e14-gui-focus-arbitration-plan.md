# Post-E14 GUI Focus Arbitration Plan

Date: 2026-04-06
Scope: Post-E14 Slice 9

## Goal

Define deterministic foreground focus arbitration policy and ownership markers for app transition paths.

## Current Baseline (Verified)

- GUI runtime composition ownership contract is stable and validated.
- Window manager and GUI demo probes report consistent PASS markers.
- App lifecycle ownership markers are stable and validated.
- Strict all-lane regression gate remains green.

## Focus Arbitration Rules

1. Settings remains default foreground owner in this baseline.
2. Terminal/editor/file manager remain eligible foreground candidates.
3. Arbitration readiness requires WM + demo readiness and completed app lifecycle ownership.

## Follow-On Stages

1. Focus arbitration baseline (this slice)
- Validate owner policy and arbitration-path readiness markers.

2. Input routing baseline
- Add deterministic marker checks for input dispatch ownership under focus transitions.

3. Focus recovery hardening
- Stage fallback behavior markers for invalid/failed focus transitions.

## Slice 9 Marker Contract

Required markers:

- `gui-focus: baseline PASS`
- `gui-focus: owner PASS`
- `gui-focus: arbitration-path PASS`
- `gui-focus: poste14-contract PASS`

## Exit Condition

Slice 9 is complete when focused focus-arbitration validator and strict all-lane gate both PASS.
