import test from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(new URL('./ci-rust-cache.sh', import.meta.url));

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'local rust cache '));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const bin = join(root, 'bin');
  mkdirSync(bin);
  const compiler = join(bin, 'rustc');
  writeFileSync(compiler, '#!/usr/bin/env bash\nprintf \'%s\\0\' "$@"\nexit "${CI_WRAPPER_COMPILER_STATUS:-0}"\n');
  const kache = join(bin, 'kache');
  writeFileSync(kache, '#!/usr/bin/env bash\nprintf \'cached\\0\'\nexec "$@"\n');
  for (const path of [compiler, kache]) chmodSync(path, 0o755);
  const workspace = join(root, 'workspace');
  return {
    workspace,
    run(manifest, status = 0) {
      const result = spawnSync('bash', [script, compiler, '--crate-name', 'example', 'source with spaces.rs', ''], {
        encoding: 'utf8',
        env: {
          ...process.env, PATH: `${bin}:${process.env.PATH}`,
          GITHUB_WORKSPACE: workspace, CARGO_MANIFEST_DIR: manifest,
          CI_WRAPPER_COMPILER_STATUS: String(status),
        },
      });
      assert.ifError(result.error);
      return result;
    },
  };
}

test('vendored crates use the compiler cache and preserve arguments', async (t) => {
  for (const crate of ['vendor/gpui-pre', 'vendor/vte']) {
    await t.test(crate, (t) => {
      const { workspace, run } = fixture(t);
      const result = run(join(workspace, crate));
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stdout, 'cached\0--crate-name\0example\0source with spaces.rs\0\0');
    });
  }
});

test('application crates, registry, git and compiler probes execute directly', async (t) => {
  for (const crate of [
    'registry/src/example', 'git/checkouts/example', 'workspace-sibling/crate',
    'workspace/target/cargo-home/registry/src/example', 'workspace/target/cargo-home/git/checkouts/example',
    'workspace/crates/agent-core', 'workspace/vendor-sibling/crate', '',
  ]) {
    await t.test(crate || 'compiler probe', (t) => {
      const { workspace, run } = fixture(t);
      const manifest = crate ? join(workspace, '..', crate) : '';
      const result = run(manifest);
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stdout, '--crate-name\0example\0source with spaces.rs\0\0');
    });
  }
});

test('compiler failures keep their status through either execution path', (t) => {
  const { workspace, run } = fixture(t);
  for (const manifest of [join(workspace, 'vendor/gpui-pre'), '']) {
    assert.equal(run(manifest, 17).status, 17);
  }
});
