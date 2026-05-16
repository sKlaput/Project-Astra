# Post-E14 Storage Persistence Planning Baseline

Date: 2026-04-06
Scope: Post-E14 Slice 4
Debt Link: TD-04 (storage persistence path)

## Goal

Define staged migration from initramfs-only rootfs toward a persistent block-backed filesystem path while keeping current runtime stable.

## Current Baseline (Verified)

- Root mount is deterministic (`rootfs`) and lookup/open/read flow is stable.
- Directory structure baseline includes `/etc`, `/etc/motd`, and `/hello.txt`.
- Existing VFS probe and all-lane regression gate remain green.

## Migration Guardrails

1. Keep initramfs read path as active fallback during early persistence staging.
2. Separate mount-policy definition from block-driver activation steps.
3. Require deterministic marker evidence for each stage.
4. Avoid introducing persistence write semantics before mount and failure-policy contract is explicit.

## Staged Plan

1. Policy and mount model stage
- Define persistent root candidate and mount fallback policy.
- Keep initramfs as default active root.

2. Block path readiness stage
- Add block-device presence and capability telemetry markers.
- Keep mount behavior unchanged.

3. Persistent mount dry-run stage
- Simulate mount-policy decision with deterministic markers.
- Validate fallback behavior when persistent target unavailable.

4. Controlled activation stage
- Enable read-only persistent mount behind explicit policy flag.
- Preserve rollback to initramfs.

5. Write-path and integrity stage
- Stage safe write semantics, journaling decision, and crash-consistency strategy.

## Slice 4 Marker Contract

Required readiness markers:

- `storage: baseline PASS`
- `storage: mount-policy PASS`
- `storage: persistence-readiness PASS`
- `storage: poste14-contract PASS`

## Exit Condition for This Slice

Slice 4 is complete when focused storage validator and strict all-lane gate both PASS.
