param(
    [string]$BuildCfg = "debug",
    [int]$TimeoutSeconds = 35,
    [string]$OutPrefix = "build/e11-validate"
)

Set-Location (Join-Path $PSScriptRoot "..")

$logPath = "$OutPrefix.log"
$summaryPath = "$OutPrefix-summary.txt"
$jsonPath = "$OutPrefix-summary.json"

Write-Host "[e11-validate] focused networking run"
& .\scripts\run-qemu.ps1 -Profile $BuildCfg -TimeoutSeconds $TimeoutSeconds -LogPath $logPath -CargoFeatures net-scaffold
$qemuExit = $LASTEXITCODE

if (-not (Test-Path $logPath)) {
    Write-Host "[e11-validate] FAIL: missing log $logPath"
    exit 1
}

$requiredMarkers = @(
    "net: scaffold PASS",
    "net: udp-lifecycle PASS",
    "net: hooks PASS",
    "net: firewall PASS",
    "net: dns-contract PASS",
    "net: socket-contract PASS",
    "net: poste14-contract PASS",
    "net: e11-contract PASS"
)

$missing = @()
foreach ($marker in $requiredMarkers) {
    if (-not (Select-String -Path $logPath -Pattern ([regex]::Escape($marker)) -Quiet)) {
        $missing += $marker
    }
}

$failHits = @(
    Select-String -Path $logPath -Pattern "net: .*FAIL|page fault|panic" -CaseSensitive:$false |
        Select-Object -ExpandProperty Line
)

$pass = ($missing.Count -eq 0 -and $failHits.Count -eq 0)
$result = if ($pass) { "PASS" } else { "FAIL" }

$missingText = if ($missing.Count -eq 0) {
    "none"
} else {
    $missing -join ";"
}

$failText = if ($failHits.Count -eq 0) {
    "none"
} else {
    ($failHits | Select-Object -Unique) -join " || "
}

$lines = @(
    "E11 Networking Validation: $result",
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

Write-Host "[e11-validate] summary -> $summaryPath"
Write-Host "[e11-validate] json -> $jsonPath"

if ($pass) {
    exit 0
}

exit 1
