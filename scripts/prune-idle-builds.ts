import { execFileSync } from 'node:child_process';
import { existsSync, lstatSync, realpathSync, rmSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

// Cargo shares dependency outputs, but each worktree still builds the workspace
// crates into its own target directory: tens of GB on the Mac. Session worktrees
// outlive their work, so outputs nobody built for this long are only disk use.
const IDLE_MS = 3 * 24 * 60 * 60 * 1000;
// scripts/dev-env.sh marks this on every command it runs in the worktree.
const MARK = '.last-used';

type Build = { target: string; lastUsed: number };

export function worktreeTargets(porcelain: string) {
  return porcelain.split('\n').filter(line => line.startsWith('worktree '))
    .map(line => join(line.slice('worktree '.length), 'target'));
}

export function idleBuilds(builds: readonly Build[], now: number) {
  return builds.filter(build => now - build.lastUsed > IDLE_MS).map(build => build.target);
}

function lastUsed(target: string) {
  const mark = join(target, MARK);
  // Outputs built before the mark existed fall back to the directory's own time.
  return statSync(existsSync(mark) ? mark : target).mtimeMs;
}

function remove(target: string) {
  // A configured build root links target to external storage; free the storage
  // and drop the link so scripts/dev-env.sh recreates it.
  rmSync(realpathSync(target), {recursive: true, force: true});
  if (lstatSync(target, {throwIfNoEntry: false})?.isSymbolicLink()) rmSync(target);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const deleting = process.argv.includes('--delete');
  const targets = worktreeTargets(execFileSync('git', ['worktree', 'list', '--porcelain'], {encoding: 'utf8'}))
    .filter(target => existsSync(target));
  const idle = idleBuilds(targets.map(target => ({target, lastUsed: lastUsed(target)})), Date.now());
  for (const target of idle) {
    execFileSync('du', ['-sh', realpathSync(target)], {stdio: 'inherit'});
    if (deleting) remove(target);
  }
  if (!deleting && idle.length > 0) console.log('Run again with --delete to remove these build outputs.');
}
