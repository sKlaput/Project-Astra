# Astra OS — Project Status

## What this is

A from-scratch desktop operating system written in Rust (`no_std`, bare-metal x86_64).
No Linux. No libc. No existing kernel. Boots via UEFI using the Limine bootloader and runs
under QEMU with a graphical desktop.

---

## What works (confirmed working as of June 2026)

### Boot & Core
- UEFI boot via Limine on QEMU (x86_64)
- Custom `no_std` heap allocator
- x86_64 interrupt handling — PIC, PIT timer @ 100 Hz
- PS/2 keyboard driver
- Serial mouse driver
- Software framebuffer renderer (writes directly to Limine linear framebuffer, 32-bpp)

### Filesystem
- FAT32 read/write driver over virtio-blk
- Long filename (LFN) support — names up to 65 characters
- Directory create, file create, read, write, delete, rename
- Files and folders survive reboot (persisted to `build/disk.img`)
- VFS abstraction layer (`fs::open` / `fs::read` / `fs::write`) unified across FAT32 and static nodes

### Desktop & Window Manager
- Graphical desktop with taskbar and launcher panel
- Desktop icons — drag, drop, rename, double-click to open
- Desktop items stored in a dedicated `Desktop/` folder on FAT32
- Desktop layout (icon positions) persists across reboots via `DESKSTAT` file
- Window manager — drag, resize (8 grab zones), focus, z-order, minimize, shadow
- Damage-tracked rendering (only redraws changed regions)
- Right-click context menus on desktop and in apps

### Apps
| App | What it does |
|---|---|
| **Terminal** | `ls`, `cd`, `cat`, `touch`, `mkdir`, `rm`, `rename`, `cp`, `mv`, `exec`, `ps`, `kill`, `mem`, `uptime`, `echo`, command history |
| **File Manager** | Browse FAT32 + VFS, create/delete/rename files and folders, copy/cut/paste (Ctrl+C/X/V + right-click), breadcrumb navigation |
| **Text Editor** | Open, edit, save files (Ctrl+S); view mode for read-only files; dirty flag, unsaved-changes prompt on close |
| **Notes** | Scratchpad auto-saved to `notes.txt` on FAT32 root |
| **Calculator** | 4-function calculator, keyboard + mouse |
| **System Monitor** | Live heap usage, uptime, scheduler stats (250 ms auto-refresh) |
| **Settings** | System info, display theme picker |
| **About** | Project info |
| **Snake** | Playable Snake game |
| **Tetris** | Playable Tetris game |
| **Image Viewer** | View images |
| **Log Viewer** | View system log output |

### Networking
- virtio-net legacy PCI NIC driver
- TX/RX virtqueues, MAC address detection, frame polling
- ARP, IPv4, ICMP, UDP, DNS, TCP, and HTTP client support
- `net`, `ping`, `dns`, `http`, and `netcheck` terminal diagnostics

### User Processes
- ELF64 binary loader — maps PT_LOAD segments into ring-3 page tables
- SYSCALL/SYSRET entry (SysV AMD64 ABI)
- `exec`, `ps`, `kill` commands in terminal

### Scheduler
- Fixed-size task table
- Cooperative yield + PIT preemption
- Priority levels, sleep timers
- Limine SMP discovery and AP startup handshake validated with two QEMU CPUs
- APs initialise CPU/interrupt state and park while multicore scheduling is developed

---

## Known limitations

- Runs in QEMU only (no physical hardware testing yet)
- Scheduler execution remains BSP-only; application processors are parked after bootstrap
- No audio
- Single user, no permissions model
- Font is a fixed 6×8 bitmap (no TrueType)
- Screen resolution fixed at 1280×800

---

## Is it ready to show?

**Yes.** The core demo path works end-to-end:

1. Boot → graphical desktop appears
2. Create a file on the desktop → name it → double-click → opens in editor → type → Ctrl+S
3. Reboot → file is still there, desktop layout is preserved
4. Open File Manager → navigate folders → create/rename/delete files
5. Open Terminal → `ls`, `cat` the file you just created from the editor

That is a complete, self-consistent demo that no existing "hobby OS" tutorial produces.

---

## What is not ready

- No installer / physical hardware support
- No networking beyond driver level
- No multi-user or security model
- Several apps are functional but minimal (Image Viewer, Log Viewer)
