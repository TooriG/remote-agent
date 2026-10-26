import test from 'node:test';
import assert from 'node:assert/strict';
import { generateKeyPairSync, verify } from 'node:crypto';
import { ascClient, deliveryId, distributeBuild, listAll, nextBuildNumber, parseBuildStatus } from './apple-release.mjs';

test('build numbers advance past source, uploaded Apple builds and published releases', () => {
  assert.equal(nextBuildNumber(['76', '76'], ['75', '77'], ['android-preview-1000', 'prd-81']), '82');
  assert.equal(nextBuildNumber(['76'], [], []), '77');
  assert.throws(() => nextBuildNumber(['75', '76'], [], []));
  assert.throws(() => nextBuildNumber([], [], []));
  assert.throws(() => nextBuildNumber(['76'], ['77.1'], []));
  assert.throws(() => nextBuildNumber(['9999'], [], []));
});

test('processing succeeds only when all three Apple delivery checks are valid', () => {
  const valid = 'BUILD-STATUS: VALID\nIMPORT-STATUS: VALID\nIS-ON-APP-STORE-CONNECT: true\n';
  parseBuildStatus(valid);
  for (const invalid of [valid.replace('BUILD-STATUS: VALID', 'BUILD-STATUS: PROCESSING'), valid.replace('IMPORT-STATUS: VALID', 'IMPORT-STATUS: INVALID'), valid.replace('true', 'false'), 'Upload succeeded']) {
    assert.throws(() => parseBuildStatus(invalid));
  }
});

test('ASC requests use a valid ES256 signature and never follow pagination to another origin', async () => {
  const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' });
  let calls = 0;
  const request = ascClient({ keyId: 'fixture', issuer: 'fixture-issuer', privateKey: privateKey.export({ type: 'pkcs8', format: 'pem' }) }, async (url, options) => {
    calls++;
    assert.equal(new URL(url).origin, 'https://api.appstoreconnect.apple.com');
    const token = options.headers.Authorization.slice(7);
    const [header, body, signature] = token.split('.');
    assert.equal(JSON.parse(Buffer.from(header, 'base64url')).kid, 'fixture');
    assert.equal(JSON.parse(Buffer.from(body, 'base64url')).aud, 'appstoreconnect-v1');
    assert.ok(verify('sha256', Buffer.from(`${header}.${body}`), { key: publicKey, dsaEncoding: 'ieee-p1363' }, Buffer.from(signature, 'base64url')));
    return { ok: true, status: 200, json: async () => ({ data: [], links: { next: 'https://example.test/v1/builds' } }) };
  });
  await assert.rejects(listAll(request, '/v1/builds'), /origin/);
  assert.equal(calls, 1);
  await assert.rejects(request('https://example.test/v1/apps'), /path/);
  assert.equal(calls, 1);
});

test('TestFlight assigns the existing group and submits only builds needing beta review', async () => {
  for (const state of ['READY_FOR_BETA_SUBMISSION', 'IN_BETA_TESTING']) {
    const mutations = [];
    const request = async (path, options = {}) => {
      if (options.method === 'POST') { mutations.push({ path, data: options.data }); return null; }
      if (path.startsWith('/v1/betaGroups?')) return { data: [{ id: 'external', attributes: { name: 'BEX External', isInternalGroup: false } }] };
      if (path.endsWith('/betaBuildLocalizations')) return { data: [] };
      return { data: { attributes: { externalBuildState: state } } };
    };
    await distributeBuild(request, 'app', 'build', 'BEX External', 'Release changes', 'ja');
    assert.deepEqual(mutations.at(-1), { path: '/v1/betaGroups/external/relationships/builds', data: { data: [{ type: 'builds', id: 'build' }] } });
    assert.equal(mutations.some(call => call.path === '/v1/betaAppReviewSubmissions'), state === 'READY_FOR_BETA_SUBMISSION');
  }
});

test('missing groups and rejected builds cannot be reported as distributed', async () => {
  await assert.rejects(distributeBuild(async () => ({ data: [] }), 'app', 'build', 'missing', 'changes', 'ja'), /missing/);
  const request = async (path, options = {}) => {
    if (options.method === 'POST') return null;
    if (path.startsWith('/v1/betaGroups?')) return { data: [{ id: 'group', attributes: { name: 'external', isInternalGroup: false } }] };
    return { data: { attributes: { externalBuildState: 'BETA_REJECTED' } } };
  };
  await assert.rejects(distributeBuild(request, 'app', 'build', 'external', 'changes', 'ja'), /blocked/);
});


test('Xcode receipts must identify one unambiguous delivery', () => {
  const a = '12345678-1234-1234-1234-123456789abc';
  const b = '12345678-1234-1234-1234-123456789abd';
  assert.equal(deliveryId([JSON.stringify({ deliveryUUID: a }), `Delivery ID: ${a}`]), a);
  assert.throws(() => deliveryId(['Upload succeeded']));
  assert.throws(() => deliveryId([`delivery-id: ${a}`, `deliveryUUID = "${b}"`]));
});
