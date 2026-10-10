# Boot Kitsune on every video adapter QEMU can emulate (BIOS/VBE and UEFI/GOP), take a screenshot
# of each, and write a report under .\hw-report\. Then it builds the UEFI image for a USB stick.
# Windows only; QEMU in C:\Program Files\qemu. Run from the project root:
#
#   .\tools\hw-check.ps1                 # build + every adapter (WHPX, falls back to TCG)
#   .\tools\hw-check.ps1 -SkipBuild      # reuse kitsune-bios.img / kitsune-uefi.img in the root
#   .\tools\hw-check.ps1 -NoAccel        # force software emulation
#   .\tools\hw-check.ps1 -Seconds 25     # how long each case runs before the screenshot
#   .\tools\hw-check.ps1 -Only std,vmware  # only some adapters
#
# What it tells you: which adapters reach the desktop (serial log has "TSC calibrated" and the
# screenshot is not blank). It does NOT test your physical GPU: QEMU cannot hand a real Radeon to
# the guest. For that, flash the USB image (docs/BOOT-USB.md) and use the checklist it prints.
param(
    [switch]$SkipBuild,
    [switch]$NoAccel,
    [int]$Seconds = 30,
    [string[]]$Only
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$qemu = 'C:\Program Files\qemu\qemu-system-x86_64.exe'
$qdir = Split-Path -Parent $qemu
if (-not (Test-Path $qemu)) { throw "QEMU nao encontrado em $qemu" }

# --- images -------------------------------------------------------------------------------------
if (-not $SkipBuild) {
    & (Join-Path $root 'run.ps1') -Usb          # builds release, copies kitsune-uefi.img to the root
    $built = Get-ChildItem (Join-Path $root 'target\release\build') -Recurse -Filter 'kitsune-bios.img' |
        Sort-Object LastWriteTime | Select-Object -Last 1
    Copy-Item $built.FullName (Join-Path $root 'kitsune-bios.img') -Force
}
$bios = Join-Path $root 'kitsune-bios.img'
$uefi = Join-Path $root 'kitsune-uefi.img'
foreach ($f in $bios, $uefi) { if (-not (Test-Path $f)) { throw "Falta $f (rode sem -SkipBuild)" } }

# --- firmware for the UEFI cases (official Windows builds ship edk2 under share\) ----------------
$code = @('edk2-x86_64-code.fd', 'OVMF_CODE.fd') | ForEach-Object { Join-Path $qdir "share\$_" } |
    Where-Object { Test-Path $_ } | Select-Object -First 1
$vars = @('edk2-i386-vars.fd', 'OVMF_VARS.fd') | ForEach-Object { Join-Path $qdir "share\$_" } |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $code) { Write-Host "Firmware UEFI nao encontrado em $qdir\share: os casos UEFI serao pulados." -ForegroundColor Yellow }

# name | modes | qemu args
$cases = @(
    @('std',           'bios uefi', '-vga std'),
    @('virtio-vga',    'bios uefi', '-vga virtio'),
    @('vmware',        'bios uefi', '-vga vmware'),
    @('qxl',           'bios uefi', '-vga qxl'),
    @('cirrus',        'bios',      '-vga cirrus'),
    @('bochs-display', 'uefi',      '-vga none -device bochs-display'),
    @('ramfb',         'uefi',      '-vga none -device ramfb'),
    @('virtio-gpu',    'uefi',      '-vga none -device virtio-gpu-pci')
)
if ($Only) { $cases = $cases | Where-Object { $Only -contains $_[0] } }

$have = (& $qemu -device help 2>&1 | Out-String)
$out = Join-Path $root 'hw-report'
Remove-Item $out -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $out | Out-Null

function Get-Shot($port, $file) {
    # QMP screendump; PNG where this QEMU can, else PPM.
    $c = New-Object System.Net.Sockets.TcpClient
    $c.Connect('127.0.0.1', $port)
    $s = $c.GetStream()
    $r = New-Object IO.StreamReader($s); $w = New-Object IO.StreamWriter($s); $w.AutoFlush = $true
    [void]$r.ReadLine()
    $w.WriteLine('{"execute":"qmp_capabilities"}'); [void]$r.ReadLine()
    $j = $file -replace '\\', '/'
    $w.WriteLine('{"execute":"screendump","arguments":{"filename":"' + $j + '","format":"png"}}')
    $resp = $r.ReadLine()
    if ($resp -match 'error') {
        $j = ($file -replace '\.png$', '.ppm') -replace '\\', '/'
        $w.WriteLine('{"execute":"screendump","arguments":{"filename":"' + $j + '"}}'); [void]$r.ReadLine()
    }
    Start-Sleep -Milliseconds 600
    $c.Close()
}

