import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync } from 'node:crypto';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { latestNightly, packages, releaseNumber, validateRelease, verifyPackages } from './release-candidate.ts';

const sha = 'a'.repeat(40);
const nightlyUrl = 'https://testflight.apple.com/join/xdk6J9qr';
const stableUrl = 'https://testflight.apple.com/join/UJbK9Ww7';

function fixture(t, draft = false) {
  const root = mkdtempSync(join(tmpdir(), 'release-promotion-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const bin = join(root, 'bin'), source = join(root, 'nightly-145');
  mkdirSync(bin); mkdirSync(source);
  const assets = {};
  for (const name of packages) {
    const bytes = Buffer.from(`immutable signed binary ${name}`);
    const hash = createHash('sha256').update(bytes).digest('hex');
    for (const [file, content] of [[name, bytes], [`${name}.sha256`, Buffer.from(`${hash}  ${name}\n`)]]) {
      writeFileSync(join(source, file), content);
      assets[file] = { size: content.length, sha256: createHash('sha256').update(content).digest('hex') };
    }
  }
  const plan = { channel: 'nightly', bundleId: 'dev.remoteagent.mobile.ios', appId: '6816015343', buildId: 'reviewed-build',
    tag: 'nightly-145', version: '145', versionName: '1.0.1', sha, notes: 'Changes', locale: 'ja', testflightUrl: nightlyUrl };
  const manifest = { ...plan, assets };
  writeFileSync(join(source, 'apple-build.json'), JSON.stringify(plan));
  if (!draft) writeFileSync(join(source, 'release.json'), JSON.stringify(manifest, null, 2) + '\n');
  const statePath = join(root, 'state.json');
  const read = () => JSON.parse(readFileSync(statePath));
  const update = patch => writeFileSync(statePath, JSON.stringify({ ...read(), ...patch }));
  writeFileSync(statePath, JSON.stringify({ beta: 'IN_BETA_TESTING', refs: { 'nightly-145': sha },
    releases: [{ tagName: 'nightly-145', isDraft: draft, isPrerelease: true }] }));
  writeFileSync(join(bin, 'gh'), `#!${process.execPath}
const fs = require('node:fs'), path = require('node:path');
const root = process.env.RELEASE_FIXTURE, args = process.argv.slice(2);
const statePath = path.join(root, 'state.json'), state = JSON.parse(fs.readFileSync(statePath));
fs.appendFileSync(path.join(root, 'commands'), JSON.stringify(args) + '\\n');
if (args[0] === 'api') {
 if (args[1].includes('matching-refs')) console.log(JSON.stringify(Object.entries(state.refs).map(([tag, sha]) => ({ ref: 'refs/tags/' + tag, object: { type: 'commit', sha } }))));
 else if (args.includes('POST')) state.refs[args[args.indexOf('-f') + 1].slice(14)] = args.at(-1).slice(4);
 else throw new Error('Unexpected GitHub API');
} else if (args[1] === 'view') {
 const release = state.releases.find(item => item.tagName === args[2]);
 if (!release) process.exit(1);
 console.log(JSON.stringify({ ...release, assets: fs.readdirSync(path.join(root, args[2])).map(name => ({ name })) }));
} else if (args[1] === 'list') console.log(JSON.stringify(state.releases));
else if (args[1] === 'download') {
 const files = args.includes('--pattern') ? [args[args.indexOf('--pattern') + 1]] : fs.readdirSync(path.join(root, args[2]));
 for (const file of files) {
  const to = path.join(args[args.indexOf('--dir') + 1], file);
  if (!args.includes('--skip-existing') || !fs.existsSync(to)) fs.copyFileSync(path.join(root, args[2], file), to);
 }
} else if (args[1] === 'upload') {
 if (!state.releases.find(item => item.tagName === args[2]).isDraft) throw new Error('Published assets must never be changed');
 if (state.failUpload) { state.failUpload = false; fs.writeFileSync(statePath, JSON.stringify(state)); process.exit(23); }
 for (const file of args.slice(3, args.indexOf('--repo'))) fs.copyFileSync(file, path.join(root, args[2], path.basename(file)));
} else if (args[1] === 'create') {
 if (state.releases.some(item => item.tagName === args[2])) throw new Error('Duplicate release');
 fs.mkdirSync(path.join(root, args[2]));
 state.releases.push({ tagName: args[2], isDraft: args.includes('--draft'), isPrerelease: false });
} else if (args[1] === 'edit') state.releases.find(item => item.tagName === args[2]).isDraft = false;
else throw new Error('Unexpected gh command');
fs.writeFileSync(statePath, JSON.stringify(state));
`);
  writeFileSync(join(bin, 'git'), `#!${process.execPath}
const fs = require('node:fs'), args = process.argv.slice(2);
const state = JSON.parse(fs.readFileSync(process.env.RELEASE_FIXTURE + '/state.json'));
if (args[0] === 'rev-parse') {
 const tag = args.at(-1).slice(10, -9);
 if (!state.refs[tag]) process.exit(1);
 console.log(state.refs[tag]);
} else if (args[0] === 'merge-base' && state.foreignSource) process.exit(1);
else if (!['merge-base', 'fetch'].includes(args[0])) throw new Error('Unexpected git command');
`);
  for (const file of ['gh', 'git']) chmodSync(join(bin, file), 0o755);
  const stub = join(root, 'fetch.mjs');
  writeFileSync(stub, `
import { readFileSync, writeFileSync, appendFileSync } from 'node:fs';
globalThis.fetch = async (url, options) => {
 const root = process.env.RELEASE_FIXTURE, statePath = root + '/state.json', state = JSON.parse(readFileSync(statePath));
 const pathname = new URL(url).pathname;
 appendFileSync(root + '/apple', JSON.stringify({ pathname, method: options.method, body: options.body && JSON.parse(options.body) }) + '\\n');
 let data;
 if (options.method !== 'GET') {
  if (pathname === '/v1/betaAppReviewSubmissions') { state.beta = 'WAITING_FOR_REVIEW'; writeFileSync(statePath, JSON.stringify(state)); }
  else if (pathname !== '/v1/buildBetaNotifications' && !pathname.endsWith('/relationships/builds')) throw new Error('Unexpected Apple mutation ' + pathname);
  return { ok: true, status: 204 };
 }
 if (pathname === '/v1/betaGroups') data = [
  { id: 'nightly', attributes: { name: 'BEX Nightly', isInternalGroup: false, publicLinkEnabled: true, publicLink: '${nightlyUrl}' } },
  { id: 'stable', attributes: { name: 'BEX External', isInternalGroup: false, publicLinkEnabled: true, publicLink: '${stableUrl}' } }
 ];
 else if (pathname.endsWith('/buildBetaDetail')) data = { id: 'detail', attributes: { externalBuildState: state.beta, autoNotifyEnabled: true } };
 else if (pathname.endsWith('/betaBuildLocalizations')) data = [{ id: 'locale', attributes: { locale: 'ja', whatsNew: 'Changes' } }];
 else throw new Error('Unexpected Apple GET ' + pathname);
 return { ok: true, status: 200, json: async () => ({ data }) };
};
`);
  const { privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' });
  const key = join(root, 'fixture.p8');
  writeFileSync(key, privateKey.export({ type: 'pkcs8', format: 'pem' }), { mode: 0o600 });
  return { root, source, manifest, read, update,
    run(command, tag, expectedStatus = 0) {
      const result = spawnSync(process.execPath, ['--import', stub, fileURLToPath(new URL('./release-candidate.ts', import.meta.url)), command, ...[tag].filter(value => value !== undefined)], {
        encoding: 'utf8', env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, RELEASE_FIXTURE: root,
          GITHUB_OUTPUT: join(root, 'outputs'), GITHUB_REPOSITORY: 'fixture/repo', AGENT_ASC_KEY_ID: 'fixture', AGENT_ASC_ISSUER_ID: 'fixture', AGENT_ASC_PRIVATE_KEY_FILE: key },
      });
      assert.equal(result.status, expectedStatus, result.stderr);
      return result;
    },
    calls(name) { try { return readFileSync(join(root, name), 'utf8').trim().split('\n').map(JSON.parse); } catch (error) { if (error.code === 'ENOENT') return []; throw error; } },
  };
}

test('tags identify one numbered candidate without accepting branches or shell syntax', () => {
  assert.equal(releaseNumber('prd-145', 'prd'), '145');
  for (const value of ['nightly-145', 'prd-0', 'prd-01', 'prd-145/other', 'prd-$(echo)', 'prd-999999999999999999']) assert.throws(() => releaseNumber(value, 'prd'));
});

test('Nightly waits for external review and production preserves every package, number and Apple build', t => {
  const build = fixture(t, true);
  build.update({ beta: 'WAITING_FOR_REVIEW' });
  build.run('seal', 'nightly-145');
  assert.equal(build.read().releases[0].isDraft, true);
  build.update({ beta: 'BETA_APPROVED' });
  build.run('sync');
  assert.equal(build.read().releases[0].isDraft, true);
  build.update({ beta: 'IN_BETA_TESTING' });
  build.run('sync');
  assert.equal(build.read().releases[0].isDraft, false);
  build.run('promote', 'nightly-145');
  const state = build.read();
  assert.equal(state.refs['prd-145'], sha);
  assert.equal(state.releases.find(item => item.tagName === 'prd-145').isDraft, false);
  const promoted = JSON.parse(readFileSync(join(build.root, 'prd-145/release.json')));
  assert.equal(promoted.version, '145');
  assert.equal(promoted.buildId, 'reviewed-build');
  assert.equal(promoted.sourceTag, 'nightly-145');
  assert.equal(promoted.testflightUrl, stableUrl);
  for (const file of [...packages, ...packages.map(name => name + '.sha256')]) {
    assert.deepEqual(readFileSync(join(build.source, file)), readFileSync(join(build.root, 'prd-145', file)));
  }
  const assignment = build.calls('apple').find(call => call.pathname === '/v1/betaGroups/stable/relationships/builds');
  assert.equal(assignment.body.data[0].id, 'reviewed-build');
  assert.match(readFileSync(join(build.root, 'outputs'), 'utf8'), /tag=prd-145/);
  const before = build.calls('commands').filter(args => args[1] === 'edit').length;
  const assignments = build.calls('apple').length;
  build.run('promote', 'nightly-145');
  assert.equal(build.calls('commands').filter(args => args[1] === 'edit').length, before);
  assert.equal(build.calls('apple').length, assignments);
  build.run('seal', 'nightly-145');
});

test('unapproved and expired builds cannot be promoted or reach the production group', t => {
  for (const beta of ['READY_FOR_BETA_SUBMISSION', 'WAITING_FOR_REVIEW', 'EXPIRED', 'BETA_REJECTED']) {
    const build = fixture(t); build.update({ beta });
    build.run('promote', 'nightly-145', 1);
    assert.equal(build.read().refs['prd-145'], undefined);
    assert.ok(!build.calls('apple').some(call => call.pathname.includes('/stable/')));
  }
});

test('corrupt packages and conflicting tags stop before changing production TestFlight', t => {
  for (const kind of ['package', 'tag', 'source']) {
    const build = fixture(t);
    if (kind === 'package') writeFileSync(join(build.source, packages[0]), 'corrupt binary');
    if (kind === 'tag') build.update({ refs: { ...build.read().refs, 'prd-145': 'b'.repeat(40) } });
    if (kind === 'source') build.update({ foreignSource: true });
    build.run('promote', 'nightly-145', 1);
    assert.ok(!build.calls('apple').some(call => call.pathname.includes('/stable/')));
  }
});

test('promotion recovers a partial upload without rebuilding or changing the selected candidate', t => {
  const build = fixture(t); build.update({ failUpload: true });
  build.run('promote', 'nightly-145', 1);
  assert.equal(build.read().releases.find(item => item.tagName === 'prd-145').isDraft, true);
  assert.ok(!build.calls('apple').some(call => call.pathname.includes('/stable/')));
  build.run('promote', 'nightly-145');
  assert.equal(build.read().releases.find(item => item.tagName === 'prd-145').isDraft, false);
  assert.deepEqual(readFileSync(join(build.root, 'prd-145', packages[0])), readFileSync(join(build.source, packages[0])));
});

test('an older promotion cannot downgrade latest and published assets are verified without replacement', t => {
  const build = fixture(t);
  build.update({ releases: [...build.read().releases, { tagName: 'prd-146', isDraft: false, isPrerelease: false }] });
  build.run('promote');
  const edit = build.calls('commands').find(args => args[1] === 'edit' && args[2] === 'prd-145');
  assert.ok(edit.includes('--latest=false'));
  writeFileSync(join(build.root, 'prd-145', packages[0]), 'damaged published asset');
  const before = build.calls('commands').filter(args => args[1] === 'upload').length;
  build.run('promote', 'nightly-145', 1);
  assert.equal(build.calls('commands').filter(args => args[1] === 'upload').length, before);
});

test('release validation binds the variant, Apple build, public link and unchanged source number', t => {
  const build = fixture(t);
  validateRelease(build.manifest, 'nightly-145', sha);
  verifyPackages(build.source, build.manifest.assets);
  for (const patch of [{ sha: 'b'.repeat(40) }, { version: '146' }, { appId: '6820657007' }, { buildId: undefined },
    { bundleId: 'dev.remoteagent.mobile.ios.preview' }, { testflightUrl: 'https://example.test/join/id' }, { assets: {} }]) {
    assert.throws(() => validateRelease({ ...build.manifest, ...patch }, 'nightly-145', sha));
  }
  const stable = { ...build.manifest, channel: 'stable', tag: 'prd-145', sourceTag: 'nightly-145', testflightUrl: stableUrl };
  validateRelease(stable, 'prd-145', sha);
  assert.throws(() => validateRelease({ ...stable, sourceTag: 'nightly-144' }, 'prd-145', sha));
});

test('latest Nightly ignores drafts, production releases and malformed tags', () => {
  const item = (tagName, isDraft = false, isPrerelease = true) => ({ tagName, isDraft, isPrerelease });
  assert.equal(latestNightly([item('nightly-9'), item('nightly-10'), item('nightly-11', true), item('prd-100', false, false), item('nightly-01')]), 'nightly-10');
  assert.throws(() => latestNightly([item('nightly-11', true), item('prd-100', false, false)]), /externally approved Nightly/);
});
