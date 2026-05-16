# E13 Integrity Staging v1

Date: 2026-04-05
Phase: E13 Security Foundations
Status: Draft v1

## Purpose

Define staged secure-boot and integrity work that is implementable in the current baseline without forcing boot protocol, ABI, or paging model changes.

## Stage Model

### Stage 1: Boot Trust Boundary and Provenance Notes

- Record trust boundary assumptions for UEFI firmware, Limine, and kernel image handoff.
- Define artifact inventory for boot-relevant binaries and config:
  - firmware image
  - bootloader binaries
  - kernel image
  - boot config
- Require traceable build provenance for kernel artifacts used in validation runs.

### Stage 2: Integrity Measurement and Signing Assumptions

- Define where integrity measurement hooks can be introduced without runtime destabilization.
- Define signing workflow assumptions for build outputs and release artifacts.
- Require explicit mismatch handling policy (deny/diagnose paths) before enforcement is enabled.

## Boundaries and Non-Goals

- No immediate secure-boot enforcement in this slice.
- No key-management implementation in this slice.
- No platform-specific firmware hardening outside documented assumptions.

## Minimum Acceptance for Slice 5

1. At least two integrity stages are explicitly documented.
2. Trust boundary and artifact inventory are captured.
3. Measurement and signing assumptions are explicitly staged.

## Output

This document provides the staged integrity plan used by E13 Slice 5 marker `security: integrity-plan PASS`.
