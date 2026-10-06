#!/usr/bin/env python3
"""agg.py <label> : aggregate A/B wall-clock runs (runs/<label>/<A|B>_<mode>_<scen>_<n>).
Per (mode, scenario, build, path): pooled per-second block averages over the repeated
runs -> median / min / max (us), plus total frames. Also input latency + CPU busy %."""
import re, sys, glob, os, statistics as st, json
from collections import defaultdict

label = sys.argv[1]
base = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'runs', label)

def blocks(path, skip=1):
    txt = open(path, 'rb').read().decode('latin1')
    lines = txt.splitlines()
    ff = next((i for i, l in enumerate(lines) if 'first desktop frame' in l), None)
    if ff is None:
        return []
    out, cur = [], None
    for l in lines[ff + 1:]:
        if l.startswith('[trace] t='):
            cur = []
            out.append(cur)
        elif cur is not None and l.startswith('[trace]'):
            cur.append(l)
    return out[skip:]

data = defaultdict(lambda: defaultdict(list))   # key -> metric -> samples
cnt = defaultdict(int)
for d in sorted(glob.glob(base + '/*_*_*_*')):
    name = os.path.basename(d)
    m = re.match(r'([AB])_(\w+?)_(\w+)_(\d+)$', name)
    if not m:
        continue
    build, mode, scen, n = m.groups()
    key = (mode, scen, build)
    cnt[key] += 1
    idle = busy = 0
    for b in blocks(d + '/serial.log'):
        for l in b:
            mm = re.match(r'\[trace\]   path (\w+)\s+n=(\d+)\s+avg=(\d+)us max=(\d+)us(?: cpu=(\d+)us)?', l)
            if mm:
                p, nn, a, mx = mm.group(1), int(mm.group(2)), int(mm.group(3)), int(mm.group(4))
                data[key]['path:' + p].append(a)
                data[key]['n:' + p].append(nn)
                if mm.group(5) is not None:
                    data[key]['cpu:' + p].append(int(mm.group(5)))
            mm = re.match(r'\[trace\]   input n=(\d+) irq->pickup avg=(\d+)us max=(\d+)us\s+irq->frame avg=(\d+)us max=(\d+)us', l)
            if mm:
                data[key]['pick'].append(int(mm.group(2)))
                data[key]['e2e'].append(int(mm.group(4)))
                data[key]['e2emax'].append(int(mm.group(5)))
            mm = re.match(r'\[trace\]   timer isr n=(\d+) avg=(\d+)cyc max=(\d+)cyc \| cpu samples idle=(\d+) busy=(\d+)', l)
            if mm:
                data[key]['isr'].append(int(mm.group(2)))
                idle += int(mm.group(4)); busy += int(mm.group(5))
            mm = re.match(r'\[trace\]   vram upload=(\d+)B changed=(\d+)B', l)
            if mm:
                data[key]['vup'].append(int(mm.group(1)))
                data[key]['vchg'].append(int(mm.group(2)))
    data[key]['cpubusy'].append(100.0 * busy / max(1, idle + busy))

def fmt(v):
    return f"{st.median(v):>8.0f} (min {min(v)}, max {max(v)}, n={len(v)})"

for key in sorted(data):
    mode, scen, build = key
    print(f"== {mode} {scen} build={build} runs={cnt[key]}")
    for k in sorted(data[key]):
        if k.startswith('path:'):
            p = k[5:]
            tot = sum(data[key]['n:' + p])
            cpu = f"  cpu us: {fmt(data[key]['cpu:' + p])}" if data[key]['cpu:' + p] else ''
            print(f"   path {p:<7} frames={tot:<5} wall us: {fmt(data[key][k])}{cpu}")
    for k in ('pick', 'e2e', 'e2emax'):
        if data[key][k]:
            print(f"   input {k:<7} us: {fmt(data[key][k])}")
    if data[key]['isr']:
        print(f"   timer isr cyc: {fmt(data[key]['isr'])}")
    print(f"   cpu busy % (sampled): median {st.median(data[key]['cpubusy']):.2f} per run: {[round(x,2) for x in data[key]['cpubusy']]}")
