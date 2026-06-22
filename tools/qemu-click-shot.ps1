# Inject mouse clicks via QMP, then screendump — used to prove the interactive
# WASM app reacts to input. Clicks the three counter cards a few times each.
param([string]$Out = "F:\Projects\expo\OSjeff\.shots\shot.ppm")
$ErrorActionPreference = 'Stop'
$c = New-Object System.Net.Sockets.TcpClient
$c.Connect('127.0.0.1', 4444)
$s = $c.GetStream()
$r = New-Object IO.StreamReader($s)
$w = New-Object IO.StreamWriter($s); $w.AutoFlush = $true
[void]$r.ReadLine()
$w.WriteLine('{"execute":"qmp_capabilities"}'); [void]$r.ReadLine()

function Click([int]$x, [int]$y) {
    $nx = [int]($x * 32767 / 1280)
    $ny = [int]($y * 32767 / 720)
    $move = '{"type":"abs","data":{"axis":"x","value":' + $nx + '}},{"type":"abs","data":{"axis":"y","value":' + $ny + '}}'
    $down = '{"execute":"input-send-event","arguments":{"events":[' + $move + ',{"type":"btn","data":{"down":true,"button":"left"}}]}}'
    $up = '{"execute":"input-send-event","arguments":{"events":[{"type":"btn","data":{"down":false,"button":"left"}}]}}'
    $w.WriteLine($down); [void]$r.ReadLine()
    Start-Sleep -Milliseconds 80
    $w.WriteLine($up); [void]$r.ReadLine()
    Start-Sleep -Milliseconds 80
}

# Button centers (screen px): A=(375,287) B=(599,287) C=(823,287).
for ($i = 0; $i -lt 6; $i++) { Click 375 287 }
for ($i = 0; $i -lt 4; $i++) { Click 599 287 }
for ($i = 0; $i -lt 2; $i++) { Click 823 287 }

$j = $Out -replace '\\', '\\\\'
$w.WriteLine('{"execute":"screendump","arguments":{"filename":"' + $j + '"}}')
Write-Host ("resp: " + $r.ReadLine())
Start-Sleep -Milliseconds 500
$c.Close()
