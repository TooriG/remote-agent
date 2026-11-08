import test from 'node:test';
import assert from 'node:assert/strict';
import { idleBuilds, worktreeTargets } from './prune-idle-builds.ts';

const DAY = 24 * 60 * 60 * 1000;

test('each listed worktree, on a branch or detached, has its target considered', () => {
  const porcelain = [
    'worktree /repo', 'HEAD 1111', 'branch refs/heads/main', '',
    'worktree /repo/.worktree/session-a/remote-agent', 'HEAD 2222', 'detached', '',
  ].join('\n');
  assert.deepEqual(worktreeTargets(porcelain), ['/repo/target', '/repo/.worktree/session-a/remote-agent/target']);
});

test('only builds unused for more than three days are idle', () => {
  const now = 100 * DAY;
  const builds = [
    {target: '/a/target', lastUsed: now - 3 * DAY - 1},
    {target: '/b/target', lastUsed: now - 3 * DAY},
    {target: '/c/target', lastUsed: now - DAY},
  ];
  assert.deepEqual(idleBuilds(builds, now), ['/a/target']);
});
