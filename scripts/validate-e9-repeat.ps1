param(
    [string]$Profile = "debug",
    [string[]]$RunIds = @("A", "B", "C"),
    [int]$TimeoutSeconds = 70,
    [string]$LogPrefix = "build/e9-stable-repeat",
    [string]$SummaryPath = "build/e9-stable-repeat-summary.txt",
    [switch]$DiagUserDeepProbe,
    [switch]$DiagKernelDeepProbe
)

Set-Location (Join-Path $PSScriptRoot "..")

$expectedMarkers = @(
    "gui: demo PASS",
    "gui: fb-map PASS",
    "gui: window-mgr PASS",
    "process: model PASS",
    "drivers: driver-model PASS",
    "fs: vfs PASS",
    "scheduler: idle loop active"
)

$failRegexes = @(
    "(?m)^gui: .* FAIL$",
    "(?m)^arch: .* FAIL$",
    "(?m)^process: .* FAIL$",
    "(?m)^drivers: .* FAIL$",
    "(?m)^fs: .* FAIL$",
    "(?m)^scheduler: .* FAIL$",
    "(?m)^syscall: .* FAIL$",
    "PAGE FAULT"
)
$allRows = @()

$cargoFeatures = @()
if ($DiagUserDeepProbe) {
    $cargoFeatures += "gui-fb-user-deep-probe"
}
if ($DiagKernelDeepProbe) {
    $cargoFeatures += "gui-fb-kernel-deep-probe"
}

# Keep runs deterministic by clearing stale QEMU instances first.
Get-Process qemu-system-x86_64 -ErrorAction SilentlyContinue | Stop-Process -Force

foreach ($id in $RunIds) {
    $logPath = "$LogPrefix-$id.log"
    Write-Host "[e9-repeat] running $id -> $logPath"

    .\scripts\run-qemu.ps1 -Profile $Profile -LogPath $logPath -TimeoutSeconds $TimeoutSeconds -CargoFeatures $cargoFeatures | Out-Null
    $exitCode = $LASTEXITCODE

    if (-not (Test-Path $logPath)) {
        $allRows += [pscustomobject]@{
            Run = $id
            LogPath = $logPath
            ExitCode = $exitCode
            Status = "FAIL"
            KernelEntry = "no"
            FaultRIP = "none"
            FaultCR2 = "none"
            MissingMarkers = ($expectedMarkers -join "; ")
            FailHits = "log-missing"
        }
        continue
    }

    $logLines = Get-Content $logPath
    $logText = $logLines -join "`n"

    $missing = @()
    foreach ($marker in $expectedMarkers) {
        if ($logText -notmatch [regex]::Escape($marker)) {
            $missing += $marker
        }
    }

    $failHits = @()
    foreach ($rx in $failRegexes) {
        if ($logText -match $rx) {
            $failHits += $rx
        }
    }

    $kernelEntry = if ($logText -match [regex]::Escape("kernel: boot entry reached")) { "yes" } else { "no" }

    $faultRIP = "none"
    $faultCR2 = "none"
    $pfMatches = [regex]::Matches($logText, "RIP:\s*(\d+)\s+CR2:\s*(\d+)")
    if ($pfMatches.Count -gt 0) {
        $last = $pfMatches[$pfMatches.Count - 1]
        $ripNum = [uint64]$last.Groups[1].Value
        $cr2Num = [uint64]$last.Groups[2].Value
        $faultRIP = "{0} (0x{1:X})" -f $ripNum, $ripNum
        $faultCR2 = "{0} (0x{1:X})" -f $cr2Num, $cr2Num
    }

    $acceptableExit = ($exitCode -eq 0 -or $exitCode -eq 124)
    $status = if ($acceptableExit -and $missing.Count -eq 0 -and $failHits.Count -eq 0) { "PASS" } else { "FAIL" }

    $allRows += [pscustomobject]@{
        Run = $id
        LogPath = $logPath
        ExitCode = $exitCode
        Status = $status
        KernelEntry = $kernelEntry
        FaultRIP = $faultRIP
        FaultCR2 = $faultCR2
        MissingMarkers = if ($missing.Count -eq 0) { "none" } else { ($missing -join "; ") }
        FailHits = if ($failHits.Count -eq 0) { "none" } else { ($failHits -join "; ") }
    }
}

$overallPass = ($allRows | Where-Object { $_.Status -ne "PASS" }).Count -eq 0
$overallText = if ($overallPass) { "PASS" } else { "FAIL" }
$summaryHeader = "E9 Repeat Summary: $overallText"

$summaryLines = @()
$summaryLines += $summaryHeader
$summaryLines += "profile=$Profile timeout_seconds=$TimeoutSeconds runs=" + ($RunIds -join ",")
$cargoFeaturesText = if ($cargoFeatures.Count -eq 0) { "none" } else { ($cargoFeatures -join ",") }
$summaryLines += "cargo_features=$cargoFeaturesText"
$summaryLines += "diag_user_deep=$($DiagUserDeepProbe.IsPresent) diag_kernel_deep=$($DiagKernelDeepProbe.IsPresent)"
$summaryLines += ""

foreach ($row in $allRows) {
    $summaryLines += "run=$($row.Run) status=$($row.Status) exit=$($row.ExitCode)"
    $summaryLines += "log=$($row.LogPath)"
    $summaryLines += "kernel_entry=$($row.KernelEntry)"
    $summaryLines += "fault_rip=$($row.FaultRIP)"
    $summaryLines += "fault_cr2=$($row.FaultCR2)"
    $summaryLines += "missing=$($row.MissingMarkers)"
    $summaryLines += "fail_hits=$($row.FailHits)"
    $summaryLines += ""
}

$summaryDir = Split-Path -Parent $SummaryPath
if (-not [string]::IsNullOrWhiteSpace($summaryDir)) {
    New-Item -ItemType Directory -Path $summaryDir -Force | Out-Null
}
$summaryLines | Set-Content -Path $SummaryPath -NoNewline:$false

$allRows | Format-Table Run, Status, ExitCode, LogPath -AutoSize
Write-Host "[e9-repeat] summary -> $SummaryPath"

if ($overallPass) {
    exit 0
}

exit 1
