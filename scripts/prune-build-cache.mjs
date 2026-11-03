import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

function cachePrefix(key) {
  const prefix = key?.match(/^(bex-gradle-v1-.+-)[0-9a-f]{40}$/)?.[1] ??
    key?.match(/^(ios-swift-xcode-[0-9]+\.[0-9]+-)[0-9a-f]{64}$/)?.[1] ??
    key?.match(/^(local-rust-[A-Za-z0-9]+-[A-Za-z0-9]+-xcode-[0-9]+\.[0-9]+-[0-9a-f]{64}-(?:mac|iphone)-)[0-9a-f]{40}$/)?.[1];
  if (!prefix) throw new Error('Invalid build snapshot key');
  return prefix;
}

export function replacedBuildCaches(entries, currentKey) {
  const prefix = cachePrefix(currentKey);
  const owned = entries.filter(entry => entry.ref === 'refs/heads/main' && entry.key.startsWith(prefix));
  // A successful action step can still report an unsuccessful cache upload.
  if (!owned.some(entry => entry.key === currentKey)) return [];
  return owned.filter(entry => entry.key !== currentKey).map(entry => entry.id);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const key = process.argv[2];
  const prefix = cachePrefix(key);
  const entries = JSON.parse(execFileSync('gh', [
    'cache', 'list', '--key', prefix, '--ref', 'refs/heads/main', '--limit', '100', '--json', 'id,key,ref',
  ], {encoding: 'utf8'}));
  for (const id of replacedBuildCaches(entries, key)) {
    execFileSync('gh', ['cache', 'delete', String(id)], {stdio: 'inherit'});
  }
}
