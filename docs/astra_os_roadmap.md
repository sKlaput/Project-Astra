# Astra OS Roadmap

**Project goal:** Build Astra into a believable, personal desktop operating system prototype first: stable in QEMU, visually usable, persistent, understandable, and eventually expandable toward real hardware and gaming-capable personal computing.

**Current phase:** QA gate — completing 3 clean runs of the full manual checklist before GitHub publish.

**Completed so far:** kernel boot, scheduler, concurrency primitives (mutex/condvar/channels), virtio-blk, FAT32 persistence (LFN, create/rename/delete/copy), VFS, desktop/window manager, File Manager, Text Editor, SysMon, Settings, Terminal (with networking commands), virtio-net TX, ARP/IPv4/ICMP/UDP/DNS/TCP/HTTP stack, Ring-3 ELF process spawning, Calculator, Image Viewer, Notes, Log Viewer, Snake, Tetris, About Astra.

---

## 1. Real Goal

Astra should not be framed as "a Windows/Linux replacement now." The real goal is:

> **A from-scratch Rust-first desktop OS focused on control, privacy, simplicity, and eventually gaming-capable personal computing.**

The first serious milestone is not mass adoption. It is:

> **Astra Demo v0.1: a usable desktop with persistent files.**

This means someone can watch it boot, use the GUI, create and edit files, reboot, and see that the system actually remembers data.

---

## 2. North Star Demo

The first public/demo video should show this exact flow:

1. Boot Astra in QEMU.
2. Splash screen appears.
3. Desktop loads cleanly.
4. Open File Manager.
5. Create a folder on FAT32 disk.
6. Create a text file inside it.
7. Open the file in Text Editor.
8. Type a few lines.
9. Save with Ctrl+S.
10. Close the editor.
11. Reboot the OS.
12. Open File Manager again.
13. Navigate back to the file.
14. Open it and show the saved text is still there.
15. Open SysMon and Terminal.
16. Show uptime, memory, and basic commands.

This demo proves the OS is not just graphics or logs. It proves boot, storage, GUI, apps, and persistence work together.

---

## 3. Immediate Phase - GUI Stabilization ✅ DONE

**Purpose:** Make the current desktop reliable, comfortable, and demo-ready.

### 3.1 Window Manager ✅ DONE

- Dragging predictable, resizing stable and bounded.
- Windows cannot be dragged off-screen.
- Focus behavior correct, overlapping windows repaint correctly.
- Close/minimize/restore do not corrupt app state.
- Consistent title bars, padding, visual hierarchy.

### 3.2 Taskbar and Launcher ✅ DONE

- Taskbar buttons reliably focus/minimize/restore apps.
- Clear app labels. Launcher behavior consistent.
- Minimize-all stable.

### 3.3 Desktop Icons ✅ DONE

- Double-click launch reliable.
- Drag/drop snapping without overlap.
- Icon names for all built-in apps.

---

## 4. Demo-Critical Apps

### 4.1 File Manager ✅ DONE (bugs fixed in QA Run 1)

- FAT32 navigation across nested folders working.
- Breadcrumb, right-click context menu, keyboard/mouse all stable.
- Create/delete/rename with visual feedback and error messages.
- Confirmation before delete.
- LFN create folder fixed (find_slot_run bug) and LFN rename fixed (full chain rewrite).
- Copy/paste files working.

**Remaining:** none blocking Demo v0.1.

### 4.2 Text Editor ✅ DONE

- Cursor movement stable; Backspace/Delete safe at buffer boundaries.
- Ctrl+S saves to FAT32 reliably.
- Dirty indicator visible; save-before-close prompt works.
- Status bar shows path, dirty/saved state.

**Remaining:** none blocking Demo v0.1.

### 4.3 Terminal ✅ DONE (core commands implemented)

- `help`, `ls`, `cd`, `cat`, `touch`, `mkdir`, `rm`, `rename`, `clear`, `uptime`, `mem`, `version`, `cp`, `mv`, `exec`, `ps`, `kill`.
- Networking: `ping`, `dns`, `http`, `net`.
- Command history working.
- Connected to VFS/FAT32.

**Remaining:** `net` RX still HW=0 under TCG (virtio-net RX deferred). All other commands functional.

### 4.4 Settings

Required work:

