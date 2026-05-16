#!/usr/bin/env pwsh
# Focused validator for Post-E14 Slice 74: GUI envelope recovery guardrails continuity v3 baseline
# Runs QEMU, captures serial output, checks for required markers, and emits summary

param(
    [string]$OutPrefix = "build/poste14-guienveloperecoverguardcont3ext-s74"
)

$TimeoutSeconds = 70
$imagePath = "build/image-root"
$qemuLogPath = "$OutPrefix.raw-qemu.log"
$summaryPath = "$OutPrefix-summary.txt"
$jsonPath = "$OutPrefix-summary.json"

# Clean up prior run artifacts
if (Test-Path $qemuLogPath) {
    Remove-Item -Force $qemuLogPath
}
if (Test-Path $summaryPath) {
    Remove-Item -Force $summaryPath
}
if (Test-Path $jsonPath) {
    Remove-Item -Force $jsonPath
}

Write-Host "Running QEMU with $TimeoutSeconds second timeout..."
& .\scripts\run-qemu.ps1 -ImagePath $imagePath -LogPath $qemuLogPath -TimeoutSeconds $TimeoutSeconds -BuildProfile debug

$log = Get-Content $qemuLogPath -ErrorAction SilentlyContinue
if ($null -eq $log) {
    $log = ""
}

# Extract the QEMU exit code from the process
$qemuExitCode = $LASTEXITCODE
Write-Host "QEMU exited with code: $qemuExitCode"

# Check for required markers
$requiredMarkers = @(
    "gui-recover-guard-cont-hyst3-ext: baseline PASS",
    "gui-recover-guard-cont-hyst3-ext: window PASS",
    "gui-recover-guard-cont-hyst3-ext: policy PASS",
    "gui-recover-guard-cont-hyst3-ext: poste14-contract PASS"
)

$missingMarkers = @()
$failHits = @()

foreach ($marker in $requiredMarkers) {
    if (-not ($log -match [regex]::Escape($marker))) {
        $missingMarkers += $marker
    }
}

# Check for failure markers
$failPatterns = @(
    "gui-recover-guard-cont-hyst3-ext: .*FAIL",
    "page fault",
    "panic"
)

foreach ($pattern in $failPatterns) {
    $matches = $log | Select-String -Pattern $pattern
    if ($null -ne $matches) {
        $failHits += @($matches)
    }
}

# Determine pass/fail
$passFlag = ($missingMarkers.Count -eq 0) -and ($failHits.Count -eq 0)

# Build summary
$summary = @()
$summary += "Post-E14 Slice 74: GUI envelope recovery guardrails continuity v3 baseline"
$summary += "Focused validator result: $(if ($passFlag) { 'PASS' } else { 'FAIL' })"
$summary += "QEMU exit code: $qemuExitCode"
$summary += ""
$summary += "Required markers found: $(($requiredMarkers.Count) - ($missingMarkers.Count)) / $($requiredMarkers.Count)"
if ($missingMarkers.Count -gt 0) {
    $summary += "Missing markers:"
    foreach ($m in $missingMarkers) {
        $summary += "  - $m"
    }
} else {
    $summary += "Missing markers: none"
}
$summary += ""
$summary += "Failure markers found: $($failHits.Count)"
if ($failHits.Count -gt 0) {
    $summary += "Fail hits:"
    foreach ($hit in $failHits) {
        $summary += "  - $hit"
    }
} else {
    $summary += "Fail hits: none"
}

# Write text summary
$summary | Out-File -FilePath $summaryPath -Encoding UTF8
Write-Host "Summary written to $summaryPath"

# Build JSON summary
$jsonSummary = @{
    slice = 74
    name = "GUI envelope recovery guardrails continuity v3 baseline"
    result = if ($passFlag) { "PASS" } else { "FAIL" }
    qemu_exit_code = $qemuExitCode
    required_markers = $requiredMarkers
    missing_markers = $missingMarkers
    fail_hits = @($failHits | ForEach-Object { $_.ToString() })
} | ConvertTo-Json

$jsonSummary | Out-File -FilePath $jsonPath -Encoding UTF8
Write-Host "JSON summary written to $jsonPath"

# Exit with appropriate code
exit $(if ($passFlag) { 0 } else { 1 })
