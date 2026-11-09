import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { validateRelease, type ReleaseManifest } from './release-candidate.ts';
import { requestFromEnvironment, compareMarketingVersions, storeBuildSelection, listAll, type AscRequest, type AscResource, type AppleBuild, type StoreVersion } from './apple-release.ts';

const relationship = (type: string, id: string) => ({ data: { type, id } });

async function productionStatus(request: AscRequest, appId: string, versionName: string, buildId: string) {
  const versions = await listAll<StoreVersion>(request, `/v1/apps/${appId}/appStoreVersions?filter[platform]=IOS&limit=200`);
  const version = versions.find(version => version.attributes.versionString === versionName)
    ?? versions.filter(version => storeBuildSelection(version.attributes.appStoreState) !== 'preserve'
      && compareMarketingVersions(version.attributes.versionString, versionName) < 0)
      .sort((a, b) => compareMarketingVersions(b.attributes.versionString, a.attributes.versionString))[0];
  if (!version) return { versionId: null, state: 'NOT_CREATED' };
  const build = (await request<{ data: AppleBuild | null }>(`/v1/appStoreVersions/${version.id}/build`)).data;
  const state = version.attributes.appStoreState;
  const selection = storeBuildSelection(state);
  if (build && build.id !== buildId && selection === 'preserve') throw new Error(`App Store ${versionName} already selects a different build; preserve its review and release it first.`);
  if (build?.id === buildId && selection === 'needs-correction') throw new Error('The rejected App Store build needs a corrected Nightly.');
  if (!build && selection === 'preserve') throw new Error('The App Store version has no selected candidate build.');
  return { versionId: version.id, state };
}

async function updateReleaseNotes(request: AscRequest, versionId: string, locale: string, notes: string) {
  const localizations = await listAll<AscResource<{ locale: string }>>(request, `/v1/appStoreVersions/${versionId}/appStoreVersionLocalizations`);
  if (!localizations.length) {
    await request('/v1/appStoreVersionLocalizations', { method: 'POST', data: { data: {
      type: 'appStoreVersionLocalizations', attributes: { locale, whatsNew: notes },
      relationships: { appStoreVersion: relationship('appStoreVersions', versionId) },
    } } });
  }
  for (const localization of localizations) {
    await request(`/v1/appStoreVersionLocalizations/${localization.id}`, { method: 'PATCH', data: { data: {
      type: 'appStoreVersionLocalizations', id: localization.id, attributes: { whatsNew: notes },
    } } });
  }
}

async function reviewSubmission(request: AscRequest, appId: string, versionId: string) {
  const submissions = await listAll<AscResource<{ state: string }>>(request, `/v1/reviewSubmissions?filter[app]=${appId}&filter[platform]=IOS&limit=200`);
  let empty;
  for (const submission of submissions.filter(item => ['READY_FOR_REVIEW', 'UNRESOLVED_ISSUES'].includes(item.attributes.state))) {
    const items = await listAll<{ id: string; attributes?: { state?: string }; relationships?: { appStoreVersion?: { data?: { id: string } } } }>(request, `/v1/reviewSubmissions/${submission.id}/items?include=appStoreVersion`);
    const matching = items.find(item => item.relationships?.appStoreVersion?.data?.id === versionId);
    if (matching) {
      if (submission.attributes.state === 'UNRESOLVED_ISSUES') {
        if (items.some(item => item.id !== matching.id && item.attributes?.state === 'REJECTED')) throw new Error('Resolve the other rejected review items before resubmitting this build.');
        if (matching.attributes?.state === 'REJECTED') await request(`/v1/reviewSubmissionItems/${matching.id}`, { method: 'PATCH', data: { data: { type: 'reviewSubmissionItems', id: matching.id, attributes: { resolved: true } } } });
      }
      return submission.id;
    }
    if (!items.length && submission.attributes.state === 'READY_FOR_REVIEW') empty = submission;
  }
  const submission = empty || (await request<{ data: { id: string } }>('/v1/reviewSubmissions', { method: 'POST', data: { data: {
    type: 'reviewSubmissions', attributes: { platform: 'IOS' }, relationships: { app: relationship('apps', appId) },
  } } })).data;
  await request('/v1/reviewSubmissionItems', { method: 'POST', data: { data: {
    type: 'reviewSubmissionItems', relationships: {
      reviewSubmission: relationship('reviewSubmissions', submission.id),
      appStoreVersion: relationship('appStoreVersions', versionId),
    },
  } } });
  return submission.id;
}

// A production promotion authorizes this exact Nightly build, including automatic
// release after approval. Never cancel an existing review or replace its build.
export async function submitProduction(request: AscRequest, appId: string, buildId: string, versionName: string, locale: string, notes: string) {
  const status = await productionStatus(request, appId, versionName, buildId);
  if (status.state === 'PENDING_DEVELOPER_RELEASE') {
    await request('/v1/appStoreVersionReleaseRequests', { method: 'POST', data: { data: {
      type: 'appStoreVersionReleaseRequests', relationships: { appStoreVersion: relationship('appStoreVersions', status.versionId!) },
    } } });
    return { state: 'PROCESSING_FOR_DISTRIBUTION' };
  }
  if (status.versionId && storeBuildSelection(status.state) === 'preserve') return { state: status.state };
  const build = (await request<{ data: AppleBuild }>(`/v1/builds/${buildId}`)).data;
  if (build.attributes.processingState !== 'VALID' || build.attributes.expired) throw new Error('The production build is invalid or expired.');
  const app = (await request<{ data: { id: string } }>(`/v1/builds/${buildId}/app`)).data;
  const preRelease = (await request<{ data: AscResource<{ version: string }> }>(`/v1/builds/${buildId}/preReleaseVersion`)).data;
  if (app.id !== appId || preRelease.attributes.version !== versionName) throw new Error('The Apple build does not match this candidate.');
  let versionId = status.versionId;
  if (!versionId) {
    versionId = (await request<{ data: { id: string } }>('/v1/appStoreVersions', { method: 'POST', data: { data: {
      type: 'appStoreVersions', attributes: { platform: 'IOS', versionString: versionName, releaseType: 'AFTER_APPROVAL' },
      relationships: { app: relationship('apps', appId), build: relationship('builds', buildId) },
    } } })).data.id;
  } else {
    await request(`/v1/appStoreVersions/${versionId}`, { method: 'PATCH', data: { data: {
      type: 'appStoreVersions', id: versionId, attributes: { versionString: versionName, releaseType: 'AFTER_APPROVAL' },
      relationships: { build: relationship('builds', buildId) },
    } } });
  }
  await updateReleaseNotes(request, versionId, locale, notes);
  const submissionId = await reviewSubmission(request, appId, versionId);
  await request(`/v1/reviewSubmissions/${submissionId}`, { method: 'PATCH', data: { data: {
    type: 'reviewSubmissions', id: submissionId, attributes: { submitted: true },
  } } });
  return { state: 'WAITING_FOR_REVIEW' };
}

async function main(path: string | undefined) {
  if (!path) throw new Error('Pass the promoted production release.json file.');
  const manifest = JSON.parse(readFileSync(path, 'utf8')) as ReleaseManifest;
  validateRelease(manifest, manifest.tag, manifest.sha);
  if (manifest.channel !== 'stable') throw new Error('Only a promoted production build can be submitted to the App Store.');
  const status = await submitProduction(requestFromEnvironment(), manifest.appId, manifest.buildId!, manifest.versionName, manifest.locale, manifest.notes);
  console.log(`${manifest.tag}: App Store ${status.state}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv[2]).catch(error => { console.error((error as Error).message); process.exitCode = 1; });
}
