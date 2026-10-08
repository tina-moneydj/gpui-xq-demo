#!/usr/bin/env python3
"""加總某 pid 及其子行程（含 WebKit 子行程）的 RSS；用來比較 wry 版與 GPUI 版。"""
import os, sys

def rss_kb(p):
    try:
        for line in open(f'/proc/{p}/status'):
            if line.startswith('VmRSS:'):
                return int(line.split()[1])
    except OSError:
        pass
    return 0

def cmd(p):
    try:
        return open(f'/proc/{p}/cmdline', 'rb').read().replace(b'\0', b' ').decode(errors='replace')[:90]
    except OSError:
        return '?'

def ppid(p):
    try:
        return int(open(f'/proc/{p}/stat').read().rsplit(')', 1)[1].split()[1])
    except (OSError, IndexError, ValueError):
        return -1

root = int(sys.argv[1])
procs = {root}
changed = True
pids = [int(e) for e in os.listdir('/proc') if e.isdigit()]
while changed:
    changed = False
    for p in pids:
        if p not in procs and ppid(p) in procs:
            procs.add(p); changed = True
total = 0
for p in sorted(procs):
    r = rss_kb(p); total += r
    print(f'{p:>8} {r/1024:8.1f} MB  {cmd(p)}')
print(f'TOTAL {total/1024:.1f} MB')
