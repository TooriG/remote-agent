import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { distributeBuild, requestFromEnvironment } from './apple-release.mjs';
import { submitProduction } from './app-store-release.mjs';

export const packages = [
  'bex-macOS-arm64.zip', 'bex-windows-x86_64-setup.exe',
  'bex-windows-x86_64-release.zip', 'bex-android.apk', 'bex-linux-x86_64.tar.gz',
];
const checksum = bytes => createHash('sha256').update(bytes).digest('hex');

export function releaseNumber(tag, channel) {
  const match = new RegExp(`^${channel}-([1-9][0-9]*)$`).exec(tag);
  if (!match || !Number.isSafeInteger(Number(match[1]))) throw new Error(`Invalid ${channel} release tag.`);
  return match[1];
}

export function validateCandidate(manifest, tag, sha) {
  const number = releaseNumber(tag, 'dev');
  if (manifest.tag !== tag || manifest.version !== number || manifest.sha !== sha || !/^[a-f0-9]{40}$/.test(sha)) throw new Error('The candidate tag, revision and build number must agree.');
  if (manifest.appId !== '6816015343' || !manifest.buildId || !/^\d+\.\d+\.\d+$/.test(manifest.versionName)) throw new Error('The candidate must identify the existing BEX App Store build.');
  const expected = packages.flatMap(name => [name, `${name}.sha256`]).sort();
  if (JSON.stringify(Object.keys(manifest.assets).sort()) !== JSON.stringify(expected)) throw new Error('The candidate must contain every platform package and checksum.');
  for (const asset of Object.values(manifest.assets)) {
    if (!/^[a-f0-9]{64}$/.test(asset.sha256) || !Number.isSafeInteger(asset.size) || asset.size <= 0) throw new Error('Invalid candidate asset verification data.');
  }
}

export function verifyPackages(directory, assets) {
  for (const [name, expected] of Object.entries(assets)) {
    const bytes = readFileSync(join(directory, name));
    if (bytes.length !== expected.size || checksum(bytes) !== expected.sha256) throw new Error(`Candidate checksum mismatch: ${name}`);
  }
  for (const name of packages) {
    const sidecar = readFileSync(join(directory, `${name}.sha256`), 'utf8').trim();
    if (!new RegExp(`^${assets[name].sha256}\\s+\\*?${name.replaceAll('.', '\\.')}\\s*$`).test(sidecar)) throw new Error(`Invalid checksum file: ${name}`);
  }
}

