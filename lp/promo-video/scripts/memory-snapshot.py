"""Sums macOS physical memory (top's MEM, as in Activity Monitor) per app bundle.

    python3 scripts/memory-snapshot.py [samples] [interval-seconds]
Every process whose executable lives in an app bundle counts toward that app,
including helpers, renderers and the Bex Host. Agent CLIs (codex, claude, node)
are excluded from every app because they are the agents, not the client.
Prints per-sample totals and the median per app as JSON.
"""

import json
import re
import statistics
import subprocess
import sys
import time

APPS = {
    'Bex': '/Applications/Bex.app/',
    'Claude': '/Applications/Claude.app/',
    'ChatGPT (Codex)': '/Applications/ChatGPT.app/',
    'T3 Code': '/Applications/T3 Code (Nightly).app/',
}
AGENT = re.compile(r'/(codex|claude|node)( |$)')
UNITS = {'B': 1 / 1024 / 1024, 'K': 1 / 1024, 'M': 1, 'G': 1024}


def owners():
    listing = subprocess.run(['/bin/ps', '-axo', 'pid=,command='], capture_output=True, text=True).stdout
    owned = {}
    for line in listing.splitlines():
        pid, _, command = line.strip().partition(' ')
        executable = command.split(' --', 1)[0]
        if AGENT.search(executable):
            continue
        for app, prefix in APPS.items():
            if executable.startswith(prefix):
                owned[int(pid)] = (app, executable[len(prefix):])
    return owned


def memory():
    output = subprocess.run(['/usr/bin/top', '-l', '1', '-stats', 'pid,mem'], capture_output=True, text=True).stdout
    sizes = {}
    for line in output.splitlines():
        match = re.match(r'\s*(\d+)\s+([\d.]+)([BKMG])', line)
        if match:
            sizes[int(match[1])] = float(match[2]) * UNITS[match[3]]
    return sizes


samples = int(sys.argv[1]) if len(sys.argv) > 1 else 5
interval = float(sys.argv[2]) if len(sys.argv) > 2 else 5
totals = {app: [] for app in APPS}
for sample in range(samples):
    owned, sizes = owners(), memory()
    for app in APPS:
        parts = [(name, sizes.get(pid, 0)) for pid, (owner, name) in owned.items() if owner == app]
        totals[app].append(round(sum(size for _, size in parts), 1))
        if sample == 0:
            for name, size in sorted(parts, key=lambda part: -part[1]):
                print(f'  {app}: {size:8.1f} MB  {name[:90]}', file=sys.stderr)
    time.sleep(interval if sample + 1 < samples else 0)
print(json.dumps({
    'measured_at': time.strftime('%Y-%m-%dT%H:%M:%S%z'),
    'metric': 'top MEM (physical footprint), MB',
    'samples': totals,
    'median': {app: statistics.median(values) for app, values in totals.items() if any(values)},
}, ensure_ascii=False, indent=2))
