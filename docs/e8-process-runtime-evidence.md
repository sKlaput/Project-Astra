# E8: Multi-Process Runtime Architecture & Limitations

**Phase E8** extends the kernel with a process abstraction layer, enabling kernel-managed multi-process execution alongside the existing task scheduler. This document defines the runtime contract, constraints, and evidence of E8 completion.

## Architecture Overview

### Process Abstraction (Separate from Task Abstraction)

Processes provide a higher-level management layer distinct from low-level scheduler tasks:

- **ProcessId**: Opaque 64-bit process identifier (monotonically allocated)
- **ProcessState**: `Empty`, `Running`, or `Exited`
- **Process Table**: Fixed 16-slot table (supports up to 16 concurrent processes)
- **Metadata per Process**:
  - Startup ABI version (currently 1)
  - Main task ID (link to the scheduler task executing this process)
  - Name string (pointer + length, up to 64 bytes)
  - Start tick (for uptime calculation)
  - Exit tick (when process terminates)

### User-Space Startup ABI v1 Contract

User-space programs must conform to a versioned startup ABI for portability:

```rust
pub struct UserStartupAbi {
    pub version: u32,        // Currently 1
    pub code_virt: u64,      // Virtual address of .text (0x400000)
    pub stack_virt: u64,     // Virtual address of user stack (0x500000+)
}
```

**Validation:** The kernel enforces `abi.version == 1` at process spawn time (`spawn_elf_process`). Processes with unsupported ABI versions are rejected.

### Process Spawn Paths

#### Kernel-Backed Processes
```
spawn_kernel_process(name, function, priority) -> Option<ProcessId>
```
- Creates a process running a kernel function directly
- No user-mode transition
- Primary use case: internal system tasks

#### ELF User Processes
```
spawn_elf_process(name, elf_image, user_stack_virt, priority) -> Option<ProcessId>
```
- Loads ELF64 binary, enforces startup ABI v1, allocates user stack
- Transitions to ring-3 user-mode on first dispatch
- Error conditions:
  - ABI version mismatch → returns `None`
  - ELF load failure → returns `None`
  - Frame allocator exhaustion → returns `None`
  - Process table full (16 slots) → returns `None`

### Process Lifecycle

1. **Spawn**: Process allocated, main task created, status = `Running`
2. **Execution**: Scheduler dispatches main task, process uptime accumulates
3. **Exit**: Main task calls `SYS_EXIT`, process state transitions to `Exited`, exit tick recorded
4. **Query**: `state(pid)` refreshes process state from main task's scheduler status

### IPC (Inter-Process Communication)

Two new syscalls support simple message-based IPC:

#### SYS_SEND_MSG (syscall #22)
```c
int send_msg(void *msg_ptr, size_t len)
// Returns: 1 on success, 0 on error
// Constraints:
//   - msg_ptr must be valid user-mode virtual address
//   - len must be 1-64 bytes
//   - Kernel copies message to global pending buffer (1 message max)
```

#### SYS_RECV_MSG (syscall #23)
```c
size_t recv_msg(void *buf_ptr)
// Returns: number of bytes received, 0 if no message waiting, -1 on error
// Constraints:
//   - buf_ptr must be valid, ≥64 bytes
//   - Non-blocking; returns immediately
//   - Clears pending message on successful read
```

**IPC Model**: Global single-message kernel buffer. All processes can send to it; all can receive from it. No per-process mailboxes, no message queues.

## Runtime Limitations

### Phase E8 Constraints (Intentional)

The following limitations are known and documented for E8. They do not represent bugs but rather incomplete features:

#### No File Descriptor Table
- Processes have no fd table per se
- `SYS_WRITE_CONSOLE` works via direct kernel call (not fd-based)
- Processes cannot `open()` or read arbitrary files yet
- **Future**: E9+ will implement fd tables tied to process VFS context

