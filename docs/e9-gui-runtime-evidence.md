# E9: GUI Runtime Evidence

This document captures completion evidence for E9 graphics runtime work:
- Step 1: Real graphics syscalls
- Step 2: User framebuffer access backend
- Step 3: Window manager demo

## Scope

E9 extends the syscall and user runtime with framebuffer-oriented graphics primitives and a composed UI demo path.

Implemented syscall surface:
- `SYS_DRAW_RECT` (25)
- `SYS_DRAW_PIXEL` (26)
- `SYS_DRAW_TEXT` (27)
- `SYS_MAP_FB` (28)

Syscall table length at E9 completion: 29 entries (`0..28`).

## Build And Boot Artifacts

Primary final validation log:
- `build/e9-validation-pack.log`

Related milestone logs (captured during implementation):
- `build/e9-gfx-hardening.log`
- `build/e9-gfx-step1-text.log`
- `build/e9-step2-stable.log`
- `build/e9-window-mgr.log`
- `build/e9-step2-safe-probe2.log`

Repeatability logs (post-hardening baseline):
- `build/e9-stable-repeat-A.log`
- `build/e9-stable-repeat-B.log`
- `build/e9-stable-repeat-C.log`
- `build/e9-stable-repeat-summary.txt`
- `build/e9-gated-skip-marker.log`

Post-fix repeatability logs (map-limit + user-fb guard fix):
- `build/e9-stable-maplimit-fix8-A.log`
- `build/e9-stable-maplimit-fix8-B.log`
- `build/e9-stable-maplimit-fix8-C.log`
- `build/e9-stable-maplimit-fix8-summary.txt`
- `build/e9-diag-user-maplimit-fix8-A.log`
- `build/e9-diag-user-maplimit-fix8-B.log`
- `build/e9-diag-user-maplimit-fix8-C.log`
- `build/e9-diag-user-maplimit-fix8-summary.txt`

Tripwire policy artifacts:
- `build/e9-tripwire-fix11-nb-summary.txt` (kernel-deep non-blocking mode)
- `build/e9-tripwire-fix11-block-summary.txt` (kernel-deep blocking mode)
- `build/e9-tripwire-fix11-nb-summary.json`
- `build/e9-tripwire-fix11-block-summary.json`
- `build/e9-gate-fix13-summary.txt` (canonical strict all-lane gate wrapper)
- `build/e9-gate-fix13-summary.json`

## Final Validation Markers

From `build/e9-validation-pack.log`:

- `syscall: table-len=29 ...`
- `syscall: dispatch PASS`
- `arch: elf-loader PASS`
- `gui: demo PASS`
- `gui: window-mgr PASS`
- `process: model PASS`
- `drivers: driver-model PASS`
- `fs: vfs PASS`
- `scheduler: idle loop active`

No `FAIL` marker was observed in the final marker set.
No `PAGE FAULT` marker was observed in the final marker set.

## Repeatability Evidence (A/B/C)

Stable baseline was re-run three times after user-task gating and probe-address hygiene hardening.

Observed in each run (`A`, `B`, `C`):
- `gui: demo PASS`
- `gui: fb-map PASS`
- `gui: window-mgr PASS`
- `process: model PASS`
- `drivers: driver-model PASS`
- `fs: vfs PASS`
- `scheduler: idle loop active`

No `FAIL` marker was observed in the extracted marker set for these runs.
No `PAGE FAULT` marker was observed in the extracted marker set for these runs.

Automation note:
- `scripts/validate-e9-repeat.ps1 -TimeoutSeconds 70` regenerates `A/B/C` logs and the consolidated summary file.
- Diagnostic mode can be enabled without source edits using cfg toggles:
	- `scripts/validate-e9-repeat.ps1 -RunIds @("A") -TimeoutSeconds 70 -LogPrefix "build/e9-diag-user" -SummaryPath "build/e9-diag-user-summary.txt" -DiagUserDeepProbe`
	- `scripts/validate-e9-repeat.ps1 -RunIds @("A") -TimeoutSeconds 70 -LogPrefix "build/e9-diag-kernel" -SummaryPath "build/e9-diag-kernel-summary.txt" -DiagKernelDeepProbe`

