# Astra OS

A from-scratch desktop operating system written entirely in Rust — `no_std`, bare-metal x86_64 — with a graphical desktop, real filesystem persistence, networking, a ring-3 user-process model, and a growing set of built-in apps.

> **Astra OS is a from-scratch Rust-first desktop operating system prototype focused on control, privacy, simplicity, and eventually gaming-capable personal computing.**

---

## Feature overview

| Feature | Status |
|---|---|
| Graphical desktop — taskbar, launcher panel, desktop icons | ✅ |
| Window manager — drag, resize (8 zones), focus, z-order, shadow | ✅ |
| **Terminal** — `ls`, `cd`, `cat`, `touch`, `mkdir`, `rm`, `rename`, `cp`, `mv`, `net`, `exec`, `ps`, `kill`, `mem`, `uptime`, `echo`, command history | ✅ |
| **File Manager** — browse, create, delete, rename, copy, cut/paste (Ctrl+C/X/V + right-click), error bar | ✅ |
| **Text Editor** — edit & save (Ctrl+S); status bar with path, dirty flag, line count | ✅ |
| **Calculator** — 4-function, fixed-point decimal, keyboard + mouse click | ✅ |
| **System Monitor** — live heap, uptime, scheduler stats | ✅ |
| **Settings** — system info and preferences | ✅ |
| FAT32 read-write filesystem (virtio-blk, 64 MiB `disk.img`) | ✅ |
| Long filename (LFN) support — names up to 65 chars | ✅ |
| Files survive reboot | ✅ |
| VFS layer — unified `fs::open` / `fs::read` / `fs::write` API | ✅ |
| **virtio-net NIC driver** — PCI scan, TX/RX virtqueues, MAC address, polling | ✅ |
| **Ring-3 user processes** — ELF64 loader, SYSCALL dispatch, `exec`/`ps`/`kill` | ✅ |
| Custom `no_std` heap allocator | ✅ |
| x86_64 interrupts — PIC, PIT timer, PS/2 keyboard, serial mouse | ✅ |
| Software framebuffer renderer (Limine linear framebuffer, 32-bpp) | ✅ |

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
- **qemu-img** on PATH (ships with QEMU)

### Build the kernel

```powershell
cargo build
```

Release build:

```powershell
cargo build --release
```

### Run in QEMU (graphical window)

```powershell
.\scripts\run-qemu.ps1 -Visual
```

### Run headless (serial output only)

```powershell
.\scripts\run-qemu.ps1
```

To pass a timeout (seconds) and capture output:

```powershell
.\scripts\run-qemu.ps1 -TimeoutSeconds 30 -LogPath build/boot.log
```

The `build/disk.img` (64 MiB FAT32) persists between runs. Delete it to get a fresh volume — it is auto-formatted on first boot.

---

## Terminal commands

Once the OS boots, open the Terminal app from the launcher or desktop icon.

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
| `mv <src> <dst>` | Move / rename file |
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
- **Right-click** for context menu: Open, Copy, Cut, Rename, Delete (folders), Paste, New File, New Dir.
- **Ctrl+C** / **Ctrl+X** / **Ctrl+V** — copy / cut / paste.
- Errors show as a red bar at the bottom; delete asks for confirmation.

---

## Repository layout

