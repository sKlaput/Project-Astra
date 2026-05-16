# Kernel Architecture v1

## Purpose

This document defines the first concrete kernel architecture so boot, memory, interrupts, scheduling, and syscall work can proceed against a stable model.

## Kernel Shape

- Architecture style: modular monolithic kernel
- Execution model at bring-up: single-core only
- Privilege model: ring 0 kernel and ring 3 user space
- Language policy: Rust by default, with minimal isolated unsafe code for CPU and memory primitives

## Boot-to-Kernel Flow

1. UEFI firmware loads Limine.
2. Limine loads the kernel ELF image and supplies boot responses.
3. Architecture entry performs minimal CPU state validation.
4. Early serial logging starts.
5. Framebuffer console starts if available.
6. GDT and IDT are installed.
7. Physical memory allocator is initialized from boot memory map.
8. Kernel page tables are installed for higher-half execution.
9. Heap allocator is initialized.
10. Interrupt handling and timer bring-up complete.
11. Scheduler begins tick-driven execution.
12. Syscall entry is enabled only after scheduler and basic process state are stable.

## Address Space Model

### Kernel Addressing

- Kernel is linked in the higher half.
- Canonical kernel base: `0xffffffff80000000`
- Kernel text and rodata: read-only after paging stabilizes where feasible
- Kernel heap: fixed virtual region mapped on demand
- Direct physical map: deferred until needed by subsystems that benefit from it

### User Addressing

- User space occupies the lower canonical range
- Null page remains unmapped
- Initial user memory layout:
  - text
  - rodata
  - data
  - heap
  - stack
- Shared memory: deferred

## Physical Memory Management

- Source of truth: bootloader memory map
- Frame size: `4 KiB`
- Allocator type: bitmap frame allocator
- Reserved regions:
  - kernel image
  - bootloader data used after handoff
  - framebuffer memory
  - ACPI and firmware tables once identified
  - MMIO regions when device support begins

Allocation rules:

- Allocation APIs return typed abstractions instead of raw integers where practical.
- Unsafe pointer creation is confined to architecture and allocator internals.
- Double-free detection should exist in debug builds where practical.

## Virtual Memory Model

- Paging mode: x86_64 4-level paging
- Large pages: deferred until baseline mappings are stable
- Kernel mappings are global to all processes
- User mappings are per-process
- Guard pages are required for kernel stacks and user stacks once those stacks exist

## Heap Strategy

- Heap exists only after frame allocator and paging are online
- The first heap is a single kernel heap region expanded in page granularity
- Allocation policy favors simplicity and debuggability over peak performance in v1
- Slab or object caches are deferred until the driver and VFS layers create pressure for them

## CPU and Execution Context

- CPU support in v1: bootstrap processor only
- SMP state: explicitly unsupported
- Per-CPU data: design placeholder only
- Context switch representation: saved register frame plus stack pointer and address-space metadata

## Interrupt and Exception Model

- Exception table: full IDT initialized during early bring-up
- Fault policy:
  - kernel faults are fatal unless explicitly handled
  - user faults terminate the offending process once user mode exists
- IRQ staging:
  - PIC and PIT first
  - APIC migration later
- Required exceptions to handle visibly:
  - breakpoint
  - double fault
  - general protection fault
  - page fault
  - invalid opcode

## Timer Model

- Initial timer source: PIT
- Tick model: periodic tick in v1
- Target tick rate: `100 Hz`
- Tickless scheduling: deferred
- High-resolution timers: deferred

## Scheduler Model

- Scheduling style in v1: preemptive round-robin
- Core assumption: single CPU
- Runnable entity: kernel task initially, then user thread
- Priority model: one priority class in v1
- Time slice target: 10 ms
- Blocking model: explicit task states for runnable, blocked, and terminated

Scheduler milestones:

