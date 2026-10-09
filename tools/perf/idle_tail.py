#!/usr/bin/env python3
"""idle_tail.py <rundir> [seconds]: frames drawn per second (all paths) in the last `seconds`
(default 8) per-second `[trace]` blocks of a tools/perf/run.sh run. An idle desktop prints zero."""
import re, sys, os

def main():
    d = sys.argv[1]
    last = int(sys.argv[2]) if len(sys.argv) > 2 else 8
    txt = open(os.path.join(d, 'serial.log'), 'rb').read().decode('latin1')
    blocks, cur = [], None
    for l in txt.splitlines():
        if l.startswith('[trace] t='):
            cur = {'paths': {}}
            blocks.append(cur)
            continue
        m = re.match(r'\[trace\]   path (\w+)\s+n=(\d+)\s+avg=(\d+)us', l)
        if m and cur is not None:
            cur['paths'][m.group(1)] = int(m.group(2))
    for b in blocks[-last:]:
        frames = sum(b['paths'].values())
        print(frames, b['paths'])

main()
