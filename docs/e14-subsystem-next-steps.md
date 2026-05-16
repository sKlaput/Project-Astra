# E14 Subsystem Next Steps Register

Date: 2026-04-06
Phase: E14 Roadmap Integration and Documentation

## Purpose

Provide one normalized next-step target for each major subsystem so continuation work is unambiguous.

## Subsystem Next Steps

| Subsystem | Current State | Next Step |
| --- | --- | --- |
| Boot and platform | Stable UEFI + Limine bring-up path. | Add secure-boot/integrity implementation staging hooks per E13 policy docs. |
| Memory management | Stable frame allocator, paging, and heap baseline. | Add diagnostics for pressure/failure patterns and prepare large-page/APIC-era compatibility notes. |
| Interrupt/timer | PIC/PIT baseline stable. | Design and stage APIC/IOAPIC migration path with compatibility guardrails. |
| Scheduler/runtime | Preemptive baseline and user-task integration active. | Define SMP-safe scheduling assumptions and migration constraints. |
| Syscall and user boundary | Authz markers and privileged deny baseline in place. | Expand policy enforcement coverage and structured reason/audit outputs. |
| Driver model | Core abstractions present with staged hooks. | Extend toward richer device classes and clearer failure taxonomy usage. |
| Filesystem/VFS | Root mount and read path baseline through initramfs model. | Select and stage persistent filesystem path and mount-policy evolution. |
| Graphics/runtime probes | E9/E10 prototype surfaces and app probes validated. | Convert probe-centric interfaces into clearer runtime ownership contracts for future GUI maturity. |
| Networking | Architecture and scaffold validated. | Execute protocol/runtime implementation milestones under existing contract gates. |
| Performance | E12 slices complete with marker evidence. | Promote performance markers into sustained regression dashboards/profiles. |
| Security | E13 slices complete with policy-first marker evidence. | Implement deeper enforcement and integrity/privacy operational controls. |
| Documentation/governance | E14 integration kickoff complete. | Keep contradiction register and debt register current at every phase boundary. |

## Working Rule

At each phase close, update this register before marking the phase complete.
