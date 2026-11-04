import test from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, truncateSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(new URL('./free-runner-space.sh', import.meta.url));
const gib = 1024 * 1024;

function fixture(t, initial, largeFreed, smallFreed = 0) {
  const root = mkdtempSync(join(tmpdir(), 'runner space '));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  const bin = join(root, 'bin'), sdk = join(root, 'unused sdk');
  mkdirSync(bin); mkdirSync(sdk);
  const large = join(sdk, 'large'), small = join(sdk, 'small'), unrelated = join(root, 'keep');
  writeFileSync(large, ''); truncateSync(large, 11 * 1024 * 1024);
  writeFileSync(small, 'SDK metadata'); writeFileSync(unrelated, 'keep');
  writeFileSync(join(bin, 'df'), `#!${process.execPath}
const {existsSync}=require('node:fs');
console.log(${initial} + (existsSync(${JSON.stringify(large)}) ? 0 : ${largeFreed}) + (existsSync(${JSON.stringify(small)}) ? 0 : ${smallFreed}));
`);
  writeFileSync(join(bin, 'sudo'), '#!/usr/bin/env bash\nexec "$@"\n');
  for (const name of ['df', 'sudo']) chmodSync(join(bin, name), 0o755);
  return {
    sdk, large, small, unrelated,
    run(ci = true, args = []) {
      const result = spawnSync('bash', [script, ...args, sdk], {encoding: 'utf8', env: {
        ...process.env, PATH: `${bin}:${process.env.PATH}`, GITHUB_ACTIONS: ci ? 'true' : '', RUNNER_OS: 'Linux',
      }});
      assert.ifError(result.error);return result;
    },
  };
}

test('a runner at the required capacity keeps every preinstalled file', t => {
  const f = fixture(t, 35 * gib, 10 * gib);
  assert.equal(f.run().status, 0);
  assert.ok(existsSync(f.large)); assert.ok(existsSync(f.small));
});

test('large binaries alone can satisfy the budget without deleting SDK metadata', t => {
  const f = fixture(t, 35 * gib - 1, 1);
  assert.equal(f.run().status, 0);
  assert.ok(!existsSync(f.large)); assert.ok(existsSync(f.small));
  assert.ok(existsSync(f.unrelated));
});

test('tight runners reclaim the remaining unused SDK files only when needed', t => {
  const f = fixture(t, 20 * gib, 14 * gib, 2 * gib);
  assert.equal(f.run().status, 0);
  assert.ok(!existsSync(f.sdk)); assert.ok(existsSync(f.unrelated));
});

test('an insufficient image fails before allocating build environments', t => {
  const f = fixture(t, 10 * gib, 5 * gib, 5 * gib);
  const result = f.run();
  assert.equal(result.status, 1);assert.match(result.stderr, /35 GiB/);
  assert.ok(existsSync(f.unrelated));
});

test('SDK cleanup refuses ordinary local environments', t => {
  const f = fixture(t, 10 * gib, 20 * gib);
  assert.equal(f.run(false).status, 2);
  assert.ok(existsSync(f.large)); assert.ok(existsSync(f.small));
});

test('the emulator budget leaves SDKs intact when enough space remains after compilation', t => {
  const f = fixture(t, 10 * gib, 10 * gib);
  assert.equal(f.run(true, ['--required-gib', '10']).status, 0);
  assert.ok(existsSync(f.large)); assert.ok(existsSync(f.small));
});

test('the emulator budget reclaims only the space needed for userdata', t => {
  const f = fixture(t, 10 * gib - 1, 1);
  assert.equal(f.run(true, ['--required-gib', '10']).status, 0);
  assert.ok(!existsSync(f.large)); assert.ok(existsSync(f.small));
  assert.ok(existsSync(f.unrelated));
});

test('invalid budgets fail before deleting unused SDKs', t => {
  const f = fixture(t, 0, 40 * gib);
  for (const budget of ['', '0', '-1', '1000', '10+1', 'word']) {
    assert.equal(f.run(true, ['--required-gib', budget]).status, 2);
    assert.ok(existsSync(f.large)); assert.ok(existsSync(f.small));
  }
});
