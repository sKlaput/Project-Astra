Set-Location (Join-Path $PSScriptRoot "..")
$env:Path = "$env:USERPROFILE\.cargo\bin;C:\Program Files\qemu;" + $env:Path

$required = @(
    "rustc",
    "cargo",
    "rustup",
    "qemu-system-x86_64",
    "qemu-img"
)

$missing = $false
foreach ($tool in $required) {
    $command = Get-Command $tool -ErrorAction SilentlyContinue
    if ($null -eq $command) {
        Write-Host "missing: $tool"
        $missing = $true
    } else {
        Write-Host "found:   $tool -> $($command.Source)"
    }
}

$bootx64 = Join-Path $PSScriptRoot "..\tools\Limine-10.x-binary\BOOTX64.EFI"
$limineArchive = Join-Path $PSScriptRoot "..\tools\limine-v10.x-binary.zip"
if ((Test-Path $bootx64) -or (Test-Path $limineArchive)) {
    Write-Host "found:   Limine UEFI boot asset"
} else {
    Write-Host "missing: Limine UEFI boot asset or archive"
    $missing = $true
}

if (Get-Command rustup -ErrorAction SilentlyContinue) {
    Write-Host ""
    Write-Host "Installed toolchains:"
    rustup toolchain list
}

if ($missing) {
    exit 1
}
