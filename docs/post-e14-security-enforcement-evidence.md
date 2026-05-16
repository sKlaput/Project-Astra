# Post-E14 Security Enforcement Evidence

## Slice 1: Authz Audit-Counter Hardening

Date: 2026-04-06

## Implemented Subset

- Extended authz telemetry in `kernel/src/syscall.rs`:
  - added reason-specific deny counters:
    - unknown syscall deny count
    - privileged-group deny count
    - default deny count
  - exposed counters in `SecurityAuthzSnapshot`.
- Extended `probe_e13_security_baseline()` in `kernel/src/main.rs`:
  - computes counter deltas for probe window
  - validates counter behavior consistency
  - adds deterministic marker `security: audit-counters PASS|FAIL`.
- Updated focused validator contract in `scripts/validate-e13-security.ps1` to require new marker.

## Validation Commands

```powershell
cargo check -Z build-std=core,alloc -p kernel
.\scripts\validate-e13-security.ps1 -OutPrefix build/e13-validate-poste14-s1
.\scripts\validate-e9-gate.ps1 -OutPrefix build/e9-gate-poste14-s1
```

## Evidence Summary

- Focused security summary:
  - `build/e13-validate-poste14-s1-summary.txt` -> `E13 Security Validation: PASS`
- Focused security JSON:
  - `build/e13-validate-poste14-s1-summary.json` -> `result = PASS`
- Strict all-lane summary:
  - `build/e9-gate-poste14-s1-summary.txt` -> `E9 Tripwire Summary: PASS`

## Outcome

Post-E14 Slice 1 strengthens security diagnosability with reason-specific authz counters while preserving strict all-lane regression stability.
