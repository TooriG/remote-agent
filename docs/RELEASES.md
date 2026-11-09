# Release operations

Develop on `main`. A revision that passes both Portable checks and Native clients
produces a numbered GitHub prerelease, `nightly-N`, containing Mac, Windows,
Android and Linux Host/CLI packages, plus an externally approved iPhone TestFlight
build. Nightly stays in draft while Apple beta review is pending. The scheduled
Production workflow finishes these pending Nightly approvals.

On macOS and Windows, select **Nightly** in Settings → Update channel to receive
these candidates. **Stable** is the default. Returning to Stable waits for a newer
stable build; it never automatically downgrades the installed app.

Stable and Nightly share BEX's existing application-data directory, conversations,
settings and local Host. Desktop debug builds use its `dev` subdirectory and an
isolated Host by default. Run `scripts/dev-env.sh just dev` on macOS to launch that
instance alongside the installed release. `AGENT_STATE_DIR` selects an explicit
state directory; `AGENT_ISOLATED_HOST` explicitly selects Host isolation. Provider
caches and project worktrees keep their existing shared locations.

## iPhone variants

| App | Bundle ID | Distribution |
| --- | --- | --- |
| BEX | `dev.remoteagent.mobile.ios` | Nightly and production TestFlight, and App Store |
| BEX Dev | `dev.remoteagent.mobile.ios.dev` | Local development builds |

Nightly and production use the same BEX app and Apple build. Nightly's external
TestFlight group is **BEX Nightly**:
https://testflight.apple.com/join/xdk6J9qr. The production external group is
**BEX External**: https://testflight.apple.com/join/UJbK9Ww7. The release notes
link to the appropriate group. Both groups require external beta review; only
approved builds are published or promoted. TestFlight replaces the App Store app.

BEX Dev can coexist with BEX. It has separate preferences, snapshots, Keychain
access and URL scheme (`bex-dev`, compared with `bex`). Pair it with the development
Host explicitly. APNs on a Host uses its configured `bundle_id`, which must match
the installed app variant.

To archive the local development app, use the signing-keychain preflight and:

```sh
scripts/dev-env.sh just ios-archive \
  "$PWD/target/release-dev/BexDev.xcarchive" "$PWD/target/release-dev/DerivedData" \
  -xcconfig "$PWD/apps/mobile/iosApp/Config/Dev.xcconfig" \
  DEVELOPMENT_TEAM=K65K9J8686
```

Select Apple Development signing for direct installation. If uploading a Dev
build, use the BEX Dev App Store Connect record (`6820657007`) and a build number
higher than its existing uploads. Pair Dev with the development Host explicitly.

## Stable release

After deciding a Nightly is ready, run **Production release → Run workflow** on
`main`, optionally specifying its `nightly-N` tag. An empty selection uses the
latest published Nightly. The workflow creates `prd-N` at the same source commit,
preserving the build number, every platform package byte for byte, and the exact
Apple build ID. It verifies downloaded checksums before publishing. Do not move
production tags or replace published assets. A failed upload can be retried with
the same selection; an existing published promotion is verified without rewriting it.

Promotion adds the already approved Apple build to **BEX External** and publishes
the production GitHub release immediately. The landing page and Stable updater
then select those promoted packages; the iPhone download remains the production
TestFlight link. Nightly uploads do not enter the production TestFlight group.

Each Nightly reserves a new marketing version beyond all existing App Store and
TestFlight versions, including candidates built before any promotion. The same
promoted Apple build is also submitted to App Review with automatic
release after approval. This separate review does not hold back the production
GitHub release, landing page or external TestFlight. An ongoing App Store review
keeps its selected build; never replace it with a different candidate. If App
Review submission is blocked by an earlier in-flight version, wait for that
version to finish and retry Production with the same Nightly tag. A corrected
Nightly can replace a rejected or otherwise editable App Store version; the
rejected binary itself is never resubmitted. A promotion older than the current
App Store version can still complete on GitHub and TestFlight, but its App Review
step cannot succeed as an update; choose a newer Nightly.

Retry failed build jobs through GitHub Actions. Never move a published tag. For
an App Review rejection, inspect the rejection and create a corrected Nightly.
If Android failed after the other packages were staged, run **Android candidate**
on `main` with the existing draft tag. It rebuilds the same verified source with
the current packaging workflow. After it succeeds, resume package verification
and publication after initializing pinned tools and setting `AGENT_ASC_KEY_ID`,
`AGENT_ASC_ISSUER_ID` and `AGENT_ASC_PRIVATE_KEY_FILE` to the authorized App Store
Connect credential (keep the private key outside the repository and Nix store):

```sh
GITHUB_REPOSITORY=ttizze/remote-agent scripts/dev-env.sh node scripts/release-candidate.ts seal nightly-N
```

A TestFlight rejection leaves its Nightly in draft. Inspect the rejection, remove
the rejected draft release while preserving its tag, and create a corrected
Nightly. Scheduled approval checks remain failed until the rejected draft is
removed so the rejection stays visible.

Android Nightly and Stable use the same distribution key and package ID. Older
APKs signed with another key require reinstallation before entering this channel.

Linux packages include Host and CLI with their shared-library closure. Extract
the archive and run `bash install.sh` to install under `~/.local`. Provide Node.js
and the selected provider CLIs as for a source installation. Updating a running
Host requires its existing update and continuation procedure; replacing a
package alone does not update that process.

For public BEX releases, retain the configured territories (US, Japan and China,
excluding France) and existing encryption declarations in App Store Connect.
