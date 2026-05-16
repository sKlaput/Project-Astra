# Post-E14 APIC Transition Planning Baseline

Date: 2026-04-06
Scope: Post-E14 Slice 3
Debt Link: TD-02 (interrupt stack modernization)

## Goal

Define a staged migration path from legacy PIC/PIT to APIC/IOAPIC without destabilizing the current single-core baseline.

## Current Baseline (Verified)

- Legacy PIC vectors are remapped to 0x20/0x28.
- PIT periodic source runs at 100 Hz.
- Timer IRQ remains on legacy vector 0x20 with staged IDT bring-up level 4.
- Existing scheduler timing and syscall/runtime probes remain stable under strict all-lane gate.

## Migration Guardrails

1. Do not switch active IRQ routing until APIC discovery and fallback logic are validated.
2. Keep legacy PIC/PIT path available as an immediate rollback/fallback mode.
3. Preserve deterministic boot-time marker evidence for every migration stage.
4. Avoid changing scheduling semantics during interrupt-controller transition slices.

## Staged Plan

1. Discovery and capability stage
- Add LAPIC/IOAPIC capability detection and serial telemetry only.
- No routing changes.

2. Timer-source abstraction stage
- Introduce timer-source selection boundary (legacy PIT vs APIC timer) behind policy flag.
- Keep PIT as default.

3. Interrupt-routing dry-run stage
- Build IOAPIC routing table plan and validation markers without enabling it.
- Verify vector consistency and reserved vector safety.

4. Controlled activation stage
- Enable LAPIC timer in controlled mode with rollback toggle.
- Keep compatibility checks for scheduler tick monotonicity and fault signatures.

5. Legacy de-risk stage
- Decide long-term legacy PIC behavior (mask vs keep fallback).
- Document production/default policy.

## Slice 3 Marker Contract

The following readiness markers are required and intentionally non-invasive:

- `apic: baseline PASS`
- `apic: vector-plan PASS`
- `apic: timer-source PASS`
- `apic: staged-compat PASS`
- `apic: poste14-contract PASS`

## Exit Condition for This Slice

Slice 3 is complete when marker contract is present in focused validation output and strict all-lane gate remains PASS.
