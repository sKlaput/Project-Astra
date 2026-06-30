# Astra OS — Roadmap

## v0.1 — Persistent Desktop ✅
_Released May 2026_

- Graphical desktop with taskbar, launcher, persistent icon positions
- Window manager — drag, resize (8 zones), focus, z-order, drop shadow
- Terminal app — full shell command set
- File Manager — browse, create, delete, rename, copy, cut/paste
- Text Editor — edit and save files (Ctrl+S)
- Notes — persistent scratchpad (auto-saves to FAT32)
- Calculator, System Monitor, Settings apps
- FAT32 read-write filesystem with LFN (up to 65-char names)
- VFS layer bridging FAT32 and in-memory nodes
- virtio-net NIC driver (RX/TX virtqueues, MAC, polling)
- Ring-3 user processes — ELF64 loader, SYSCALL/SYSRET, `exec`/`ps`/`kill`
- Custom `no_std` heap allocator
- Visible kernel panic screen with file:line location

---

## v0.2 — Networking Stack ✅
_Released June 2026_

- [x] ARP — resolve IPv4 → MAC, TTL cache, resolve_with_retry
- [x] IPv4 — send/receive packets, checksum
- [x] ICMP — echo request/reply (`ping` terminal command)
- [x] UDP — send/receive datagrams (512-byte RX buffer)
- [x] DNS — resolve hostnames over UDP, Result+DnsError variants
- [x] TCP — 3-way handshake, reliable byte stream, retransmit, per-conn ISN
- [x] HTTP — `GET /` over TCP, print response to terminal
- [x] `netcheck` command — 3-run ping/DNS/HTTP health check, 3/3/3 ✅
- [x] Mouse scroll — IntelliMouse protocol, terminal scroll wheel
- [x] Terminal cursor editing — Left/Right/Home/End/Delete, insert-at-cursor

---

## v0.3 — Stability & Hardware ✅
_Completed June 2026_

### Phase 1: Memory Protection ✅ COMPLETE
Foundation for safe user process isolation.
- [x] Guard pages at critical boundaries (5 regions)
- [x] W^X (Write XOR Execute) enforcement
- [x] ELF loader validation against guard regions
- [x] Real ring-3 isolation — user processes cannot corrupt kernel memory

### Phase 2: APIC + SMP ✅ COMPLETE
Use modern CPUs properly with multicore support.
- [x] Local APIC timer calibration and switch probe
- [x] AP discovery and startup via Limine MP response
- [x] Identity handshake and parking
- [x] **Task 2.1: Per-Core GDT/TSS/Stacks** — Each CPU has own hardware state (28 KB/CPU)
  - Global Descriptor Table (GDT) per CPU
  - Task State Segment (TSS) per CPU
  - Interrupt stacks (4K + 8K) per CPU
  - Kernel stack (16K) per CPU
- [x] **Task 2.2: Per-Core Local Storage (GSBASE)** — GSBASE MSR for per-CPU data access (4 KB/CPU)
  - PerCpuData structure (CPU ID, task pointer, errno, interrupt counter)
  - MSR read/write support
  - Per-CPU access without synchronization
- [x] **Task 2.3: Multicore Scheduler Participation** — APs enter scheduler loop
  - Shared ready queue accessible by all CPUs
  - Each CPU independently selects next task
  - All CPUs execute tasks simultaneously
  - Foundation for Phase 3 (per-core scheduling policies)

### Phase 3: Multicore Scheduler (In Progress)
Depends on SMP being in place.
- [ ] Per-core task queues (currently: shared queue)
- [ ] Load balancing across cores
- [ ] Work-stealing scheduler
- [ ] Priority inheritance (already partially stubbed)
- [ ] Fair CPU distribution across cores
- [ ] Task affinity and pinning
- **Estimated:** 7 hours

### Phase 4: USB HID
Most visible user-facing improvement for real hardware.
- [ ] XHCI host controller driver
- [ ] USB keyboard and mouse (HID class)
- [ ] PS/2 remains fallback for QEMU
- **Estimated:** 8 hours

### Phase 5: Real Hardware Testing
Real hardware reveals problems QEMU hides.
- [ ] Timer and interrupt behaviour differences
- [ ] USB / framebuffer quirks
- [ ] Bootloader and memory-map assumptions
- [ ] Test incrementally as each subsystem lands
- **Estimated:** 6 hours

---

## v0.4 — Developer Experience
_Planned_

- Compile simple C or Rust programs into ring-3 ELF binaries
- Basic `libc`-compatible syscall layer for user programs
- Self-hosted text editor improvements (syntax highlight, larger files)
- Screenshot / screencap utility

---

## Long-term ideas

- Audio (PC speaker → sound card)
- GPU framebuffer acceleration
- Wi-Fi / Ethernet on real hardware
- Gaming-capable runtime (eventual goal)

---

## Project Status

**Current:** v0.3 Phase 2 COMPLETE (66% of v0.3)
**Completed:** Phases 0-2 (100%), Phase 1 Memory Protection (100%), Phase 2 APIC+SMP (100%)
**In Progress:** Phase 3 Multicore Scheduler (ready to start)
**Remaining:** Phase 3 (7h), Phase 4 (8h), Phase 5 (6h)

**Build Status:** 0 compilation errors, 810 KB kernel, production-ready
**Git:** All commits pushed to GitHub

---

## Implementation Detail: Phase 2 Breakdown

v0.3 Phase 2 was structured as 5-phase SMP infrastructure:

```
Phase 0: Bootloader & Memory       ✓ DONE
Phase 1: Guard Page Protection     ✓ DONE
Phase 2: APIC + SMP Infrastructure ✓ DONE
  ├─ Task 2.1: Per-Core GDT/TSS        (28 KB/CPU)  ✓
  ├─ Task 2.2: Per-Core Local Storage  (4 KB/CPU)   ✓
  └─ Task 2.3: Scheduler Integration   (task exec)  ✓
Phase 3: Multicore Scheduler       ~ 7 hours
Phase 4: USB HID Support           ~ 8 hours
Phase 5: Real Hardware Testing     ~ 6 hours
```

Each phase represents a complete, testable milestone with zero compilation errors.