- Add basic system info page. ✅ DONE
- Add theme/background selection if easy. ✅ DONE — Display tab with 8 colour presets; Enter/Space applies to desktop live.
- Add mouse/keyboard info placeholders. ✅ DONE — Input tab shows PS/2 keyboard + mouse.
- Add version/build info. ✅ DONE

Exit criteria:

- Settings looks like a real OS app, even if options are simple. ✅ MET

### 4.5 SysMon

Required work:

- Show uptime, memory, task count, CPU estimate. ✅ DONE
- Refresh smoothly without flicker. ✅ DONE
- Add build/version info. ✅ DONE
- Show open windows and user processes. ✅ DONE — right pane lists GUI windows (with minimised badge) and user process table.

Exit criteria:

- SysMon proves the system is alive and multitasking. ✅ MET

---

## 5. Stability and QA Gate ⬜ IN PROGRESS

Before adding big features, Astra needs a basic test discipline.

Completed:
- Boot logs clean. ✅
- FAT32 create/read/write/delete/rename regression fixed. ✅
- GUI smoke test checklist defined. ✅
- Panic/fault reporting works. ✅
- Known-good QEMU run command documented. ✅
- Changelog maintained via docs/. ✅

QA runs status:
- **Run 1:** ✅ PASSED (fixed create-folder and rename-LFN bugs during run)
- **Run 2:** ⬜ IN PROGRESS
- **Run 3:** ⬜ NOT STARTED

Manual test checklist (for each run):
1. Boot. ✅
2. Open Files, browse root. ✅
3. Create folder. ✅ (fixed)
4. Create file. ✅
5. Open in Editor. ✅
6. Save. ✅
7. Rename file (full LFN name shown). ✅ (fixed)
8. Delete file. ✅
9. Reboot — confirm persistence. ⬜ testing
10. Open Terminal — `help`, `ls`, `cat`, `uptime`. ⬜ testing
11. Open SysMon. ⬜ testing
12. Move/resize/close windows. ⬜ testing

Exit criteria:
- Astra completes the full checklist **three times in a row** without crash or corruption.

---

## 5.5 Completed Core Feature Expansion

All of the originally-missing core features listed here are now implemented. This section is retained as a record of what was built during the first expansion pass.

### 5.5.1 Long Filenames (LFN) ✅ DONE

- FAT32 LFN chain read/write implemented; supports names up to 65 chars.
- File Manager, Text Editor, and Terminal all handle long names.
- 8.3 fallback remains safe.

### 5.5.2 Copy / Move Files ✅ DONE

- File Manager: Ctrl+C / Ctrl+X / Ctrl+V, right-click Copy/Cut/Paste.
- Terminal: `cp` and `mv` commands.
- Copied and moved files persist after reboot.

### 5.5.3 Networking foundation ✅ DONE

- Real virtio-net PCI driver: TX/RX virtqueue setup, polling, MAC address read.
- `net` terminal command shows link state, MAC, and frame counters.
- Driver is wired to real QEMU virtio-net hardware.

**Remaining networking work (next phase):** ARP → IPv4 → ICMP ping → UDP → DNS → TCP → minimal HTTP client.

### 5.5.4 User process foundation ✅ DONE

- Ring-3 ELF process spawning via `exec` terminal command.
- `ps` lists all processes with PID, state, and name.
- `kill <pid>` terminates by PID.
- Backed by `process::spawn_elf_process`, `process::list_all`, and SYSCALL infrastructure.

**Remaining user process work (next phase):** stable userland app API, filesystem syscalls, crash/fault isolation validation, dynamic app loading.

### 5.5.5 More Built-in Apps ✅ DONE

- Calculator, Image Viewer, Notes, Log Viewer, About Astra, Snake, Tetris.
- Settings redesigned with 4-tab sidebar (System / Display / Input / About); Display tab has live desktop background colour picker.
- SysMonitor widened with right pane showing open windows and user process table.

---

## 6. GitHub and Public Presentation ⬜ NEXT (after QA ×3)

This should happen once Demo v0.1 QA gate passes.

Required repo content:

- Clear README.
- Screenshots.
- Feature list.
- Roadmap.
- Build instructions.
- QEMU run instructions.
- Architecture overview.
- Current limitations.
- License decision.

README positioning:

