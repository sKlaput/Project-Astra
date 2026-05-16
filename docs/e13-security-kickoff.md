# E13 Security Foundations Kickoff

Date: 2026-04-05
Phase: E13 Security Foundations

## Scope

This kickoff note defines the first staged security baseline for the kernel and user-space architecture without introducing premature feature complexity.

## E13 Exit Criteria Mapping Plan

1. Permission model scope is documented.
Plan:
- Define permission domains: process, filesystem, device, network.
- Define subject model for early implementation: kernel task, user task, future app identity.

2. Sandboxing direction is documented.
Plan:
- Use capability-oriented syscall policy checks before broad ACL design.
- Keep first sandbox policy deny-by-default for privileged surfaces.

3. Secure boot and integrity plan is described as staged work.
Plan:
- Stage 1: document trust boundaries for Limine/UEFI path and kernel image provenance.
- Stage 2: define integrity measurement hooks and artifact signing workflow assumptions.

4. Kernel and user isolation guarantees are reviewed.
Plan:
- Restate current isolation guarantees and known limits in single-core baseline.
- Add explicit non-goals for E13 to avoid accidental ABI or paging-model churn.

5. Privacy defaults are restated in implementable terms.
Plan:
- Minimize persistent sensitive telemetry by default.
- Keep diagnostic logs explicit, scoped, and removable.

## Initial Threat Model Seeds

- Threat A: malformed user input through syscall interfaces.
- Threat B: misuse of privileged kernel subsystems by untrusted user tasks.
- Threat C: integrity drift in boot artifacts or kernel image pipeline.
- Threat D: excessive diagnostic data exposure.

## First E13 Deliverables

1. `docs/e13-security-model-v1.md`:
- permission vocabulary and policy surface.

2. `docs/e13-threat-model-v1.md`:
- threat table, trust boundaries, and staged mitigations.

3. `docs/e13-isolation-review.md`:
- kernel/user boundary guarantees and known limitations.

4. Focused validation script contract:
- add a script that checks for deterministic `security:` markers once probe hooks are introduced.

## Working Rules

- Prefer policy notes and deterministic probes over broad refactors.
- Keep E9 strict all-lane gate green while E13 scaffolding is added.
- Stop and document if an E13 task would force boot protocol, syscall ABI, or paging model changes.

## Kickoff Decision

E13 starts with documentation-first security baselining, then moves to narrow probe-backed implementation slices.
