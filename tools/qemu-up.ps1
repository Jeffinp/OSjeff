# Launch OSjeff in QEMU with a QMP control socket, wait for boot, and grab a
# screenshot via `screendump`. Leaves QEMU running (PID in .shots/qemu.pid) so
# further shots / input can be sent. Driven from WSL via powershell.exe.
param(
    [string]$Out = "F:\Projects\expo\OSjeff\.shots\desktop.ppm",
    [int]$Delay = 10,
    [switch]$VirtioGpu   # use -device virtio-vga (for virtio-gpu driver bring-up)
)
$ErrorActionPreference = 'Stop'
$qemu = 'C:\Program Files\qemu\qemu-system-x86_64.exe'
$root = Split-Path $PSScriptRoot   # tools\ -> project root
Set-Location $root

$fs = Join-Path $root 'osjeff-fs.img'
if (-not (Test-Path $fs)) { [IO.File]::WriteAllBytes($fs, (New-Object byte[] (64 * 1024))) }
$shots = Join-Path $root '.shots'
New-Item -ItemType Directory -Force -Path $shots | Out-Null

$serialLog = Join-Path $shots 'serial.log'
$qargs = @(
    '-accel', 'whpx', '-m', '256M',
    '-drive', "format=raw,file=$(Join-Path $root 'osjeff-bios.img')",
    '-drive', "format=raw,file=$fs,if=ide,index=2",
    '-netdev', 'user,id=n0',
    '-device', 'ne2k_isa,netdev=n0,mac=52:54:00:12:34:56',
    '-display', 'gtk',
    '-serial', "file:$serialLog",
    '-qmp', 'tcp:127.0.0.1:4444,server,nowait'
)
# virtio-vga keeps VBE compatibility (so the bootloader still gets a framebuffer)
# while exposing a virtio-gpu PCI device for the accelerated driver to drive.
if ($VirtioGpu) { $qargs += @('-vga', 'none', '-device', 'virtio-vga') }
$p = Start-Process -FilePath $qemu -ArgumentList $qargs -PassThru
$p.Id | Out-File (Join-Path $shots 'qemu.pid') -Encoding ascii
Write-Host "QEMU pid $($p.Id); aguardando ${Delay}s para o boot..."
Start-Sleep -Seconds $Delay

# QMP: negotiate, then screendump (default format PPM).
$c = New-Object System.Net.Sockets.TcpClient
$c.Connect('127.0.0.1', 4444)
$s = $c.GetStream()
$r = New-Object IO.StreamReader($s)
$w = New-Object IO.StreamWriter($s); $w.AutoFlush = $true
[void]$r.ReadLine()                                   # greeting
$w.WriteLine('{"execute":"qmp_capabilities"}'); [void]$r.ReadLine()
$j = $Out -replace '\\', '\\\\'
$w.WriteLine('{"execute":"screendump","arguments":{"filename":"' + $j + '"}}')
Write-Host ("screendump resp: " + $r.ReadLine())
Start-Sleep -Milliseconds 500
$c.Close()
Write-Host "shot salvo: $Out"