const run = (command, args) => execFileSync(command, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
const repository = () => {
  const value = process.env.GITHUB_REPOSITORY;
  if (!/^[\w.-]+\/[\w.-]+$/.test(value || '')) throw new Error('GITHUB_REPOSITORY is required.');
  return value;
};
const gh = (...args) => run('gh', args);
const release = tag => JSON.parse(gh('api', `repos/${repository()}/releases/tags/${tag}`));
const revision = tag => run('git', ['rev-parse', '--verify', `refs/tags/${tag}^{commit}`]);

async function inDirectory(action) {
  const directory = mkdtempSync(join(tmpdir(), 'bex-release-'));
  try { return await action(directory); }
  finally { rmSync(directory, { recursive: true, force: true }); }
}

async function seal(tag) {
  releaseNumber(tag, 'dev');
  const candidate = release(tag);
  if (!candidate.prerelease) throw new Error('A dev candidate must be a prerelease.');
  await inDirectory(async directory => {
    gh('release', 'download', tag, '--repo', repository(), '--dir', directory);
    const previous = candidate.draft ? null : readFileSync(join(directory, 'release.json'), 'utf8');
    const manifest = JSON.parse(readFileSync(join(directory, 'apple-build.json'), 'utf8'));
    manifest.assets = Object.fromEntries(packages.flatMap(name => [name, `${name}.sha256`]).map(name => {
      const bytes = readFileSync(join(directory, name));
      return [name, { size: bytes.length, sha256: checksum(bytes) }];
    }));
    validateCandidate(manifest, tag, revision(tag));
    verifyPackages(directory, manifest.assets);
    const path = join(directory, 'release.json');
    writeFileSync(path, JSON.stringify(manifest, null, 2) + '\n');
    if (!candidate.draft) {
      if (readFileSync(path, 'utf8') !== previous) throw new Error('Published candidates cannot be changed.');
      return;
    }
    gh('release', 'upload', tag, path, '--repo', repository(), '--clobber');
    gh('release', 'edit', tag, '--repo', repository(), '--draft=false', '--prerelease', '--latest=false');
  });
}

function readManifest(directory, tag) {
  const manifest = JSON.parse(readFileSync(join(directory, 'release.json'), 'utf8'));
  const source = `dev-${releaseNumber(tag, tag.startsWith('prd-') ? 'prd' : 'dev')}`;
  validateCandidate(manifest, source, revision(tag));
  return manifest;
}

async function withManifest(tag, action) {
  return inDirectory(async directory => {
    gh('release', 'download', tag, '--repo', repository(), '--dir', directory);
    const manifest = readManifest(directory, tag);
    verifyPackages(directory, manifest.assets);
    return action(manifest, directory);
  });
}

async function promote(tag) {
  const number = releaseNumber(tag, 'prd');
  const source = `dev-${number}`;
  const candidate = release(source);
  if (candidate.draft || !candidate.prerelease || revision(source) !== revision(tag)) throw new Error('Production must select the exact published dev candidate revision.');
  const request = requestFromEnvironment();
  await withManifest(source, async (manifest, directory) => {
    const releases = JSON.parse(gh('release', 'list', '--repo', repository(), '--limit', '1000', '--json', 'tagName,isDraft'));
    const existing = releases.find(item => item.tagName === tag);
    if (!existing) {
      const files = [...Object.keys(manifest.assets), 'release.json'].map(name => join(directory, name));
      gh('release', 'create', tag, ...files, '--repo', repository(), '--draft', '--title', `BEX ${manifest.versionName} (${number})`,
        '--notes', `Source: ${manifest.sha}\n\n${manifest.notes}\n\nApp Store build: ${manifest.version}.`);
    } else {
      await withManifest(tag, async existingManifest => {
        if (JSON.stringify(existingManifest) !== JSON.stringify(manifest)) throw new Error('The existing production release selects a different candidate.');
      });
    }
    const status = await submitProduction(request, manifest.appId, manifest.buildId, manifest.versionName, manifest.locale, manifest.notes);
    if (status.published) publish(tag);
    console.log(`${tag}: ${status.state}`);
  });
}

function publish(tag) {
  const releases = JSON.parse(gh('release', 'list', '--repo', repository(), '--limit', '1000', '--json', 'tagName,isDraft,isPrerelease'));
  const newer = releases.some(item => !item.isDraft && !item.isPrerelease && /^prd-[1-9][0-9]*$/.test(item.tagName)
    && Number(releaseNumber(item.tagName, 'prd')) > Number(releaseNumber(tag, 'prd')));
  gh('release', 'edit', tag, '--repo', repository(), '--draft=false', '--prerelease=false', `--latest=${!newer}`);
}

async function sync() {
  const request = requestFromEnvironment();
  const releases = JSON.parse(gh('release', 'list', '--repo', repository(), '--limit', '1000', '--json', 'tagName,isDraft,isPrerelease'));
  const latestDev = releases.filter(item => !item.isDraft && item.isPrerelease && /^dev-[1-9][0-9]*$/.test(item.tagName))
    .sort((a, b) => Number(releaseNumber(b.tagName, 'dev')) - Number(releaseNumber(a.tagName, 'dev')))[0];
  const failures = [];
  try {
    if (latestDev) await inDirectory(async directory => {
      gh('release', 'download', latestDev.tagName, '--repo', repository(), '--pattern', 'release.json', '--dir', directory);
      const manifest = readManifest(directory, latestDev.tagName);
      const state = await distributeBuild(request, manifest.appId, manifest.buildId, 'BEX External', manifest.notes, manifest.locale);
      console.log(`${latestDev.tagName} TestFlight: ${state}`);
    });
  } catch (error) { failures.push(error.message); }
  for (const item of releases.filter(item => item.isDraft && /^prd-[1-9][0-9]*$/.test(item.tagName))) {
    try {
      await inDirectory(async directory => {
        gh('release', 'download', item.tagName, '--repo', repository(), '--pattern', 'release.json', '--dir', directory);
        const manifest = readManifest(directory, item.tagName);
        const status = await submitProduction(request, manifest.appId, manifest.buildId, manifest.versionName, manifest.locale, manifest.notes);
        if (status.published) {
          gh('release', 'download', item.tagName, '--repo', repository(), '--skip-existing', '--dir', directory);
          verifyPackages(directory, manifest.assets);
          publish(item.tagName);
        }
        console.log(`${item.tagName}: ${status.state}`);
      });
    } catch (error) { failures.push(error.message); }
  }
  if (failures.length) throw new Error(failures.join('\n'));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [command, tag] = process.argv.slice(2);
  const task = command === 'seal' ? seal(tag) : command === 'promote' ? promote(tag) : command === 'sync' ? sync() : Promise.reject(new Error('Use seal dev-N, promote prd-N, or sync.'));
  task.catch(error => { console.error(error.message); process.exitCode = 1; });
}
