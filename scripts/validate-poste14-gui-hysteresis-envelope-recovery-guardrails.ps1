param(
    [string]$BuildCfg = "debug",
    [int]$TimeoutSeconds = 70,
    [string]$OutPrefix = "build/poste14-gui-hyst-envelope-recover-guard-validate"
)

Set-Location (Join-Path $PSScriptRoot "..")

$logPath = "$OutPrefix.log"
$summaryPath = "$OutPrefix-summary.txt"
$jsonPath = "$OutPrefix-summary.json"

Write-Host "[poste14-gui-hyst-envelope-recover-guard] focused gui hysteresis envelope recovery guardrails run"
& .\scripts\run-qemu.ps1 -Profile $BuildCfg -TimeoutSeconds $TimeoutSeconds -LogPath $logPath
$qemuExit = $LASTEXITCODE

if (-not (Test-Path $logPath)) {
    Write-Host "[poste14-gui-hyst-envelope-recover-guard] FAIL: missing log $logPath"
    exit 1
}

$requiredMarkers = @(
    "gui-hyst-envelope-recover-guard: baseline PASS",
    "gui-hyst-envelope-recover-guard: window PASS",
    "gui-hyst-envelope-recover-guard: policy PASS",
    "gui-hyst-envelope-recover-guard: poste14-contract PASS"
)

$missing = @()
foreach ($marker in $requiredMarkers) {
    if (-not (Select-String -Path $logPath -Pattern ([regex]::Escape($marker)) -Quiet)) {
        $missing += $marker
    }
}

$failHits = @(
    Select-String -Path $logPath -Pattern "gui-hyst-envelope-recover-guard: .*FAIL|page fault|panic" -CaseSensitive:$false |
        Select-Object -ExpandProperty Line
)

$pass = ($missing.Count -eq 0 -and $failHits.Count -eq 0)
$result = if ($pass) { "PASS" } else { "FAIL" }

$missingText = if ($missing.Count -eq 0) { "none" } else { $missing -join ";" }
$failText = if ($failHits.Count -eq 0) { "none" } else { ($failHits | Select-Object -Unique) -join " || " }

$lines = @(
    "Post-E14 GUI Hysteresis Envelope Recovery Guardrails Validation: $result",
    "profile=$BuildCfg timeout_seconds=$TimeoutSeconds",
    "qemu_exit=$qemuExit",
    "log=$logPath",
    "required_markers=" + ($requiredMarkers -join ";"),
    "missing_markers=$missingText",
    "fail_hits=$failText"
)

$summaryDir = Split-Path -Parent $summaryPath
if (-not [string]::IsNullOrWhiteSpace($summaryDir)) {
    New-Item -ItemType Directory -Path $summaryDir -Force | Out-Null
}
$lines | Set-Content -Path $summaryPath -NoNewline:$false

$json = [pscustomobject]@{
    result = $result
    profile = $BuildCfg
    timeout_seconds = $TimeoutSeconds
    qemu_exit = $qemuExit
    log = $logPath
    required_markers = $requiredMarkers
    missing_markers = $missing
    fail_hits = $failHits | Select-Object -Unique
    summary = $summaryPath
}
$json | ConvertTo-Json -Depth 4 | Set-Content -Path $jsonPath -NoNewline:$false

Write-Host "[poste14-gui-hyst-envelope-recover-guard] summary -> $summaryPath"
Write-Host "[poste14-gui-hyst-envelope-recover-guard] json -> $jsonPath"

if ($pass) {
    exit 0
}

exit 1
