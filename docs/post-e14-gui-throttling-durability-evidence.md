# Post-E14 GUI Throttling Durability Evidence

Date: 2026-04-07
Scope: Post-E14 Slice 25

## Objective

Add deterministic GUI throttling-durability marker contract and focused validator coverage based on durability-window and policy readiness.

## Implementation

1. kernel/src/main.rs
- Added probe_poste14_gui_throttling_durability_baseline() and boot integration after resilience-hardening baseline.
- Added marker contract:
  - gui-throttle-dur: baseline PASS or FAIL
  - gui-throttle-dur: window PASS or FAIL
  - gui-throttle-dur: policy PASS or FAIL
  - gui-throttle-dur: poste14-contract PASS or FAIL

2. scripts/validate-poste14-gui-throttling-durability.ps1
- Added focused validator for GUI throttling-durability marker contract.
- Emits text and JSON summary artifacts.

## Validation Runs

Compile check:
- Command: cargo check -Z build-std=core,alloc -p kernel
- Result: PASS (warnings only)

Focused validator:
- Command: ./scripts/validate-poste14-gui-throttling-durability.ps1 -OutPrefix build/poste14-guithrottledur-s25
- Summary: build/poste14-guithrottledur-s25-summary.txt
- Result: Post-E14 GUI Throttling Durability Validation: PASS

Strict all-lane gate:
- Command: ./scripts/validate-e9-gate.ps1 -OutPrefix build/e9-gate-poste14-s25
- Summary: build/e9-gate-poste14-s25-summary.txt
- Result: PASS
- Lane summaries:
  - build/e9-gate-poste14-s25-stable-summary.txt PASS
  - build/e9-gate-poste14-s25-diag-user-summary.txt PASS
  - build/e9-gate-poste14-s25-diag-kernel-summary.txt PASS

## Outcome

Post-E14 Slice 25 is complete:

- GUI throttling-durability marker contract implemented,
- focused validator added and passing,
- strict all-lane regression gate remains green,
- next GUI slice can target resilience soak policy.
