# Astra OS Testing Guide

## Quick Start: Verify Everything Works

### 1. Build the Project
```bash
cd C:\Users\szymo\OneDrive\Desktop\OS
cargo build --release
```

**Expected Output:**
- All code compiles without errors
- 0 errors, ~93 warnings (pre-existing)
- Release build completes in ~4-5 seconds

### 2. Run in QEMU
```bash
qemu-system-x86_64 -drive format=raw,file=disk.img -m 2G -enable-kvm
```

**Expected Behavior:**
- OS boots with kernel messages
- Desktop appears with taskbar and icons
- Mouse cursor responds to movement
- Applications launch and run smoothly

---

## Testing After Each Change

### Immediate (After Every Commit)

**Step 1: Compile Check (30 seconds)**
```bash
cargo check
```
✅ Should show: `Finished 'dev' profile`
❌ If error: Fix compilation issues before testing

**Step 2: Release Build (5 seconds)**
```bash
cargo build --release
```
✅ Should show: `Finished 'release' profile`
❌ If error: Something in refactoring broke the build

### Functional (After Significant Changes)

**Step 3: Boot Test (10 seconds)**
```bash
qemu-system-x86_64 -drive format=raw,file=disk.img -m 2G -enable-kvm
```

**What to watch for:**
1. ✅ Serial output shows boot progress (E1, E2, E3 phases)
2. ✅ No kernel panics appear
3. ✅ Desktop GUI appears after ~3 seconds
4. ✅ Taskbar visible at top
5. ✅ Desktop icons visible on left

**If boot fails:**
- Check for error messages in serial output
- Look for "panic" in output
- May need to rebuild with `cargo clean && cargo build --release`

### Application Testing (2-3 minutes)

**Step 4: Application Tests**

In QEMU terminal (appears at top-left):

```
# Test 1: Terminal Commands
ping 8.8.8.8
# Expected: Shows ICMP responses with RTT values

# Test 2: File Operations
ls /
# Expected: Shows directory listing

# Test 3: Process Management
ps
# Expected: Shows running processes

# Test 4: Network Check
netcheck
# Expected: Shows 3/3 checks passed ✓

# Test 5: Application Launch
# Click on File Manager icon in launcher
# Expected: Window appears, shows directory listing
```

**If application crashes:**
- Note which app failed
- Check recent changes to that module
- Look for memory safety issues or syscall errors

### Regression Testing (1 minute each)

After refactoring, test these critical paths:

```
1. Desktop Interaction
   - Move mouse around
   - Click on icons
   - Drag window (should move smoothly)
   - Close window

2. Terminal I/O
   - Type: `echo hello`
   - Type: `ps` (shows processes)
   - Type: `ls /` (shows files)
   - Ctrl+C should work

3. Networking
   - `ping 8.8.8.8` (should respond)
   - `netcheck` (should show 3/3)

4. Persistence
   - Create file: `echo test > /tmp/testfile`
   - Shutdown OS (Ctrl+C in QEMU)
   - Reboot: run qemu command again
   - Check: `cat /tmp/testfile` (should show "test")
```

---

## Testing Specific v0.3 Changes

### After Network Stack Reorganization (Commit 2dd4c40)

**Compile Test:**
```bash
cargo check
# Should show: Finished in ~1.2s
# No errors about missing modules
```

**Network Test:**
```bash
# Boot OS in QEMU
qemu-system-x86_64 -drive format=raw,file=disk.img -m 2G -enable-kvm

# In terminal:
netcheck
# Expected: 3 successful checks (ping, DNS, HTTP)

ping 8.8.8.8
# Expected: Responses from Google's DNS server
```

### After Memory Protection Module (Commit 1fdee7b)

**Compilation Test:**
```bash
cargo check
# Should show: Finished in ~1.2s
# Look for any warnings about unused protection functions
```

**Stability Test:**
```bash
# Boot OS
qemu-system-x86_64 -drive format=raw,file=disk.img -m 2G -enable-kvm

# Stress test memory:
# 1. Launch Terminal
# 2. Launch File Manager  
# 3. Launch Text Editor
# 4. Launch Calculator
# 5. All simultaneously - verify no crashes

# In terminal, fill memory:
dd if=/dev/zero of=/tmp/bigfile bs=1M count=100
# Should complete without segfault
```

**Guard Page Test (Future):**
Once guard page enforcement is active:
```bash
# These should be rejected by loader:
# - Programs trying to use 0x0 (NULL)
# - Programs with rwx (writable+executable) segments
# - Programs with stack overflow attempts (would trigger page fault)
```

---

## Complete Test Workflow

Use this workflow every time you commit:

```
1. Make changes to code
   ↓
2. cargo check (verify compilation)
   ↓
3. cargo build --release (full build)
   ↓
4. QEMU boot test (verify OS starts)
   ↓
5. Run critical tests:
   - ping 8.8.8.8
   - netcheck
   - ps
   - ls /
   - Launch 2-3 apps
   ↓
6. Shutdown cleanly (Ctrl+C in QEMU)
   ↓
7. Reboot to verify persistence
   ↓
8. If all pass → Commit is good!
   ↓
9. If any fail → Fix issues, retry from step 2
```

**Total Time: ~5-10 minutes per commit**

---

## Quick Reference: Common Issues & Fixes

| Issue | Cause | Fix |
|-------|-------|-----|
| `error[E0433]: cannot find module` | Missing `mod` declaration | Add `pub mod xyz;` to mod.rs |
| `error: cannot find function` | Private function | Add `pub` keyword |
| `warning: unused variable` | Dead code after refactoring | Use `_var` or `#[allow(unused)]` |
| OS won't boot | Compilation or runtime error | Check serial output with `-serial stdio` |
| Apps crash | Syscall or memory issue | Check refactored modules (scheduler, syscall, memory) |
| GUI unresponsive | Desktop module issue | Verify desktop::run() executes |
| Network doesn't work | L2/L3/L4 module issue | Test with `ping 8.8.8.8` |

---

## QEMU Commands Reference

### Boot with Serial Output (for debugging)
```bash
qemu-system-x86_64 -drive format=raw,file=disk.img -m 2G -enable-kvm -serial stdio
```

### Boot with Graphics (default)
```bash
qemu-system-x86_64 -drive format=raw,file=disk.img -m 2G -enable-kvm
```

### Allocate Different RAM
```bash
qemu-system-x86_64 -drive format=raw,file=disk.img -m 4G  # 4 GiB
qemu-system-x86_64 -drive format=raw,file=disk.img -m 8G  # 8 GiB
```

### Enable CPU Features
```bash
qemu-system-x86_64 -drive format=raw,file=disk.img -m 2G -enable-kvm -smp 4
# Tests multicore (4 CPUs) - useful for SMP testing
```

---

## Summary: Testing Checklist

After EVERY significant change:

- [ ] `cargo check` ✅
- [ ] `cargo build --release` ✅
- [ ] OS boots without panics ✅
- [ ] Desktop GUI appears ✅
- [ ] Terminal accepts commands ✅
- [ ] `netcheck` returns 3/3 ✅
- [ ] File operations work ✅
- [ ] Applications launch ✅
- [ ] Can reboot and verify persistence ✅

**If all checkmarks pass → Your changes are good!**

