import { appendFileSync, existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { createPrivateKey, sign } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';

export function nextBuildNumber(sourceVersions, appleVersions, releaseTags) {
  const source = new Set(sourceVersions);
  if (source.size !== 1 || !sourceVersions.length) throw new Error('Apple source build numbers must agree.');
  const versions = [...source, ...appleVersions, ...releaseTags.flatMap(tag => /^prd-(\d+)$/.exec(tag)?.slice(1) ?? [])];
  if (versions.some(version => !/^[1-9]\d*$/.test(version))) throw new Error('Apple build numbers must be positive integers.');
  const next = Math.max(...versions.map(Number)) + 1;
  if (!Number.isSafeInteger(next) || next > 9999) throw new Error('The Apple build number needs a new numbering range.');
  return String(next);
}

export function parseBuildStatus(output) {
  const status = key => new RegExp(`^\\s*=?\\s*${key}:\\s*(\\S+)`, 'mi').exec(output)?.[1];
  if (status('BUILD-STATUS') !== 'VALID' || status('IMPORT-STATUS') !== 'VALID' || status('IS-ON-APP-STORE-CONNECT') !== 'true') {
    throw new Error('Apple has not confirmed a VALID imported build on App Store Connect.');
  }
}

export function deliveryId(logs) {
  const ids = new Set(logs.flatMap(log => [...log.matchAll(/delivery[-_ ]?(?:uuid|id)["']?\s*[:=]\s*["']?([a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12})/gi)].map(match => match[1].toLowerCase())));
  if (ids.size !== 1) throw new Error('The Xcode upload must have exactly one delivery receipt.');
  return [...ids][0];
}

export function ascClient({ keyId, issuer, privateKey }, fetcher = fetch) {
  const key = createPrivateKey(privateKey);
  return async (path, { method = 'GET', data } = {}) => {
    if (!path.startsWith('/v1/') || path.includes('://')) throw new Error('Invalid App Store Connect API path.');
    const encode = value => Buffer.from(JSON.stringify(value)).toString('base64url');
    const now = Math.floor(Date.now() / 1000);
    const payload = `${encode({ alg: 'ES256', kid: keyId, typ: 'JWT' })}.${encode({ iss: issuer, iat: now, exp: now + 600, aud: 'appstoreconnect-v1' })}`;
    const signature = sign('sha256', Buffer.from(payload), { key, dsaEncoding: 'ieee-p1363' }).toString('base64url');
    const response = await fetcher(`https://api.appstoreconnect.apple.com${path}`, {
      method, headers: { Authorization: `Bearer ${payload}.${signature}`, 'Content-Type': 'application/json' },
      ...(data === undefined ? {} : { body: JSON.stringify(data) }), signal: AbortSignal.timeout(60_000),
    });
    if (!response.ok) throw new Error(`App Store Connect ${method} ${path.split('?')[0]} failed (HTTP ${response.status}).`);
    return response.status === 204 ? null : response.json();
  };
}

export async function listAll(request, path) {
  const result = [];
  while (path) {
    const page = await request(path);
    result.push(...page.data);
    const next = page.links?.next;
    if (!next) break;
    const url = new URL(next);
    if (url.origin !== 'https://api.appstoreconnect.apple.com') throw new Error('Unexpected App Store Connect pagination origin.');
    path = url.pathname + url.search;
  }
  return result;
}

export async function distributeBuild(request, appId, buildId, groupName, notes, locale) {
  const groups = await listAll(request, `/v1/betaGroups?filter[app]=${appId}`);
  const group = groups.find(group => group.attributes.name === groupName);
  if (!group) throw new Error(`TestFlight group ${groupName} is missing.`);
  const detail = await request(`/v1/builds/${buildId}/buildBetaDetail`);
  const state = detail.data.attributes[group.attributes.isInternalGroup ? 'internalBuildState' : 'externalBuildState'];
  if (!state || ['BETA_REJECTED', 'EXPIRED', 'PROCESSING_EXCEPTION', 'MISSING_EXPORT_COMPLIANCE'].includes(state)) throw new Error(`TestFlight distribution is blocked (${state ?? 'unknown state'}).`);
  const localized = (await request(`/v1/builds/${buildId}/betaBuildLocalizations`)).data;
  const existing = localized.find(info => info.attributes.locale === locale);
  if (existing) {
    await request(`/v1/betaBuildLocalizations/${existing.id}`, { method: 'PATCH', data: { data: { type: 'betaBuildLocalizations', id: existing.id, attributes: { whatsNew: notes } } } });
  } else {
    await request('/v1/betaBuildLocalizations', { method: 'POST', data: { data: { type: 'betaBuildLocalizations', attributes: { locale, whatsNew: notes }, relationships: { build: { data: { type: 'builds', id: buildId } } } } } });
  }
  const submit = !group.attributes.isInternalGroup && state === 'READY_FOR_BETA_SUBMISSION';
  if (submit) {
    await request('/v1/betaAppReviewSubmissions', { method: 'POST', data: { data: { type: 'betaAppReviewSubmissions', relationships: { build: { data: { type: 'builds', id: buildId } } } } } });
  }
  if (!group.attributes.hasAccessToAllBuilds) {
    await request(`/v1/betaGroups/${group.id}/relationships/builds`, { method: 'POST', data: { data: [{ type: 'builds', id: buildId }] } });
  }
  return submit ? 'Submitted for TestFlight beta review' : state;
}

async function main(command) {
  const required = name => { const value = process.env[name]; if (!value) throw new Error(`${name} is required.`); return value; };
  const planPath = 'target/apple-release/plan.json';
  if (command === 'record-delivery') {
    const directory = process.argv[3];
    const marker = process.argv[4];
    if (!directory || !marker) throw new Error('record-delivery requires the native temporary directory and upload start marker.');
    const since = statSync(marker).mtimeMs;
    const plan = JSON.parse(readFileSync(planPath, 'utf8'));
    // Inspect only Xcode-owned bundles, without entering protected system folders.
    const paths = readdirSync(directory, { withFileTypes: true })
      .filter(entry => entry.isDirectory() && entry.name.endsWith('.xcdistributionlogs'))
      .map(entry => join(directory, entry.name, 'ContentDelivery.log'))
      .filter(path => existsSync(path) && statSync(path).mtimeMs > since);
    plan.deliveryId = deliveryId(paths.map(path => readFileSync(path, 'utf8')));
    writeFileSync(planPath, JSON.stringify(plan) + '\n');
    console.log('Xcode delivery receipt recorded.');
    return;
  }
  const keyId = required('BEX_ASC_KEY_ID');
  const issuer = required('BEX_ASC_ISSUER_ID');
  const keyPath = required('BEX_ASC_PRIVATE_KEY_FILE');
  const request = ascClient({ keyId, issuer, privateKey: readFileSync(keyPath) });
  if (command === 'plan') {
    const app = (await request('/v1/apps?filter[bundleId]=dev.remoteagent.mobile.ios')).data[0];
    if (!app) throw new Error('BEX is missing from App Store Connect.');
    const builds = await listAll(request, `/v1/builds?filter[app]=${app.id}&limit=200`);
    const source = readFileSync('apps/mobile/iosApp/Bex.xcodeproj/project.pbxproj', 'utf8');
    const versions = [...source.matchAll(/CURRENT_PROJECT_VERSION = (\d+);/g)].map(match => match[1]);
    const mac = readFileSync('apps/desktop/macos/Info.plist', 'utf8');
    const macVersion = /<key>CFBundleVersion<\/key>\s*<string>(\d+)<\/string>/.exec(mac)?.[1];
    const releases = JSON.parse(readFileSync('target/apple-release/releases.json', 'utf8'));
    const version = nextBuildNumber([...versions, macVersion], builds.map(build => build.attributes.version), releases.map(release => release.tagName));
    const plan = { appId: app.id, version, tag: `prd-${version}`, sha: required('BEX_RELEASE_SHA'), locale: app.attributes.primaryLocale, notes: execFileSync('git', ['log', '-1', '--format=%s'], { encoding: 'utf8' }).trim() };
    writeFileSync(planPath, JSON.stringify(plan) + '\n');
    appendFileSync(required('GITHUB_ENV'), `BEX_BUILD_NUMBER=${version}\nBEX_RELEASE_TAG=${plan.tag}\n`);
    console.log(`Apple release build ${version} planned for ${plan.sha}.`);
  } else if (command === 'verify') {
    const plan = JSON.parse(readFileSync(planPath, 'utf8'));
    let output;
    try {
      if (!plan.deliveryId) throw new Error('Missing delivery receipt.');
      output = execFileSync('xcrun', ['altool', '--build-status', '--delivery-id', plan.deliveryId,
        '--wait', '--apiKey', keyId, '--apiIssuer', issuer, '--p8-file-path', keyPath],
      { encoding: 'utf8', timeout: 30 * 60_000, stdio: ['ignore', 'pipe', 'pipe'] });
    } catch { throw new Error('Apple build processing did not complete successfully.'); }
    parseBuildStatus(output);
    const builds = (await request(`/v1/builds?filter[app]=${plan.appId}&filter[version]=${plan.version}`)).data;
    const build = builds.find(build => build.attributes.version === plan.version && build.attributes.processingState === 'VALID');
    if (!build) throw new Error('The processed BEX build could not be verified.');
    const distribution = await distributeBuild(request, plan.appId, build.id, process.env.BEX_TESTFLIGHT_GROUP || 'BEX External', plan.notes, plan.locale);
    console.log(`TestFlight ${plan.version}: ${distribution}.`);
    appendFileSync(required('GITHUB_STEP_SUMMARY'), `TestFlight build **${plan.version}**: ${distribution}.\n\nSource: \`${plan.sha}\`.\n`);
  } else throw new Error('Use apple-release.mjs plan, record-delivery or verify.');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv[2]).catch(error => { console.error(error.message); process.exitCode = 1; });
}
