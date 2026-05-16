# Post-E14 GUI Input Routing Evidence

Date: 2026-04-06
Scope: Post-E14 Slice 10

## Objective

Add deterministic GUI input routing marker contract and focused validator coverage based on focus ownership readiness and routing matrix signals.

## Implementation

1. `kernel/src/main.rs`
- Added `probe_poste14_gui_input_routing_baseline()` and boot integration after focus arbitration baseline.
- Added marker contract:
  - `gui-input: baseline PASS|FAIL`
  - `gui-input: ownership PASS|FAIL`
  - `gui-input: routing-path PASS|FAIL`
  - `gui-input: poste14-contract PASS|FAIL`

2. `scripts/validate-poste14-gui-input.ps1`
- Added focused validator for GUI input routing marker contract.
- Emits text/JSON summary artifacts.

## Validation Runs

Compile check:
- Command: `cargo check -Z build-std=core,alloc -p kernel`
- Result: PASS (warnings only)

Focused validator:
- Command: `./scripts/validate-poste14-gui-input.ps1 -OutPrefix build/poste14-guiinput-s10`
- Summary: `build/poste14-guiinput-s10-summary.txt`
- Result: `Post-E14 GUI Input Routing Validation: PASS`

Strict all-lane gate:
- Command: `./scripts/validate-e9-gate.ps1 -OutPrefix build/e9-gate-poste14-s10`
- Summary: `build/e9-gate-poste14-s10-summary.txt`
- Result: PASS
- Lane summaries:
  - `build/e9-gate-poste14-s10-stable-summary.txt` PASS
  - `build/e9-gate-poste14-s10-diag-user-summary.txt` PASS
  - `build/e9-gate-poste14-s10-diag-kernel-summary.txt` PASS

## Outcome

Post-E14 Slice 10 is complete:

- GUI input routing marker contract implemented,
- focused validator added and passing,
- strict all-lane regression gate remains green,
- next GUI slice can target focus recovery ownership policy.
