# Post-E14 GUI App Lifecycle Ownership Plan

Date: 2026-04-06
Scope: Post-E14 Slice 7

## Goal

Formalize foreground/background ownership transitions for GUI app probes with deterministic marker evidence.

## Current Baseline (Verified)

- Terminal/editor/file-manager/settings probes pass and emit deterministic lifecycle-related markers.
- Settings probe already demonstrates placeholder foreground/background/foreground transition intent.
- Strict regression gate remains green across stable and deep lanes.

## Ownership Rules for This Stage

1. Settings app is current foreground-transition owner.
2. Terminal/editor/file manager are background-capable peer apps.
3. Ownership transition evidence must be deterministic and script-validated.

## Staged Follow-On

1. Lifecycle ownership baseline (this slice)
- Validate app readiness map and foreground-transition ownership.
- Validate lifecycle transition marker chain for settings flow.

2. Runtime composition baseline
- Add ownership contract between window manager and app lifecycle transitions.
- Add deterministic marker checks for composition handoff boundaries.

3. GUI policy hardening
- Add policy assertions for app focus arbitration and fallback behavior.

## Slice 7 Marker Contract

Required markers:

- `gui-life: baseline PASS`
- `gui-life: ownership-map PASS`
- `gui-life: transitions PASS`
- `gui-life: poste14-contract PASS`

## Exit Condition

Slice 7 is complete when focused lifecycle validator and strict all-lane gate both PASS.
