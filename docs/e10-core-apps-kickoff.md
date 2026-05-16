# E10 Core Apps Kickoff

This document defines the first concrete implementation slice for E10 (Core Apps) while preserving the currently stable E9 runtime path.

## Scope For This Slice

Target only one app first: Terminal.

Deliverables in this slice:
- terminal app lifecycle contract (launch, foreground, exit)
- one built-in command path (`help`) with deterministic output
- minimal launcher hook from existing user-space startup flow
- serial evidence markers for launch and command execution

Out of scope in this slice:
- text editor UI/logic
- file manager UI/logic
- settings UI/logic
- shell scripting, pipes, job control

## E10 Exit Criteria Mapping (Partial)

This slice contributes to E10 criteria:
- terminal application exists and can launch at least one simple command
- app lifecycle expectations are documented

Remaining E10 criteria are tracked as later slices.

## Implementation Plan

1. Add terminal lifecycle markers in kernel/user startup path:
- `apps: terminal launch PASS`
- `apps: terminal command help PASS`

2. Add minimal command dispatch table for terminal v0:
- command: `help`
- output: static command list for v0

3. Keep launch and command execution bounded by timeout-safe dispatch loops.

4. Validate with existing gate scripts plus focused app logs:
- strict gate remains green
- app markers present in focused run log

## Evidence Requirements For This Slice

Required evidence artifacts:
- `build/e10-terminal-kickoff.log`
- summary note appended to E10 evidence doc once created

Required marker set:
- `apps: terminal launch PASS`
- `apps: terminal command help PASS`
- no `FAIL`
- no `PAGE FAULT`

## Risk Controls

- Do not alter paging model, syscall ABI, or boot protocol.
- Keep all new behavior behind existing startup probe sequencing.
- Preserve current E9 markers and pass conditions.

## Next Slices (Preview)

- Slice 2: text editor plain-text open/display path
- Slice 3: file manager directory listing via VFS API
- Slice 4: settings shell placeholders + lifecycle documentation

## Current Status

Slice 1 is now implemented and evidenced:
- runtime markers present in `build/e10-terminal-kickoff.log`
- strict all-lane regression gate remains PASS in `build/e9-gate-e10-slice1-summary.txt`
- evidence recorded in `docs/e10-core-apps-evidence.md`

Slice 2 is now implemented and evidenced:
- runtime markers present in `build/e10-editor-slice2.log`
- strict all-lane regression gate remains PASS in `build/e9-gate-e10-slice2-summary.txt`
- evidence recorded in `docs/e10-core-apps-evidence.md`

Slice 3 is now implemented and evidenced:
- runtime markers present in `build/e10-filemgr-slice3.log`
- strict all-lane regression gate remains PASS in `build/e9-gate-e10-slice3-summary.txt`
- evidence recorded in `docs/e10-core-apps-evidence.md`

Slice 4 is now implemented and evidenced:
- runtime markers present in `build/e10-settings-slice4.log`
- strict all-lane regression gate remains PASS in `build/e9-gate-e10-slice4-summary.txt`
- evidence recorded in `docs/e10-core-apps-evidence.md`

Next active slice:
- E10 implementation slices complete; next phase handoff target is E11 networking architecture notes.
