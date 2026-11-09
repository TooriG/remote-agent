import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { appendFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { distributeBuild, externalTestingAvailable, iosDistribution, testflightGroups, requestFromEnvironment, type AscRequest, type AppleBuildPlan } from './apple-release.ts';

type AssetVerification = { size: number; sha256: string };
export type ReleaseManifest = AppleBuildPlan & { assets: Record<string, AssetVerification> };
type ReleaseSummary = { tagName: string; isDraft?: boolean; isPrerelease?: boolean };

export const packages = [
  'bex-macOS-arm64.zip', 'bex-windows-x86_64-setup.exe',
  'bex-windows-x86_64-release.zip', 'bex-android.apk', 'bex-linux-x86_64.tar.gz',
];
const checksum = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex');

export function releaseNumber(tag: string | undefined, channel: string) {
  const match = new RegExp(`^${channel}-([1-9][0-9]*)$`).exec(tag ?? '');
  if (!match || !Number.isSafeInteger(Number(match[1]))) throw new Error(`Invalid ${channel} release tag.`);
  return match[1];
}

export function latestNightly(releases: readonly ReleaseSummary[]) {
  const latest = releases.filter(item => !item.isDraft && item.isPrerelease && /^nightly-[1-9][0-9]*$/.test(item.tagName))
    .sort((a, b) => Number(releaseNumber(b.tagName, 'nightly')) - Number(releaseNumber(a.tagName, 'nightly')))[0];
  if (!latest) throw new Error('Publish an externally approved Nightly before promoting it.');
  return latest.tagName;
}

export function validateRelease(manifest: ReleaseManifest, tag: string, sha: string) {
  const channel = tag.startsWith('nightly-') ? 'nightly' : 'stable';
  const number = releaseNumber(tag, channel === 'nightly' ? 'nightly' : 'prd');
  if (manifest.channel !== channel || manifest.tag !== tag || manifest.version !== number
    || manifest.sha !== sha || !/^[a-f0-9]{40}$/.test(sha)) throw new Error('The release tag, source revision, channel and build number must agree.');
  if (manifest.bundleId !== iosDistribution.bundleId || manifest.appId !== '6816015343' || !manifest.buildId
    || !/^\d+\.\d+\.\d+$/.test(manifest.versionName)
    || manifest.testflightUrl !== testflightGroups[channel === 'nightly' ? 'nightly' : 'production'].url) throw new Error('The release must identify its BEX TestFlight build and public link.');
  if (channel === 'stable') {
    if (releaseNumber(manifest.sourceTag, 'nightly') !== number) throw new Error('Production must preserve the selected Nightly build number.');
  } else if (manifest.sourceTag) throw new Error('Nightly cannot name a promoted source.');
  const expected = packages.flatMap(name => [name, `${name}.sha256`]).sort();
  if (JSON.stringify(Object.keys(manifest.assets).sort()) !== JSON.stringify(expected)) throw new Error('The release must contain every platform package and checksum.');
  for (const asset of Object.values(manifest.assets)) {
    if (!/^[a-f0-9]{64}$/.test(asset.sha256) || !Number.isSafeInteger(asset.size) || asset.size <= 0) throw new Error('Invalid release asset verification data.');
  }
}

export function verifyPackages(directory: string, assets: Record<string, AssetVerification>) {
  for (const [name, expected] of Object.entries(assets)) {
    const bytes = readFileSync(join(directory, name));
    if (bytes.length !== expected.size || checksum(bytes) !== expected.sha256) throw new Error(`Release checksum mismatch: ${name}`);
  }
  for (const name of Object.keys(assets).filter(name => !name.endsWith('.sha256'))) {
    const sidecar = readFileSync(join(directory, `${name}.sha256`), 'utf8').trim();
    if (!new RegExp(`^${assets[name].sha256}\\s+\\*?${name.replaceAll('.', '\\.')}\\s*$`).test(sidecar)) throw new Error(`Invalid checksum file: ${name}`);
  }
}

const run = (command: string, args: string[]) => execFileSync(command, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
const repository = () => {
  const value = process.env.GITHUB_REPOSITORY;
  if (!/^[\w.-]+\/[\w.-]+$/.test(value || '')) throw new Error('GITHUB_REPOSITORY is required.');
  return value!;
};
const gh = (...args: string[]) => run('gh', args);
const releases = (): ReleaseSummary[] => JSON.parse(gh('release', 'list', '--repo', repository(), '--limit', '1000', '--json', 'tagName,isDraft,isPrerelease'));
const revision = (tag: string) => run('git', ['rev-parse', '--verify', `refs/tags/${tag}^{commit}`]);

async function inDirectory<T>(action: (directory: string) => T | Promise<T>) {
  const directory = mkdtempSync(join(tmpdir(), 'bex-release-'));
  try { return await action(directory); }
  finally { rmSync(directory, { recursive: true, force: true }); }
}

async function withManifest<T>(tag: string, action: (manifest: ReleaseManifest, directory: string) => T | Promise<T>) {
  return inDirectory(async directory => {
    gh('release', 'download', tag, '--repo', repository(), '--pattern', 'release.json', '--dir', directory);
    const manifest = JSON.parse(readFileSync(join(directory, 'release.json'), 'utf8')) as ReleaseManifest;
    validateRelease(manifest, tag, revision(tag));
    return action(manifest, directory);
  });
}

function publish(tag: string, channel: string) {
  const newer = channel === 'stable' && releases().some(item => !item.isDraft && !item.isPrerelease && /^prd-[1-9][0-9]*$/.test(item.tagName)
    && Number(releaseNumber(item.tagName, 'prd')) > Number(releaseNumber(tag, 'prd')));
  gh('release', 'edit', tag, '--repo', repository(), '--draft=false', `--prerelease=${channel === 'nightly'}`, `--latest=${channel === 'stable' && !newer}`);
}

async function nightlyStatus(request: AscRequest, appId: string, buildId: string, notes: string, locale: string) {
  const status = await distributeBuild(request, appId, buildId, testflightGroups.nightly.name, notes, locale);
  if (status.url !== testflightGroups.nightly.url) throw new Error('The Nightly TestFlight link changed after verification.');
  return status.state;
}

async function seal(tag: string) {
  releaseNumber(tag, 'nightly');
  const isDraft = JSON.parse(gh('release', 'view', tag, '--repo', repository(), '--json', 'isDraft')).isDraft;
  await inDirectory(async directory => {
    gh('release', 'download', tag, '--repo', repository(), '--dir', directory);
    const previous = isDraft ? null : readFileSync(join(directory, 'release.json'), 'utf8');
    const plan = JSON.parse(readFileSync(join(directory, 'apple-build.json'), 'utf8')) as AppleBuildPlan;
    const manifest: ReleaseManifest = { ...plan, assets: Object.fromEntries(packages.flatMap(name => [name, `${name}.sha256`]).map(name => {
      const bytes = readFileSync(join(directory, name));
      return [name, { size: bytes.length, sha256: checksum(bytes) }];
    })) };
    validateRelease(manifest, tag, revision(tag));
    verifyPackages(directory, manifest.assets);
    const path = join(directory, 'release.json');
    writeFileSync(path, JSON.stringify(manifest, null, 2) + '\n');
    if (!isDraft) {
      if (readFileSync(path, 'utf8') !== previous) throw new Error('Published releases cannot be changed.');
      return;
    }
    const request = requestFromEnvironment();
    gh('release', 'upload', tag, path, '--repo', repository(), '--clobber');
    const state = await nightlyStatus(request, manifest.appId, manifest.buildId!, manifest.notes, manifest.locale);
    if (externalTestingAvailable(state)) publish(tag, 'nightly');
    console.log(`${tag}: ${state}`);
  });
}

function promotion(source: ReleaseManifest): ReleaseManifest {
  return { ...source, channel: 'stable', tag: `prd-${source.version}`, sourceTag: source.tag, testflightUrl: testflightGroups.production.url };
}

function ensureTag(tag: string, sha: string) {
  const refs = JSON.parse(gh('api', `repos/${repository()}/git/matching-refs/tags/${tag}`)) as { ref: string; object: { type: string; sha: string } }[];
  const existing = refs.find(ref => ref.ref === `refs/tags/${tag}`);
  if (existing) {
    if (existing.object.type !== 'commit' || existing.object.sha !== sha) throw new Error('The production tag already points to another source; preserve it.');
  } else gh('api', `repos/${repository()}/git/refs`, '--method', 'POST', '-f', `ref=refs/tags/${tag}`, '-f', `sha=${sha}`);
}

async function promote(selected: string | undefined) {
  const available = releases();
  const sourceTag = selected || latestNightly(available);
  releaseNumber(sourceTag, 'nightly');
  if (!available.some(item => item.tagName === sourceTag && !item.isDraft && item.isPrerelease)) throw new Error('Select a published Nightly.');
  await withManifest(sourceTag, async (source, directory) => {
    run('git', ['merge-base', '--is-ancestor', source.sha, 'origin/main']);
    const target = `prd-${source.version}`;
    const existing = available.find(item => item.tagName === target);
    if (existing) {
      const info = JSON.parse(gh('release', 'view', target, '--repo', repository(), '--json', 'assets')) as { assets: { name: string }[] };
      if (info.assets.some(asset => asset.name === 'release.json')) await withManifest(target, (manifest, verified) => {
        if (JSON.stringify(manifest) !== JSON.stringify(promotion(source))) throw new Error('The existing production release differs from this Nightly.');
        if (!existing.isDraft) {
          gh('release', 'download', target, '--repo', repository(), '--skip-existing', '--dir', verified);
          verifyPackages(verified, source.assets);
        }
      });
      else if (!existing.isDraft) throw new Error('The published production manifest is missing.');
      if (!existing.isDraft) {
        if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `tag=${target}\n`);
        return;
      }
    }
    gh('release', 'download', sourceTag, '--repo', repository(), '--skip-existing', '--dir', directory);
    verifyPackages(directory, source.assets);
    const request = requestFromEnvironment();
    const state = await nightlyStatus(request, source.appId, source.buildId!, source.notes, source.locale);
    if (!externalTestingAvailable(state)) throw new Error(`The selected Nightly is not approved for external testing (${state}).`);
    ensureTag(target, source.sha);
    run('git', ['fetch', 'origin', `refs/tags/${target}:refs/tags/${target}`]);
    const manifest = promotion(source);
    validateRelease(manifest, target, source.sha);
    const notes = join(directory, 'notes.md');
    writeFileSync(notes, `Promoted ${sourceTag} without rebuilding.\n\nSource: ${source.sha}.\n\n[iPhone: Get on TestFlight](${manifest.testflightUrl})\n`);
    if (!existing) gh('release', 'create', target, '--repo', repository(), '--verify-tag', '--draft', '--title', `BEX ${target}`, '--notes-file', notes);
    writeFileSync(join(directory, 'release.json'), JSON.stringify(manifest, null, 2) + '\n');
    gh('release', 'upload', target, ...[...Object.keys(manifest.assets), 'release.json'].map(name => join(directory, name)), '--repo', repository(), '--clobber');
    await withManifest(target, (uploaded, verified) => {
      if (JSON.stringify(uploaded) !== JSON.stringify(manifest)) throw new Error('The uploaded production manifest changed.');
      gh('release', 'download', target, '--repo', repository(), '--skip-existing', '--dir', verified);
      verifyPackages(verified, source.assets);
    });
    const stable = await distributeBuild(request, source.appId, source.buildId!, testflightGroups.production.name, source.notes, source.locale);
    if (!externalTestingAvailable(stable.state) || stable.url !== manifest.testflightUrl) throw new Error('The production TestFlight group is not ready.');
    publish(target, 'stable');
    if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `tag=${target}\n`);
    console.log(`Promoted ${sourceTag} to ${target}, preserving every package and Apple build ${source.buildId}.`);
  });
}

