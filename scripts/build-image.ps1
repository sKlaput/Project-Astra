param(
    [string]$Profile = "debug",
    [string[]]$CargoFeatures = @()
)

Set-Location (Join-Path $PSScriptRoot "..")

$env:Path = "$env:USERPROFILE\.cargo\bin;C:\Program Files\qemu;" + $env:Path

$target = "x86_64-os"
$kernelBinary = Join-Path $PSScriptRoot "..\target\$target\$Profile\kernel"
$imageRoot = Join-Path $PSScriptRoot "..\build\image-root"
$bootDir = Join-Path $imageRoot "boot"
$efiBootDir = Join-Path $imageRoot "EFI\BOOT"
$limineDir = Join-Path $PSScriptRoot "..\tools\Limine-10.x-binary"
$bootx64 = Join-Path $limineDir "BOOTX64.EFI"

Write-Host "Building kernel for target:" $target
$featureArgs = @()
if ($CargoFeatures.Count -gt 0) {
    $featureArgs = @("--features", ($CargoFeatures -join ","))
}

if ($Profile -eq "release") {
    cargo build -Z build-std=core,alloc -p kernel --release @featureArgs
} else {
    cargo build -Z build-std=core,alloc -p kernel @featureArgs
}

if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

if (-not (Test-Path $bootDir)) {
    New-Item -ItemType Directory -Path $bootDir -Force | Out-Null
}

if (-not (Test-Path $efiBootDir)) {
    New-Item -ItemType Directory -Path $efiBootDir -Force | Out-Null
}

if (-not (Test-Path $bootx64)) {
    Write-Error "Missing Limine UEFI binary at $bootx64"
    exit 1
}

if (-not (Test-Path $kernelBinary)) {
    Write-Error "Missing kernel binary at $kernelBinary"
    exit 1
}

Copy-Item (Join-Path $PSScriptRoot "..\limine.conf") (Join-Path $imageRoot "limine.conf") -Force
Copy-Item $kernelBinary (Join-Path $bootDir "kernel") -Force
Copy-Item $bootx64 (Join-Path $efiBootDir "BOOTX64.EFI") -Force

Write-Host "Prepared UEFI image root at:" $imageRoot