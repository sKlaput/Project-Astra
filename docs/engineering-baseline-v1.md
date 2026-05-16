# Engineering Baseline v1

## Purpose

This document fixes the initial engineering choices required to start implementation without architectural guesswork. It complements the high-level product specification and the AI instruction pack.

If a future document conflicts with this baseline, stop and resolve the conflict explicitly instead of silently changing direction.

## Scope

This baseline applies to the first implementation cycle covering boot, kernel bring-up, memory management foundations, interrupts, scheduler bring-up, syscall introduction, and early user space.

## Platform Baseline

- Primary CPU architecture: `x86_64`
- Firmware target for v1: `UEFI` only
- Boot mode not supported in v1: legacy BIOS
- SMP support in v1: deferred
- Hardware focus for v1: QEMU and broadly compatible desktop-class PCs
- Initial kernel style: modular monolithic kernel with strict subsystem boundaries

## Boot Strategy

- Boot protocol: `Limine` boot protocol
- Boot path: UEFI firmware -> Limine -> Rust kernel entry
- Kernel image format: `ELF64`
- Initial display path: Limine framebuffer console
- Initial debug path: serial console on `COM1` at `115200`
- VGA text mode status: optional legacy fallback only; not relied on for UEFI bring-up

### Rationale

UEFI-only reduces early scope. Limine provides a stable handoff path, memory map, framebuffer information, and a practical bootstrap for a Rust kernel. Serial output is the primary diagnostic channel because it remains reliable in emulation and early boot.

## Toolchain and Build Baseline

- Rust channel: `nightly`
- Target triple: `x86_64-unknown-none`
- Core build command: `cargo build -Z build-std=core,alloc`
- Linker: `rust-lld`
- Required Rust components: `rust-src`, `llvm-tools-preview`
- Formatting tool: `rustfmt`
- Linting tool: `clippy`
- Image assembly: scripted kernel + Limine image build

## Development Environment

- Primary emulator: `QEMU`
- UEFI firmware for emulation: `OVMF`
- Primary debug workflow: QEMU serial log + GDB stub when needed
- Host OS support target for developers: Windows and Linux

### QEMU Baseline

- Machine type: `q35`
- CPU model: `qemu64` or host-compatible equivalent
- Memory for standard test boot: `512M`
- Core count for v1 tests: `1`
- Serial: `-serial stdio`
- Debug option: `-s -S` for breakpoint-driven bring-up

## Repository Layout

The repository should be created with this structure:

```text
docs/
kernel/
kernel/src/
kernel/src/arch/x86_64/
kernel/src/memory/
kernel/src/interrupts/
kernel/src/scheduler/
kernel/src/syscall/
kernel/src/drivers/
kernel/src/fs/
user/
tools/
scripts/
```

## Logging and Diagnostics

- Primary early log sink: serial console
- Secondary early log sink: framebuffer text console
- Panic policy in v1: halt CPU after panic message and register/context dump where available
- Fault handling in v1: print exception name and halt unless explicitly marked recoverable
- Required bring-up logs:
  - boot entry reached
  - memory map parsed
  - heap initialized
  - IDT loaded
  - timer interrupts observed
  - first scheduler tick observed
  - syscall trap observed

## Hardware Bring-up Order

Implementation order for hardware-dependent features:

1. Serial output
2. Framebuffer console
3. Memory map parsing
4. GDT and IDT setup
5. PIC remap and PIT timer
6. Keyboard interrupt via PS/2 path in QEMU
7. PCI enumeration scaffold
8. Disk abstraction stub
9. Storage controller integration
10. Network interface abstraction stub

## Interrupt Controller Baseline

- First interrupt controller path: legacy PIC for emulator bring-up
- First timer source: PIT
- APIC and IOAPIC status: planned after stable interrupt handling exists
- HPET status: deferred

This is a staging decision, not a long-term architecture preference.

## Memory Management Baseline

- Memory map source: Limine bootloader response
- Page size in v1: `4 KiB`
- Virtual addressing model: higher-half kernel
- Kernel virtual base: `0xffffffff80000000`
- Physical frame allocation strategy: bitmap allocator built from boot memory map
- Kernel heap allocator: fixed virtual heap region backed by frame allocator
- Heap implementation style: simple general-purpose allocator suitable for `alloc`
- User-space virtual memory: introduced only after kernel paging is stable
- Copy-on-write: deferred
- Swap: deferred

## Driver and Device Model Baseline

- Driver boundary style: trait-driven interfaces with explicit ownership and init order
- Driver loading in v1: statically linked kernel modules only
- Hot-plug support: deferred
- DMA-capable driver support: deferred until memory manager is stable

## Filesystem Baseline

- First filesystem for execution milestones: `initramfs` or equivalent in-memory boot image
- Persistent filesystem support: deferred until storage path stabilizes
- VFS requirement: yes, from the start
- FAT interoperability target: first external filesystem target after VFS stabilizes
- NTFS interoperability target: deferred and likely read-only first

## User-space Baseline

- First user programs: statically linked `ELF64` binaries
- Dynamic linking: deferred
- libc equivalent: minimal project-owned runtime only
- Initial process model: one process, one thread at bring-up; multi-process follows after syscall and address-space work stabilize
- Shell requirement: minimal command environment after userspace launch works

## Security Baseline

- Telemetry: none by default and none in v1
- Kernel/userspace isolation: mandatory before calling user-space support complete
- Unsafe Rust policy: isolate and document every unsafe block
- Secure boot integration: design placeholder only in v1
- Update signing: design placeholder only until packaging exists
- Secrets management: not in scope until persistent storage and user accounts exist

## Compatibility Baseline

- Native applications are the only execution target in v1
- Windows compatibility layer work is explicitly out of scope until the native runtime, process model, VFS, and graphics paths exist

## Testing Baseline

- Every phase must include at least one repeatable emulator boot test
- Kernel changes affecting boot, memory, interrupts, or scheduler must be exercised under QEMU before merge
- Subsystems should expose small diagnostic hooks rather than claiming readiness without evidence
- Panic, page-fault, and syscall paths must each have at least one directed test by the time they are introduced

## Documentation Baseline

Each major subsystem must maintain:

- purpose
- current status
- assumptions
- known limitations
- next implementation target

## Governance Rule

The product specification defines intent.

The instruction pack defines execution discipline.

This engineering baseline defines concrete implementation defaults for v1.

If a missing detail would otherwise force the assistant or a contributor to guess, this document should be updated before implementation continues.