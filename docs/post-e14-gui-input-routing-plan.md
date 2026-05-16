# Post-E14 GUI Input Routing Plan

Date: 2026-04-06
Scope: Post-E14 Slice 10

## Goal

Define deterministic input-routing ownership policy under focus transitions.

## Current Baseline (Verified)

- GUI focus arbitration ownership contract is stable and validated.
- GUI demo and window-manager probes report consistent PASS state.
- Terminal/editor/file-manager/settings app probes are complete and stable.
- Strict all-lane regression gate remains green.

## Input Routing Rules

1. Focus ownership readiness is a prerequisite for input routing PASS.
2. Terminal/editor/file manager/settings each expose a deterministic routing readiness signal.
3. Input routing contract is PASS only when owner policy and routing matrix both PASS.

## Follow-On Stages

1. Input routing baseline (this slice)
- Validate ownership and routing-path marker contracts.

2. Focus recovery baseline
- Add deterministic marker checks for focus fallback behavior after route failures.

3. Event ordering hardening
- Stage stricter event-order markers for cross-app focus/input transitions.

## Slice 10 Marker Contract

Required markers:

- `gui-input: baseline PASS`
- `gui-input: ownership PASS`
- `gui-input: routing-path PASS`
- `gui-input: poste14-contract PASS`

## Exit Condition

Slice 10 is complete when focused input-routing validator and strict all-lane gate both PASS.
