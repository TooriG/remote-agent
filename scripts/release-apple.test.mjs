import test from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';

async function waitFor(check) {
  const deadline = Date.now() + 10000;
  while (!check()) {
    assert.ok(Date.now() < deadline, 'release did not reach the expected boundary');
    await delay(10);
  }
}

function fixture(t, mode) {
  const root = mkdtempSync(join(tmpdir(), 'apple-pipeline-'));
  const bin = join(root, 'bin');
  mkdirSync(bin);
  mkdirSync(join(root, 'scripts'));
  // Replace only fixed macOS executable paths so the real pipeline also runs on Linux.
  const script = readFileSync(new URL('./release-apple.sh', import.meta.url), 'utf8')
    .replaceAll('/usr/libexec/PlistBuddy', 'PlistBuddy')
    .replaceAll('/usr/bin/getconf', 'getconf');
  writeFileSync(join(root, 'scripts/release-apple.sh'), script);
  writeFileSync(join(bin, 'command'), `#!${process.execPath}
const fs = require('node:fs');
const path = require('node:path');
const root = process.env.APPLE_PIPELINE_ROOT;
const mode = process.env.APPLE_PIPELINE_MODE;
const name = path.basename(process.argv[1]);
const args = process.argv.slice(2);
const log = (name, args = []) => fs.appendFileSync(path.join(root, 'commands'), JSON.stringify({ name, args }) + '\\n');
const mark = name => { fs.writeFileSync(path.join(root, name), ''); log(name); };
const write = file => { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, 'fixture'); };
const wait = async name => { while (!fs.existsSync(path.join(root, name))) await new Promise(resolve => setTimeout(resolve, 10)); };
log(name, args);
(async () => {
  if (name === 'git' && args[0] === 'rev-parse') console.log('fixture-source');
  if (name === 'just' && args[0] === 'build-desktop-macos') fs.mkdirSync(path.join(root, 'target/release/bex.app'), { recursive: true });
  if (name === 'ditto') write(args.at(-1));
  if (name === 'xcrun' && args[0] === 'notarytool') {
    fs.writeFileSync(path.join(root, 'notary.pid'), String(process.pid));
    mark('notary.started');
    await wait('notary.release');
    console.log(JSON.stringify({ status: mode === 'rejected' ? 'Invalid' : 'Accepted' }));
    mark('notary.completed');
    if (mode === 'notary-failure') process.exit(23);
  }
  if (name === 'just' && args[0] === 'ios-archive') {
    await wait('notary.started');
    mark('archive.started');
    if (mode === 'archive-failure') process.exit(41);
    if (mode === 'cancel' || mode === 'interrupt') await wait('archive.release');
    mark('archive.completed');
  }
  if (name === 'PlistBuddy') console.log(args[1].includes('CFBundleVersion') ? '101' : 'dev.remoteagent.mobile.ios');
  if (name === 'jq') {
    if (args.includes('-e')) process.exit(JSON.parse(fs.readFileSync(args.at(-1), 'utf8')).status === 'Accepted' ? 0 : 1);
    console.log(args.includes('-er') ? 'fixture-profile' : '{}');
  }
  if (name === 'plutil') {
    process.stdin.resume();
    await new Promise(resolve => process.stdin.on('end', resolve));
    write(args[args.indexOf('-o') + 1]);
  }
  if (name === 'shasum') console.log('fixture-checksum');
  if (name === 'getconf') console.log(root);
})().catch(error => { console.error(error); process.exit(70); });
`);
  chmodSync(join(bin, 'command'), 0o755);
  for (const name of ['git', 'just', 'ditto', 'xcrun', 'codesign', 'spctl', 'node', 'jq', 'plutil', 'shasum', 'PlistBuddy', 'getconf', 'xcodebuild']) {
    symlinkSync('command', join(bin, name));
  }
  writeFileSync(join(root, 'commands'), '');
  const child = spawn('bash', ['scripts/release-apple.sh'], {
    cwd: root, detached: true,
    env: {
      ...process.env, PATH: `${bin}:${process.env.PATH}`,
      APPLE_PIPELINE_ROOT: root, APPLE_PIPELINE_MODE: mode,
      AGENT_BUILD_NUMBER: '101', AGENT_RELEASE_SHA: 'fixture-source',
      AGENT_CODE_SIGN_KEYCHAIN: 'fixture-keychain', AGENT_ASC_PRIVATE_KEY_FILE: 'fixture-key',
      AGENT_ASC_KEY_ID: 'fixture-key-id', AGENT_ASC_ISSUER_ID: 'fixture-issuer',
    },
  });
  let stderr = '';
  child.stdout.resume();
  child.stderr.setEncoding('utf8').on('data', chunk => { stderr += chunk; });
  const completed = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => resolve({ code, signal, stderr }));
  });
  t.after(async () => {
    try { process.kill(-child.pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
    await completed;
    rmSync(root, { recursive: true, force: true });
  });
  return {
    completed,
    exists: name => existsSync(join(root, name)),
    release: () => writeFileSync(join(root, 'notary.release'), ''),
    commands: () => readFileSync(join(root, 'commands'), 'utf8').trim().split('\n').map(line => JSON.parse(line)),
    cancel: signal => process.kill(-child.pid, signal),
    assertNotaryExited() {
      const pid = Number(readFileSync(join(root, 'notary.pid'), 'utf8'));
      assert.throws(() => process.kill(pid, 0), { code: 'ESRCH' });
    },
  };
}

test('iPhone archiving overlaps Mac notarization and publication waits for both', { timeout: 20000 }, async t => {
  const build = fixture(t, 'success');
  await waitFor(() => build.exists('archive.completed'));
  assert.ok(!build.exists('notary.completed'));
  assert.ok(!build.commands().some(command => command.name === 'xcodebuild'));
  build.release();
  const result = await build.completed;
  assert.equal(result.code, 0, result.stderr);
  const commands = build.commands();
  const archive = commands.find(command => command.name === 'just' && command.args[0] === 'ios-archive');
  assert.ok(archive.args.includes('AGENT_IOS_PROFILE_SPECIFIER=fixture-profile $(PRODUCT_BUNDLE_IDENTIFIER)'));
  const exports = commands.filter(command => command.name === 'xcodebuild');
  assert.deepEqual(exports.map(command => command.args[command.args.indexOf('-exportPath') + 1].split('/').at(-1)), ['export', 'upload']);
  for (const name of ['archive.completed', 'notary.completed', 'spctl']) {
    assert.ok(commands.findIndex(command => command.name === name) < commands.findIndex(command => command.name === 'xcodebuild'));
  }
  build.assertNotaryExited();
});

test('notarization failure and rejection prevent every iPhone export and upload', { timeout: 20000 }, async t => {
  for (const [mode, code] of [['notary-failure', 23], ['rejected', 1]]) {
    await t.test(mode, async t => {
      const build = fixture(t, mode);
      await waitFor(() => build.exists('archive.completed'));
      build.release();
      const result = await build.completed;
      assert.equal(result.code, code, result.stderr);
      assert.ok(!build.commands().some(command => command.name === 'xcodebuild' || command.args[0] === 'stapler'));
      build.assertNotaryExited();
    });
  }
});

test('archive failure and cancellation stop and reap the pending notary process', { timeout: 20000 }, async t => {
  for (const [mode, code, signal] of [['archive-failure', 41], ['cancel', 143, 'SIGTERM'], ['interrupt', 130, 'SIGINT']]) {
    await t.test(mode, async t => {
      const build = fixture(t, mode);
      await waitFor(() => build.exists('archive.started'));
      if (signal) build.cancel(signal);
      const result = await build.completed;
      assert.equal(result.code, code, result.stderr);
      assert.ok(!build.exists('notary.completed'));
      assert.ok(!build.commands().some(command => command.name === 'xcodebuild'));
      build.assertNotaryExited();
    });
  }
});
