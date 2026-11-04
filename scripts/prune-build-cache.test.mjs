import test from 'node:test';
import assert from 'node:assert/strict';
import { replacedBuildCaches } from './prune-build-cache.mjs';

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
  assert.deepEqual(replacedBuildCaches(entries, current), [2]);
  assert.deepEqual(entries, before);
});

test('an unconfirmed upload preserves every existing snapshot', () => {
  assert.deepEqual(replacedBuildCaches(entries.filter(entry => entry.id !== 1), current), []);
  assert.deepEqual(replacedBuildCaches(entries, prefix + 'c'.repeat(40)), []);
  assert.throws(() => replacedBuildCaches(entries, 'other-cache'), /Invalid build snapshot key/);
});

test('iPhone snapshots retire only older builds for their Xcode version', () => {
  const key = 'ios-swift-xcode-26.6-' + 'a'.repeat(64);
  const snapshots = [
    {id: 1, key, ref: 'refs/heads/main'},
    {id: 2, key: 'ios-swift-xcode-26.6-' + 'b'.repeat(64), ref: 'refs/heads/main'},
    {id: 3, key: 'ios-swift-xcode-26.5-' + 'b'.repeat(64), ref: 'refs/heads/main'},
    {id: 4, key: 'ios-swift-xcode-26.6-' + 'b'.repeat(64), ref: 'refs/pull/10/merge'},
    ...entries,
  ];
  assert.deepEqual(replacedBuildCaches(snapshots, key), [2]);
  assert.deepEqual(replacedBuildCaches(snapshots.filter(entry => entry.id !== 1), key), []);
});

test('iPhone release snapshots retire only older releases for their Xcode version', () => {
  const key = 'ios-release-xcode-26.6-' + 'a'.repeat(40);
  const snapshots = [
    {id: 1, key, ref: 'refs/heads/main'},
    {id: 2, key: 'ios-release-xcode-26.6-' + 'b'.repeat(40), ref: 'refs/heads/main'},
    {id: 3, key: 'ios-release-xcode-26.5-' + 'b'.repeat(40), ref: 'refs/heads/main'},
    {id: 4, key: 'ios-swift-xcode-26.6-' + 'b'.repeat(64), ref: 'refs/heads/main'},
    ...entries,
  ];
  assert.deepEqual(replacedBuildCaches(snapshots, key), [2]);
  assert.deepEqual(replacedBuildCaches(snapshots.filter(entry => entry.id !== 1), key), []);
});

test('repository compiler snapshots preserve other jobs, toolchains and refs', () => {
  const prefix = 'local-rust-macOS-ARM64-xcode-26.6-' + 'a'.repeat(64) + '-iphone-';
  const current = prefix + 'b'.repeat(40);
  const previous = prefix + 'c'.repeat(40);
  const snapshots = [
    {id: 10, key: current, ref: 'refs/heads/main'},
    {id: 11, key: previous, ref: 'refs/heads/main'},
    {id: 12, key: previous.replace('-iphone-', '-mac-'), ref: 'refs/heads/main'},
    {id: 13, key: previous.replace('26.6', '26.5'), ref: 'refs/heads/main'},
    {id: 14, key: previous.replace('a'.repeat(64), 'd'.repeat(64)), ref: 'refs/heads/main'},
    {id: 15, key: previous, ref: 'refs/pull/10/merge'},
    ...entries,
  ];
  assert.deepEqual(replacedBuildCaches(snapshots, current), [11]);
  assert.deepEqual(replacedBuildCaches(snapshots.filter(entry => entry.id !== 10), current), []);
});