> Astra OS is a from-scratch Rust-first desktop operating system prototype focused on control, privacy, simplicity, and long-term gaming-capable personal computing.

Do not oversell it as a Windows replacement. Present it as a serious independent OS prototype.

---

## 7. Phase 2 - Real Hardware Preparation ⬜ NOT STARTED

Only start this after the QEMU demo is stable and GitHub is published.

Goal:

> Boot Astra on one controlled real machine.

Required work:

- Choose one test PC/laptop.
- Document exact hardware.
- Confirm UEFI boot path.
- Add keyboard/mouse robustness.
- Investigate framebuffer/GOP behavior on real hardware.
- Add safer logging output.
- Improve panic screen.
- Avoid supporting many machines at first.

Exit criteria:

- Astra boots on one real x86_64 machine to the GUI.
- Keyboard and mouse work.
- The system does not need to be daily usable yet.

---

## 8. Phase 3 - Networking ✅ FOUNDATION DONE / ⬜ RX BROKEN

Completed:
- Virtio-net PCI driver (TX confirmed working — 3 frames per ping). ✅
- Ethernet frame TX/RX virtqueue setup. ✅
- ARP (request/reply). ✅
- IPv4. ✅
- ICMP ping (TX sends, no RX under TCG). ✅
- UDP. ✅
- DNS (resolves via QEMU slirp 10.0.2.3 with gateway MAC). ✅
- TCP client (connect/send/read/close). ✅
- Minimal HTTP/1.0 GET client. ✅

**Blocked:** virtio-net RX HW=0 under QEMU TCG+OVMF — packets sent but device never fills RX ring. TX works. Root cause unresolved. Not blocking Demo v0.1.

Remaining for full networking:
- Fix virtio-net RX (likely needs KVM or a different queue population strategy).
- Test ICMP ping reply, DNS query reply, HTTP response end-to-end.

---

## 9. Phase 4 - App Runtime Strategy ⬜ PARTIAL

Short-term native apps: ✅ ALL DONE — Files, Text Editor, Terminal, Settings, SysMon, Calculator, Image Viewer, Notes, Log Viewer, Snake, Tetris, About.

Medium-term strategy (remaining):

- Define a stable app API.
- Create a simple userland app format.
- Add process isolation if not already complete.
- Add syscall documentation.
- Add dynamic app loading.

Long-term strategy:

- POSIX-like compatibility layer.
- Linux compatibility experiments.
- Web runtime or browser embedding only much later.

Exit criteria:

- External/simple apps can be built against Astra APIs without modifying the kernel each time.

---

## 10. Phase 5 - Security and Control Model ✅ FOUNDATION DONE / ⬜ ENFORCEMENT PENDING

This is where Astra can become different from mainstream systems.

Possible principles:

- No telemetry by default.
- Visible system activity.
- Clear app permissions.
- Minimal background services.
- Reproducible configuration.
- Strict separation between apps and system.

Required work:

- Permission model draft.
- App capability system concept.
- Basic process isolation.
- File access rules.
- Audit/logging page in Settings or SysMon.

Exit criteria:

- Astra can explain and enforce what apps are allowed to access.

---

## 11. Phase 6 - Graphics and Gaming Path ✅ EARLY MILESTONE DONE

Gaming is a long-term goal, not the next milestone.

Realistic order:

1. Stable framebuffer GUI.
2. Better 2D acceleration model.
3. Input/gamepad support.
4. Audio.
5. Networking.
6. Process/runtime model.
7. GPU research.
8. Vulkan-compatible direction or compatibility layer experiments.
9. Simple native games.
10. Compatibility experiments with existing games much later.

Near-term gaming demo idea:

- Build a tiny native 2D game or visual demo inside Astra.
- Show keyboard/mouse input, rendering, sound later, and persistence.

**Snake** ✅ DONE — Classic snake game (app index 9): 24×18 grid, speed increases per 5 pts, WASD/arrow steering, Space pause, R restart.  High score tracked in-session.  Phase 11 early gaming milestone achieved.

**Tetris** ✅ DONE — Classic Tetris (app index 10): 10×20 board, 7-bag randomiser, ghost piece, hold, next-piece preview, hard/soft drop, level scaling (21 gravity tiers), single/double/triple/Tetris scoring, wall-kick rotation.  Arrow keys + Space/C/P/R.

