# Phase Exit Criteria

## Purpose

This document defines what counts as complete for each execution phase. A phase is not complete because code exists; it is complete only when the required behavior is demonstrated and documented.

## Roadmap Mapping

Execution phases `E1` to `E14` map to product phases as follows:

- Product Phase 1: `E1` to `E5`
- Product Phase 2: `E8` to `E10`
- Product Phase 3: `E6`, `E7`, `E11`
- Product Phase 4: `E12`
- Product Phase 5: `E13` to `E14`

This preserves the high-level product roadmap while keeping implementation work granular.

## Global Exit Rules

Every phase must satisfy all of the following before being marked complete:

1. The system builds from a clean checkout using the documented commands.
2. The phase has a reproducible QEMU test path.
3. New unsafe blocks are documented.
4. The current subsystem status is described in `docs`.
5. Known limitations are listed explicitly.
6. No later-phase subsystem is introduced unless required by the phase.

## E1: Kernel Skeleton

Exit criteria:

1. The kernel boots under QEMU through the chosen boot path.
2. Panic handler prints a visible message to serial output.
3. Framebuffer text output works when framebuffer information is available.
4. Repository structure matches the baseline document.
5. The build and run steps are documented.

Evidence required:

- boot log showing kernel entry
- screenshot or serial log of early text output

## E2: Memory Management Foundations

Exit criteria:

1. Boot memory map is parsed into internal structures.
2. Frame allocator can allocate and free frames in controlled tests.
3. Kernel paging is initialized for higher-half execution.
4. Heap allocation through `alloc` works for basic kernel objects.
5. Page-fault handler reports fault address and error bits.

Evidence required:

- boot log confirming memory initialization
- directed test or demo allocation log

## E3: Interrupts and Timer

Exit criteria:

1. GDT and IDT are loaded successfully.
2. PIC remap completes without conflicting vectors.
3. PIT timer interrupts fire repeatedly.
4. Keyboard interrupts produce visible logs or events.
5. Fault handlers distinguish at least breakpoint, page fault, and general protection fault.

Evidence required:

- serial log showing timer ticks
- serial log showing keyboard event capture

## E4: Basic Scheduler and Tasks

Exit criteria:

1. Scheduler runs from timer interrupts rather than manual yielding only.
2. At least two kernel tasks alternate execution.
3. Idle task runs when no other work is runnable.
4. Task state transitions are visible in logs or tracing hooks.
5. A stuck task cannot prevent timer interrupts from occurring.

Evidence required:

- log sequence showing repeated task switches

## E5: Syscall Layer and User Space

Exit criteria:

1. Ring 3 execution is entered successfully.
2. A statically linked ELF user program loads and runs.
3. At least `write_console`, `yield_now`, and `exit` syscalls work.
4. Invalid syscall numbers are rejected safely.
5. User faults terminate the user task without corrupting the kernel.

Evidence required:

- serial log of user-space hello-world
- log of successful syscall dispatch

## E6: Driver Model

Exit criteria:

1. Driver traits or interfaces are defined and used by at least two driver categories.
2. Keyboard driver uses the shared driver abstraction rather than ad-hoc calls.
3. Block device path exists as a real driver or documented stub behind the same abstraction.
4. Initialization order for drivers is documented.
5. Driver errors are surfaced through a defined error type.

Evidence required:

- architecture note for driver model
- boot log proving driver registration or initialization

## E7: Filesystem Abstraction

Exit criteria:

1. VFS types for mount, node, and file handle exist.
2. Root filesystem mounts successfully.
3. Path lookup works for a defined subset of paths.
4. At least open and read operations work against the initial filesystem.
5. Filesystem limitations are documented clearly.

Evidence required:

- demo showing a file read through the VFS layer

## E8: User-Space Runtime and App Model

Exit criteria:

1. Process abstraction exists separately from bare task representation.
2. User-space startup code is defined and versioned.
3. Basic IPC direction is chosen and documented, even if implementation is partial.
4. At least one second user program beyond hello-world can run.
5. Runtime limitations are documented.

Evidence required:

- process and runtime note in `docs`
- demo launching two user programs sequentially or concurrently

## E9: GUI Prototype

Exit criteria:

1. A compositor or window manager prototype runs in user space.
2. One demonstrator window can be created and redrawn.
3. Keyboard input reaches the GUI layer.
4. Mouse input is either functional or explicitly documented as deferred.
5. Rendering path and ownership between kernel and user space are documented.

Evidence required:

- screenshot or screen capture
- architecture note for GUI prototype

## E10: Core Apps

Exit criteria:

1. Terminal application exists and can launch at least one simple command.
2. Text editor can open and display plain text.
3. File manager can display directory contents through the system file API.
4. Settings shell exists even if most settings are placeholders.
5. App lifecycle expectations are documented.

Evidence required:

- demo notes or screenshots for each app

## E11: Networking Architecture

Exit criteria:

1. Networking subsystem interfaces are documented.
2. Socket API direction is defined.
3. IPv4 path is prioritized explicitly.
4. DNS, DHCP, and firewall hook points are described.
5. Any implemented networking code passes basic architecture sanity checks.

Evidence required:

- networking architecture note
- if code exists, a boot log or test output showing the implemented subset

## E12: Performance and Gaming Considerations

Exit criteria:

1. Known latency sources are identified.
2. Background work minimization rules are documented.
3. Graphics and scheduler decisions are reviewed against gaming goals.
4. A first Game Mode concept note exists.
5. Optimizations do not obscure correctness or diagnosability.

Evidence required:

- performance review note with action items

## E13: Security Foundations

Exit criteria:

1. Permission model scope is documented.
2. Sandboxing direction is documented.
3. Secure boot and integrity plan is described as staged work.
4. Kernel and user isolation guarantees are reviewed.
5. Privacy defaults are restated in implementable terms.

Evidence required:

- security note covering threat model and staged controls

## E14: Roadmap Integration and Documentation

Exit criteria:

1. All implemented modules are reflected in the documentation.
2. Technical debt is listed without hiding placeholders.
3. Each subsystem has an identified next step.
4. The detailed execution phases are mapped back to product phases.
5. Contradictions between docs are removed or explicitly marked for decision.

Evidence required:

- documentation review checklist completed

## Stop Conditions

Implementation must stop for clarification if any of the following occur:

1. A new feature requires changing the boot protocol, syscall ABI, or paging model.
2. A phase cannot be demonstrated under the agreed emulator workflow.
3. A document conflict changes expected behavior.
4. A subsystem depends on undefined product policy such as packaging trust, permission semantics, or update authority.