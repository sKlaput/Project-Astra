#!/usr/bin/env pwsh
# Quick focused validator for refactored architecture
# Tests that the consolidated baseline GUI probe works correctly

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Run QEMU with image and capture output
Write-Host "Running QEMU with refactored kernel..." -ForegroundColor Cyan
$qemu_out = $env:TEMP + "\refactor-output.txt"

$qemu_cmd = @(
    "qemu-system-x86_64",
    "-m", "1024",
    "-bios", "build\image-root\EFI\BOOT\BOOTX64.EFI",
    "-serial", "file:$qemu_out",
    "-nographic",
    "-no-reboot",
    "-timeout", "120"
) | ForEach-Object { '"' + $_ + '"' }

$qemu_ps = Start-Process -FilePath "qemu-system-x86_64" -ArgumentList `
    '-m', '1024', `
    '-bios', "build\image-root\EFI\BOOT\BOOTX64.EFI", `
    '-serial', "file:$qemu_out", `
    '-nographic', `
    '-no-reboot', `
    '-timeout', '120' `
    -Wait -PassThru

$exit_code = $qemu_ps.ExitCode
if ($exit_code -eq 124) {
    Write-Host "QEMU timeout (expected)" -ForegroundColor Yellow
} elseif ($exit_code -ne 0) {
    Write-Host "QEMU exit code: $exit_code" -ForegroundColor Yellow
}

# Read output
if ((Test-Path $qemu_out)) {
    $output = Get-Content $qemu_out -Raw
    
    # Check for key architecture validation markers
    $checks = @{
        "consolidated-baseline found" = $output -match "gui: consolidated-baseline"
        "scheduler validation active" = $output -match "validate_scheduler_operational"
        "syscall validation active" = $output -match "validate_syscall_dispatch_safe"
        "process validation active" = $output -match "validate_process_subsystem_present"
        "context switch detection" = $output -match "validate_scheduler_context_switching"
        "baseline PASS" = $output -match "consolidated-baseline PASS"
    }
    
    Write-Host "`n=== Refactored Architecture Validation ===" -ForegroundColor Cyan
    $pass_count = 0
    $fail_count = 0
    
    foreach ($check in $checks.GetEnumerator()) {
        $status = if ($check.Value) { "✓ PASS"; $pass_count++ } else { "✗ FAIL"; $fail_count++ }
        Write-Host "$status : $($check.Name)"
    }
    
    Write-Host "`nResults: $pass_count/6 validation checks passed" -ForegroundColor Cyan
    
    if ($fail_count -eq 0) {
        Write-Host "`nRefactored architecture VALIDATED" -ForegroundColor Green
        exit 0
    } else {
        Write-Host "`nRefactored architecture INCOMPLETE" -ForegroundColor Yellow
        exit 1
    }
    
} else {
    Write-Host "ERROR: No output file generated" -ForegroundColor Red
    exit 1
}
