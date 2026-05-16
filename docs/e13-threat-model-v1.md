# E13 Threat Model v1

Date: 2026-04-05
Phase: E13 Security Foundations
Status: Draft v1

## Purpose

Capture first-pass threat model boundaries for the current OS baseline and define staged mitigations that are compatible with ongoing implementation.

## Trust Boundaries

1. Boot boundary:
- UEFI firmware and Limine chain load the kernel image.

2. Kernel boundary:
- Ring 0 code and data are trusted computing base.

3. User boundary:
- Ring 3 tasks are untrusted by default.

4. Interface boundary:
- Syscalls, driver interfaces, VFS path handling, and future networking surfaces.

## Assets

- Kernel control-flow integrity.
- Kernel memory integrity.
- Boot artifact integrity.
- User data confidentiality and integrity.
- Diagnostic data minimization.

## Threat Table

| ID | Threat | Surface | Impact | Initial Mitigation Direction |
| --- | --- | --- | --- | --- |
| T1 | Malformed syscall arguments | syscall handlers | Kernel fault or policy bypass | Strict argument validation + fail-closed authz checks |
| T2 | Privileged operation misuse from user task | syscall/device/network surfaces | Unauthorized state mutation | Deny-by-default policy and allowlisted privileged ops |
| T3 | Path confusion in VFS lookups | filesystem APIs | Unauthorized read/write scope | Canonical path normalization and policy checks post-lookup |
| T4 | Boot artifact tampering | boot/image pipeline | Compromised kernel start state | Staged integrity plan with measured artifacts and signing policy |
| T5 | Overexposed diagnostics | serial/log telemetry | Information leakage | Redaction defaults and diagnostic verbosity policy |

## Risk Prioritization (Current)

- High: T1, T2, T4
- Medium: T3, T5

## Staged Mitigation Plan

### Stage A (E13 early)

- Document authorization model and hook points.
- Add deterministic `security:` probe markers.
- Enforce deny-by-default for privileged syscall categories.

### Stage B (E13 middle)

- Define boot integrity checklist and artifact handling policy.
- Add policy reason codes for all authz denies.
- Add VFS/path policy normalization checks.

### Stage C (E13 late)

- Expand policy granularity for service-style user tasks.
- Add bounded diagnostic policy profiles (minimal/engineering).
- Prepare handoff note for E14 documentation integration.

## Out of Scope for v1

- Full multi-user accounts and credential stores.
- Complete cryptographic key lifecycle implementation.
- Production secure-boot implementation details.

## Evidence Plan

- `docs/e13-security-model-v1.md`
- `docs/e13-isolation-review.md`
- focused validation output once `security:` markers are introduced
- strict E9 all-lane gate summaries for regression safety