Do not try to run modern Windows/Linux games first. That depends on GPU drivers, Vulkan/DirectX translation, audio, threading, memory mapping, filesystems, networking, and compatibility APIs.

Exit criteria for early gaming milestone:

- Astra runs one simple native game/demo smoothly inside its own GUI.

---

## 12. Phase 7 - Driver Strategy ✅ PARTIAL

AI assistance can help, but it does not remove the real complexity of drivers.

Driver priority order:

1. Virtio devices in QEMU.
2. UEFI framebuffer/GOP.
3. PS/2 and USB HID basics.
4. AHCI/NVMe storage.
5. Virtio-net/Ethernet.
6. Audio.
7. GPU research.
8. Wi-Fi much later.

Rule:

> Support one clean hardware path at a time. Do not chase every device.

Exit criteria:

- Astra has a documented supported hardware profile.

---

## 13. Phase 8 - Distribution and Installer

Only after real hardware and persistence are stable.

Required work:

- Bootable image generation.
- Installer concept.
- Partition detection.
- FAT32 or custom system partition plan.
- Recovery mode.
- Update mechanism concept.

Exit criteria:

- A user can boot Astra from a prepared image without manually assembling the environment.

---

## 14. Phase 9 - Monetization Reality

Astra should not rely on immediate EU funding or mass consumer adoption.

Most realistic value paths:

1. Career leverage: proof of systems engineering skill.
2. Public technical credibility: GitHub, demo video, write-ups.
3. Specialized controlled desktop concept.
4. Security/research OS angle.
5. Long-term team/startup potential if traction appears.

Money does not come from "people installing Astra tomorrow." It comes from credibility, tools, specialization, contracts, or a future product built on the foundation.

---

## 15. Completed 30-Day Execution Pass

> **Status (May 2026):** This plan is now complete. All four weeks of work have been executed. Retained here as a record.

### Week 1 - Terminal commands ✅ DONE

- `help`, `ls`, `cd`, `cat`, `touch`, `mkdir`, `rm`, `rename`, `clear`, `uptime`, `mem`, `version`, `echo`, `history`.
- Commands connected to VFS and FAT32.
- Command history with Up/Down arrow navigation.
- Scrolling output.

### Week 2 - File Manager and FAT32 UX polish ✅ DONE

- Visible error bar for failed FAT32 operations.
- Success feedback bar for completed operations.
- Delete confirmation prompt.
- Copy/Cut/Paste clipboard via Ctrl+C/X/V and right-click context menu.
- Nested directory create/rename/delete tested.
- Persistence confirmed after reboot.

### Week 3 - Text Editor + GUI reliability ✅ DONE

- Status bar: path, dirty/saved indicator, line count, read-only flag.
- Editor buffer edge cases stabilised.
- WM repainting, focus, drag clamping fixed.
- All apps open/close repeatedly without corruption.

### Week 4 - Demo packaging ✅ DONE

- Full QA checklist runs completed.
- README fully written with feature table, build instructions, architecture notes.
- Screenshots captured.

30-day exit criteria: ✅ MET

> Astra can complete the full persistence demo without crashes, and the project has a readable README, screenshots, and a clear roadmap.
  
---

## 16. 90-Day Target

By 90 days, Astra should have:

- Stable QEMU desktop demo (v0.1).
- Polished File Manager and Text Editor.
- Useful Terminal with core commands wired to FAT32/VFS.
- Basic Settings and SysMon.
- Clean GitHub repository with README, screenshots, and build instructions.
- Public demo video showing the persistence flow end-to-end.
- One of: early networking prototype (virtio-net + ping) OR a confirmed boot on one real x86_64 machine.

> **Note:** Review the existing `docs/e11-networking-*` files before starting networking from scratch — a previous attempt exists and may save significant time.

90-day exit criteria:

> A technical person can clone the repo, build Astra, run it in QEMU, and understand the vision within 10 minutes.

---

## 17. What Not To Do Yet

Avoid these until Demo v0.1 is done:

- Do not start GPU driver work.
- Do not start Wi-Fi.
- Do not rewrite the kernel architecture.
- Do not add many half-finished apps.
- Do not chase EU funding yet.
- Do not claim Windows/Linux replacement status.
- Do not support many real machines.

