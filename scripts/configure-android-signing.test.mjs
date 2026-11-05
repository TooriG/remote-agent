import test from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

test('Android distribution keeps private signing material out of logs and exported environment values', t => {
  const root = mkdtempSync(join(tmpdir(), 'android-signing-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'scripts'));
  const bin = join(root, 'bin');
  mkdirSync(bin);
  copyFileSync(new URL('./configure-android-signing.sh', import.meta.url), join(root, 'scripts/configure-android-signing.sh'));
  writeFileSync(join(bin, 'git'), '#!/bin/sh\nif [ "$2" = --is-shallow-repository ]; then echo false; else echo 1234; fi\n');
  chmodSync(join(bin, 'git'), 0o755);
  const payload = Buffer.from('fixture keystore, not a real credential').toString('base64');
  const output = join(root, 'env');
  const run = key => spawnSync('bash', [join(root, 'scripts/configure-android-signing.sh')], {
    encoding: 'utf8', env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, RUNNER_TEMP: root,
      GITHUB_ENV: output, AGENT_ANDROID_KEYSTORE_BASE64: key, AGENT_ANDROID_KEY_ALIAS: 'bex' },
  });
  assert.notEqual(run('').status, 0);
  assert.equal(existsSync(output), false);
  const result = run(payload);
  assert.equal(result.status, 0, result.stderr);
  assert.doesNotMatch(result.stdout + result.stderr, new RegExp(payload));
  const exported = readFileSync(output, 'utf8');
  assert.ok(exported.includes('AGENT_ANDROID_VERSION_CODE=1234\n'));
  assert.ok(exported.includes('AGENT_ANDROID_KEY_ALIAS=bex\n'));
  assert.ok(!exported.includes(payload));
  assert.equal(statSync(join(root, 'bex-android.p12')).mode & 0o777, 0o600);
});
