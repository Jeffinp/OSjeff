# Steer the snake game with a few WASD keys via QMP, then screendump — proves
# the Rust->wasm game runs and reacts (turns/grows) on the continuous frame pump.
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
    $d = '{"execute":"input-send-event","arguments":{"events":[{"type":"key","data":{"down":true,"key":{"type":"qcode","data":"' + $qcode + '"}}}]}}'
    $u = '{"execute":"input-send-event","arguments":{"events":[{"type":"key","data":{"down":false,"key":{"type":"qcode","data":"' + $qcode + '"}}}]}}'
    $w.WriteLine($d); [void]$r.ReadLine(); Start-Sleep -Milliseconds 50
    $w.WriteLine($u); [void]$r.ReadLine()
}

# Curve the snake around the field so it stays alive and visibly turns.
Key 's'; Start-Sleep -Milliseconds 350
Key 'd'; Start-Sleep -Milliseconds 350
Key 's'; Start-Sleep -Milliseconds 350
Key 'a'; Start-Sleep -Milliseconds 350

$j = $Out -replace '\\', '\\\\'
$w.WriteLine('{"execute":"screendump","arguments":{"filename":"' + $j + '"}}')
Write-Host ("resp: " + $r.ReadLine())
Start-Sleep -Milliseconds 500
$c.Close()
