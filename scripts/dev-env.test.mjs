import test from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

function put(root, path, content) {
  const destination = join(root, path);
  mkdirSync(join(destination, '..'), { recursive: true });
  writeFileSync(destination, content);
  return destination;
}

function fixture(t, builder = '') {
  const root = mkdtempSync(join(tmpdir(), 'dev-env-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const git = (...args) => {
    const result = spawnSync('git', args, { cwd: root, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
  };
  git('init', '--quiet');
  if (builder) git('config', 'bex.remoteBuilder', builder);
  for (const path of [
    'flake.nix', 'flake.lock', 'tools/github-release-binary.nix', 'tools/cccc/package.nix',
    'tools/kache/package.nix', 'tools/node-runtime/package.nix', 'tools/agent-peer/Cargo.lock',
  ]) put(root, path, '');
  const lock = ['archived', 'source-only', 'missing'].map(name =>
    `[[package]]\nname = "${name}"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n`).join('\n');
  put(root, 'Cargo.lock', lock);
  put(root, 'justfile', '[group("portable")]\ncheck:\n    true\n');
  put(root, 'scripts/remote-run.sh', '#!/usr/bin/env bash\nprintf "%s\\n" "$@"\nexit "${DEV_ENV_REMOTE_STATUS:-0}"\n');
  chmodSync(join(root, 'scripts/remote-run.sh'), 0o755);
  copyFileSync('scripts/dev-env.sh', join(root, 'scripts/dev-env.sh'));
  const nixLog = join(root, 'nix.log');
  const nix = put(root, 'bin/nix', `#!/usr/bin/env bash
printf '%s\\n' "$*" >> "$DEV_ENV_NIX_LOG"
for argument; do
  case $argument in .|.\\#native) printf 'export DEV_ENV_SELECTED_SHELL=%q\\n' "$argument" ;; esac
done
printf 'export NIX_BUILD_TOP=$(mktemp -d)\\n'
`);
  chmodSync(nix, 0o755);
  const cargo = put(root, 'bin/cargo', '#!/usr/bin/env bash\necho local-cargo\n');
  chmodSync(cargo, 0o755);
  const registry = 'registry/index/index.crates.io-fixture/config.json';
  put(root, `seed/${registry}`, '{}');
  const archive = 'registry/cache/index.crates.io-fixture/archived-1.0.0.crate';
  put(root, `seed/${archive}`, 'archive');
  for (const name of ['archived', 'source-only', 'unlocked']) {
    put(root, `seed/registry/src/index.crates.io-fixture/${name}-1.0.0/src/lib.rs`, name);
  }
  return {
    archive, registry, nixLog,
    home: join(root, 'target/cargo-home'),
    run(args = ['true'], overrides = {}) {
      const result = spawnSync('bash', ['scripts/dev-env.sh', ...args], {
        cwd: root, encoding: 'utf8',
        env: { ...process.env, PATH: `${join(root, 'bin')}:${process.env.PATH}`, CARGO_HOME: join(root, 'seed'), DEV_ENV_NIX_LOG: nixLog, ...overrides },
      });
      assert.ifError(result.error);
      return result;
    },
  };
}

test('local setup seeds archives and source-only entries without duplicating extracted sources', (t) => {
  const build = fixture(t);
  const result = build.run();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(join(build.home, build.archive), 'utf8'), 'archive');
  assert.ok(existsSync(join(build.home, build.registry)));
  assert.equal(readFileSync(join(build.home, 'registry/src/index.crates.io-fixture/source-only-1.0.0/src/lib.rs'), 'utf8'), 'source-only');
  for (const name of ['archived', 'unlocked', 'missing']) {
    assert.ok(!existsSync(join(build.home, `registry/src/index.crates.io-fixture/${name}-1.0.0`)));
  }
  assert.ok(existsSync(join(build.home, '.seeded')));
});

test('portable commands reach the builder before local Cargo cache setup, including failures', async (t) => {
  for (const args of [
    ['cargo', 'check', '--locked'],
    ['env', '-u', 'CARGO_TARGET_DIR', 'CARGO_INCREMENTAL=0', 'cargo', 'test'],
    ['just', 'check'],
  ]) {
    await t.test(args.join(' '), (t) => {
      const build = fixture(t, 'test-builder');
      const result = build.run(args, { DEV_ENV_REMOTE_STATUS: '23' });
      assert.equal(result.status, 23, result.stderr);
      assert.deepEqual(result.stdout.trim().split('\n'), ['--native', 'test-builder', ...args]);
      assert.ok(!existsSync(build.home));
    });
  }
});

test('native and mobile environments are cached independently and reused', (t) => {
  const build = fixture(t);
  for (const options of [[], ['--native'], [], ['--native']]) {
    const result = build.run([...options, 'bash', '-c', 'printf "%s" "$DEV_ENV_SELECTED_SHELL"']);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, options.length ? '.#native' : '.');
  }
  assert.equal(readFileSync(build.nixLog, 'utf8').trim().split('\n').length, 2);
});

test('Apple checks keep local cache setup when a remote builder is configured', async (t) => {
  for (const target of [['--target', 'aarch64-apple-darwin'], ['--target=aarch64-apple-ios']]) {
    await t.test(target.join(' '), (t) => {
      const build = fixture(t, 'test-builder');
      const result = build.run(['cargo', 'check', ...target]);
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stdout.trim(), 'local-cargo');
      assert.ok(existsSync(join(build.home, build.archive)));
    });
  }
});
