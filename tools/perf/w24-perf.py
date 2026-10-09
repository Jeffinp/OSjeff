#!/usr/bin/env python3
"""w24-perf.py <rundir>: the browser lines of a perf-trace run (tools/perf/scen/w24-perf.sh).

Prints every `[trace] browser ...` note (parse, layout, load to first paint) and, for each
per-second block, the frame paths with their count, mean and worst frame time, so the
scroll phases (many `animdm`/`steady` frames per second) can be told from idle ones."""
import re, sys, os

d = sys.argv[1]
lines = open(os.path.join(d, 'serial.log'), 'rb').read().decode('latin1').splitlines()
ff = next((i for i, l in enumerate(lines) if 'first desktop frame' in l), None)
if ff is None:
    sys.exit('no desktop frame')
blk = 0
for l in lines[ff + 1:]:
    if l.startswith('[trace] browser'):
        print(l[8:])
    elif l.startswith('[trace] t='):
        blk += 1
        print(f'-- second {blk}')
    else:
        m = re.match(r'\[trace\]   path (\w+)\s+n=(\d+)\s+avg=(\d+)us max=(\d+)us cpu=(\d+)us', l)
        if m and m.group(1) not in ('clock', 'clockl', 'cursor'):
            print(f'   {m.group(1):7s} n={m.group(2):>3} avg={m.group(3):>6}us max={m.group(4):>6}us cpu={m.group(5):>6}us')
        m = re.search(r'cpu samples idle=(\d+) busy=(\d+) \(comp=(\d+)', l)
        if m:
            i, b, c = map(int, m.groups())
            print(f'   cpu busy {100*b/max(1,i+b):.1f}% compositor {100*c/max(1,i+b):.1f}%')
