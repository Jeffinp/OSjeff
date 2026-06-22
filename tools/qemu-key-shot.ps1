# Inject Space key presses via QMP, then screendump — proves the focused
# interactive WASM app reacts to keyboard input (on_key bumps contador A).
param([string]$Out = "F:\Projects\expo\OSjeff\.shots\shot.ppm")
$ErrorActionPreference = 'Stop'
$c = New-Object System.Net.Sockets.TcpClient
$c.Connect('127.0.0.1', 4444)
$s = $c.GetStream()
$r = New-Object IO.StreamReader($s)
$w = New-Object IO.StreamWriter($s); $w.AutoFlush = $true
[void]$r.ReadLine()
$w.WriteLine('{"execute":"qmp_capabilities"}'); [void]$r.ReadLine()

function Key([string]$qcode) {
    $down = '{"execute":"input-send-event","arguments":{"events":[{"type":"key","data":{"down":true,"key":{"type":"qcode","data":"' + $qcode + '"}}}]}}'
    $up = '{"execute":"input-send-event","arguments":{"events":[{"type":"key","data":{"down":false,"key":{"type":"qcode","data":"' + $qcode + '"}}}]}}'
    $w.WriteLine($down); [void]$r.ReadLine()
    Start-Sleep -Milliseconds 60
    $w.WriteLine($up); [void]$r.ReadLine()
    Start-Sleep -Milliseconds 90
}

for ($i = 0; $i -lt 9; $i++) { Key 'spc' }

$j = $Out -replace '\\', '\\\\'
$w.WriteLine('{"execute":"screendump","arguments":{"filename":"' + $j + '"}}')
Write-Host ("resp: " + $r.ReadLine())
Start-Sleep -Milliseconds 500
$c.Close()
