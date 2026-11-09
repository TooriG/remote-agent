import { appendFileSync, existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { createPrivateKey, sign } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';

export type AscRequest = <T>(path: string, options?: { method?: string; data?: unknown }) => Promise<T>;
export type AscResource<A> = { id: string; attributes: A };
export type AppleBuild = AscResource<{ version: string; processingState: string; expired: boolean }>;
export type StoreVersion = AscResource<{ versionString: string; appStoreState: string; platform: string }>;
export type AppleBuildPlan = { channel: 'nightly' | 'stable'; sourceTag?: string; bundleId: string; appId: string;
  version: string; versionName: string; tag: string; sha: string; locale: string; notes: string;
  deliveryId?: string; buildId?: string; testflightUrl?: string };

export function nextBuildNumber(sourceVersions: readonly (string | undefined)[], appleVersions: readonly string[], releaseTags: readonly string[]) {
  const source = new Set(sourceVersions);
  if (source.size !== 1 || !sourceVersions.length) throw new Error('Apple source build numbers must agree.');
  const versions = [...source, ...appleVersions, ...releaseTags.flatMap(tag => /^(?:nightly|prd)-(\d+)$/.exec(tag)?.slice(1) ?? [])];
  if (versions.some(version => !/^[1-9]\d*$/.test(version ?? ''))) throw new Error('Apple build numbers must be positive integers.');
  const next = Math.max(...versions.map(Number)) + 1;
  if (!Number.isSafeInteger(next) || next > 9999) throw new Error('The Apple build number needs a new numbering range.');
  return String(next);
}

// Every uploaded Nightly reserves its marketing version, so a later candidate
// can reach App Review unchanged even after an earlier candidate is released.
export function nextMarketingVersion(source: string | undefined, reservedVersions: readonly string[]) {
  let next = marketingVersionParts(source).join('.');
  for (const version of reservedVersions) {
    if (compareMarketingVersions(next, version) <= 0) {
      const [major, minor, patch] = marketingVersionParts(version);
      next = `${major}.${minor}.${patch + 1}`;
    }
  }
  marketingVersionParts(next);
  return next;
}

function marketingVersionParts(value: string | undefined) {
  if (!/^\d+\.\d+(?:\.\d+)?$/.test(value ?? '')) throw new Error('Invalid marketing version.');
  const parts = value!.split('.').map(Number).concat([0]).slice(0, 3);
  if (parts.some(part => !Number.isSafeInteger(part))) throw new Error('Invalid marketing version.');
  return parts;
}

export function compareMarketingVersions(left: string, right: string) {
  const a = marketingVersionParts(left), b = marketingVersionParts(right);
  return a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
}

export function storeBuildSelection(state: string) {
  if (['REJECTED', 'METADATA_REJECTED', 'DEVELOPER_REJECTED', 'INVALID_BINARY'].includes(state)) return 'needs-correction';
  return ['PREPARE_FOR_SUBMISSION', 'READY_FOR_REVIEW'].includes(state) ? 'editable' : 'preserve';
}

export const testflightGroups = {
  nightly: { name: 'BEX Nightly', url: 'https://testflight.apple.com/join/xdk6J9qr' },
  production: { name: 'BEX External', url: 'https://testflight.apple.com/join/UJbK9Ww7' },
} as const;
export const iosDistribution = { bundleId: 'dev.remoteagent.mobile.ios', profileType: 'IOS_APP_STORE', method: 'app-store-connect', product: 'Bex' } as const;

export function externalTestingAvailable(state: string | undefined) {
  return state === 'BETA_APPROVED' || state === 'IN_BETA_TESTING';
}

const required = (name: string) => { const value = process.env[name]; if (!value) throw new Error(`${name} is required.`); return value; };

export function requestFromEnvironment() {
  return ascClient({ keyId: required('AGENT_ASC_KEY_ID'), issuer: required('AGENT_ASC_ISSUER_ID'),
    privateKey: readFileSync(required('AGENT_ASC_PRIVATE_KEY_FILE')) });
}

export function parseBuildStatus(output: string) {
  const status = (key: string) => new RegExp(`^\\s*=?\\s*${key}:\\s*(\\S+)`, 'mi').exec(output)?.[1];
  if (status('BUILD-STATUS') !== 'VALID' || status('IMPORT-STATUS') !== 'VALID' || status('IS-ON-APP-STORE-CONNECT') !== 'true') {
    throw new Error('Apple has not confirmed a VALID imported build on App Store Connect.');
  }
}

export function deliveryId(logs: readonly string[]) {
  const ids = new Set(logs.flatMap(log => [...log.matchAll(/delivery[-_ ]?(?:uuid|id)["']?\s*[:=]\s*["']?([a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12})/gi)].map(match => match[1].toLowerCase())));
  if (ids.size !== 1) throw new Error('The Xcode upload must have exactly one delivery receipt.');
  return [...ids][0];
}

export function ascClient({ keyId, issuer, privateKey }: { keyId: string; issuer: string; privateKey: string | Buffer }, fetcher = fetch): AscRequest {
  const key = createPrivateKey(privateKey);
  return async <T>(path: string, { method = 'GET', data }: { method?: string; data?: unknown } = {}): Promise<T> => {
    if (!path.startsWith('/v1/') || path.includes('://')) throw new Error('Invalid App Store Connect API path.');
    const encode = (value: unknown) => Buffer.from(JSON.stringify(value)).toString('base64url');
    const now = Math.floor(Date.now() / 1000);
    const payload = `${encode({ alg: 'ES256', kid: keyId, typ: 'JWT' })}.${encode({ iss: issuer, iat: now, exp: now + 600, aud: 'appstoreconnect-v1' })}`;
    const signature = sign('sha256', Buffer.from(payload), { key, dsaEncoding: 'ieee-p1363' }).toString('base64url');
    const response = await fetcher(`https://api.appstoreconnect.apple.com${path}`, {
      method, headers: { Authorization: `Bearer ${payload}.${signature}`, 'Content-Type': 'application/json' },
      ...(data === undefined ? {} : { body: JSON.stringify(data) }), signal: AbortSignal.timeout(60_000),
    });
    if (!response.ok) throw Object.assign(new Error(`App Store Connect ${method} ${path.split('?')[0]} failed (HTTP ${response.status}).`), { status: response.status });
    return (response.status === 204 ? null : await response.json()) as T;
  };
}

export async function listAll<T>(request: AscRequest, path: string): Promise<T[]> {
  const result: T[] = [];
  while (path) {
    const page: { data: T[]; links?: { next?: string | null } } = await request(path);
    result.push(...page.data);
    const next = page.links?.next;
    if (!next) break;
    const url = new URL(next);
    if (url.origin !== 'https://api.appstoreconnect.apple.com') throw new Error('Unexpected App Store Connect pagination origin.');
    path = url.pathname + url.search;
  }
  return result;
}

export async function appForBundleId(request: AscRequest, bundleId: string) {
  // ASC's bundle filter also returns apps whose identifiers have this prefix.
  const apps = await listAll<AscResource<{ bundleId: string; primaryLocale: string }>>(request, `/v1/apps?filter[bundleId]=${encodeURIComponent(bundleId)}`);
  const app = apps.find(app => app.attributes.bundleId === bundleId);
  if (!app) throw new Error(`App ${bundleId} is missing from App Store Connect.`);
  return app;
}

export async function distributeBuild(request: AscRequest, appId: string, buildId: string, groupName: string, notes: string, locale: string) {
  const groups = await listAll<AscResource<{ name: string; isInternalGroup: boolean; publicLinkEnabled?: boolean; publicLink?: string }>>(request, `/v1/betaGroups?filter[app]=${appId}`);
  const group = groups.find(group => group.attributes.name === groupName && !group.attributes.isInternalGroup);
  if (!group) throw new Error(`External TestFlight group ${groupName} is missing.`);
  const url = group.attributes.publicLinkEnabled ? group.attributes.publicLink : undefined;
  const detail = await request<{ data: AscResource<{ externalBuildState?: string; autoNotifyEnabled: boolean }> }>(`/v1/builds/${buildId}/buildBetaDetail`);
  const state = detail.data.attributes.externalBuildState;
  if (!state || ['BETA_REJECTED', 'EXPIRED', 'PROCESSING_EXCEPTION', 'MISSING_EXPORT_COMPLIANCE'].includes(state)) throw new Error(`TestFlight distribution is blocked (${state ?? 'unknown state'}).`);
  // Keep automatic notification enabled until beta review approves the build.
  // Approved and active builds are already externally available; changing their
  // notification setting is unnecessary and can conflict with Apple's API.
  const desiredAutoNotify = externalTestingAvailable(state) ? undefined : true;
  if (desiredAutoNotify !== undefined && detail.data.attributes.autoNotifyEnabled !== desiredAutoNotify) {
    await request(`/v1/buildBetaDetails/${detail.data.id}`, { method: 'PATCH', data: {
      data: { type: 'buildBetaDetails', id: detail.data.id, attributes: { autoNotifyEnabled: desiredAutoNotify } },
    } });
  }
  const localized = (await request<{ data: AscResource<{ locale: string; whatsNew: string }>[] }>(`/v1/builds/${buildId}/betaBuildLocalizations`)).data;
  const existing = localized.find(info => info.attributes.locale === locale);
  if (existing) {
    if (existing.attributes.whatsNew !== notes) await request(`/v1/betaBuildLocalizations/${existing.id}`, { method: 'PATCH', data: { data: { type: 'betaBuildLocalizations', id: existing.id, attributes: { whatsNew: notes } } } });
  } else {
    await request('/v1/betaBuildLocalizations', { method: 'POST', data: { data: { type: 'betaBuildLocalizations', attributes: { locale, whatsNew: notes }, relationships: { build: { data: { type: 'builds', id: buildId } } } } } });
  }
  await request(`/v1/betaGroups/${group.id}/relationships/builds`, { method: 'POST', data: { data: [{ type: 'builds', id: buildId }] } });
  const submit = state === 'READY_FOR_BETA_SUBMISSION';
  if (submit) {
    try {
      await request('/v1/betaAppReviewSubmissions', { method: 'POST', data: { data: { type: 'betaAppReviewSubmissions', relationships: { build: { data: { type: 'builds', id: buildId } } } } } });
    } catch (error) {
      // Apple limits concurrent beta reviews and submissions per day. Keep
      // the candidate in draft so the scheduled approval job can retry.
      if ((error as Error & { status?: number }).status === 409 || (error as Error & { status?: number }).status === 429) return { state: 'Waiting for TestFlight review capacity', url };
      throw error;
    }
  }
  return { state: submit ? 'Submitted for TestFlight beta review' : state, url };
}

async function main(command: string | undefined) {
  const planPath = 'target/apple-release/plan.json';
  if (command === 'record-delivery') {
    const directory = process.argv[3];
    const marker = process.argv[4];
    if (!directory || !marker) throw new Error('record-delivery requires the native temporary directory and upload start marker.');
    const since = statSync(marker).mtimeMs;
    const plan = JSON.parse(readFileSync(planPath, 'utf8')) as AppleBuildPlan;
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
  const keyId = required('AGENT_ASC_KEY_ID');
  const issuer = required('AGENT_ASC_ISSUER_ID');
  const keyPath = required('AGENT_ASC_PRIVATE_KEY_FILE');
  const request = requestFromEnvironment();
  if (command === 'plan') {
    const app = await appForBundleId(request, iosDistribution.bundleId);
    const builds = await listAll<AppleBuild>(request, `/v1/builds?filter[app]=${app.id}&limit=200`);
    const source = readFileSync('apps/mobile/iosApp/Bex.xcodeproj/project.pbxproj', 'utf8');
    const versions = [...source.matchAll(/CURRENT_PROJECT_VERSION = (\d+);/g)].map(match => match[1]);
    const mac = readFileSync('apps/nsh/macos/Info.plist', 'utf8');
    const macVersion = /<key>CFBundleVersion<\/key>\s*<string>(\d+)<\/string>/.exec(mac)?.[1];
    const releases = JSON.parse(readFileSync('target/apple-release/releases.json', 'utf8')) as { tagName: string }[];
    const version = nextBuildNumber([...versions, macVersion], builds.map(build => build.attributes.version), releases.map(release => release.tagName));
    const storeVersions = await listAll<StoreVersion>(request, `/v1/apps/${app.id}/appStoreVersions?limit=200`);
    const sourceVersion = /MARKETING_VERSION = ([\d.]+);/.exec(source)?.[1];
    const betaVersions = await listAll<AscResource<{ version: string }>>(request, `/v1/preReleaseVersions?filter[app]=${app.id}&filter[platform]=IOS&limit=200`);
    const reservedVersions = [...storeVersions.filter(version => version.attributes.platform === 'IOS').map(version => version.attributes.versionString),
      ...betaVersions.map(version => version.attributes.version)];
    const versionName = nextMarketingVersion(sourceVersion, reservedVersions);
    const plan = { channel: 'nightly', bundleId: iosDistribution.bundleId, appId: app.id,
      version, versionName, tag: `nightly-${version}`,
      sha: required('AGENT_RELEASE_SHA'), locale: app.attributes.primaryLocale,
      notes: execFileSync('git', ['log', '-1', '--format=%s'], { encoding: 'utf8' }).trim() };
    writeFileSync(planPath, JSON.stringify(plan) + '\n');
    appendFileSync(required('GITHUB_ENV'), `AGENT_BUILD_NUMBER=${version}\nAGENT_RELEASE_TAG=${plan.tag}\nAGENT_MARKETING_VERSION=${versionName}\n`);
    console.log(`Apple release build ${version} planned for ${plan.sha}.`);
  } else if (command === 'verify') {
    const plan = JSON.parse(readFileSync(planPath, 'utf8')) as AppleBuildPlan;
    let output;
    try {
      if (!plan.deliveryId) throw new Error('Missing delivery receipt.');
      output = execFileSync('xcrun', ['altool', '--build-status', '--delivery-id', plan.deliveryId,
        '--wait', '--apiKey', keyId, '--apiIssuer', issuer, '--p8-file-path', keyPath],
      { encoding: 'utf8', timeout: 30 * 60_000, stdio: ['ignore', 'pipe', 'pipe'] });
    } catch { throw new Error('Apple build processing did not complete successfully.'); }
    parseBuildStatus(output);
    const builds = (await request<{ data: AppleBuild[] }>(`/v1/builds?filter[app]=${plan.appId}&filter[version]=${plan.version}`)).data;
    const build = builds.find(build => build.attributes.version === plan.version && build.attributes.processingState === 'VALID');
    if (!build) throw new Error('The processed BEX build could not be verified.');
    plan.buildId = build.id;
    const distribution = await distributeBuild(request, plan.appId, build.id, testflightGroups.nightly.name, plan.notes, plan.locale);
    if (distribution.url !== testflightGroups.nightly.url) throw new Error('Nightly requires an enabled public TestFlight link.');
    plan.testflightUrl = distribution.url;
    writeFileSync('target/apple-release/apple-build.json', JSON.stringify(plan) + '\n');
    console.log(`TestFlight ${plan.version}: ${distribution.state}.`);
    appendFileSync(required('GITHUB_STEP_SUMMARY'), `TestFlight build **${plan.version}**: ${distribution.state}.\n\n${plan.testflightUrl}\n\nSource: \`${plan.sha}\`.\n`);
  } else throw new Error('Use apple-release.ts plan, record-delivery or verify.');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv[2]).catch(error => { console.error((error as Error).message); process.exitCode = 1; });
}
