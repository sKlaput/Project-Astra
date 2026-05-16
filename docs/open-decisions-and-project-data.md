# Open Decisions and Project Data Register

## Purpose

This register captures the non-code project data and unresolved decisions that still need an explicit owner or answer. Items here should not remain implicit across multiple phases.

## Project Identity Data

| Item | Current State | Needed Action |
| --- | --- | --- |
| OS name | Placeholder | Choose a working product name for repository, boot banner, and docs |
| Logo | Placeholder | Define later; not required for engineering start |
| Slogan | Placeholder | Optional; not required for engineering start |
| Date fields | Placeholder | Stamp current draft dates on all formal docs |
| Prepared by / owner | Placeholder | Record project owner or team name |
| Palette and typography | Placeholder | Needed only when visual identity work begins |

## Engineering Decisions That Must Stay Visible

| Topic | Decision | Status |
| --- | --- | --- |
| Firmware target | UEFI only for v1 | Fixed |
| Boot protocol | Limine | Fixed |
| Emulator baseline | QEMU + OVMF | Fixed |
| CPU scope | x86_64, single-core first | Fixed |
| Interrupt path | PIC + PIT first, APIC later | Fixed |
| Kernel style | Modular monolithic | Fixed |
| Syscall entry | `syscall` / `sysretq` | Fixed |
| Early filesystem | initramfs-backed root | Fixed |
| Early graphics | Framebuffer console, GUI in user space later | Fixed |

## Decisions Still Open

| Topic | Why It Matters | Target Phase |
| --- | --- | --- |
| Native filesystem format after initramfs | Affects persistence, recovery, and tooling | E7 |
| Package format and signature model | Affects app distribution and trust chain | E13 baseline staged, refinement E14+ |
| Permission vocabulary | Needed for apps, devices, files, and networking | E13 baseline documented, refinement E14+ |
| User account model | Needed before multi-user or protected storage | After E8 |
| Update authority and release channel design | Required before signed updates and enterprise controls | After E10 |
| Crash dump retention policy | Needed for privacy and support posture | E13 baseline policy captured, enforcement E14+ |
| Compatibility strategy | Needed before any Windows app support investigation | After E12 |
| Licensing model details | Needed for productization, not kernel bring-up | Product planning |

## Missing Data To Add to Existing Source Documents

The two original source documents should eventually be updated with these concrete additions:

1. A statement that the instruction pack is the execution document and the specification is the intent document.
2. The chosen v1 boot and toolchain baseline.
3. The product-to-execution phase mapping.
4. The fact that VGA text is not guaranteed under the UEFI-first path and serial plus framebuffer are the real early-console defaults.
5. The initial syscall ABI and single-core scheduler assumption.
6. The rule that unresolved architectural conflicts must stop work rather than be guessed.

## Recommended Immediate Metadata Updates

Before the next formal revision of the source documents, add the following minimum metadata:

- draft date
- project owner name
- document owner
- version number for the instruction pack
- version number for the engineering baseline
- version number for the kernel architecture baseline

## Review Rule

At the end of each execution phase, review this register and either:

1. convert an open item into a fixed decision, or
2. mark it explicitly deferred with a reason

No item should disappear silently.