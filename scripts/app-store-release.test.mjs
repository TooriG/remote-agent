import test from 'node:test';
import assert from 'node:assert/strict';
import { generateKeyPairSync } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { packages } from './release-candidate.ts';
import { submitProduction } from './app-store-release.ts';

function fixture({ state = 'PREPARE_FOR_SUBMISSION', selected = 'candidate', valid = true, submission = null, failOnce = false, storeVersion = '1.0.1', candidateVersion = '1.0.1', blockNextVersion = false, reviewItems = [] } = {}) {
  let version = state === 'NOT_CREATED' ? null : { id: 'version', attributes: { versionString: storeVersion, appStoreState: state } };
  let linked = selected;
  let items = reviewItems;
  let interrupted = failOnce;
  const changes = [];
  const request = async (path, options = {}) => {
    if (!options.method) {
      if (path === '/v1/builds/candidate') return { data: { id: 'candidate', attributes: { processingState: valid ? 'VALID' : 'PROCESSING', expired: false } } };
      if (path.endsWith('/preReleaseVersion')) return { data: { attributes: { version: candidateVersion } } };
      if (path.endsWith('/app')) return { data: { id: '6816015343' } };
      if (path.includes('/appStoreVersions?')) return { data: version ? [version] : [] };
      if (path.endsWith('/build')) return { data: linked ? { id: linked } : null };
      if (path.endsWith('/appStoreVersionLocalizations')) return { data: [{ id: 'ja' }, { id: 'en' }] };
      if (path.startsWith('/v1/reviewSubmissions?')) return { data: submission ? [submission] : [] };
      if (path.includes('/items?')) return { data: items };
      throw new Error(`Unexpected GET ${path}`);
    }
    changes.push({ path, ...options });
    const data = options.data.data;
    if (path === '/v1/appStoreVersions') {
      if (blockNextVersion && version && version.attributes.appStoreState !== 'READY_FOR_DISTRIBUTION') throw new Error('Previous version is still in review');
      version = { id: 'version', attributes: { ...data.attributes, appStoreState: 'PREPARE_FOR_SUBMISSION' } };
      linked = data.relationships.build.data.id;
      return { data: version };
    }
    if (path === '/v1/appStoreVersions/version') {
      Object.assign(version.attributes, data.attributes);
      linked = data.relationships.build.data.id;
    }
    if (path === '/v1/reviewSubmissions') {
      submission = { id: 'submission', attributes: { state: 'READY_FOR_REVIEW' } };
      return { data: submission };
    }
    if (path === '/v1/reviewSubmissionItems') {
      if (interrupted) { interrupted = false; throw new Error('Temporary API failure'); }
      items.push({ relationships: data.relationships });
    }
    if (path === '/v1/reviewSubmissions/submission') version.attributes.appStoreState = 'WAITING_FOR_REVIEW';
    return { data: version };
  };
  const submit = () => submitProduction(request, '6816015343', 'candidate', candidateVersion, 'ja', 'Release notes');
  return { request, submit, changes, finish: () => { version.attributes.appStoreState = 'READY_FOR_DISTRIBUTION'; } };
}

test('the promoted Apple build is submitted to App Review with release after approval', async () => {
  const api = fixture({ state: 'NOT_CREATED', selected: null });
  assert.equal((await api.submit()).state, 'WAITING_FOR_REVIEW');
  const creation = api.changes.find(change => change.path === '/v1/appStoreVersions').data.data;
  assert.equal(creation.attributes.releaseType, 'AFTER_APPROVAL');
  assert.equal(creation.relationships.build.data.id, 'candidate');
  assert.equal(api.changes.filter(change => change.path.startsWith('/v1/appStoreVersionLocalizations/')).length, 2);
  const count = api.changes.length;
  assert.equal((await api.submit()).state, 'WAITING_FOR_REVIEW');
  assert.equal(api.changes.length, count, 'retrying an in-flight review must not modify it');
  api.finish();
  assert.equal((await api.submit()).state, 'READY_FOR_DISTRIBUTION');
});

test('an interrupted submission reuses the empty review instead of leaving duplicate submissions', async () => {
  const api = fixture({ failOnce: true });
  await assert.rejects(api.submit(), /Temporary API failure/);
  assert.equal((await api.submit()).state, 'WAITING_FOR_REVIEW');
  assert.equal(api.changes.filter(change => change.path === '/v1/reviewSubmissions').length, 1);
});

test('approved builds and ongoing reviews can never be replaced by a different candidate', async () => {
  for (const state of ['PENDING_DEVELOPER_RELEASE', 'WAITING_FOR_REVIEW', 'IN_REVIEW', 'READY_FOR_DISTRIBUTION', 'READY_FOR_SALE', 'UNKNOWN_STATE']) {
    const api = fixture({ state, selected: 'approved-115' });
    await assert.rejects(api.submit(), /different build/);
    assert.deepEqual(api.changes, []);
  }
});

test('invalid and rejected builds fail before further release side effects', async () => {
  for (const options of [{ valid: false }, { state: 'REJECTED' }]) {
    const api = fixture(options);
    await assert.rejects(api.submit());
    assert.deepEqual(api.changes, []);
  }
});

test('manual approval releases only the build selected by the production tag', async () => {
  const api = fixture({ state: 'PENDING_DEVELOPER_RELEASE' });
  assert.equal((await api.submit()).state, 'PROCESSING_FOR_DISTRIBUTION');
  assert.deepEqual(api.changes.map(change => change.path), ['/v1/appStoreVersionReleaseRequests']);
});


