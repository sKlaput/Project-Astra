#!/usr/bin/env pwsh
# Focused validator for Post-E14 Slice 88: GUI envelope recovery guardrails continuity v3 extended baseline (third cycle)

param(
    [string]$OutPrefix = "build/poste14-guienveloperecoverguardcont3ext3-s88"
)

$TimeoutSeconds = 70
$imagePath = "build/image-root"
$qemuLogPath = "$OutPrefix.raw-qemu.log"
$summaryPath = "$OutPrefix-summary.txt"
$jsonPath = "$OutPrefix-summary.json"

if (Test-Path $qemuLogPath) { Remove-Item -Force $qemuLogPath }
if (Test-Path $summaryPath) { Remove-Item -Force $summaryPath }
if (Test-Path $jsonPath) { Remove-Item -Force $jsonPath }

Write-Host "Running QEMU with $TimeoutSeconds second timeout..."
& .\scripts\run-qemu.ps1 -ImagePath $imagePath -LogPath $qemuLogPath -TimeoutSeconds $TimeoutSeconds -BuildProfile debug

$log = Get-Content $qemuLogPath -ErrorAction SilentlyContinue
if ($null -eq $log) { $log = "" }
$qemuExitCode = $LASTEXITCODE

$requiredMarkers = @(
    "gui-envelope-recover-guard-cont3-ext3: baseline PASS",
    "gui-envelope-recover-guard-cont3-ext3: window PASS",
    "gui-envelope-recover-guard-cont3-ext3: policy PASS",
    "gui-envelope-recover-guard-cont3-ext3: poste14-contract PASS"
)

$missingMarkers = @()
$failHits = @()
foreach ($marker in $requiredMarkers) {
    $markerHit = $log | Select-String -SimpleMatch -Pattern $marker
    if ($null -eq $markerHit) { $missingMarkers += $marker }
}

$failHits += @($log | Where-Object {
    $_ -like "gui-envelope-recover-guard-cont3-ext3: *FAIL" -or
    $_ -like "*page fault*" -or
    $_ -like "*panic*"
})

$passFlag = ($missingMarkers.Count -eq 0) -and ($failHits.Count -eq 0)

$summary = @()
$summary += "Post-E14 Slice 88: GUI envelope recovery guardrails continuity v3 extended baseline (third cycle)"
$summary += "Focused validator result: $(if ($passFlag) { 'PASS' } else { 'FAIL' })"
$summary += "QEMU exit code: $qemuExitCode"
$summary += ""
$summary += "Required markers found: $(($requiredMarkers.Count) - ($missingMarkers.Count)) / $($requiredMarkers.Count)"
if ($missingMarkers.Count -gt 0) {
    $summary += "Missing markers:"
    foreach ($m in $missingMarkers) { $summary += "  - $m" }
} else {
    $summary += "Missing markers: none"
}
$summary += ""
$summary += "Failure markers found: $($failHits.Count)"
if ($failHits.Count -gt 0) {
    $summary += "Fail hits:"
    foreach ($hit in $failHits) { $summary += "  - $hit" }
} else {
    $summary += "Fail hits: none"
}

$summary | Out-File -FilePath $summaryPath -Encoding UTF8

$jsonSummary = @{
    slice = 88
    name = "GUI envelope recovery guardrails continuity v3 extended baseline (third cycle)"
    result = if ($passFlag) { "PASS" } else { "FAIL" }
    qemu_exit_code = $qemuExitCode
    required_markers = $requiredMarkers
    missing_markers = $missingMarkers
    fail_hits = @($failHits | ForEach-Object { $_.ToString() })
} | ConvertTo-Json
$jsonSummary | Out-File -FilePath $jsonPath -Encoding UTF8
exit $(if ($passFlag) { 0 } else { 1 })
