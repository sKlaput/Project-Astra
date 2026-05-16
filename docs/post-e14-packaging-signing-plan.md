# Post-E14 Packaging and Signing Planning Baseline

Date: 2026-04-06
Scope: Post-E14 Slice 5
Debt Link: TD-06 (trusted artifact packaging/signing)

## Goal

Define trusted artifact packaging and signing assumptions that can be validated deterministically before implementing full tooling.

## Current Baseline (Verified)

- Build and image assembly flow is script-driven and reproducible.
- Boot artifact set is known (`kernel`, bootloader artifacts, config).
- Regression gate remains stable across all required lanes.

## Guardrails

1. Keep signing integration non-invasive until key lifecycle and verification policy are explicit.
2. Separate packaging format decisions from cryptographic implementation details.
3. Maintain deterministic marker evidence for each planning milestone.
4. Preserve boot reproducibility while introducing trust metadata.

## Staged Plan

1. Packaging policy stage
- Define artifact bundle boundary and manifest schema.
- Define required boot artifact inclusion set.

2. Signing policy stage
- Define signature algorithm family and key-rotation model.
- Define verification step placement in build/release workflow.

3. Artifact metadata stage
- Add manifest fields for version, hash set, and signature references.
- Keep verification in report-only mode initially.

4. Enforcement stage
- Introduce blocking verification mode for release artifacts.
- Keep development flow with explicit override policy.

## Slice 5 Marker Contract

Required markers:

- `package: baseline PASS`
- `package: packaging-policy PASS`
- `package: signing-policy PASS`
- `package: poste14-contract PASS`

## Exit Condition for This Slice

Slice 5 is complete when focused packaging validator and strict all-lane gate both PASS.
