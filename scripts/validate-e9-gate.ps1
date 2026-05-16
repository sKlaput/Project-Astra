param(
    [string]$Profile = "debug",
    [int]$TimeoutSeconds = 70,
    [string]$OutPrefix = "build/e9-gate",
    [switch]$Full
)

Set-Location (Join-Path $PSScriptRoot "..")

$runIds = if ($Full) { @("A", "B", "C") } else { @("A") }

Write-Host "[e9-gate] strict all-lane tripwire (stable + user-deep + kernel-deep)"
& .\scripts\validate-e9-tripwire.ps1 `
    -Profile $Profile `
    -RunIds $runIds `
    -TimeoutSeconds $TimeoutSeconds `
    -OutPrefix $OutPrefix `
    -IncludeKernelDeepLane `
    -KernelDeepBlocking

$exitCode = $LASTEXITCODE

if ($exitCode -eq 0) {
    Write-Host "[e9-gate] PASS"
    exit 0
}

Write-Host "[e9-gate] FAIL"
exit $exitCode
