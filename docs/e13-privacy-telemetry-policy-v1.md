# E13 Privacy Telemetry Policy v1

Date: 2026-04-05
Phase: E13 Security Foundations
Status: Draft v1

## Purpose

Define privacy-default telemetry behavior for engineering diagnostics so security visibility does not degrade user privacy posture.

## Defaults

1. Minimal by default:
- emit only bounded diagnostic markers needed for health and regression checks.

2. Redaction by default:
- do not emit raw sensitive payload data in normal logs.

3. Explicit escalation:
- higher-detail diagnostics require explicit debug mode enablement.

4. Bounded retention:
- diagnostic artifacts should be scoped to validation runs and removable.

## Telemetry Categories

- Allowed baseline:
  - pass/fail marker lines
  - bounded counters
  - non-identifying timing windows

- Restricted by default:
  - raw message contents
  - unbounded dumps
  - persistent high-detail traces

## Policy Checks for E13 Slice 5

1. Privacy defaults are defined.
2. Retention is bounded and operationally removable.
3. Marker-level observability remains intact for focused and strict gates.

## Output

This document defines the policy referenced by E13 Slice 5 marker `security: privacy-policy PASS`.
