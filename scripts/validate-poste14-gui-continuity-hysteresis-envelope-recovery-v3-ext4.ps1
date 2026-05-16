#!/usr/bin/env pwsh
# Focused validator for Post-E14 Slice 91: GUI continuity hysteresis envelope recovery v3 extended baseline (fourth cycle)

param(
    [string]$OutPrefix = "build/poste14-guiconthystenveloperecover3ext4-s91"
)

$TimeoutSeconds = 120
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
    "gui-cont-hyst-envelope-recover3-ext4: baseline PASS",
    "gui-cont-hyst-envelope-recover3-ext4: window PASS",
    "gui-cont-hyst-envelope-recover3-ext4: policy PASS",
    "gui-cont-hyst-envelope-recover3-ext4: poste14-contract PASS"
)

$missingMarkers = @()
$failHits = @()
foreach ($marker in $requiredMarkers) {
    if (-not ($log -match [regex]::Escape($marker))) { $missingMarkers += $marker }
}

$failPatterns = @(
    "gui-cont-hyst-envelope-recover3-ext4: .*FAIL",
    "page fault",
    "panic"
)
foreach ($pattern in $failPatterns) {
    $matches = $log | Select-String -Pattern $pattern
    if ($null -ne $matches) { $failHits += @($matches) }
}

$passFlag = ($missingMarkers.Count -eq 0) -and ($failHits.Count -eq 0)

$summary = @()
$summary += "Post-E14 Slice 91: GUI continuity hysteresis envelope recovery v3 extended baseline (fourth cycle)"
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
    slice = 91
    name = "GUI continuity hysteresis envelope recovery v3 extended baseline (fourth cycle)"
    result = if ($passFlag) { "PASS" } else { "FAIL" }
    qemu_exit_code = $qemuExitCode
    required_markers = $requiredMarkers
    missing_markers = $missingMarkers
    fail_hits = @($failHits | ForEach-Object { $_.ToString() })
} | ConvertTo-Json
$jsonSummary | Out-File -FilePath $jsonPath -Encoding UTF8
exit $(if ($passFlag) { 0 } else { 1 })
