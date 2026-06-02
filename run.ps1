# Build (release) + run OSjeff in QEMU on Windows.
#
# The kernel is compiled inside WSL (cargo/Rust live there), the resulting
# bootable image is copied next to this script, and QEMU runs on Windows.
#
# Usage:
#   .\run.ps1              # build release + boot with WHPX acceleration
#   .\run.ps1 -NoAccel     # build release + boot with software emulation (TCG)
#   .\run.ps1 -SkipBuild   # skip cargo build, just boot the existing image
param(
    [switch]$NoAccel,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$qemu = 'C:\Program Files\qemu\qemu-system-x86_64.exe'
$img = Join-Path $PSScriptRoot 'osjeff-bios.img'

if (-not (Test-Path $qemu)) { throw "QEMU nao encontrado em $qemu" }

if (-not $SkipBuild) {
    # Translate "C:\path\to" -> "/mnt/c/path/to" for WSL.
    $drive = $PSScriptRoot.Substring(0, 1).ToLower()
    $rest = ($PSScriptRoot.Substring(2)) -replace '\\', '/'
    $wslDir = "/mnt/$drive$rest"
    Write-Host "Compilando OSjeff (release) em $wslDir ..." -ForegroundColor Cyan

    # `bash -lc` loads the login profile so cargo/rustup are on PATH.
    wsl -e bash -lc "cd '$wslDir' && cargo build --package os --release"
    if ($LASTEXITCODE -ne 0) { throw "cargo build falhou (exit $LASTEXITCODE)" }

    # Copy the freshest generated image to the project root.
    $built = Get-ChildItem -Path (Join-Path $PSScriptRoot 'target\release\build') `
        -Recurse -Filter 'osjeff-bios.img' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime | Select-Object -Last 1
    if (-not $built) { throw "imagem osjeff-bios.img nao encontrada apos o build" }
    Copy-Item $built.FullName $img -Force
    Write-Host "Imagem pronta: $img" -ForegroundColor Green
}

if (-not (Test-Path $img)) { throw "Imagem nao existe: $img (rode sem -SkipBuild)" }

# Persistent filesystem disk (secondary IDE master). Created blank on first run;
# the kernel formats it on first boot and persists files there across reboots.
$fsImg = Join-Path $PSScriptRoot 'osjeff-fs.img'
if (-not (Test-Path $fsImg)) {
    [System.IO.File]::WriteAllBytes($fsImg, (New-Object byte[] (64 * 1024)))
    Write-Host "Disco de arquivos criado: $fsImg" -ForegroundColor Green
}

$qargs = @('-m', '256M', '-drive', "format=raw,file=$img",
    '-drive', "format=raw,file=$fsImg,if=ide,index=2")
if (-not $NoAccel) { $qargs = @('-accel', 'whpx') + $qargs }

Write-Host "Booting OSjeff..." -ForegroundColor Cyan
& $qemu @qargs
