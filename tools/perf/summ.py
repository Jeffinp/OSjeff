#!/usr/bin/env python3
"""summ.py <rundir>... : per-frame-path cost of one or more tools/perf/run.sh runs.

For each run dir prints, over the per-second `[trace]` blocks after the first
desktop frame, the frame count, the n-weighted mean and the worst frame (us) of every
path (`animrb`, `animdm`, `settle`, `steady`, `clockl`, `cursor`, `ovrb`, `ovh`...), the
per-second CPU share of the compositor and the boot-time UI lines. Works on any build
with the `perf-trace` feature."""
import re, sys, os
from collections import defaultdict

def one(d):
    txt = open(os.path.join(d, 'serial.log'), 'rb').read().decode('latin1')
    lines = txt.splitlines()
    ff = next((i for i, l in enumerate(lines) if 'first desktop frame' in l), None)
    if ff is None:
        print(f'{d}: no desktop frame'); return
    n = defaultdict(int); tot = defaultdict(int); mx = defaultdict(int)
    idle = busy = comp = 0
    for l in lines[ff + 1:]:
        m = re.match(r'\[trace\]   path (\w+)\s+n=(\d+)\s+avg=(\d+)us max=(\d+)us', l)
        if m:
            p, c, a, x = m.group(1), int(m.group(2)), int(m.group(3)), int(m.group(4))
            n[p] += c; tot[p] += c * a; mx[p] = max(mx[p], x)
        m = re.search(r'cpu samples idle=(\d+) busy=(\d+) \(comp=(\d+)', l)
        if m:
            idle += int(m.group(1)); busy += int(m.group(2)); comp += int(m.group(3))
    print(f'== {d}')
    for p in sorted(n):
        print(f'  {p:8s} n={n[p]:5d} mean={tot[p]/n[p]:9.0f}us worst={mx[p]:7d}us')
    if idle + busy:
        print(f'  cpu busy {100*busy/(idle+busy):.2f}% (compositor {100*comp/(idle+busy):.2f}%)')
    for l in lines:
        if l.startswith('[trace] ui:') or l.startswith('[trace] bench'):
            print('  ' + l[8:][:220])

for d in sys.argv[1:]:
    one(d)