$rows = @()
$port = 4500
foreach ($case in $cases) {
    $name, $modes, $extra = $case
    foreach ($mode in $modes.Split(' ')) {
        if ($mode -eq 'uefi' -and -not $code) { continue }
        $dev = ($extra -split ' ') | Where-Object { $_ -like '*-pci' -or $_ -eq 'bochs-display' -or $_ -eq 'ramfb' }
        if ($dev -and $have -notmatch "name `"$($dev | Select-Object -Last 1)`"") {
            $rows += [pscustomobject]@{ Case = "$mode-$name"; Result = 'SKIP'; Note = 'dispositivo ausente neste QEMU' }
            continue
        }
        $id = "$mode-$name"
        $dir = Join-Path $out $id
        New-Item -ItemType Directory $dir | Out-Null
        $fs = Join-Path $dir 'fs.img'
        $f = [System.IO.File]::Create($fs); $f.SetLength(64MB); $f.Close()
        $img = if ($mode -eq 'uefi') { $uefi } else { $bios }
        $serial = Join-Path $dir 'serial.log'
        $port++
        $qa = @('-m', '512M', '-no-reboot', '-display', 'none',
            '-drive', "format=raw,file=$img", '-drive', "format=raw,file=$fs,if=ide,index=2",
            '-netdev', 'user,id=n0', '-device', 'ne2k_isa,netdev=n0,mac=52:54:00:12:34:56',
            '-serial', "file:$serial", '-qmp', "tcp:127.0.0.1:$port,server,nowait")
        if (-not $NoAccel) { $qa = @('-accel', 'whpx,kernel-irqchip=off', '-accel', 'tcg') + $qa }
        if ($have -match 'name "virtio-rng-pci"') { $qa += @('-device', 'virtio-rng-pci') }
        if ($mode -eq 'uefi') {
            $vcopy = Join-Path $dir 'vars.fd'; Copy-Item $vars $vcopy
            $qa = @('-drive', "if=pflash,format=raw,readonly=on,file=$code",
                    '-drive', "if=pflash,format=raw,file=$vcopy") + $qa
        }
        $qa += ($extra -split ' ')
        # Start-Process joins the arguments with spaces and does not quote them: do it here
        # (the firmware path under "Program Files" has a space).
        $qa = $qa | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }
        Write-Host "[$id] ..." -NoNewline
        $p = Start-Process $qemu -ArgumentList $qa -PassThru -WindowStyle Hidden
        Start-Sleep -Seconds $Seconds
        $png = Join-Path $dir 'screen.png'
        try { Get-Shot $port $png } catch { Write-Host " (sem captura: $_)" -NoNewline }
        if (-not $p.HasExited) { Stop-Process $p -Force }
        $log = if (Test-Path $serial) { Get-Content $serial -Raw } else { '' }
        $booted = $log -match 'TSC calibrated'
        $panic = $log -match 'KERNEL PANIC|FATAL|panicked at'
        $res = if ($booted -and -not $panic) { 'OK' } else { 'FALHOU' }
        $note = ''
        if ($panic) { $note = ($log -split "`n" | Where-Object { $_ -match 'panicked|FATAL|PANIC' } | Select-Object -First 1) }
        elseif (-not $booted) { $note = 'nao chegou ao desktop (veja serial.log)' }
        Write-Host " $res"
        $rows += [pscustomobject]@{ Case = $id; Result = $res; Note = $note.Trim() }
    }
}

$report = Join-Path $out 'report.txt'
$gpu = Get-CimInstance Win32_VideoController | ForEach-Object { "$($_.Name)  driver $($_.DriverVersion)  $($_.CurrentHorizontalResolution)x$($_.CurrentVerticalResolution)" }
$fw = (Get-CimInstance Win32_ComputerSystem).Model
@(
    "Kitsune hw-check  $(Get-Date -Format s)",
    "Maquina: $fw",
    "GPU(s) do Windows: $($gpu -join '; ')",
    "Aceleracao: $(if ($NoAccel) { 'TCG (software)' } else { 'WHPX se disponivel, senao TCG' })",
    '',
    ($rows | Format-Table -AutoSize | Out-String)
) | Set-Content $report -Encoding UTF8
Write-Host ""
Get-Content $report
Write-Host "Capturas e logs: $out" -ForegroundColor Green

Write-Host @"

=== Hardware real (a placa de video de verdade) ===
1. Grave kitsune-uefi.img (agora na raiz do projeto) num pendrive: docs\BOOT-USB.md
2. Firmware UEFI, Secure Boot DESLIGADO, boot pelo pendrive.
3. Anote/fotografe:
   a) o desktop aparece? resolucao e cores certas (sem tons trocados, sem linhas deslocadas)?
   b) mexer o mouse (PS/2 ou USB legado) e digitar no Terminal funciona?
   c) abra o app Registro (Apps > Registro) e fotografe: lista os dispositivos PCI (vendor:device)
      e diz se ha disco/rede detectados. E isso que diz quais drivers faltam.
   d) Ajustes > Sobre: memoria e resolucao.
4. Se a tela ficar preta ou a maquina reiniciar, diga o modelo e se o Windows usa Secure Boot/CSM.
"@ -ForegroundColor Cyan
