import { execFileSync } from 'node:child_process';
import { X509Certificate } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { ascClient, listAll } from './apple-release.mjs';

const bundleIds = ['dev.remoteagent.mobile.ios', 'dev.remoteagent.mobile.ios.TaskActivity'];

// Reuse profiles for the installed certificate; certificate renewal gets new names.
export async function distributionProfiles(request, serial, now = Date.now()) {
  if (!/^[A-F0-9]+$/.test(serial)) throw new Error('Invalid distribution certificate serial.');
  const certificates = await listAll(request, '/v1/certificates?limit=200');
  const certificate = certificates.find(item => item.attributes.serialNumber.toUpperCase() === serial
    && ['DISTRIBUTION', 'IOS_DISTRIBUTION'].includes(item.attributes.certificateType)
    && Date.parse(item.attributes.expirationDate) > now);
  if (!certificate) throw new Error('The installed distribution certificate is unavailable on App Store Connect.');
  const prefix = `BEX CI ${serial}`;
  const profiles = {};
  for (const identifier of bundleIds) {
    const bundles = await listAll(request, `/v1/bundleIds?filter[identifier]=${identifier}&limit=200`);
    const bundle = bundles.find(item => item.attributes.identifier === identifier);
    if (!bundle) throw new Error(`Registered bundle ${identifier} is missing.`);
    const name = `${prefix} ${identifier}`;
    const candidates = await listAll(request, `/v1/profiles?filter[name]=${encodeURIComponent(name)}&include=bundleId,certificates&limit=200`);
    let profile = candidates.find(item => item.attributes.name === name
      && item.attributes.profileType === 'IOS_APP_STORE' && item.attributes.profileState === 'ACTIVE'
      && Date.parse(item.attributes.expirationDate) > now
      && item.relationships.bundleId.data?.id === bundle.id
      && item.relationships.certificates.data?.some(cert => cert.id === certificate.id));
    if (!profile) {
      // Do not revoke or replace profiles used by other developers or builds.
      profile = (await request('/v1/profiles', { method: 'POST', data: { data: {
        type: 'profiles', attributes: { name, profileType: 'IOS_APP_STORE' },
        relationships: {
          bundleId: { data: { type: 'bundleIds', id: bundle.id } },
          certificates: { data: [{ type: 'certificates', id: certificate.id }] },
        },
      } } })).data;
    }
    if (!/^[A-F0-9]{8}(?:-[A-F0-9]{4}){3}-[A-F0-9]{12}$/i.test(profile.attributes.uuid) || !profile.attributes.profileContent
      || profile.attributes.name !== name || profile.attributes.profileType !== 'IOS_APP_STORE'
      || profile.attributes.profileState !== 'ACTIVE' || !(Date.parse(profile.attributes.expirationDate) > now)) {
      throw new Error(`Apple did not return an active distribution profile for ${identifier}.`);
    }
    profiles[identifier] = profile.attributes;
  }
  return { prefix, profiles };
}

async function main() {
  let pem;
  try {
    pem = execFileSync('/usr/bin/security', ['find-certificate', '-a', '-p', process.env.BEX_CODE_SIGN_KEYCHAIN], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
  } catch { throw new Error('Could not read the dedicated signing certificates.'); }
  const certificates = [...pem.matchAll(/-----BEGIN CERTIFICATE-----[\s\S]+?-----END CERTIFICATE-----/g)]
    .map(match => new X509Certificate(match[0])).filter(cert => /^CN=Apple Distribution:/m.test(cert.subject) && /^OU=K65K9J8686$/m.test(cert.subject));
  if (certificates.length !== 1) throw new Error('The BEX distribution certificate is missing or ambiguous.');
  const request = ascClient({ keyId: process.env.BEX_ASC_KEY_ID, issuer: process.env.BEX_ASC_ISSUER_ID,
    privateKey: readFileSync(process.env.BEX_ASC_PRIVATE_KEY_FILE) });
  const result = await distributionProfiles(request, certificates[0].serialNumber);
  const directory = join(process.env.HOME, 'Library/Developer/Xcode/UserData/Provisioning Profiles');
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const mapping = {};
  for (const [identifier, profile] of Object.entries(result.profiles)) {
    writeFileSync(join(directory, `${profile.uuid}.mobileprovision`), Buffer.from(profile.profileContent, 'base64'), { mode: 0o600 });
    mapping[identifier] = profile.name;
  }
  writeFileSync('target/apple-release/provisioning.json', JSON.stringify({ profiles: mapping }) + '\n');
  // Conditional settings belong in xcconfig: xcodebuild CLI assignments split at the first '='.
  writeFileSync('target/apple-release/signing.xcconfig', [
    'CODE_SIGN_STYLE[sdk=iphoneos*] = Manual',
    'CODE_SIGN_IDENTITY[sdk=iphoneos*] = Apple Distribution',
    'DEVELOPMENT_TEAM[sdk=iphoneos*] = K65K9J8686',
    `PROVISIONING_PROFILE_SPECIFIER[sdk=iphoneos*] = ${result.prefix} $(PRODUCT_BUNDLE_IDENTIFIER)`,
    '',
  ].join('\n'));
  console.log('Installed App Store provisioning profiles for BEX and its activity extension.');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
