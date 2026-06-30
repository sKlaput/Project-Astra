# Testing Template for v0.3 Features

Use this template EVERY TIME you implement a new feature or refactoring.
Copy, fill in, and run before committing.

---

## Feature: [Feature Name]
**Date:** YYYY-MM-DD
**Related Commits:** [commit hashes]
**Expected Impact:** [What should improve/change]

### Phase 1: Compilation (30 seconds)

**Test Command:**
```bash
cargo check
```

**Expected Result:**
```
   Checking kernel v0.3.0-dev
    Finished `dev` profile [unoptimized + debuginfo] target(s) in X.XXs
```

**Pass:** ✅ YES / ❌ NO

If NO, fix compilation errors and retry.

---

### Phase 2: Full Build (5 seconds)

**Test Command:**
```bash
cargo build --release
```

**Expected Result:**
```
   Compiling kernel v0.3.0-dev
    Finished `release` profile [optimized] target(s) in X.XXs
```

**Pass:** ✅ YES / ❌ NO

If NO, there's a linking or runtime initialization error. Check:
- Did you add all module declarations?
- Do new modules have proper visibility?
- Any circular dependencies introduced?

---

### Phase 3: Boot Test (10 seconds)

**Test Command:**
```bash
qemu-system-x86_64 -drive format=raw,file=disk.img -m 2G -enable-kvm
```

**Expected Observations:**
1. Serial output shows boot messages (E1, E2, E3)
2. No kernel panics appear
3. Desktop GUI appears within 5 seconds
4. Taskbar visible at top
5. Desktop icons visible on left side

**Specific to [Feature Name]:**
[Insert feature-specific checks here]

**Overall Pass:** ✅ YES / ❌ NO

---

### Phase 4: Functional Tests (2-5 minutes)

**Test A: Core Functionality**
```bash
# In QEMU terminal:
ping 8.8.8.8
# Expected: Shows ICMP responses

netcheck
# Expected: 3 successful checks

ps
# Expected: Shows process list
```
**Result:** ✅ PASS / ❌ FAIL

**Test B: Application Launch**
```bash
# Click on applications in GUI
# Test: Terminal, File Manager, Text Editor
# Expected: All launch within 1-2 seconds
```
**Result:** ✅ PASS / ❌ FAIL

**Test C: Feature-Specific** [Insert your test here]
```bash
[Your specific test steps]
# Expected: [Specific expected behavior]
```
**Result:** ✅ PASS / ❌ FAIL

---

### Phase 5: Regression Tests (2-3 minutes)

Verify old features still work:

```bash
# 1. Desktop
#    ✅ Mouse moves smoothly
#    ✅ Clicks register
#    ✅ Windows drag/resize work

# 2. Terminal
#    ✅ Commands execute
#    ✅ Output displays
#    ✅ Input responsive

# 3. Network
#    ✅ ping works
#    ✅ netcheck passes
#    ✅ HTTP works

# 4. Files
#    ✅ Create/delete files
#    ✅ Read/write operations
#    ✅ Persistence across reboot
```

**All Regressions Pass:** ✅ YES / ❌ NO

---

### Phase 6: Stress Test (5-10 minutes)

If feature involved memory/performance:

```bash
# Launch multiple apps simultaneously
# Create large files
# Stress memory allocation
# Verify no crashes or slowdowns
```

**Result:** ✅ STABLE / ❌ UNSTABLE

---

### Final Checklist

Before committing:

- [ ] Phase 1: Compilation ✅
- [ ] Phase 2: Full Build ✅
- [ ] Phase 3: Boot Test ✅
- [ ] Phase 4: Functional Tests ✅
- [ ] Phase 5: Regression Tests ✅
- [ ] Phase 6: Stress Test ✅
- [ ] No new panics observed
- [ ] No performance regressions
- [ ] Feature works as intended

**FINAL STATUS:** ✅ READY TO COMMIT / ❌ NEEDS FIXES

---

## If Tests Fail

### Compilation Fails
```bash
cargo check 2>&1 | head -20
# Check for:
# - Missing module declarations
# - Type mismatches
# - Visibility issues
```

### Boot Fails
```bash
qemu-system-x86_64 -drive format=raw,file=disk.img -m 2G -enable-kvm -serial stdio
# Check for:
# - Panic messages with file:line
# - "page fault" or "exception"
# - "out of memory"
```

### Apps Crash
- Which app? Terminal, File Manager, Text Editor, Calculator?
- Did you modify that app's code?
- Or did you modify shared code (scheduler, syscall, memory)?

### Network Broken
- Did you modify net/, scheduler/, or syscall/?
- Test: `ping 8.8.8.8` (layer 3 works?)
- Test: `netcheck` (full stack works?)

---

## Time Budget

| Phase | Time | Risk |
|-------|------|------|
| Compile | 30s | LOW - catches syntax errors |
| Build | 5s | LOW - catches linking issues |
| Boot | 10s | MEDIUM - catches runtime panics |
| Functional | 5m | HIGH - verifies feature works |
| Regression | 3m | HIGH - catches side effects |
| Stress | 10m | MEDIUM - finds edge cases |

**Total: ~20-30 minutes per feature**

---

## Commit Message Template

```
feat: [Feature name] - brief description

Detailed explanation of what changed and why.

Testing:
✅ Compilation passed
✅ Boot test passed
✅ Feature works as intended
✅ No regressions detected

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
```

