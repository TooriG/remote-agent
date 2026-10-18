"""Windows desktop baseline: isolated profiles; no prompts or UI input.

Requires Python 3.10+ and psutil. Only processes started by this harness are stopped.
Window timing is OS visibility detection, not rendered-content or task readiness.
"""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time

import psutil


def first_window(process_ids):
    """Read-only instrumentation: find a visible >=320x200 top-level window."""
    user = ctypes.windll.user32
    found = []
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    @callback_type
    def visit(hwnd, _):
        process_id = wintypes.DWORD()
        user.GetWindowThreadProcessId(hwnd, ctypes.byref(process_id))
        if process_id.value in process_ids and user.IsWindowVisible(hwnd):
            rect = wintypes.RECT()
            user.GetWindowRect(hwnd, ctypes.byref(rect))
            if rect.right - rect.left >= 320 and rect.bottom - rect.top >= 200:
                found.append(int(hwnd))
        return True

    user.EnumWindows(visit, 0)
    return found


def descendants(root, known):
    try:
        for process in root.children(recursive=True):
            known[process.pid] = process
    except psutil.Error:
        pass
    return [p for p in known.values() if p.is_running()]


def measure(app, profile, index):
    profile.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ)
    environment.update({
        'BEX_STATE_DIR': str(profile / 'bex'),
        'BEX_ISOLATED_HOST': '1',
        'BEX_ACCOUNT_STATE_DIR': str(profile / 'accounts'),
        'CODEX_ELECTRON_USER_DATA_PATH': str(profile / 'codex-ui'),
        'CODEX_HOME': str(profile / 'codex-home'),
        'CLAUDE_USER_DATA_DIR': str(profile / 'claude-ui'),
        'CLAUDE_CONFIG_DIR': str(profile / 'claude-config'),
    })
    command = [app['executable']]
    if app['name'] == 'Codex':
        command.append('--user-data-dir=' + str(profile / 'codex-ui'))
    elif app['name'] == 'Claude':
        command.append('--user-data-dir=' + str(profile / 'claude-ui'))
    start = time.perf_counter()
    child = subprocess.Popen(command, env=environment,
                             cwd=str(profile), stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)
    root = psutil.Process(child.pid)
    known = {root.pid: root}
    result = {'app': app['name'], 'run': index, 'window_ms': None, 'samples': []}
    try:
        while time.perf_counter() - start < 15:
            processes = descendants(root, known)
            if first_window({p.pid for p in processes}):
                result['window_ms'] = round((time.perf_counter() - start) * 1000, 2)
                break
            if child.poll() is not None:
                raise RuntimeError(f"{app['name']} exited before its own window: {child.returncode}")
            time.sleep(0.02)
        if result['window_ms'] is None:
            raise RuntimeError(f"{app['name']}: no visible window within 15 seconds")
        for target_second in range(20, 31):
            time.sleep(max(0, target_second - (time.perf_counter() - start)))
            processes = descendants(root, known)
            rows = []
            for process in processes:
                try:
                    memory = process.memory_full_info()
                    rows.append({'name': process.name(), 'pid': process.pid,
                                 'uss_bytes': memory.uss, 'rss_bytes': memory.rss,
                                 'private_commit_bytes': memory.private})
                except psutil.NoSuchProcess:
                    pass
            if not rows:
                raise RuntimeError('No live processes in sample')
            result['samples'].append({
                'elapsed_seconds': round(time.perf_counter() - start, 3),
                'uss_bytes': sum(p['uss_bytes'] for p in rows),
                'rss_bytes': sum(p['rss_bytes'] for p in rows),
                'private_commit_bytes': sum(p['private_commit_bytes'] for p in rows),
                'processes': rows,
            })
        result['median_uss_mib'] = round(statistics.median(s['uss_bytes'] for s in result['samples']) / 2**20, 3)
        result['median_rss_mib'] = round(statistics.median(s['rss_bytes'] for s in result['samples']) / 2**20, 3)
        result['median_private_commit_mib'] = round(statistics.median(s['private_commit_bytes'] for s in result['samples']) / 2**20, 3)
        result['visible_at_end'] = bool(first_window({p.pid for p in descendants(root, known)}))
        if not result['visible_at_end']:
            raise RuntimeError('Window disappeared during measurement')
        return result
    finally:
        processes = descendants(root, known)
        for process in reversed(processes):
            try:
                process.terminate()
            except psutil.NoSuchProcess:
                pass
        psutil.wait_procs(processes, timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--runs', type=int, default=5)
    args = parser.parse_args()
    args.output = args.output.resolve()
    config = json.loads(args.config.read_text(encoding='utf-8-sig'))
    args.output.mkdir(parents=True, exist_ok=True)
    metadata = {'date': time.strftime('%Y-%m-%d'), 'os': platform.platform(),
                'python': platform.python_version(), 'psutil': psutil.__version__,
                'physical_ram_bytes': psutil.virtual_memory().total,
                'logical_cpus': psutil.cpu_count(), 'applications': []}
    for app in config:
        metadata['applications'].append({
            'name': app['name'], 'version': app['version'],
            'executable_sha256': hashlib.sha256(Path(app['executable']).read_bytes()).hexdigest(),
        })
    (args.output / 'metadata.json').write_text(json.dumps(metadata, indent=2), encoding='utf8')
    # First launch per profile primes application caches. Exclude it from results.
    for app in config:
        result = measure(app, args.output / 'profiles' / app['name'], 0)
        (args.output / f"{app['name']}-warmup.json").write_text(json.dumps(result, indent=2), encoding='utf8')
        print(json.dumps({'warmup': app['name'], 'window_ms': result['window_ms'], 'uss_mib': result['median_uss_mib']}), flush=True)
    for index in range(1, args.runs + 1):
        # Rotate order to reduce systematic ordering bias; retain warm disk caches.
        order = config[(index-1) % len(config):] + config[:(index-1) % len(config)]
        for app in order:
            result = measure(app, args.output / 'profiles' / app['name'], index)
            (args.output / f"{app['name']}-{index}.json").write_text(json.dumps(result, indent=2), encoding='utf8')
            print(json.dumps({'app': app['name'], 'run': index, 'window_ms': result['window_ms'], 'uss_mib': result['median_uss_mib']}), flush=True)


if __name__ == '__main__':
    main()
