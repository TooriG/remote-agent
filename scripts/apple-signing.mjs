import { execFileSync } from 'node:child_process';
import { appendFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(process.env.RUNNER_TEMP, 'agent-apple-signing');
const chain = join(root, 'BEX.keychain-db');
const security = (...args) => {
  try { return execFileSync('/usr/bin/security', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim(); }
  catch { throw new Error(`Signing keychain operation ${args[0]} failed.`); }
};
try {
  if (process.argv[2] === 'prepare') {
    for (const name of ['AGENT_SIGNING_KEYCHAIN_BASE64', 'AGENT_SIGNING_KEYCHAIN_PASSWORD', 'AGENT_ASC_PRIVATE_KEY', 'AGENT_ASC_KEY_ID', 'AGENT_ASC_ISSUER_ID']) {
      if (!process.env[name]) throw new Error(`GitHub secret ${name} is required.`);
    }
    mkdirSync(root, { mode: 0o700 });
    const previous = security('list-keychains', '-d', 'user').split('\n').map(line => line.trim().replace(/^"|"$/g, '')).filter(Boolean);
    writeFileSync(join(root, 'previous.json'), JSON.stringify(previous), { mode: 0o600 });
    writeFileSync(chain, Buffer.from(process.env.AGENT_SIGNING_KEYCHAIN_BASE64, 'base64'), { mode: 0o600 });
    security('unlock-keychain', '-p', process.env.AGENT_SIGNING_KEYCHAIN_PASSWORD, chain);
    security('set-keychain-settings', '-lut', '21600', chain);
    security('set-key-partition-list', '-S', 'apple-tool:,apple:,codesign:', '-s', '-k', process.env.AGENT_SIGNING_KEYCHAIN_PASSWORD, chain);
    security('list-keychains', '-d', 'user', '-s', chain, ...previous);
    const identities = security('find-identity', '-v', '-p', 'codesigning', chain);
    const developerIds = [...identities.matchAll(/([A-F0-9]{40}) "Developer ID Application:[^\n]+\(K65K9J8686\)"/g)];
    if (developerIds.length !== 1 || !identities.includes('Apple Distribution:') || !identities.includes('(K65K9J8686)')) throw new Error('BEX distribution signing identities are missing or ambiguous.');
    const keyPath = join(root, 'AuthKey.p8');
    writeFileSync(keyPath, process.env.AGENT_ASC_PRIVATE_KEY, { mode: 0o600 });
    appendFileSync(process.env.GITHUB_ENV, `AGENT_CODE_SIGN_KEYCHAIN=${chain}\nBEX_CODE_SIGN_IDENTITY=${developerIds[0][1]}\nBEX_ASC_PRIVATE_KEY_FILE=${keyPath}\n`);
  } else if (process.argv[2] === 'cleanup') {
    let previous;
    try { previous = JSON.parse(readFileSync(join(root, 'previous.json'), 'utf8')); } catch { previous = null; }
    if (previous) security('list-keychains', '-d', 'user', '-s', ...previous);
    rmSync(root, { recursive: true, force: true });
  } else throw new Error('Use apple-signing.mjs prepare or cleanup.');
} catch (error) { console.error(error.message); process.exitCode = 1; }
