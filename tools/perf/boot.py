#!/usr/bin/env python3
"""boot.py <rundir>... : median boot milestones over several runs.

Reads the `[trace] boot ...` lines the kernel prints on COM1 (always compiled in)
and prints, per milestone, the median/min/max of "ms since kernel entry" and of
the step from the previous milestone, plus the firmware+bootloader time before
the kernel started (TSC counts from VM reset)."""
import re
import statistics as st
import sys
from collections import OrderedDict

pre, since, step = [], OrderedDict(), OrderedDict()
for d in sys.argv[1:]:
    try:
        txt = open(d.rstrip('/') + '/serial.log', 'rb').read().decode('latin1')
    except OSError:
        continue
    for l in txt.splitlines():
        m = re.match(r'\[trace\] boot firmware\+bootloader before kernel entry.*: (\d+)\.(\d+) ms', l)
        if m:
            pre.append(int(m.group(1)) + int(m.group(2)) / 1000)
        m = re.match(r'\[trace\] boot \+\s*(\d+)\.(\d+) ms \(step\s+(\d+)\.(\d+) ms\)\s+(.*)$', l)
        if m:
            name = m.group(5)
            since.setdefault(name, []).append(int(m.group(1)) + int(m.group(2)) / 1000)
            step.setdefault(name, []).append(int(m.group(3)) + int(m.group(4)) / 1000)

def f(v):
    return f"{st.median(v):9.1f} (min {min(v):.1f}, max {max(v):.1f}, n={len(v)})"

if pre:
    print(f"firmware+bootloader before kernel entry [ms]: {f(pre)}")
for k in since:
    print(f"{k:<46} since entry {f(since[k])} | step {st.median(step[k]):8.1f}")
