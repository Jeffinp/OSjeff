#!/usr/bin/env python3
"""parse.py <rundir>... : aggregate [trace] blocks after the first desktop frame.
Prints one JSON-ish dict per run to stdout (and a summary with --summary)."""
import re, sys, json, statistics as st

def parse(path, skip_blocks=1, drop_tail=0):
    txt = open(path, 'rb').read().decode('latin1')
    lines = txt.splitlines()
    ff = None
    for i, l in enumerate(lines):
        if 'first desktop frame' in l:
            ff = i
            break
    if ff is None:
        return None
    blocks = []
    cur = None
    for l in lines[ff + 1:]:
        if l.startswith('[trace] t='):
            cur = {'raw': []}
            blocks.append(cur)
        elif cur is not None and l.startswith('[trace]'):
            cur['raw'].append(l)
    blocks = blocks[skip_blocks:len(blocks) - drop_tail if drop_tail else None]
    agg = {'paths': {}, 'blocks': len(blocks)}
    stage = {'compose': 0, 'blit': 0, 'cursor': 0, 'hud': 0}
    idle = busy = 0
    allocn = allocb = allocc = 0
    scanned = []
    isr = []
    inp = {'n': 0, 'pick': 0, 'e2e': 0, 'maxpick': 0, 'maxe2e': 0}
    loops = []
    ata = []
    for b in blocks:
        for l in b['raw']:
            m = re.match(r'\[trace\]   path (\w+)\s+n=(\d+)\s+avg=(\d+)us max=(\d+)us(?: cpu=(\d+)us)?', l)
            if m:
                p = agg['paths'].setdefault(m.group(1), {'n': 0, 'sum': 0, 'max': 0, 'avgs': []})
                n, a, mx = int(m.group(2)), int(m.group(3)), int(m.group(4))
                p['n'] += n; p['sum'] += n * a; p['max'] = max(p['max'], mx); p['avgs'].append(a)
            m = re.match(r'\[trace\]   stage sum/s: compose=(\d+)us blit=(\d+)us cursor=(\d+)us hud=(\d+)us', l)
            if m:
                for k, v in zip(['compose', 'blit', 'cursor', 'hud'], m.groups()):
                    stage[k] += int(v)
            m = re.match(r'\[trace\]   input n=(\d+) irq->pickup avg=(\d+)us max=(\d+)us\s+irq->frame avg=(\d+)us max=(\d+)us', l)
            if m:
                n = int(m.group(1))
                inp['n'] += n; inp['pick'] += n * int(m.group(2)); inp['e2e'] += n * int(m.group(4))
                inp['maxpick'] = max(inp['maxpick'], int(m.group(3))); inp['maxe2e'] = max(inp['maxe2e'], int(m.group(5)))
            m = re.match(r'\[trace\]   alloc n=(\d+) bytes=(\d+) avg=(\d+)cyc max=(\d+)cyc scanned/alloc=(\d+)', l)
            if m:
                n = int(m.group(1)); allocn += n; allocb += int(m.group(2)); allocc += n * int(m.group(3)); scanned.append(int(m.group(5)))
            m = re.match(r'\[trace\]   timer isr n=(\d+) avg=(\d+)cyc max=(\d+)cyc \| cpu samples idle=(\d+) busy=(\d+)', l)
            if m:
                isr.append(int(m.group(2))); idle += int(m.group(4)); busy += int(m.group(5))
            m = re.match(r'\[trace\] t=(\d+)ticks loops=(\d+)', l)
        for l in [b0 for b0 in [b['raw']]]:
            pass
    nb = max(1, len(blocks))
    agg['stage_per_s_us'] = {k: v // nb for k, v in stage.items()}
    agg['cpu_busy_pct'] = round(100.0 * busy / max(1, idle + busy), 2)
    agg['alloc_per_s'] = allocn // nb
    agg['alloc_bytes_per_s'] = allocb // nb
    agg['alloc_avg_cyc'] = allocc // max(1, allocn)
    agg['isr_avg_cyc'] = int(st.median(isr)) if isr else 0
    agg['input'] = {k: v for k, v in inp.items()}
    if inp['n']:
        agg['input']['pick_avg'] = inp['pick'] // inp['n']
        agg['input']['e2e_avg'] = inp['e2e'] // inp['n']
    for k, p in agg['paths'].items():
        p['avg'] = p['sum'] // max(1, p['n'])
        p['med'] = int(st.median(p['avgs']))
        del p['avgs']
    return agg

if __name__ == '__main__':
    for d in sys.argv[1:]:
        a = parse(d.rstrip('/') + '/serial.log')
        print(d, json.dumps(a))
