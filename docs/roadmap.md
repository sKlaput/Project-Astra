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

## v0.2 — Networking Stack 🔧
_In progress_

Goal: make `ping` work from the terminal, then basic HTTP GET.

- [ ] ARP — resolve IPv4 → MAC, reply to incoming ARP requests
- [ ] IPv4 — send/receive packets, checksum, fragmentation-free path
- [ ] ICMP — echo request/reply (`ping` terminal command)
- [ ] UDP — send/receive datagrams
- [ ] DNS — resolve hostnames over UDP
- [ ] TCP — 3-way handshake, reliable byte stream
- [ ] HTTP — `GET /` over TCP, print response to terminal

---

## v0.3 — Stability & Hardware
_Planned_

- Test on real x86_64 hardware (not just QEMU)
- APIC + multi-core (SMP) support
- PS/2 → USB HID keyboard/mouse
- Better memory protection (per-process page tables, proper ring-3 isolation)
- Improved scheduler (priority inheritance, better preemption)

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