test('a later reserved Nightly can be submitted unchanged after an earlier App Store review finishes', async () => {
  const api = fixture({ state: 'WAITING_FOR_REVIEW', selected: 'earlier-build', storeVersion: '1.0.1', candidateVersion: '1.0.2', blockNextVersion: true });
  await assert.rejects(api.submit(), /still in review/);
  assert.ok(!api.changes.some(change => change.path === '/v1/appStoreVersions/version'));
  api.finish();
  assert.equal((await api.submit()).state, 'WAITING_FOR_REVIEW');
  const created = api.changes.findLast(change => change.path === '/v1/appStoreVersions').data.data;
  assert.equal(created.attributes.versionString, '1.0.2');
  assert.equal(created.relationships.build.data.id, 'candidate');
});

test('a corrected Nightly replaces an editable or rejected version without resubmitting its rejected binary', async () => {
  for (const state of ['PREPARE_FOR_SUBMISSION', 'READY_FOR_REVIEW', 'REJECTED', 'METADATA_REJECTED', 'DEVELOPER_REJECTED', 'INVALID_BINARY']) {
    const api = fixture({ state, selected: 'previous-build', storeVersion: '1.0.1', candidateVersion: '1.0.2' });
    assert.equal((await api.submit()).state, 'WAITING_FOR_REVIEW');
    const replaced = api.changes.find(change => change.path === '/v1/appStoreVersions/version').data.data;
    assert.equal(replaced.attributes.versionString, '1.0.2');
    assert.equal(replaced.relationships.build.data.id, 'candidate');
  }
});


test('the App Store CLI reads the promoted identity and rejects a Nightly before calling Apple', t => {
  const root = mkdtempSync(join(tmpdir(), 'app-review-cli-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const path = join(root, 'release.json'), calls = join(root, 'apple-calls');
  const manifest = { channel: 'stable', tag: 'prd-145', sourceTag: 'nightly-145', version: '145', versionName: '1.0.1', sha: 'a'.repeat(40),
    appId: '6816015343', bundleId: 'dev.remoteagent.mobile.ios', buildId: 'exact-reviewed-build', locale: 'ja', notes: 'Changes',
    testflightUrl: 'https://testflight.apple.com/join/UJbK9Ww7',
    assets: Object.fromEntries(packages.flatMap(name => [name, `${name}.sha256`]).map(name => [name, { size: 1, sha256: 'b'.repeat(64) }])) };
  writeFileSync(path, JSON.stringify(manifest));
  const stub = join(root, 'apple.mjs');
  writeFileSync(stub, `
import { appendFileSync } from 'node:fs';
globalThis.fetch = async (url, options) => {
 if (options.method !== 'GET') throw new Error('An in-flight review must be preserved');
 const parsed = new URL(url); appendFileSync(process.env.APP_REVIEW_CALLS, parsed.pathname + '\\n');
 let data;
 if (parsed.pathname === '/v1/apps/6816015343/appStoreVersions') data = [{ id: 'existing-version', attributes: { versionString: '1.0.1', appStoreState: 'WAITING_FOR_REVIEW' } }];
 else if (parsed.pathname === '/v1/appStoreVersions/existing-version/build') data = { id: 'exact-reviewed-build' };
 else throw new Error('The CLI forwarded a different candidate');
 return { ok: true, status: 200, json: async () => ({ data }) };
};`);
  const { privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' });
  const key = join(root, 'fixture.p8');
  writeFileSync(key, privateKey.export({ type: 'pkcs8', format: 'pem' }), { mode: 0o600 });
  const run = () => spawnSync(process.execPath, ['--import', stub, fileURLToPath(new URL('./app-store-release.ts', import.meta.url)), path], {
    encoding: 'utf8', env: { ...process.env, AGENT_ASC_KEY_ID: 'fixture', AGENT_ASC_ISSUER_ID: 'fixture', AGENT_ASC_PRIVATE_KEY_FILE: key, APP_REVIEW_CALLS: calls },
  });
  const result = run();
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /prd-145: App Store WAITING_FOR_REVIEW/);
  const before = readFileSync(calls, 'utf8');
  const nightly = { ...manifest, channel: 'nightly', tag: 'nightly-145', testflightUrl: 'https://testflight.apple.com/join/xdk6J9qr' };
  delete nightly.sourceTag;
  writeFileSync(path, JSON.stringify(nightly));
  const rejected = run();
  assert.equal(rejected.status, 1);
  assert.match(rejected.stderr, /Only a promoted production build/);
  assert.equal(readFileSync(calls, 'utf8'), before);
});


test('corrected builds reuse unresolved review items and preserve unrelated rejected items', async () => {
  const item = { id: 'rejected-version-item', attributes: { state: 'REJECTED' }, relationships: { appStoreVersion: { data: { id: 'version' } } } };
  const options = { state: 'REJECTED', selected: 'previous-build', storeVersion: '1.0.1', candidateVersion: '1.0.2',
    submission: { id: 'submission', attributes: { state: 'UNRESOLVED_ISSUES' } } };
  const corrected = fixture({ ...options, reviewItems: [item] });
  assert.equal((await corrected.submit()).state, 'WAITING_FOR_REVIEW');
  const resolution = corrected.changes.find(change => change.path === '/v1/reviewSubmissionItems/rejected-version-item');
  assert.equal(resolution.data.data.attributes.resolved, true);
  assert.ok(!corrected.changes.some(change => change.path === '/v1/reviewSubmissions'));
  const unrelated = fixture({ ...options, reviewItems: [item, { id: 'other-item', attributes: { state: 'REJECTED' } }] });
  await assert.rejects(unrelated.submit(), /other rejected review items/);
  assert.ok(!unrelated.changes.some(change => change.path === '/v1/reviewSubmissions/submission' || change.path.startsWith('/v1/reviewSubmissionItems/')));
});
