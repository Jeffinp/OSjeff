# Take a screenshot of the already-running QEMU via its QMP socket (port 4444).
param([string]$Out = "F:\Projects\expo\OSjeff\.shots\shot.ppm")
$ErrorActionPreference = 'Stop'
$c = New-Object System.Net.Sockets.TcpClient
$c.Connect('127.0.0.1', 4444)
$s = $c.GetStream()
$r = New-Object IO.StreamReader($s)
$w = New-Object IO.StreamWriter($s); $w.AutoFlush = $true
[void]$r.ReadLine()
$w.WriteLine('{"execute":"qmp_capabilities"}'); [void]$r.ReadLine()
$j = $Out -replace '\\', '\\\\'
$w.WriteLine('{"execute":"screendump","arguments":{"filename":"' + $j + '"}}')
Write-Host ("resp: " + $r.ReadLine())
Start-Sleep -Milliseconds 400
$c.Close()
