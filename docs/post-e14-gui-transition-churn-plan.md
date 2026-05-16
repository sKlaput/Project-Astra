# Post-E14 GUI Transition Churn Plan

Date: 2026-04-06
Scope: Post-E14 Slice 14

## Goal

Define deterministic behavior policy for repeated focus and input transition churn.

## Current Baseline (Verified)

- GUI recovery escalation markers are stable and validated.
- GUI event ordering, focus recovery, and routing contracts remain stable.
- Window manager and GUI demo probes report consistent PASS state.
- Strict all-lane regression gate remains green.

## Transition Churn Rules

1. Arbitration, routing, and runtime readiness are prerequisites for churn stability.
2. Churn-path readiness requires all app surfaces to remain completed in the same pass.
3. Transition churn contract is PASS only when stability and churn-path markers both PASS.

## Follow-On Stages

1. Transition churn baseline (this slice)
- Validate stability and churn-path marker contracts.

2. Escalation cooldown baseline
- Add deterministic marker checks for cooldown behavior after escalation events.

3. Churn stress baseline
- Stage marker checks for extended repeated transition churn behavior.

## Slice 14 Marker Contract

Required markers:

- gui-churn: baseline PASS
- gui-churn: stability PASS
- gui-churn: churn-path PASS
- gui-churn: poste14-contract PASS

## Exit Condition

Slice 14 is complete when focused transition-churn validator and strict all-lane gate both PASS.
