# Post-E14 GUI Focus Arbitration Evidence

Date: 2026-04-06
Scope: Post-E14 Slice 9

## Objective

Add deterministic GUI focus arbitration marker contract and focused validator coverage based on foreground ownership policy.

## Implementation

1. `kernel/src/main.rs`
- Added `probe_poste14_gui_focus_arbitration_baseline()` and boot integration after composition probe.
- Added marker contract:
  - `gui-focus: baseline PASS|FAIL`
  - `gui-focus: owner PASS|FAIL`
  - `gui-focus: arbitration-path PASS|FAIL`
  - `gui-focus: poste14-contract PASS|FAIL`

2. `scripts/validate-poste14-gui-focus.ps1`
- Added focused validator for GUI focus arbitration marker contract.
- Emits text/JSON summary artifacts.

## Validation Runs

Compile check:
- Command: `cargo check -Z build-std=core,alloc -p kernel`
- Result: PASS (warnings only)

Focused validator:
- Command: `./scripts/validate-poste14-gui-focus.ps1 -OutPrefix build/poste14-guifocus-s9`
- Summary: `build/poste14-guifocus-s9-summary.txt`
- Result: `Post-E14 GUI Focus Validation: PASS`

Strict all-lane gate:
- Command: `./scripts/validate-e9-gate.ps1 -OutPrefix build/e9-gate-poste14-s9`
- Summary: `build/e9-gate-poste14-s9-summary.txt`
- Result: PASS
- Lane summaries:
  - `build/e9-gate-poste14-s9-stable-summary.txt` PASS
  - `build/e9-gate-poste14-s9-diag-user-summary.txt` PASS
  - `build/e9-gate-poste14-s9-diag-kernel-summary.txt` PASS

## Outcome

Post-E14 Slice 9 is complete:

- GUI focus arbitration marker contract implemented,
- focused validator added and passing,
- strict all-lane regression gate remains green,
- next GUI slice can target input routing ownership policy.
