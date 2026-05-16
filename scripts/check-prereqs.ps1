Set-Location (Join-Path $PSScriptRoot "..")

$required = @(
    "rustc",
    "cargo",
    "rustup",
    "qemu-system-x86_64"
)

foreach ($tool in $required) {
    $command = Get-Command $tool -ErrorAction SilentlyContinue
    if ($null -eq $command) {
        Write-Host "missing: $tool"
    } else {
        Write-Host "found:   $tool -> $($command.Source)"
    }
}

if (Get-Command rustup -ErrorAction SilentlyContinue) {
    Write-Host ""
    Write-Host "Installed toolchains:"
    rustup toolchain list
}
