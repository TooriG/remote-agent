import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { setTimeout } from 'node:timers/promises';

function kache(args) {
  const result = spawnSync('kache', args, {stdio: 'inherit'});
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`kache ${args.join(' ')} failed (${result.status})`);
}

async function waitForExit(pid) {
  const deadline = Date.now() + 10_000;
  for (;;) {
    try {
      process.kill(pid, 0);
    } catch (error) {
      if (error.code === 'ESRCH') return;
      throw error;
    }
    if (Date.now() >= deadline) throw new Error(`Compiler cache daemon ${pid} is still running`);
    await setTimeout(100);
  }
}

if (!process.env.KACHE_RUNTIME_DIR) throw new Error('KACHE_RUNTIME_DIR is required');
kache(['daemon', 'start']);
let pid;
try {
  const state = JSON.parse(readFileSync(join(process.env.KACHE_RUNTIME_DIR, 'daemon.state.json'), 'utf8'));
  if (!Number.isSafeInteger(state.pid) || state.pid <= 1) throw new Error('Invalid compiler cache daemon PID');
  pid = state.pid;
  kache(['stats', '--last-build']);
  kache(['gc']);
} finally {
  kache(['daemon', 'stop']);
  // Ownership release can precede process exit and final SQLite file cleanup.
  if (pid !== undefined) await waitForExit(pid);
}