#### No Argument / Environment Passing
- Processes launch with no `argv` or `environ`
- No command-line argument parsing
- Entry point receives no startup parameters
- **Future**: E9+ will define a user-space startup header with arg pointers

#### No Spatial Isolation
- All ring-3 processes share the same address space
- One process can read/write other processes' memory
- No MMU-based protection domain separation
- **Note**: Ring-3 itself provides some isolation; ring-3 page faults are caught and abort the task without affecting the kernel

#### IPC Limitations
- Single global message buffer (1 pending message max)
- No message queues or per-process mailboxes
- No addressing scheme (messages are broadcast-read)
- No flow control or blocking receive
- **Future**: E10+ will implement per-process message queues and blocking syscall variants

#### Process Table Exhaustion
- Maximum 16 concurrent processes
- Exceeding limit returns `None` from spawn functions
- No process cleanup or automatic reuse
- **Future**: E9+ will implement process exit hooks and table cleanup

#### No Process Signals or Asynchronous Events
- No `SIGTERM`, `SIGKILL`, or other signals
- Processes cannot be terminated except via `SYS_EXIT`
- No way to interrupt a CPU-bound user process remotely
- **Future**: E11+ will add signal delivery infrastructure

#### No Scheduler Preemption Tuning
- Processes cannot adjust their priority post-spawn
- No `nice()` or `setpriority()` syscalls
- Priority is fixed at spawn time

## E8 Completion Criteria

### ✅ Criteria Met

1. **Process Abstraction**: ProcessId, ProcessState, process table, metadata tracking
2. **ABI Versioning**: UserStartupAbi struct, validation enforced at spawn
3. **Dual User Programs**: Two embedded ELF binaries (hello, ticker) coexist
4. **IPC Syscalls**: `SYS_SEND_MSG` (#22) and `SYS_RECV_MSG` (#23) functional
5. **Boot Validation**: Process model probe emits `PASS` in serial log
6. **Zero Regression**: E6 (drivers), E7 (VFS), and E8 (process) probes all pass

### Evidence Artifacts

- **Boot Log**: `build/e8-complete-boot.log` (final validation)
- **HELLO_ELF**: Embedded 171-byte ELF binary printing "hello from elf\n" via `SYS_WRITE_CONSOLE`
- **TICKER_ELF**: Embedded 160-byte ELF binary sending timestamped messages via `SYS_SEND_MSG`
- **Syscall Table**: 24 entries (expanded from 22 in E7)

## Known Deviations from Production OS

These properties are acceptable for a research/proof-of-concept phase:

| Aspect | E8 Reality | Production | Rationale |
|--------|-----------|-----------|-----------|
| Process isolation | None; shared address space | Full MMU isolation | Research scope allows shared AS for simplicity |
| IPC model | Single global buffer | Queues + multiplexing | Deferred to E10+ |
| Process limits | Fixed 16-slot reusable table | Elastic (heap-backed + reclaim policies) | Research constraint: bounded metadata, deterministic footprint |
| ELF loading | Narrow static ET_EXEC subset (page-aligned PT_LOAD) | Broad ABI-compatible ELF support | Current loader intentionally rejects unsupported layouts |
| Signal delivery | None | Async signal framework | Complex; deferred to E11+ |
| Init process | None; direct spawn | Init + fork + exec | Simplified for boot sequence |

## Next Steps (E9+)

1. **E9 GUI Prototype**: Graphical console framebuffer + keyboard integration (see `docs/e9-gui-prototype.md`)
2. **E10 Advanced IPC**: Per-process message queues, blocking recv
3. **E11 Networking**: Basic network stack (UDP)
4. **E12 Signals**: Signal delivery + handlers
5. **E13 Dynamic Memory**: Heap-backed process table
6. **E14 Multi-core**: SMP execution

---

**Document Version**: E8 Phase 1 (2026-04-01)  
**Status**: Process abstraction complete; IPC and dual-program execution validated in QEMU.
