param(
    [string]$Profile = "debug",
    [string[]]$RunIds = @("A"),
    [int]$TimeoutSeconds = 70,
    [string]$OutPrefix = "build/e9-tripwire",
    [switch]$IncludeKernelDeepLane,
    [switch]$KernelDeepBlocking
)

Set-Location (Join-Path $PSScriptRoot "..")

$stableSummary = "$OutPrefix-stable-summary.txt"
$diagSummary = "$OutPrefix-diag-user-summary.txt"
$kernelDiagSummary = "$OutPrefix-diag-kernel-summary.txt"
$combinedSummary = "$OutPrefix-summary.txt"
$combinedJson = "$OutPrefix-summary.json"

$stableLogPrefix = "$OutPrefix-stable"
$diagLogPrefix = "$OutPrefix-diag-user"
$kernelDiagLogPrefix = "$OutPrefix-diag-kernel"

Write-Host "[e9-tripwire] stable lane"
& .\scripts\validate-e9-repeat.ps1 `
    -Profile $Profile `
    -RunIds $RunIds `
    -TimeoutSeconds $TimeoutSeconds `
    -LogPrefix $stableLogPrefix `
    -SummaryPath $stableSummary
$stableExit = $LASTEXITCODE

Write-Host "[e9-tripwire] user-deep diagnostic lane"
& .\scripts\validate-e9-repeat.ps1 `
    -Profile $Profile `
    -RunIds $RunIds `
    -TimeoutSeconds $TimeoutSeconds `
    -LogPrefix $diagLogPrefix `
    -SummaryPath $diagSummary `
    -DiagUserDeepProbe
$diagExit = $LASTEXITCODE

$kernelDiagExit = 0
$kernelDiagStatus = "SKIPPED"
if ($IncludeKernelDeepLane) {
    Write-Host "[e9-tripwire] kernel-deep diagnostic lane"
    & .\scripts\validate-e9-repeat.ps1 `
        -Profile $Profile `
        -RunIds $RunIds `
        -TimeoutSeconds $TimeoutSeconds `
        -LogPrefix $kernelDiagLogPrefix `
        -SummaryPath $kernelDiagSummary `
        -DiagKernelDeepProbe
    $kernelDiagExit = $LASTEXITCODE
    $kernelDiagStatus = if ($kernelDiagExit -eq 0) { "PASS" } else { "FAIL" }
}

$kernelCountsForGate = ($IncludeKernelDeepLane -and $KernelDeepBlocking)
$kernelGateOk = if ($kernelCountsForGate) { $kernelDiagExit -eq 0 } else { $true }
$overallPass = ($stableExit -eq 0 -and $diagExit -eq 0 -and $kernelGateOk)
$overallText = if ($overallPass) { "PASS" } else { "FAIL" }
$kernelMode = if (-not $IncludeKernelDeepLane) {
    "off"
} elseif ($KernelDeepBlocking) {
    "blocking"
} else {
    "non-blocking"
}

$runList = [string]::Join(",", $RunIds)

$lines = @(
    "E9 Tripwire Summary: $overallText",
    "profile=$Profile timeout_seconds=$TimeoutSeconds runs=$runList",
    "include_kernel_deep=$($IncludeKernelDeepLane.IsPresent)",
    "kernel_deep_mode=$kernelMode",
    "stable_exit=$stableExit",
    "diag_user_exit=$diagExit",
    "diag_kernel_exit=$kernelDiagExit",
    "diag_kernel_status=$kernelDiagStatus",
    "stable_summary=$stableSummary",
    "diag_summary=$diagSummary",
    "diag_kernel_summary=$kernelDiagSummary"
)

$summaryDir = Split-Path -Parent $combinedSummary
if (-not [string]::IsNullOrWhiteSpace($summaryDir)) {
    New-Item -ItemType Directory -Path $summaryDir -Force | Out-Null
}

$lines | Set-Content -Path $combinedSummary -NoNewline:$false
Write-Host "[e9-tripwire] summary -> $combinedSummary"

$json = [pscustomobject]@{
    result = $overallText
    profile = $Profile
    timeout_seconds = $TimeoutSeconds
    runs = $RunIds
    include_kernel_deep = $IncludeKernelDeepLane.IsPresent
    kernel_deep_mode = $kernelMode
    exits = [pscustomobject]@{
        stable = $stableExit
        diag_user = $diagExit
        diag_kernel = $kernelDiagExit
    }
    statuses = [pscustomobject]@{
        stable = if ($stableExit -eq 0) { "PASS" } else { "FAIL" }
        diag_user = if ($diagExit -eq 0) { "PASS" } else { "FAIL" }
        diag_kernel = $kernelDiagStatus
    }
    summaries = [pscustomobject]@{
        stable = $stableSummary
        diag_user = $diagSummary
        diag_kernel = $kernelDiagSummary
        combined = $combinedSummary
    }
}
$json | ConvertTo-Json -Depth 6 | Set-Content -Path $combinedJson -NoNewline:$false
Write-Host "[e9-tripwire] json -> $combinedJson"

if ($overallPass) {
    exit 0
}

exit 1
