# Astra OS

A from-scratch desktop operating system written entirely in Rust — `no_std`, bare-metal x86_64 — with a graphical desktop, real filesystem persistence, user processes, and a growing set of built-in apps.

> **Astra OS is a from-scratch Rust-first desktop OS prototype focused on control, privacy, and simplicity.**

---

## Features

| Feature | Status |
|---|---|
| Graphical desktop — taskbar, launcher panel, persistent desktop icons | ✅ |
| Window manager — drag, resize (8 zones), focus, z-order, drop shadow | ✅ |
| **Terminal** — `ls`, `cd`, `cat`, `touch`, `mkdir`, `rm`, `rename`, `cp`, `mv`, `net`, `exec`, `ps`, `kill`, `mem`, `uptime`, `echo`, command history | ✅ |
| **File Manager** — browse, create, delete, rename, copy, cut/paste (Ctrl+C/X/V + right-click) | ✅ |
| **Text Editor** — edit & save (Ctrl+S); status bar with path, dirty flag, line count | ✅ |
| **Notes** — persistent scratchpad, auto-saves to FAT32 | ✅ |
| **Calculator** — 4-function, fixed-point decimal, keyboard + mouse | ✅ |
| **System Monitor** — live heap, uptime, scheduler stats | ✅ |
| **Settings** — system info panel | ✅ |
| FAT32 read-write filesystem (virtio-blk, 64 MiB `disk.img`), LFN support (up to 65-char names) | ✅ |
| Files survive reboot | ✅ |
| VFS layer — unified `fs::open` / `fs::read` / `fs::write` API | ✅ |
| virtio-net NIC driver — PCI scan, TX/RX virtqueues, MAC, polling | ✅ |
| Ring-3 user processes — ELF64 loader, SYSCALL dispatch, `exec`/`ps`/`kill` | ✅ |
| Custom `no_std` heap allocator | ✅ |
| x86_64 — PIC, PIT timer, PS/2 keyboard, serial mouse, UEFI boot via Limine | ✅ |

---

## Building

### Prerequisites

- **Rust nightly** toolchain
  ```powershell
  rustup toolchain install nightly
  rustup target add x86_64-unknown-none
  rustup component add rust-src llvm-tools-preview
  ```
- **QEMU** with OVMF (`qemu-system-x86_64` on PATH)
  Default OVMF path: `C:\Program Files\qemu\share\edk2-x86_64-code.fd`
- **`qemu-img`** on PATH (ships with QEMU)

### Build

```powershell
cargo build
# or
cargo build --release
```

### Run in QEMU

```powershell
# Graphical window
.\scripts\run-qemu.ps1 -Visual

# Headless (serial output only)
.\scripts\run-qemu.ps1

# With timeout and log capture
.\scripts\run-qemu.ps1 -TimeoutSeconds 30 -LogPath build/boot.log
```

`build/disk.img` (64 MiB FAT32) persists between runs. Delete it to get a fresh volume — it is auto-formatted on first boot.

---

## Terminal commands

Open the Terminal app from the launcher (bottom bar) or double-click its desktop icon.

| Command | Description |
|---|---|
| `help` | List all commands |
| `ls [path]` | List directory |
| `cd <dir>` | Change directory |
| `cat <file>` | Print file contents |
| `touch <name>` | Create empty file |
| `mkdir <name>` | Create directory |
| `rm <name>` | Delete file or folder |
| `rename <old> <new>` | Rename entry |
| `cp <src> <dst>` | Copy file |
| `mv <src> <dst>` | Move/rename file |
| `net` | Network status (NIC, MAC, TX/RX frames) |
| `exec hello\|gui` | Spawn a ring-3 ELF user process |
| `ps` | List all processes |
| `kill <pid>` | Terminate process by PID |
| `mem` | Heap usage |
| `uptime` | Time since boot |
| `echo <text>` | Print text |
| ↑ / ↓ | Navigate command history |

---

## File Manager

- **Double-click** a folder or file to open it.
- **Right-click** for context menu: Open, Copy, Cut, Rename, Delete, Paste, New File, New Folder.
- **Ctrl+C** / **Ctrl+X** / **Ctrl+V** — copy / cut / paste.
- Errors appear as a red bar at the bottom; delete always asks for confirmation.

---

## Architecture

- **No standard library** — `#![no_std]` throughout; no libc, no OS calls.
- **Software renderer** — writes directly to the Limine linear framebuffer; no GPU required.
- **FAT32** — 64 MiB raw `disk.img` over virtio-blk; LFN chains (up to 65-char names); auto-formatted on first boot. Desktop icon positions saved in `Desktop/DESKSTAT`.
- **VFS** — static node tree plus FAT32 backend; all apps share `fs::open` / `fs::read` / `fs::write`.
- **Networking** — virtio-net legacy PCI driver with RX/TX virtqueues and polling; higher-layer stack stubs ready to expand.
- **User processes** — ELF64 loader maps PT_LOAD segments into ring-3 page tables; SYSCALL/SYSRET; SysV AMD64 ABI.
- **Scheduler** — fixed-size task table, cooperative yield + PIT preemption, priority levels, sleep timers.

### Limitations

- Runs in QEMU only (x86_64 + OVMF). Physical hardware not tested.
- Single CPU core. No SMP.
- No networking stack above the driver layer yet (ARP/IP planned for v0.2).
- Ring-3 user programs are hand-assembled ELF stubs, not compiled from C/Rust.
- No audio, no USB, no GPU acceleration.

---

## Repository layout

```
kernel/src/
  main.rs            — entry point, boot phases, subsystem init
  desktop.rs         — window manager, compositor, damage tracking
  terminal.rs        — Terminal app + shell command dispatch
  filemanager.rs     — File Manager app (browse, clipboard, FAT32)
  editor.rs          — Text Editor app
  notes.rs           — Notes scratchpad app
  calculator.rs      — Calculator app
  sysmonitor.rs      — System Monitor app
  settings.rs        — Settings app
  fat32.rs           — FAT32 driver (LFN, read-write, virtio-blk)
  fs.rs              — VFS layer
  framebuffer.rs     — pixel rendering primitives
  input.rs           — PS/2 keyboard + serial mouse pipeline
  loader.rs          — ELF64 loader (static binaries into ring-3)
  process.rs         — process table, spawn_elf_process
  scheduler.rs       — cooperative + preemptive task scheduler
  syscall.rs         — SYSCALL entry and dispatch table
  net/               — network subsystem (driver facade, stack stubs)
  drivers/
    virtio_blk.rs    — virtio-blk storage driver
    virtio_net.rs    — virtio-net NIC driver
  memory/            — frame allocator, page tables, HHDM, heap
  arch/x86_64/       — GDT, IDT, interrupts, PIC, PIT, ring3, SYSCALL
  boot/              — Limine protocol helpers
scripts/
  run-qemu.ps1       — build + launch QEMU
  build-image.ps1    — assemble EFI boot image
docs/
  astra_os_roadmap.md — full project roadmap
```

---

## Roadmap

| Milestone | Status |
|---|---|
| v0.1 — Persistent desktop, full app suite, user processes | ✅ Done |
| v0.2 — ARP → IPv4 → ICMP → UDP → DNS → TCP → HTTP GET | 🔧 In progress |
| v0.3 — Physical hardware, multi-core | 🗓 Planned |

See [docs/astra_os_roadmap.md](docs/astra_os_roadmap.md) for details.

---

## License

Proprietary — all rights reserved.
