# Boots OSjeff in QEMU on Windows.
# Usage:
#   .\run.ps1            # WHPX acceleration (fast), falls back to TCG
#   .\run.ps1 -NoAccel   # force software emulation (TCG)
param([switch]$NoAccel)

$ErrorActionPreference = 'Stop'

$qemu = 'C:\Program Files\qemu\qemu-system-x86_64.exe'
$img  = Join-Path $PSScriptRoot 'osjeff-bios.img'

if (-not (Test-Path $qemu)) { throw "QEMU nao encontrado em $qemu" }
if (-not (Test-Path $img))  { throw "Imagem nao encontrada: $img (rode 'cargo build --package os --release' e copie a img)" }

$args = @('-m', '256M', '-drive', "format=raw,file=$img")

if (-not $NoAccel) {
    # WHPX = Windows Hypervisor Platform. Massive speedup vs software emulation.
    $args = @('-accel', 'whpx') + $args
}

Write-Host "Booting OSjeff..." -ForegroundColor Cyan
& $qemu @args
