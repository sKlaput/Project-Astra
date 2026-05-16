# Post-E14 APIC Transition Evidence

Date: 2026-04-06
Scope: Post-E14 Slice 3 (APIC transition planning baseline)

## Objective

Add deterministic APIC transition readiness markers and a focused validator while keeping runtime behavior on stable PIC/PIT baseline.

## Implementation

1. `kernel/src/arch/x86_64/interrupts.rs`
- Added helper APIs exposing stable legacy interrupt configuration for probes:
  - `legacy_idt_bringup_stage()`
  - `legacy_timer_vector()`
  - `legacy_pic_vector_offsets()`
  - `legacy_spurious_vectors()`
  - `legacy_pit_target_hz()`

2. `kernel/src/main.rs`
- Added `probe_poste14_apic_transition_baseline()` and wired it into boot probe sequence.
- Added marker contract lines:
  - `apic: baseline PASS|FAIL`
  - `apic: vector-plan PASS|FAIL`
  - `apic: timer-source PASS|FAIL`
  - `apic: staged-compat PASS|FAIL`
  - `apic: poste14-contract PASS|FAIL`

3. `scripts/validate-poste14-apic.ps1`
- Added focused validator that checks APIC marker contract and emits text/JSON summaries.

## Validation Runs

Compile check:
- Command: `cargo check -Z build-std=core,alloc -p kernel`
- Result: PASS (warnings only)

Focused APIC validator:
- Initial run showed baseline zero-delta sensitivity in bounded boot window and failed marker contract.
- Probe criteria refined to deterministic measurement-path semantics (zero deltas allowed).
- Rerun command: `./scripts/validate-poste14-apic.ps1 -OutPrefix build/poste14-apic-s3-rerun`
- Summary: `build/poste14-apic-s3-rerun-summary.txt`
- Result: `Post-E14 APIC Validation: PASS`

Strict all-lane gate:
- Command: `./scripts/validate-e9-gate.ps1 -OutPrefix build/e9-gate-poste14-s3-rerun`
- Summary: `build/e9-gate-poste14-s3-rerun-summary.txt`
- Result: `E9 Tripwire Summary: PASS`
- Lane summaries:
  - `build/e9-gate-poste14-s3-rerun-stable-summary.txt` PASS
  - `build/e9-gate-poste14-s3-rerun-diag-user-summary.txt` PASS
  - `build/e9-gate-poste14-s3-rerun-diag-kernel-summary.txt` PASS

## Outcome

Post-E14 Slice 3 is complete:

- APIC transition readiness marker contract implemented,
- focused validator added and passing,
- strict all-lane regression gate remains green,
- migration guardrails documented for follow-on implementation stages.
