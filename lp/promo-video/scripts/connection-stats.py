"""iPhone connection timings that clients report to a Host (client.connection).

    python3 scripts/connection-stats.py data/ios-connections.jsonl
    python3 scripts/connection-stats.py --refresh ~/Library/Application\\ Support/app.bex.BEX-Dev/logs data/ios-connections.jsonl
Host logs rotate every 5 MiB, so the video's figures come from a saved snapshot.
total_ms ends when the encrypted connection is authenticated (see agent-transport diagnostics).
"""

import glob
import json
import os
import re
import statistics
import sys


def refresh(logs, snapshot):
    rows = []
    for path in glob.glob(os.path.join(logs, 'host.jsonl*')):
        with open(path, errors='replace') as log:
            for line in log:
                if '"operation":"client.connection"' in line and 'platform=Ios' in line:
                    record = json.loads(line)
                    rows.append({key: record[key] for key in ('timestampMs', 'revision', 'message')})
    rows.sort(key=lambda row: row['timestampMs'])
    with open(snapshot, 'w') as out:
        out.writelines(json.dumps(row) + '\n' for row in rows)


if sys.argv[1] == '--refresh':
    refresh(sys.argv[2], sys.argv[3])
snapshot = sys.argv[-1]
connections = [dict(re.findall(r'(\w+)=(\S+)', json.loads(line)['message'])) for line in open(snapshot)]
direct = [c for c in connections if c['route'] == 'Direct']
fresh = [int(c['total_ms']) for c in direct if c['reused'] == 'false']
print(f'iPhone connections: {len(connections)}, direct: {len(direct)} ({round(100 * len(direct) / len(connections))}%)')
print(f'new direct connections: {len(fresh)}, median {statistics.median(fresh)} ms, max {max(fresh)} ms')
