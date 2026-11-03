import test from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'native-validation-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const bin = join(root, 'bin');
  const log = join(root, 'commands.log');
  mkdirSync(bin);
  writeFileSync(log, '');
  const command = join(bin, 'command');
  writeFileSync(command, `#!/bin/bash
name=\${0##*/}
if [[ $name == uname ]]; then
  if [[ $1 == -s ]]; then echo Darwin; else echo arm64; fi
  exit 0
fi
printf '%s %s\\n' "$name" "$*" >> "$NATIVE_VALIDATION_LOG"
if [[ -n \${NATIVE_VALIDATION_FAIL:-} && "$name $*" == "$NATIVE_VALIDATION_FAIL"* ]]; then exit 17; fi
`);
  chmodSync(command, 0o755);
  for (const name of ['swiftformat', 'swiftlint', 'actionlint', 'nix', 'cargo', 'just', 'uname']) {
    symlinkSync(command, join(bin, name));
  }
  const target = join(root, 'target');
  mkdirSync(join(target, 'debug'), { recursive: true });
  symlinkSync(command, join(target, 'debug', 'xtask'));
  return {
    run(script, args, failure = '') {
      const result = spawnSync('/bin/bash', [script, ...args], {
        encoding: 'utf8',
        env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, CARGO_TARGET_DIR: target, NATIVE_VALIDATION_LOG: log, NATIVE_VALIDATION_FAIL: failure },
      });
      assert.ifError(result.error);
      return { ...result, commands: readFileSync(log, 'utf8').trim().split('\n').filter(Boolean) };
    },
  };
}

test('Swift formatting and lint failures stop before Markdown builds and UI acceptance', async (t) => {
  for (const failure of ['swiftformat', 'swiftlint']) {
    await t.test(failure, (t) => {
      const result = fixture(t).run('scripts/quality.sh', ['swift', '--build-only'], failure);
      assert.equal(result.status, 17, result.stderr);
      assert.equal(result.commands.length, failure === 'swiftformat' ? 1 : 2);
      assert.ok(result.commands.every(command => command.startsWith('swift')));
    });
  }
});

test('Rust formatting fails before compiling and contract failures stop native acceptance', async (t) => {
  for (const [failure, lastCommand] of [
    ['cargo fmt', 'cargo fmt --all --check'],
    ['cargo clippy', 'cargo clippy --locked --workspace --all-targets -- --no-deps -D warnings'],
    ['just unit-tests', null],
  ]) {
    await t.test(failure, (t) => {
      const result = fixture(t).run('scripts/quality.sh', ['rust'], failure);
      assert.equal(result.status, 17, result.stderr);
      assert.ok(!result.commands.includes('just macos-e2e'));
      if (lastCommand) assert.equal(result.commands.at(-1), lastCommand);
      if (failure === 'cargo fmt') assert.ok(!result.commands.some(command => command.startsWith('nix ')));
      if (failure === 'just unit-tests') assert.ok(result.commands.at(-1).startsWith('just unit-tests --test errors'));
    });
  }
});

test('Swift build checks run once and forward the selected build mode', (t) => {
  const result = fixture(t).run('scripts/quality.sh', ['swift', '--build-only']);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.commands.map(command => command.split(' ').slice(0, 2).join(' ')), [
    'swiftformat --lint', 'swiftlint lint', 'just ios-markdown', 'just conversation-ui', 'just clean-builds',
  ]);
  assert.equal(result.commands[3], 'just conversation-ui --build-only');
});

test('prebuilt acceptance executes the shipped runner without invoking Cargo', (t) => {
  const result = fixture(t).run('scripts/ios-e2e.sh', ['--prebuilt', '--without-codex', 'testSimulatorExample']);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.commands, ['xtask ios-e2e --prebuilt --without-codex testSimulatorExample']);
});

test('build-only mode forwards the requested tests without starting acceptance', (t) => {
  const result = fixture(t).run('scripts/ios-e2e.sh', ['--build-only', 'testSimulatorExample']);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.commands, ['cargo xtask ios-e2e --build-only testSimulatorExample']);
});
