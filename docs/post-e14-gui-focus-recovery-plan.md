# Post-E14 GUI Focus Recovery Plan

Date: 2026-04-06
Scope: Post-E14 Slice 11

## Goal

Define deterministic fallback ownership policy for failed or invalid focus transitions.

## Current Baseline (Verified)

- GUI focus arbitration and input routing ownership contracts are stable and validated.
- Window manager and GUI demo probes report consistent PASS state.
- Settings lifecycle ownership markers remain stable and validated.
- Strict all-lane regression gate remains green.

## Focus Recovery Rules

1. Settings remains deterministic fallback owner for focus recovery.
2. Recovery-path readiness requires WM + demo readiness plus input-routing readiness.
3. Focus recovery contract is PASS only when fallback ownership and recovery path are both PASS.

## Follow-On Stages

1. Focus recovery baseline (this slice)
- Validate fallback-owner and recovery-path marker contracts.

2. Event ordering hardening baseline
- Add deterministic marker checks for focus/input event ordering under transition churn.

3. Recovery escalation baseline
- Stage marker checks for recovery escalation policy after repeated transition failures.

## Slice 11 Marker Contract

Required markers:

- `gui-recover: baseline PASS`
- `gui-recover: fallback-owner PASS`
- `gui-recover: recovery-path PASS`
- `gui-recover: poste14-contract PASS`

## Exit Condition

Slice 11 is complete when focused focus-recovery validator and strict all-lane gate both PASS.
