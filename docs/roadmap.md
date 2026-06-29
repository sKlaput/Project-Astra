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

## v0.3 — Stability & Hardware
_Planned_

### 1. Better Memory Protection _(highest priority)_
Foundation for the entire future software ecosystem.
- Per-process page tables
- Real ring-3 isolation — user processes cannot corrupt kernel memory
- Guard pages, kernel/user address space split

### 2. APIC + SMP
Use modern CPUs properly.
- Local APIC timer calibration and switch probe complete; PIT remains the production tick source
- AP discovery, startup, interrupt initialisation, identity handshake, and parking complete in QEMU
- Next: per-core GDT/TSS/stacks and multicore scheduler participation
- Enables background tasks and better responsiveness

### 3. Improved Scheduler
Depends on SMP being in place.
- Better preemption and timeslicing
- Priority inheritance (already partially stubbed)
- Fair CPU distribution across cores

### 4. USB HID
Most visible user-facing improvement for real hardware.
- XHCI host controller driver
- USB keyboard and mouse (HID class)
- PS/2 remains fallback for QEMU

### 5. Real Hardware Testing _(start early, before everything above is done)_
Real hardware reveals problems QEMU hides.
- Timer and interrupt behaviour differences
- USB / framebuffer quirks
- Bootloader and memory-map assumptions
- Test incrementally as each subsystem lands

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