1. Tick increments a global scheduler clock.
2. Two kernel demo tasks alternate under timer-driven switching.
3. Idle task runs when no runnable work exists.
4. User tasks enter the same scheduler once ring 3 execution exists.

### Scheduler-backed User Execution Delta (2026-03-29)

The baseline now has a concrete, running implementation for persistent user-mode
execution under the same scheduler used by kernel tasks.

- User tasks are spawned through scheduler-owned helpers and registered with
  per-task user metadata (code virtual address, stack virtual address,
  entry RIP, and user RSP).
- Dispatch still happens from normal kernel context via `dispatch_once`, but
  user-task slots use a scheduler trampoline that performs ring transition:
  1. Load user entry metadata for current task.
  2. Enter ring 3 via `iretq` path (`enter_user_mode`).
  3. Return to kernel through user `int3` trap handling.
  4. Re-enable interrupts and sleep for one tick.
  5. Re-dispatch and repeat.
- Breakpoint handling supports both probe mode and scheduler-owned user tasks:
  ring-3 `int3` can resume the saved kernel stack even when the explicit probe
  flag is not armed, as long as the current task is registered as a user task.

This establishes a real persistent ring-0 <-> ring-3 loop, rather than a
single ad hoc roundtrip from probe-local code.

Validation evidence:

- Boot log: `build/persist-user-scheduler-final.log.err`
- Observed marker: `arch: persistent-user-task map=1,1,1 spawn=1 count=3 hit=1 ... PASS`
- System continues to `scheduler: idle loop active` after the probe

## Process Model

- Process abstraction introduced with user-space support
- Initial process shape: one thread per process
- Fork semantics: not in v1
- Exec semantics: load-and-run ELF image into a fresh address space
- Handle table: reserved for later file, device, and IPC object references

## Syscall ABI

- Entry instruction: `syscall`
- Return instruction: `sysretq` where valid
- ABI version: `v0`
- Register convention:
  - syscall number: `rax`
  - arg0: `rdi`
  - arg1: `rsi`
  - arg2: `rdx`
  - arg3: `r10`
  - arg4: `r8`
  - arg5: `r9`
  - return value: `rax`
- Error convention: negative error codes in `rax`

Initial syscall set:

1. `write_console`
2. `exit`
3. `yield_now`
4. `get_time_ticks`

No syscall may expose raw kernel pointers to user space.

## ELF Loader Baseline

- Binary format: `ELF64`
- Linking mode: static
- Dynamic loader: deferred
- Relocation support: only the minimum required for the chosen binary format in v1
- User entry stack includes:
  - argc
  - argv pointer table
  - null terminator

## Driver Boundary

- Drivers remain kernel-resident in v1
- Driver interface categories:
  - console
  - input
  - block device
  - timer
  - interrupt source
- Driver registration is explicit during init; no runtime discovery framework beyond what the relevant bus requires

## Filesystem Boundary

- VFS objects: superblock, inode-like node, file handle, mount
- Initial mount model: root mount from initramfs
- Path resolution exists before persistent storage support
- Permissions model in VFS: placeholder structure first, enforcement later

## Graphics Boundary

- Kernel graphics responsibility in v1: framebuffer ownership and basic mode information only
- Compositor/window manager: user-space component
- GPU acceleration: not required for initial GUI prototype
- Vulkan-first direction applies to future graphics stack design, not early kernel bring-up

## Security Boundary

- Kernel memory is never directly mapped writable in user space
- User programs execute in ring 3 only
- Syscall dispatch validates syscall number and user pointers
- Kernel stacks are private per task
- Capability and permission framework begins as API design before policy enforcement is fully mature

## Non-Goals for v1

- SMP
- dynamic linking
- loadable kernel modules
- power management
- USB stack
- network stack implementation
- GPU drivers
- Windows compatibility layer
- package manager implementation

## Change Control

Any change to boot protocol, paging model, syscall ABI, scheduler model, or process model must update this document before implementation proceeds.