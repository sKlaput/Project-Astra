param(
    [string]$Profile = "debug",
    [string]$LogPath = "",
    [int]$TimeoutSeconds = 0,
    [string[]]$CargoFeatures = @(),
    [switch]$Visual
)

Set-Location (Join-Path $PSScriptRoot "..")

$env:Path = "$env:USERPROFILE\.cargo\bin;C:\Program Files\qemu;" + $env:Path

$imageRoot = Join-Path $PSScriptRoot "..\build\image-root"
$ovmfCode = "C:\Program Files\qemu\share\edk2-x86_64-code.fd"
$buildDir = Join-Path $PSScriptRoot "..\build"
$ovmfLocalCode = Join-Path $buildDir "edk2-x86_64-code.fd"
$diskImg = Join-Path $buildDir "disk.img"
$fatDrive = "file=fat:rw:$imageRoot,format=raw,if=ide"
$pflashDrive = "if=pflash,format=raw,readonly=on,file=$ovmfLocalCode"
$dataDrive = "file=$diskImg,if=none,id=datadisk,format=raw"
$dataDevice = "virtio-blk-pci,drive=datadisk"

# Create a 64 MiB blank disk image for persistent user data if it doesn't exist yet.
if (-not (Test-Path $diskImg)) {
    Write-Host "Creating persistent data disk at $diskImg (64 MiB)"
    & qemu-img create -f raw $diskImg 64M | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Error "qemu-img failed to create disk.img"
        exit 1
    }
}

& (Join-Path $PSScriptRoot "build-image.ps1") -Profile $Profile -CargoFeatures $CargoFeatures

if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

if (-not (Test-Path $ovmfCode)) {
    Write-Error "Missing OVMF firmware at $ovmfCode"
    exit 1
}

if (-not (Test-Path $buildDir)) {
    New-Item -ItemType Directory -Path $buildDir -Force | Out-Null
}

Copy-Item $ovmfCode $ovmfLocalCode -Force

# ── Acceleration ─────────────────────────────────────────────────────────────
# Try Windows Hypervisor Platform (WHPX) first — uses hardware virtualisation
# and makes HLT instructions in the guest actually yield the host CPU, dropping
# host CPU usage from ~99% (TCG software emulation) to ~5%.
# Requires: Windows 10/11 with Hyper-V / Windows Hypervisor Platform feature enabled.
# Fallback: remove "-accel","whpx" to use TCG (slow but always available).
# TCG software emulation — WHPX is incompatible with OVMF's UEFI firmware
# init which touches legacy ISA/PCI MMIO that WHPX cannot emulate (status 2).
# TCG is slower but the kernel-side fixes (damage rects, scissor clip, smart
# sleep) dramatically reduce the actual work per frame.
$accelArgs = @("-accel", "tcg")

if ($Visual) {
    $serialLogFile = Join-Path $buildDir "qemu-visual-serial.log"
    $pcapFile = Join-Path $buildDir "net.pcap"
    $qemuArgs = @(
        "-machine", "q35",
        "-m", "512M",
        "-smp", "1",
        "-no-reboot"
    ) + $accelArgs + @(
        "-serial", "file:$serialLogFile",
        "-vga", "std",
        "-netdev", "user,id=net0",
        "-device", "virtio-net-pci,netdev=net0",
        "-object", "filter-dump,id=dump0,netdev=net0,file=$pcapFile",
        "-drive", $pflashDrive,
        "-drive", $fatDrive,
        "-drive", $dataDrive,
        "-device", $dataDevice
    )
} else {
    $qemuArgs = @(
        "-machine", "q35",
        "-m", "512M",
        "-smp", "1",
        "-no-reboot"
    ) + $accelArgs + @(
        "-serial", "stdio",
        "-display", "none",
        "-vga", "std",
        "-netdev", "user,id=net0",
        "-device", "virtio-net-pci,netdev=net0",
        "-drive", $pflashDrive,
        "-drive", $fatDrive,
        "-drive", $dataDrive,
        "-device", $dataDevice
    )
}

if ([string]::IsNullOrWhiteSpace($LogPath)) {
    qemu-system-x86_64 @qemuArgs
} else {
    $resolvedLogPath = Resolve-Path -LiteralPath (Split-Path -Parent $LogPath) -ErrorAction SilentlyContinue
    if (-not $resolvedLogPath) {
        $parentDir = Split-Path -Parent $LogPath
        if (-not [string]::IsNullOrWhiteSpace($parentDir)) {
            New-Item -ItemType Directory -Path $parentDir -Force | Out-Null
        }
    }

    $stderrPath = "$LogPath.err"
    Write-Host "Capturing QEMU output to:" $LogPath

    $process = Start-Process -FilePath "qemu-system-x86_64" -ArgumentList $qemuArgs -NoNewWindow -RedirectStandardOutput $LogPath -RedirectStandardError $stderrPath -PassThru

    $timedOut = $false
    if ($TimeoutSeconds -gt 0) {
        Wait-Process -Id $process.Id -Timeout $TimeoutSeconds -ErrorAction SilentlyContinue
        $process.Refresh()
        if (-not $process.HasExited) {
            # Kill the full process tree to avoid orphaned QEMU instances.
            & taskkill /PID $process.Id /T /F | Out-Null
            Wait-Process -Id $process.Id -ErrorAction SilentlyContinue
            $process.Refresh()
            $timedOut = $true
        }
    } else {
        Wait-Process -Id $process.Id
        $process.Refresh()
    }

    if (Test-Path $stderrPath) {
        $stderrContent = Get-Content $stderrPath -Raw
        if (-not [string]::IsNullOrWhiteSpace($stderrContent)) {
            Add-Content -Path $LogPath -Value "`n--- stderr ---`n$stderrContent"
        }
        Remove-Item $stderrPath -Force
    }

    if ($timedOut) {
        Write-Warning "QEMU timed out after $TimeoutSeconds seconds and was force-stopped."
        exit 124
    }

    exit $process.ExitCode
}