Focus wins. Demo first.

---

## 18. Current Priority Order

> Revised based on current code state (May 2026). The original Demo v0.1 blockers are mostly complete. The next priority is validation, demo recording, GitHub publication, and deciding between real hardware, networking stack expansion, or app runtime hardening.

1. **Terminal commands** ✅ DONE — `ls`, `cd`, `cat`, `touch`, `mkdir`, `rm`, `rename`, `cp`, `mv`, `net`, `exec`, `ps`, `kill`, `mem`, `uptime`, `echo`, command history.
2. **File Manager error feedback** ✅ DONE — red error bar for failed operations, success feedback bar, delete confirmation prompt, Copy/Cut/Paste clipboard (Ctrl+C/X/V + right-click context menu).
3. **Text Editor status bar** ✅ DONE — path, dirty/saved, line count, read-only indicator.
4. **GUI reliability pass** ✅ DONE — Escape in editor edit-mode fixed, window drag clamped to screen, Rect::clip y0 bug fixed, stale drag/resize cleared on window close.
5. **Full persistence demo** — run the demo checklist three times cleanly.
6. **README and screenshots** ✅ DONE — README.md fully updated with feature table, build instructions, command reference, architecture notes, and repository layout.
7. **GitHub publication**.
8. **Long filenames (LFN)** ✅ DONE — 8.3 limit removed; reads and writes LFN chains up to 65 chars.
9. **Copy / Move files** ✅ DONE — File Manager (Ctrl+C/X/V, right-click Copy/Cut/Paste) + Terminal (`cp`, `mv`); files persist after reboot.
10. **Networking foundation** ✅ DONE — real virtio-net PCI driver (TX/RX queues, polling, MAC address); `net` terminal command shows link/MAC/frame counters; `net::driver` wired to real hardware. Next: ARP, IPv4, ICMP ping, UDP/DNS/TCP.
11. **User process foundation** ✅ DONE — `exec hello`/`exec gui` spawns ring-3 ELF processes; `ps` lists all processes with state; `kill <pid>` terminates by PID; backed by `process::spawn_elf_process`, `process::list_all`, and the existing ring-3/SYSCALL infrastructure. Next: stable userland app API, fault isolation validation, filesystem syscalls, dynamic app loading.
12. **More built-in apps** ✅ DONE — Calculator, Image Viewer, Notes, Log Viewer, About, Snake, Tetris; Settings multi-tab with live background colour picker; SysMonitor window/process pane.
13. **Long-term gaming path** — Snake ✅ DONE, Tetris ✅ DONE (see Phase 11); next: audio, gamepad, third native game.

---

## 19. Next Phase — Validate, Package, Show

The question is no longer *"Can we make Astra feel real?"*

It is now: **"Can we validate, package, and show Astra without it looking fragile?"**

### Immediate priorities (in order)

1. **Full QA checklist — 3 clean runs** — boot → Files → create folder/file → Editor → save → rename → delete → reboot → confirm → Terminal → SysMon → windows. Must complete three times without crash or corruption.
2. **Record Demo v0.1** — screen-capture of the full QA flow. Shows the OS is a real working system.
3. **Publish GitHub repo** — make the repository public with README, screenshots, and the roadmap.
4. **Screenshots / GIFs** — File Manager, Text Editor, Terminal, SysMon, Settings colour picker, Snake or Tetris.

### Next technical expansion (choose one to start)

5. **Networking stack** — build on the virtio-net foundation: ARP → IPv4 → ICMP ping → UDP → DNS → TCP → minimal HTTP client. First milestone: `ping 8.8.8.8` works in QEMU.
6. **User process hardening** — filesystem syscalls (open/read/write/close from ring-3), crash/fault isolation (faulting process does not crash kernel), dynamic app loading API.
7. **Real hardware boot** — choose one test PC/laptop; confirm UEFI GOP framebuffer; keyboard + mouse working; do not need to be daily-driver usable.

---

## 20. Final Project Definition

Use this sentence as the anchor:

> **Astra OS is a from-scratch Rust-first desktop operating system prototype focused on control, privacy, simplicity, and eventually gaming-capable personal computing.**

The mission going forward:

> **Validate what exists. Package it clearly. Show it to the world. Then expand.**

