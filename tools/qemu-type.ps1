# Send a sequence of QMP qcodes (keystrokes) to the running QEMU (port 4444).
# Example: -Keys h,e,l,p,ret
param([string[]]$Keys)
$ErrorActionPreference = 'Stop'
$c = New-Object System.Net.Sockets.TcpClient
$c.Connect('127.0.0.1', 4444)
$s = $c.GetStream()
$r = New-Object IO.StreamReader($s)
$w = New-Object IO.StreamWriter($s); $w.AutoFlush = $true
[void]$r.ReadLine()
$w.WriteLine('{"execute":"qmp_capabilities"}'); [void]$r.ReadLine()
foreach ($k in $Keys) {
    $w.WriteLine('{"execute":"send-key","arguments":{"keys":[{"type":"qcode","data":"' + $k + '"}]}}')
    [void]$r.ReadLine()
    Start-Sleep -Milliseconds 90
}
$c.Close()
