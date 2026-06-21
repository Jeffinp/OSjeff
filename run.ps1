# Build (release) + run OSjeff in QEMU on Windows.
#
# The kernel is compiled inside WSL (cargo/Rust live there), the resulting
# bootable image is copied next to this script, and QEMU runs on Windows.
#
# Usage:
#   .\run.ps1                # build release + boot (WHPX + SDL display)
#   .\run.ps1 -NoAccel       # software CPU emulation (TCG) instead of WHPX
#   .\run.ps1 -Gl            # SDL with gl=on: GPU-uploads the framebuffer for a
#                            #   faster blit (needs a QEMU build with OpenGL)
#   .\run.ps1 -SoftwareGfx   # keep QEMU's default display (escape hatch if the
#                            #   SDL/GL path misbehaves on this machine)
#   .\run.ps1 -SkipBuild     # skip cargo build, just boot the existing image
#   .\run.ps1 -Usb           # build the UEFI image + copy it out for USB flashing
#                            #   (real hardware boot; does NOT launch QEMU)
param(
    [switch]$NoAccel,
    [switch]$SkipBuild,
    [switch]$Gl,
    [switch]$SoftwareGfx,
    [switch]$Usb
)

$ErrorActionPreference = 'Stop'
$qemu = 'C:\Program Files\qemu\qemu-system-x86_64.exe'
$img = Join-Path $PSScriptRoot 'osjeff-bios.img'

# Build the project's `os` package (release) inside WSL. Returns nothing; throws
# on failure. Factored out so both the QEMU and USB paths share it.
function Build-OSjeff {
    $drive = $PSScriptRoot.Substring(0, 1).ToLower()
    $rest = ($PSScriptRoot.Substring(2)) -replace '\\', '/'
    $wslDir = "/mnt/$drive$rest"
    Write-Host "Compilando OSjeff (release) em $wslDir ..." -ForegroundColor Cyan
    wsl -e bash -lc "cd '$wslDir' && cargo build --package os --release"
    if ($LASTEXITCODE -ne 0) { throw "cargo build falhou (exit $LASTEXITCODE)" }
}

if ($Usb) {
    # Real-hardware boot: build, then copy the UEFI image to the project root so
    # it can be flashed RAW to a USB stick. No QEMU involved.
    Build-OSjeff
    $built = Get-ChildItem -Path (Join-Path $PSScriptRoot 'target\release\build') `
        -Recurse -Filter 'osjeff-uefi.img' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime | Select-Object -Last 1
    if (-not $built) { throw "osjeff-uefi.img nao encontrada apos o build" }
    $usbImg = Join-Path $PSScriptRoot 'osjeff-uefi.img'
    Copy-Item $built.FullName $usbImg -Force
    Write-Host ""
    Write-Host "Imagem UEFI pronta: $usbImg" -ForegroundColor Green
    Write-Host "Grave-a CRUA num pendrive (apaga o pendrive inteiro!):" -ForegroundColor Yellow
    Write-Host "  - Rufus: selecione a imagem, modo 'DD Image'; ou" -ForegroundColor Gray
    Write-Host "  - balenaEtcher: Flash from file -> escolha o pendrive." -ForegroundColor Gray
    Write-Host "No PC alvo: firmware UEFI, Secure Boot DESLIGADO, dê boot pelo pendrive." -ForegroundColor Gray
    Write-Host "Detalhes e limitacoes: docs/BOOT-USB.md" -ForegroundColor Gray
    exit 0
}

if (-not (Test-Path $qemu)) { throw "QEMU nao encontrado em $qemu" }

if (-not $SkipBuild) {
    Build-OSjeff

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

$pcap = Join-Path $PSScriptRoot 'osjeff-net.pcap'
$qargs = @('-m', '256M', '-drive', "format=raw,file=$img",
    '-drive', "format=raw,file=$fsImg,if=ide,index=2",
    '-netdev', 'user,id=n0',
    '-device', 'ne2k_isa,netdev=n0,mac=52:54:00:12:34:56',
    '-object', "filter-dump,id=dump,netdev=n0,file=$pcap")
if (-not $NoAccel) { $qargs = @('-accel', 'whpx') + $qargs }

# Display. The kernel paints a full 1920x1080x4 (~8 MiB) linear framebuffer each
# dirty frame; QEMU's default Windows display (GTK/Cairo) re-uploads that surface
# in software, which dominates the per-frame cost. SDL uses a faster host
# renderer; `-Gl` adds gl=on to offload the VRAM->window upload to the GPU.
#
# Official Windows QEMU builds differ in which backends they ship -- naming an
# absent one ("-display sdl" on a GTK-only build) makes QEMU exit immediately
# (window flashes open then closes). So probe `-display help` and pick the best
# backend actually present, preferring SDL; if neither is found we omit the flag
# and let QEMU use its default. `-vga std` is pinned because the bootloader takes
# its framebuffer from VBE -- virtio GPUs expose none without a guest driver and
# would boot to a black screen.
if (-not $SoftwareGfx) {
    # GTK is the only reliable backend on the official Windows QEMU builds: SDL is
    # listed by `-display help` but exits at init (window flashes and closes), and
    # gl=on is buggy on Windows for both backends -- so we do NOT enable GL by
    # default. `-Gl` lets you try gtk,gl=on at your own risk.
    #   refs: gitlab.com/qemu-project/qemu issues #2200, #1530; quickemu #967
    $avail = (& $qemu -display help 2>&1 | Out-String)
    $backend = @('gtk', 'sdl') | Where-Object { $avail -match "\b$_\b" } | Select-Object -First 1
    if ($backend) {
        $display = if ($Gl) { "$backend,gl=on" } else { $backend }
        $qargs += @('-vga', 'std', '-display', $display)
        Write-Host "Display: $display" -ForegroundColor DarkGray
    } else {
        Write-Host "Nenhum backend GTK/SDL; usando display padrao do QEMU." -ForegroundColor Yellow
    }
}

Write-Host "Booting OSjeff..." -ForegroundColor Cyan
& $qemu @qargs
