"""Validate the full Windows baseline and explicitly separated Codex references."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import statistics


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--cpu', help='Verified CPU model of the measurement machine')
    parser.add_argument('--gpu', help='Verified GPU model of the measurement machine')
    parser.add_argument('--timezone', help='Timezone used for the measurement date')
    parser.add_argument('--os-name', help='Verified OS product name (platform.platform may call Windows 11 Windows 10)')
    args = parser.parse_args()
    metadata = json.loads((args.input / 'metadata.json').read_text())
    for key, value in [('cpu', args.cpu), ('gpu', args.gpu), ('measurement_timezone', args.timezone), ('os_marketing_name', args.os_name)]:
        if value:
            metadata[key] = value
    metadata['measurement_script_sha256'] = hashlib.sha256(Path(__file__).with_name('benchmark-desktop.py').read_bytes()).hexdigest()
    metadata['aggregation_script_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    summary = []
    runs = []
    warmups = []
    for app in metadata['applications']:
        name = app['name']
        count = 5
        actual = {p.name for p in args.input.glob(f'{name}-*.json') if re.fullmatch(re.escape(name) + r'-\d+\.json', p.name)}
        assert actual == {f'{name}-{n}.json' for n in range(1, count + 1)}, 'Missing or unaccounted measured runs'
        results = [json.loads((args.input / f'{name}-{n}.json').read_text()) for n in range(1, count + 1)]
        for n, result in enumerate(results, 1):
            assert result['app'] == name and result['run'] == n
            assert result['visible_at_end'] and 0 < result['window_ms'] < 15000
            assert len(result['samples']) == 11
            for index, sample in enumerate(result['samples']):
                assert 20 + index <= sample['elapsed_seconds'] < 21 + index
                for metric in ('uss_bytes', 'rss_bytes', 'private_commit_bytes'):
                    assert sample[metric] == sum(p[metric] for p in sample['processes'])
                    assert sample[metric] > 0
                if name == 'BEX':
                    assert any(p['name'] == 'host-daemon.exe' for p in sample['processes'])
                assert len({p['pid'] for p in sample['processes']}) == len(sample['processes'])
            for metric, source in [('uss', 'uss_bytes'), ('rss', 'rss_bytes'), ('private_commit', 'private_commit_bytes')]:
                computed = round(statistics.median(s[source] for s in result['samples']) / 2**20, 3)
                assert computed == result[f'median_{metric}_mib']
        row = {'app': name, 'runs': count, 'authentication': 'signed in (Windows auto-restore); reference only' if name == 'Codex' else 'signed out'}
        for field in ['window_ms', 'median_uss_mib', 'median_rss_mib', 'median_private_commit_mib']:
            values = [r[field] for r in results]
            row[field] = {'median': statistics.median(values), 'min': min(values), 'max': max(values)}
        summary.append(row)
        runs.extend(results)
        warmups.append(json.loads((args.input / f'{name}-warmup.json').read_text()))
    document = {
        'schema_version': 1, 'metadata': metadata,
        'method': {
            'state': 'Isolated profiles; no project, conversation or generation. BEX and Claude signed out; Codex auto-restored existing Windows login (verified by login status). Codex is a reference, not an auth-matched comparison.',
            'cache': 'One excluded warm-up per app; five valid launches per app; warm OS caches; rotating order (last BEX/Claude completed after authentication audit)',
            'window': 'perf_counter before Popen to first visible top-level window >=320x200; 20ms sleep between checks; includes process enumeration overhead; not usable-content readiness',
            'memory': 'Process tree including BEX Host and all discovered descendants; psutil memory_full_info; 11 samples at 20..30 seconds; median per run, then median across five valid runs',
            'uss': 'Unique/private resident RAM in MiB (2^20 bytes), excluding shared pages and GPU memory',
            'rss': 'Sum of process resident working sets; shared pages may be counted multiple times',
            'private_commit': 'Private committed virtual memory; may be paged out, not physical RAM',
            'shutdown': 'Only harness-launched processes are terminated after each run; profiles reused',
            'interruption': 'Initial sequence completed four runs per app and the fifth Codex run before authentication audit. Claude and BEX final runs completed afterward with the same profiles. Codex original fifth result was retained, not replaced by the higher-memory duplicate diagnostic. Failed diagnostics and a BEX sample missing its preexisting Host were excluded; BEX final valid run started a new Host.',
            'limits': 'Single PC; unrelated apps and tasks remained running; no reboot/cache flush; not an authenticated coding workload, mobile test, network reliability test, or causal test of Rust versus other languages',
        },
        'summary': summary, 'runs': runs, 'excluded_warmups': warmups,
        'rejected_attempts': [
            {'app': 'Codex', 'experiment': 'Diagnostic retry after auth audit', 'reason': 'Window disappeared during measurement; no valid result file. Original planned fifth run remained valid and was retained.'},
            {'app': 'Codex', 'experiment': 'Duplicate diagnostic launch', 'reason': 'Not part of planned five; original fifth result was retained before duplicate completed'},
            {'app': 'BEX', 'experiment': 'Resume attempt', 'reason': 'No live processes at sampling; no valid result file'},
            {'app': 'BEX', 'experiment': 'Resume with preexisting test Host', 'reason': 'Host was outside descendant tree; failed Host-inclusion requirement; retested with a freshly started Host'},
            {'app': 'Codex', 'experiment': 'Separate file-only authentication pilot', 'reason': 'Still signed in; access denied reading a descendant; pilot rejected'},
            {'app': 'Codex', 'experiment': 'Initial launch setup pilot', 'reason': 'Process exited before its own window; user-data-dir command-line option was subsequently added'},
        ],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(document, indent=2) + '\n', encoding='utf8')
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
