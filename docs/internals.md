# Astra OS — Internals Reference

Technical notes for contributors. Covers decisions that aren't obvious from reading the code.

---

## Boot flow

1. UEFI firmware loads Limine
2. Limine loads the kernel ELF and supplies framebuffer info, memory map, boot responses
3. Architecture entry — minimal CPU state validation
4. Serial logging starts (COM1, 115200 baud)
5. Framebuffer console starts
6. GDT and IDT installed
7. Physical frame allocator initialized from boot memory map
8. Kernel page tables installed (higher-half)
9. Heap allocator initialized
10. PIC remapped, PIT timer started (100 Hz)
11. Scheduler starts
12. SYSCALL entry enabled
13. Desktop / apps launch

---

## Address space

| Region | Virtual address |
|---|---|
| Kernel higher-half base | `0xFFFFFFFF_80000000` |
| Kernel heap | Fixed virtual region, expanded in page granularity |
| User text/data/stack | Lower canonical range (`0x0000_0000_00400000` and up) |
| Null page | Always unmapped |

- 4-level paging (48-bit canonical addresses)
- Kernel mappings are global to all processes
- User mappings are per-process (separate page tables)
- 4 KiB frame size; bitmap frame allocator

---

## Syscall ABI

Entry: `syscall` instruction. Return: `sysretq`.

| Register | Role |
|---|---|
| `rax` | Syscall number (in) / return value (out) |
| `rdi` | arg0 |
| `rsi` | arg1 |
| `rdx` | arg2 |
| `r10` | arg3 |
| `r8` | arg4 |
| `r9` | arg5 |

Error convention: negative value in `rax`.  
No raw kernel pointers are ever exposed to user space.

### Syscall table (current)

| Number | Name | Description |
|---|---|---|
| 4 | `SYS_TICKS` | Current tick count |
| 19 | `SYS_WRITE_CONSOLE` | Print to serial/terminal |
| 20 | `SYS_YIELD` | Yield scheduler slice |
| 21 | `SYS_EXIT` | Terminate process |
| 22 | `SYS_SEND_MSG` | Send message to kernel |
| 24 | `SYS_GET_FB_INFO` | Query framebuffer dimensions |
| 25 | `SYS_DRAW_RECT` | Draw filled rectangle |
| 26 | `SYS_DRAW_PIXEL` | Draw single pixel |

---

## Memory management

- **Frame allocator**: bitmap over the entire physical memory map supplied by Limine
- **Kernel heap**: single virtual region, expanded page-by-page on demand; backed by the frame allocator
- **User page tables**: allocated per-process at `spawn_elf_process` time; PT_LOAD segments are mapped read/execute; user stack is mapped separately
- Kernel text and rodata are read-only after paging stabilizes
- Guard pages on kernel stacks and user stacks

---

## Scheduler

- Cooperative + PIT preemption (100 Hz tick → ~10 ms slices)
- BSP-only scheduling; application processors complete their Limine bootstrap handshake and then park
- Fixed-size task table (`MAX_TASKS`)
- Task states: `Runnable`, `Blocked`, `Sleeping(wake_tick)`, `Terminated`
- User tasks enter the same scheduler as kernel tasks; dispatch switches to ring 3 via `iretq` trampoline, returns through SYSCALL/exception
- Priority levels exist but currently all tasks share one class

---

## ELF loader

- Format: ELF64 static executables, little-endian x86_64
- Handles: `ET_EXEC`, up to `MAX_PT_LOAD` segments of type `PT_LOAD`
- Maps each PT_LOAD segment page-by-page into the calling process's page tables
- Returns entry RIP or `Err(LoadError)` — never panics on bad input
- Stack setup: caller provides `USER_TASK_STACK_VIRT` + size; loader does not set up `argc`/`argv` yet

---

## FAT32 / filesystem

- 64 MiB raw `build/disk.img` over virtio-blk, auto-formatted on first boot
- LFN chains supported (up to 65-char names in the driver; 28-char cap enforced in the UI)
- `Desktop/DESKSTAT` stores icon positions; `Desktop/` subfolder holds all desktop file entries
- VFS node IDs:
  - `0..4` — static built-in nodes
  - `>= 100` (`DYN_ID_BASE`) — dynamic in-memory nodes
  - `>= 0x4000` (`FAT32_ID_BASE`) — FAT32-backed nodes

---

## Hardware bring-up order

The order below matters — each step depends on the one above being stable.

1. Serial output
2. Framebuffer console
3. Memory map parsing (Limine)
4. GDT + IDT
5. PIC remap + PIT timer
6. PS/2 keyboard interrupt
7. PCI enumeration
8. virtio-blk (disk)
9. virtio-net (NIC)
10. Higher network layers (v0.2+)

---

## Open decisions / known gaps

| Topic | Status |
|---|---|
| Physical hardware testing | Not done — QEMU only |
| SMP / multi-core | Explicitly deferred |
| APIC migration (from PIC/PIT) | Planned post-v0.1 |
| Networking stack (ARP → TCP) | v0.2 target |
| Dynamic ELF loading | Deferred — static only |
| User-space libc / runtime | Deferred |
| Secure boot / signing | Design placeholder only |
| Multi-user / accounts | Deferred |
| Package format | Deferred |
