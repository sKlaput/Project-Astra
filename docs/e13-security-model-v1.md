# E13 Security Model v1

Date: 2026-04-05
Phase: E13 Security Foundations
Status: Draft v1

## Purpose

Define an implementable initial permission model for the current kernel and user-space baseline without changing boot protocol, syscall ABI, or paging model.

## Design Constraints

- Single-core x86_64 baseline remains in effect.
- Existing syscall entry path (`syscall`/`sysretq`) stays unchanged.
- Policy checks must be explicit and diagnosable.
- Default posture for privileged operations is deny-by-default.

## Subjects

- `KernelTask`: trusted in-kernel execution context.
- `UserTask`: untrusted user execution context.
- `ServiceTask` (planned): constrained privileged user-space service identity.

## Objects

- `ProcessControl`: task lifecycle and scheduler-affecting operations.
- `Filesystem`: namespace, file read/write, mount-related surfaces.
- `Device`: direct hardware-facing operations and driver control surfaces.
- `Network`: socket and packet control surfaces.
- `Diagnostics`: high-detail telemetry and debug interfaces.

## Permission Vocabulary

- `Read`: non-mutating access to object state.
- `Write`: mutating access to object state.
- `Execute`: operation invocation rights.
- `Admin`: policy-changing or globally impactful operations.
- `Diagnose`: access to sensitive diagnostic surfaces.

## Default Policy Matrix (v1)

| Subject | ProcessControl | Filesystem | Device | Network | Diagnostics |
| --- | --- | --- | --- | --- | --- |
| KernelTask | Admin | Admin | Admin | Admin | Admin |
| UserTask | Execute (self-scoped) | Read (policy-scoped) | Deny | Deny (until E13 net policy slice) | Deny |
| ServiceTask (planned) | Execute (service-scoped) | Read/Write (service-scoped) | Execute (explicit allowlist) | Execute (explicit allowlist) | Diagnose (bounded) |

## Enforcement Direction

1. Introduce explicit authorization hooks at syscall dispatch boundaries.
2. Represent policy outcomes with deterministic markers:
- `security: authz PASS|FAIL`
- `security: default-deny PASS|FAIL`
3. Fail closed on missing policy definitions for privileged objects.

## Logging and Diagnostics Rule

- Every deny decision should emit a bounded, parseable reason code.
- Sensitive argument data must be redacted by default.

## Known Limitations (v1)

- No user identity/account model yet.
- No persistent policy storage yet.
- Network permission granularity deferred to later E13 slices.
- Capability transfer semantics not defined yet.

## Immediate Next Slice Actions

1. Define syscall authorization hook points and no-op stubs in architecture notes.
2. Add first deterministic `security:` marker probe path in kernel diagnostics.
3. Keep strict E9 all-lane gate green while adding E13 scaffolding.