```
kernel/
  src/
    main.rs            — entry point, boot phases, subsystem init
    desktop.rs         — window manager, compositor, damage tracking
    terminal.rs        — Terminal app + shell command dispatch
    filemanager.rs     — File Manager app (browse, edit, clipboard)
    editor.rs          — Text Editor app
    calculator.rs      — Calculator app
    sysmonitor.rs      — System Monitor app
    settings.rs        — Settings app
    fat32.rs           — FAT32 driver (LFN, read-write, virtio-blk)
    fs.rs              — VFS layer
    framebuffer.rs     — pixel rendering primitives (fill_rect, draw_text)
    input.rs           — PS/2 keyboard + serial mouse pipeline
    loader.rs          — ELF64 loader (static binaries into ring-3)
    process.rs         — process table, spawn_elf_process, list_all
    scheduler.rs       — co-operative + preemptive task scheduler
    syscall.rs         — SYSCALL entry and dispatch table
    net/               — network subsystem (driver facade, stack stubs)
    drivers/
      virtio_blk.rs    — virtio-blk storage driver
      virtio_net.rs    — virtio-net NIC driver
    memory/            — frame allocator, page table, HHDM, heap
    arch/x86_64/       — GDT, IDT, interrupts, PIC, PIT, ring3, SYSCALL entry
    boot/              — Limine protocol helpers
scripts/
  run-qemu.ps1         — build + launch QEMU
  build-image.ps1      — assemble EFI boot image
docs/
  astra_os_roadmap.md  — project roadmap
```

---

## Architecture notes

- **No standard library** — `#![no_std]` throughout; no libc, no OS calls.
- **Software renderer** — writes directly to the Limine linear framebuffer; no GPU required.
- **FAT32** — 64 MiB raw `disk.img` over virtio-blk; LFN chains (up to 65-char names); auto-formatted on first boot.
- **VFS** — static node tree plus FAT32 backend; all apps share `fs::open` / `fs::read` / `fs::write`.
- **Networking** — virtio-net legacy PCI driver with RX/TX virtqueues and polling; higher-layer stack stubs are wired in and ready to expand.
- **User processes** — ELF64 loader maps PT_LOAD segments into ring-3 page tables; SYSCALL/SYSRET entry stub; SysV AMD64 ABI; `exec`, `ps`, `kill` exposed in terminal.
- **Scheduler** — fixed-size task table, cooperative yield + PIT preemption, priority levels, sleep timers.

---

## Roadmap

See [docs/astra_os_roadmap.md](docs/astra_os_roadmap.md) for the full plan.

**Done**

1. ~~Terminal commands~~ ✅
2. ~~File Manager error feedback~~ ✅
3. ~~Text Editor status bar~~ ✅
4. ~~GUI reliability pass~~ ✅
8. ~~Long filenames (LFN)~~ ✅
9. ~~Copy / Move files~~ ✅
10. ~~Networking (virtio-net)~~ ✅
11. ~~User processes / app runtime~~ ✅
12. ~~Calculator app~~ ✅

**Upcoming**

5. Full persistence demo (3 clean QA runs in QEMU)
6. ~~README~~ ✅ (this file)
7. GitHub publication
13. Long-term gaming path

---

## License

Proprietary — all rights reserved.


## What it does

Astra boots under QEMU/OVMF (UEFI) via the Limine bootloader and gives you a working desktop environment:

| Feature | Status |
|---|---|
| Graphical desktop with taskbar, launcher, icons | ✅ |
| Window manager — drag, resize (8 zones), focus, z-order | ✅ |
| **Terminal** — `ls`, `cd`, `cat`, `touch`, `mkdir`, `rm`, `rename`, `mem`, `uptime`, `echo`, command history | ✅ |
| **File Manager** — browse, create, delete, rename files and folders; FAT32 + VFS | ✅ |
| **Text Editor** — edit and save files with Ctrl+S; status bar with dirty/saved indicator | ✅ |
| **Settings** and **System Monitor** apps | ✅ |
| FAT32 read-write filesystem on a persistent virtio-blk disk | ✅ |
| Files survive reboot (written to `build/disk.img`) | ✅ |
| PS/2 keyboard + serial mouse input | ✅ |
| x86_64 interrupt handling, PIC, PIT timer | ✅ |
| Custom `no_std` heap allocator | ✅ |

---

## Building

### Prerequisites

- Rust **nightly** toolchain (`rustup toolchain install nightly`)
- QEMU with OVMF firmware (`qemu-system-x86_64` on PATH, OVMF at `C:\Program Files\qemu\share\edk2-x86_64-code.fd`)
- `qemu-img` on PATH

