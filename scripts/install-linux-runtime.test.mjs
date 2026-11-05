import test from 'node:test';
import assert from 'node:assert/strict';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

test('Linux installation can retry relocation failure and leaves existing Host processes alone', { skip: process.platform !== 'linux' }, t => {
  const scope = mkdtempSync(join(tmpdir(), 'linux-install-'));
  t.after(() => rmSync(scope, { recursive: true, force: true }));
  const source = join(scope, 'source');
  const root = join(scope, 'installed');
  const bin = join(scope, 'bin');
  mkdirSync(join(source, 'lib'), { recursive: true });
  copyFileSync(new URL('./install-linux-runtime.sh', import.meta.url), join(source, 'install.sh'));
  writeFileSync(join(source, 'revision'), 'a'.repeat(40));
  for (const name of ['host-daemon', 'agent-cli', 'agent-provider-supervisor', 'patchelf']) writeFileSync(join(source, name), name);
  const log = join(scope, 'relocations');
  writeFileSync(join(source, 'lib/ld-linux-x86-64.so.2'), `#!${process.execPath}
const fs = require('node:fs');
if (process.env.INSTALL_FAIL === '1') process.exit(41);
fs.appendFileSync(process.env.INSTALL_LOG, process.argv.at(-1) + '\\n');
`, { mode: 0o755 });
  const run = fail => spawnSync('bash', [join(source, 'install.sh'), root, bin], {
    encoding: 'utf8', env: { ...process.env, INSTALL_FAIL: fail ? '1' : '0', INSTALL_LOG: log },
  });
  assert.equal(run(true).status, 41);
  assert.equal(existsSync(root), false);
  assert.equal(existsSync(join(bin, 'host-daemon')), false);
  assert.equal(run(false).status, 0);
  assert.equal(readlinkSync(join(bin, 'host-daemon')), join(root, 'host-daemon'));
  assert.equal(readlinkSync(join(bin, 'agent-cli')), join(root, 'agent-cli'));
  assert.equal(readFileSync(log, 'utf8').trim().split('\n').length, 3);
  assert.equal(run(false).status, 1, 'an existing installation is never overwritten');
});
