# E14 Technical Debt Register

Date: 2026-04-06
Phase: E14 Roadmap Integration and Documentation

## Purpose

List current technical debt explicitly without hiding placeholders, and assign actionable next steps.

## Debt Items

| ID | Area | Debt Description | Impact | Priority | Next Action |
| --- | --- | --- | --- | --- | --- |
| TD-01 | CPU/Scheduling | Single-core baseline only; SMP remains deferred. | Limits scalability and realistic multiprocessor validation. | High | Create SMP readiness note and staged APIC migration plan. |
| TD-02 | Interrupt stack | PIC/PIT baseline retained; APIC/IOAPIC path not integrated. | Limits modern hardware alignment and timing flexibility. | High | Add APIC transition design slice and compatibility constraints. |
| TD-03 | Networking | Architecture/scaffold in place, full runtime stack still partial. | Limits end-to-end networked app behavior validation. | High | Define E11 follow-on implementation package with concrete protocol milestones. |
| TD-04 | Storage | Early filesystem path is initramfs-centric; persistent fs work remains open. | Blocks persistence/productization readiness. | High | Finalize next persistent filesystem target and mount model roadmap. |
| TD-05 | Security enforcement | E13 policy markers are in place; enforcement hardening remains staged. | Security posture is baseline-ready but not production-complete. | High | Add enforcement slices for policy application, auditing, and deny handling depth. |
| TD-06 | Packaging/signing | Package/signature model baseline direction exists but implementation is pending. | Prevents trusted distribution path. | Medium | Define artifact signing workflow and key lifecycle assumptions in follow-on plan. |
| TD-07 | Privacy operations | Privacy-default policy exists; operational tooling/retention automation pending. | Risk of inconsistent diagnostics hygiene. | Medium | Add retention and log-profile operational tasks and scripts. |
| TD-08 | Docs cohesion | Some subsystem next steps historically spread across multiple docs. | Raises onboarding and handoff friction. | Medium | Keep a single subsystem next-step register updated per phase close. |

## Notes

- This register is intentionally explicit about deferred scope and unresolved implementation depth.
- Debt item IDs should be referenced in subsequent phase planning and closure notes.