```powershell
rustup target add x86_64-unknown-none
rustup component add rust-src llvm-tools-preview
```

### Build the kernel

```powershell
cargo build
```

Or for a release build:

```powershell
cargo build --release
```

### Run in QEMU (visual window)

```powershell
.\scripts\run-qemu.ps1 -Visual
```

### Run headless (serial output only)

```powershell
.\scripts\run-qemu.ps1
```

The `disk.img` in `build/` persists between runs. Delete it to get a fresh FAT32 volume (auto-formatted on first boot).

---

## Repository layout

```
kernel/
  src/
    main.rs           — entry point, boot phases
    desktop.rs        — window manager and compositor
    terminal.rs       — terminal app and shell commands
    filemanager.rs    — file manager app
    editor.rs         — text editor app
    fat32.rs          — FAT32 driver (read-write, virtio-blk)
    fs.rs             — VFS layer bridging static nodes + FAT32
    framebuffer.rs    — pixel rendering primitives
    input.rs          — keyboard + mouse event pipeline
    memory/           — heap allocator, paging, HHDM
    arch/x86_64/      — interrupts, PIC, PIT, serial, power
scripts/
  run-qemu.ps1        — build + launch QEMU
  build-image.ps1     — assemble the boot image
docs/
  astra_os_roadmap.md — current project roadmap
```

---

## Architecture notes

- **No standard library** — `#![no_std]` throughout; libc does not exist here.
- **Single address space** — all kernel code and app code runs in ring 0 for now; a user-process model is planned.
- **FAT32** — files are stored on a 64 MiB raw `disk.img` (virtio-blk), formatted automatically if blank. Only 8.3 filenames currently (LFN planned).
- **VFS** — a static node tree overlays the FAT32 volume so the editor, terminal, and file manager all share one `fs::open` / `fs::write_file` API.
- **Rendering** — software rasteriser writing directly to the Limine framebuffer. No GPU acceleration.

---

## Roadmap

See [docs/astra_os_roadmap.md](docs/astra_os_roadmap.md) for the full plan.

Near-term priorities:
1. ~~Terminal commands~~ ✅
2. ~~File Manager error feedback~~ ✅
3. ~~Text Editor status bar~~ ✅
4. ~~GUI reliability pass~~ ✅
5. Full persistence demo (3 clean QA runs)
6. **README and screenshots** ← here
7. GitHub publication
8. Long filenames (LFN)
9. Copy / Move files

---

## License

Proprietary — all rights reserved.

## E9 Tripwire Commands

Advanced policy script (under the canonical gate):

`./scripts/validate-e9-tripwire.ps1 ...`

Quick gate (stable + user-deep):

`./scripts/validate-e9-tripwire.ps1 -RunIds @("A") -TimeoutSeconds 70 -OutPrefix "build/e9-tripwire"`

Stronger gate (A/B/C):

`./scripts/validate-e9-tripwire.ps1 -RunIds @("A","B","C") -TimeoutSeconds 70 -OutPrefix "build/e9-tripwire-abc"`

Include kernel-deep diagnostic lane:

`./scripts/validate-e9-tripwire.ps1 -RunIds @("A") -TimeoutSeconds 70 -OutPrefix "build/e9-tripwire-kernel" -IncludeKernelDeepLane`

Include kernel-deep as a required blocking lane:

`./scripts/validate-e9-tripwire.ps1 -RunIds @("A") -TimeoutSeconds 70 -OutPrefix "build/e9-tripwire-kernel-block" -IncludeKernelDeepLane -KernelDeepBlocking`

The script emits text and JSON summaries:
- `build/<prefix>-summary.txt`
- `build/<prefix>-summary.json`

Kernel-deep policy:
- default (`-IncludeKernelDeepLane` only): non-blocking lane, always reported in summaries
- strict (`-IncludeKernelDeepLane -KernelDeepBlocking`): lane is required for overall PASS