async function sync() {
  const request = requestFromEnvironment();
  const failures = [];
  for (const item of releases().filter(item => item.isDraft && /^nightly-[1-9][0-9]*$/.test(item.tagName))) {
    try {
      const info = JSON.parse(gh('release', 'view', item.tagName, '--repo', repository(), '--json', 'assets')) as { assets: { name: string }[] };
      if (!info.assets.some(asset => asset.name === 'release.json')) continue;
      await withManifest(item.tagName, async (manifest, directory) => {
        const state = await nightlyStatus(request, manifest.appId, manifest.buildId!, manifest.notes, manifest.locale);
        if (externalTestingAvailable(state)) {
          gh('release', 'download', item.tagName, '--repo', repository(), '--skip-existing', '--dir', directory);
          verifyPackages(directory, manifest.assets);
          publish(item.tagName, 'nightly');
        }
        console.log(`${item.tagName}: ${state}`);
      });
    } catch (error) { failures.push((error as Error).message); }
  }
  if (failures.length) throw new Error(failures.join('\n'));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [command, tag] = process.argv.slice(2);
  const task = command === 'promote' ? promote(tag) : command === 'seal' ? seal(tag!) : command === 'sync' ? sync()
    : Promise.reject(new Error('Use promote [nightly-N], seal nightly-N, or sync.'));
  task.catch(error => { console.error((error as Error).message); process.exitCode = 1; });
}
