# Two screendumps a few seconds apart with NO input in between — proves the
# focused WASM app animates on its own (the compositor's continuous frame pump).
$ErrorActionPreference = 'Stop'
$c = New-Object System.Net.Sockets.TcpClient
$c.Connect('127.0.0.1', 4444)
$s = $c.GetStream()
$r = New-Object IO.StreamReader($s)
$w = New-Object IO.StreamWriter($s); $w.AutoFlush = $true
[void]$r.ReadLine()
$w.WriteLine('{"execute":"qmp_capabilities"}'); [void]$r.ReadLine()

function Shot([string]$path) {
    $j = $path -replace '\\', '\\\\'
    $w.WriteLine('{"execute":"screendump","arguments":{"filename":"' + $j + '"}}')
    Write-Host ("resp: " + $r.ReadLine())
    Start-Sleep -Milliseconds 400
}

Shot 'F:\Projects\expo\OSjeff\.shots\frame_a.ppm'
Start-Sleep -Seconds 4
Shot 'F:\Projects\expo\OSjeff\.shots\frame_b.ppm'
$c.Close()
