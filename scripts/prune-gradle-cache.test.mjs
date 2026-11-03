import test from 'node:test';
import assert from 'node:assert/strict';
import { replacedGradleCaches } from './prune-gradle-cache.mjs';

const prefix = 'bex-gradle-v1-Linux-X64-toolchain-';
const current = prefix + 'a'.repeat(40);
const previous = prefix + 'b'.repeat(40);
const entries = [
  {id: 1, key: current, ref: 'refs/heads/main'},
  {id: 2, key: previous, ref: 'refs/heads/main'},
  {id: 3, key: previous, ref: 'refs/pull/10/merge'},
  {id: 4, key: 'unrelated-' + 'b'.repeat(40), ref: 'refs/heads/main'},
  {id: 5, key: 'bex-gradle-v1-Linux-X64-other-' + 'b'.repeat(40), ref: 'refs/heads/main'},
];

test('only replaced snapshots from the same main toolchain are retired', () => {
  const before = structuredClone(entries);
  assert.deepEqual(replacedGradleCaches(entries, current), [2]);
  assert.deepEqual(entries, before);
});

test('an unconfirmed upload preserves every existing snapshot', () => {
  assert.deepEqual(replacedGradleCaches(entries.filter(entry => entry.id !== 1), current), []);
  assert.deepEqual(replacedGradleCaches(entries, prefix + 'c'.repeat(40)), []);
  assert.throws(() => replacedGradleCaches(entries, 'other-cache'), /Invalid Gradle snapshot key/);
});
