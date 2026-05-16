# Post-E14 GUI Event Ordering Hardening Plan

Date: 2026-04-06
Scope: Post-E14 Slice 12

## Goal

Define deterministic focus and input event ordering policy under transition churn.

## Current Baseline (Verified)

- GUI focus recovery ownership contract is stable and validated.
- GUI focus arbitration and input routing ownership contracts remain stable.
- Window manager and GUI demo probes report consistent PASS state.
- Strict all-lane regression gate remains green.

## Event Ordering Rules

1. Focus ownership readiness must be satisfied before routing readiness.
2. Event-ordering readiness requires both ownership/routing policy readiness and runtime readiness.
3. Event ordering contract is PASS only when policy and ordered-path markers are both PASS.

## Follow-On Stages

1. Event ordering hardening baseline (this slice)
- Validate policy and event-ordering-path marker contracts.

2. Recovery escalation baseline
- Add deterministic marker checks for escalation policy after repeated transition failures.

3. Transition churn baseline
- Stage marker checks for repeated focus/input transition churn behavior.

## Slice 12 Marker Contract

Required markers:

- `gui-order: baseline PASS`
- `gui-order: policy PASS`
- `gui-order: event-ordering-path PASS`
- `gui-order: poste14-contract PASS`

## Exit Condition

Slice 12 is complete when focused event-ordering validator and strict all-lane gate both PASS.
