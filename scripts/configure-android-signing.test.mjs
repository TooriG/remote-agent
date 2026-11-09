import test from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

for (const [platform, filename, stepName] of [
  ['Android', 'android-download.yml', 'Require a verified draft candidate'],
  ['Linux', 'native.yml', 'Require a verified draft Nightly for Linux packaging'],
]) {
test(`${platform} candidate recovery requires an unpublished Nightly and successful checks for its exact source`, t => {
  const workflow = readFileSync(new URL(`../.github/workflows/${filename}`, import.meta.url), 'utf8');
  const block = workflow.match(new RegExp(String.raw`      - name: ${stepName}[\s\S]*?        run: \|\n([\s\S]*?)(?=      -(?: name:| uses:))`));
  assert.ok(block);
  const script = block[1].split('\n').map(line => line.replace(/^          /, '')).join('\n');
  const root = mkdtempSync(join(tmpdir(), 'android-recovery-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const revision = 'a'.repeat(40);
  writeFileSync(join(root, 'git'), `#!/bin/sh\necho ${revision}\n`);
  writeFileSync(join(root, 'gh'), `#!${process.execPath}
const args = process.argv.slice(2), state = JSON.parse(process.env.BEX_RECOVERY_STATE);
if (args[0] === 'release' && args[1] === 'view') console.log(state.draft);
else if (args[0] === 'run' && args[1] === 'list') {
 const value = name => args[args.indexOf(name) + 1];
 if (value('--commit') !== '${revision}' || value('--branch') !== 'main' || value('--event') !== 'push' || value('--status') !== 'success') process.exit(3);
 console.log(state[value('--workflow')] || '');
} else process.exit(2);
`);
  for (const name of ['git', 'gh']) chmodSync(join(root, name), 0o755);
  const ready = { draft: true, 'portable.yml': revision, 'native.yml': revision };
  for (const [tag, state, success] of [
    ['nightly-149', ready, true], ['prd-150', ready, false],
    ['main', ready, false], ['nightly-149', { ...ready, draft: false }, false],
    ['nightly-149', { ...ready, 'portable.yml': '' }, false],
    ['nightly-149', { ...ready, 'native.yml': 'b'.repeat(40) }, false],
  ]) {
    const result = spawnSync('bash', ['-euo', 'pipefail', '-c', script], {
      encoding: 'utf8', env: { ...process.env, PATH: `${root}:${process.env.PATH}`,
        GITHUB_REPOSITORY: 'fixture/repo', CANDIDATE_TAG: tag, BEX_RECOVERY_STATE: JSON.stringify(state) },
    });
    assert.ifError(result.error);
    assert.equal(result.status === 0, success, result.stderr);
  }
});

}

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

test('Windows release packaging accepts only a verified unpublished Nightly', t => {
  const workflow = readFileSync(new URL('../.github/workflows/native.yml', import.meta.url), 'utf8');
  const block = workflow.match(/      - name: Require a verified Nightly revision for release packaging[\s\S]*?        run: \|\n([\s\S]*?)(?=      # Native Windows)/);
  assert.ok(block);
  const script = block[1].split('\n').map(line => line.replace(/^          /, '')).join('\n');
  const root = mkdtempSync(join(tmpdir(), 'windows-release-guard-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const file = join(root, 'guard.ps1');
  writeFileSync(file, `
$ErrorActionPreference = 'Stop'
$mock = ConvertFrom-Json -AsHashtable $env:BEX_WINDOWS_GUARD
function git { $global:LASTEXITCODE = 0; return 'fixture-source' }
function gh {
  $global:LASTEXITCODE = 0
  if ($args[0] -eq 'release') { return [string]$mock.draft }
  if ($args[0] -eq 'run') {
    if ($args[$args.IndexOf('--commit') + 1] -ne 'fixture-source') { throw 'Wrong source' }
    return $mock[$args[$args.IndexOf('--workflow') + 1]]
  }
  throw 'Unexpected gh command'
}
${script}`);
  const ready = { draft: true, 'portable.yml': 'fixture-source', 'native.yml': 'fixture-source' };
  for (const [tag, state, success] of [
    ['nightly-149', ready, true], ['prd-150', ready, false],
    ['nightly-149', { ...ready, draft: false }, false],
    ['nightly-149', { ...ready, 'portable.yml': '' }, false],
    ['nightly-149', { ...ready, 'native.yml': 'another-source' }, false],
  ]) {
    const result = spawnSync('pwsh', ['-NoLogo', '-NoProfile', '-NonInteractive', '-File', file], {
      encoding: 'utf8', env: { ...process.env, AGENT_RELEASE_TAG: tag, GITHUB_REPOSITORY: 'fixture/repo', BEX_WINDOWS_GUARD: JSON.stringify(state) },
    });
    assert.ifError(result.error);
    assert.equal(result.status === 0, success, result.stderr);
  }
});
