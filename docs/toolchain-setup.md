# Toolchain Setup

## Required Host Tools

For the current baseline, the minimum expected host tools are:

- Rustup
- Rust nightly
- `rust-src`
- `llvm-tools-preview`
- QEMU
- OVMF firmware files

## Expected Rust Setup

```powershell
rustup toolchain install nightly
rustup default nightly
rustup component add rust-src llvm-tools-preview rustfmt clippy
```

## Verification

Run:

```powershell
./scripts/check-prereqs.ps1
rustc --version
cargo --version
```

## Build And Run

Build the boot image:

```powershell
./scripts/build-image.ps1
```

Run under QEMU and stream output:

```powershell
./scripts/run-qemu.ps1
```

Run under QEMU and capture output to a file:

```powershell
./scripts/run-qemu.ps1 -LogPath "build/qemu-boot.log"
```

Run with bounded execution time (recommended for automated validation loops):

```powershell
./scripts/run-qemu.ps1 -LogPath "build/qemu-boot.log" -TimeoutSeconds 70
```

When `-TimeoutSeconds` is set and the timeout is reached, the script force-stops QEMU and exits with code `124`.

Run the E9 stable-repeat validation pack with automatic summary output:

```powershell
./scripts/validate-e9-repeat.ps1 -TimeoutSeconds 70
```

This generates per-run logs (`build/e9-stable-repeat-A/B/C.log`) and a consolidated summary at `build/e9-stable-repeat-summary.txt`.

Optional diagnostic toggles (no source edits required):

```powershell
./scripts/validate-e9-repeat.ps1 -RunIds @("A") -TimeoutSeconds 70 -LogPrefix "build/e9-diag-user" -SummaryPath "build/e9-diag-user-summary.txt" -DiagUserDeepProbe
./scripts/validate-e9-repeat.ps1 -RunIds @("A") -TimeoutSeconds 70 -LogPrefix "build/e9-diag-kernel" -SummaryPath "build/e9-diag-kernel-summary.txt" -DiagKernelDeepProbe
```

These switches map to kernel Cargo features:
- `gui-fb-user-deep-probe`
- `gui-fb-kernel-deep-probe`

Summary files now include `kernel_entry=yes|no` so early boot stalls can be distinguished from later probe failures.

## Current Repository Constraint

Until the toolchain is installed and verified, repository work can continue on structure, boot scripts, and documentation, but the kernel cannot be treated as build-verified.