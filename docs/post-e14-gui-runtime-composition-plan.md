# Post-E14 GUI Runtime Composition Plan

Date: 2026-04-06
Scope: Post-E14 Slice 8

## Goal

Define deterministic ownership contracts for window-manager and app composition handoff paths.

## Current Baseline (Verified)

- GUI demo and window-manager probes complete successfully.
- App lifecycle ownership markers are stable and validated.
- Strict all-lane regression gate remains green.

## Composition Ownership Rules

1. Window manager is composition owner.
2. Settings lifecycle transitions are a required handoff prerequisite.
3. Terminal/editor/file-manager/settings app probes must be completed before composition path PASS.

## Follow-On Stages

1. Composition ownership baseline (this slice)
- Validate WM handoff and app composition path readiness.

2. Focus arbitration baseline
- Add deterministic focus arbitration markers for foreground app changes.

3. Runtime policy hardening
- Stage stricter composition policy checks and fallback behavior markers.

## Slice 8 Marker Contract

Required markers:

- `gui-comp: baseline PASS`
- `gui-comp: handoff PASS`
- `gui-comp: composition-path PASS`
- `gui-comp: poste14-contract PASS`

## Exit Condition

Slice 8 is complete when focused composition validator and strict all-lane gate both PASS.