Deep-probe gating telemetry:
- Stable boot now emits `gui: fb-map-user SKIP (gated)` when the experimental ring-3 deep probe is disabled.
- Experimental deep-probe user pages are placed in a dedicated high user region (`0x0000_4000_8000_0000+`) to reduce overlap risk with existing demo/probe ranges.
- Boot also emits the active diagnostic profile line: `gui: diag kernel_deep=<0|1> user_deep=<0|1>`.
- Repeat summaries include `kernel_entry=yes|no` for fast triage of early boot stalls vs late probe failures.
- Diagnostic toggles are feature-driven (`gui-fb-user-deep-probe`, `gui-fb-kernel-deep-probe`) via `scripts/validate-e9-repeat.ps1` switches.

Current diagnostic smoke result:
- `build/e9-diag-user-maplimit-fix8-summary.txt` reports PASS for runs `A/B/C` with:
	- `kernel_entry=yes`
	- `missing=none`
	- `fail_hits=none`
	- `fault_rip=none`, `fault_cr2=none`
- `build/e9-diag-user-maplimit-fix8-*.log` includes `gui: fb-map-user PASS` in all three runs.
- Stable mode also reports PASS for runs `A/B/C` in `build/e9-stable-maplimit-fix8-summary.txt`.

Kernel-deep probe status:
- Current kernel-deep mode (`gui-fb-kernel-deep-probe`) now passes with explicit clean-deny semantics for kernel-task context (`gui: fb-map ... ret=0 ... PASS`) and no PF signatures.
- Validation evidence:
	- `build/e9-diag-kernel-fix12b-summary.txt` reports PASS
	- strict tripwire with kernel lane blocking passes in `build/e9-tripwire-fix12b-block-summary.txt`

## Step Coverage

### Step 1: Real Graphics Syscalls

Evidence:
- `gui: demo PASS`
- `syscall: dispatch PASS`

Interpretation:
- Pixel and rectangle rendering paths execute successfully from user mode via syscall dispatch.
- Text drawing syscall is present in the table and integrated into the kernel graphics path.

### Step 2: User Framebuffer Access Backend

Evidence:
- `syscall: table-len=29` confirms `SYS_MAP_FB` is exported.
- Stable boot and full probe chain pass with Step 2 backend enabled.
- `gui: fb-map PASS` from a fail-safe smoke probe path (`SYS_MAP_FB` with null output pointer returns 0 cleanly).

Interpretation:
- Kernel-side mapping path is integrated without destabilizing runtime.
- Deep ring-3 framebuffer probe now passes deterministically after fixing physical map-limit coverage and accepting huge-page-backed framebuffer source addresses.
- `SYS_MAP_FB` completes with non-zero `virt` and `bytes` in diagnostic user-deep runs while keeping baseline stable.

### Step 3: Window Manager Demo

Evidence:
- `gui: window-mgr PASS`

Interpretation:
- A composed window-like UI is rendered from user mode using graphics syscalls.
- End-to-end path (ELF load -> ring3 execution -> graphics syscalls -> task exit) is functioning.

## Regression Status

The final E9 validation run keeps all baseline subsystem probes green:
- ELF loader
- Process model
- Driver model
- VFS
- Scheduler idle stability

This indicates no detected regression to previously completed phases while E9 features are active.

## Conclusion

E9 is complete at runtime evidence level:
- Graphics syscall stack is active and validated
- User framebuffer mapping syscall backend is integrated
- Window manager demo path is operational
- Boot remains stable under full probe chain

Document version: E9 validation pack + map-limit/user-fb fix8 repeatability (2026-04-04)
