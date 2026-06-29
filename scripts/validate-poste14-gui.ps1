param(
    [string]$Profile = "debug",
    [int]$TimeoutSeconds = 120,
    [int]$Smp = 2,
    [string]$OutPrefix = "build/poste14-gui-validate"
)

Set-Location (Join-Path $PSScriptRoot "..")

$logPath = "$OutPrefix.log"
$summaryPath = "$OutPrefix-summary.txt"
$jsonPath = "$OutPrefix-summary.json"
$markersPath = Join-Path $PSScriptRoot "validation/poste14-gui-markers.txt"

if (-not (Test-Path $markersPath)) {
    Write-Error "Missing GUI validation marker manifest at $markersPath"
    exit 1
}

Write-Host "[poste14-gui] running consolidated GUI validation"
& (Join-Path $PSScriptRoot "run-qemu.ps1") `
    -Profile $Profile `
    -TimeoutSeconds $TimeoutSeconds `
    -Smp $Smp `
    -CargoFeatures @("boot-probe-gui") `
    -LogPath $logPath
$qemuExit = $LASTEXITCODE

if (-not (Test-Path $logPath)) {
    Write-Error "Missing QEMU log at $logPath"
    exit 1
}

$requiredMarkers = @(
    Get-Content $markersPath |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) -and -not $_.StartsWith("#") }
)

$missingMarkers = @(
    foreach ($marker in $requiredMarkers) {
        if (-not (Select-String -Path $logPath -Pattern ([regex]::Escape($marker)) -Quiet)) {
            $marker
        }
    }
)

$failurePattern = "gui-.*: .*FAIL|kernel panic|panic:|general protection fault|triple fault|page fault"
$failureHits = @(
    Select-String -Path $logPath -Pattern $failurePattern -CaseSensitive:$false |
        Select-Object -ExpandProperty Line -Unique
)

$passed = $missingMarkers.Count -eq 0 -and $failureHits.Count -eq 0
$result = if ($passed) { "PASS" } else { "FAIL" }
$summary = [pscustomobject]@{
    result = $result
    profile = $Profile
    smp = $Smp
    timeout_seconds = $TimeoutSeconds
    qemu_exit = $qemuExit
    log = $logPath
    required_marker_count = $requiredMarkers.Count
    missing_markers = $missingMarkers
    failure_hits = $failureHits
}

$summaryDir = Split-Path -Parent $summaryPath
if (-not [string]::IsNullOrWhiteSpace($summaryDir)) {
    New-Item -ItemType Directory -Path $summaryDir -Force | Out-Null
}

@(
    "Post-E14 GUI Validation: $result"
    "profile=$Profile smp=$Smp timeout_seconds=$TimeoutSeconds"
    "qemu_exit=$qemuExit"
    "log=$logPath"
    "required_markers=$($requiredMarkers.Count)"
    "missing_markers=$($missingMarkers.Count)"
    "failure_hits=$($failureHits.Count)"
) | Set-Content -Path $summaryPath

$summary | ConvertTo-Json -Depth 4 | Set-Content -Path $jsonPath

Write-Host "[poste14-gui] $result - $($requiredMarkers.Count - $missingMarkers.Count)/$($requiredMarkers.Count) markers found"
Write-Host "[poste14-gui] summary -> $summaryPath"

if (-not $passed) {
    if ($missingMarkers.Count -gt 0) {
        Write-Host "Missing markers:"
        $missingMarkers | ForEach-Object { Write-Host "  $_" }
    }
    if ($failureHits.Count -gt 0) {
        Write-Host "Failure lines:"
        $failureHits | ForEach-Object { Write-Host "  $_" }
    }
    exit 1
}
