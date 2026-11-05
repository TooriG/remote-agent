# Release operations

Develop on `main`. A revision that passes both Portable checks and Native clients
produces a numbered GitHub prerelease, `dev-N`. Download its packages from GitHub;
on iPhone, use the existing [BEX TestFlight public link](https://testflight.apple.com/join/UJbK9Ww7).
External TestFlight availability follows Apple's beta review and automatic tester
notification. The GitHub job summary distinguishes submission from availability.

On macOS and Windows, select **Development** in Settings → Update channel to
receive these candidates. **Stable** is the default. Returning to Stable waits
for a newer stable build; it never automatically downgrades the installed app.

After testing a candidate, create the production tag at that exact revision:

```sh
git fetch origin --tags
git tag prd-N dev-N
git push origin prd-N
```

Replace `N` with the chosen candidate number. Production promotion checks the
revision and every package checksum, selects the existing iPhone build for App
Review, and requests release after approval. All desktop, Android and Linux
packages retain the bytes tested in dev. Production downloads remain on the
previous release until App Store distribution is ready; the scheduled Production
release workflow then publishes the draft and advances the stable channel.

Retry a failed promotion from **Production release → Run workflow**, specifying
the same `prd-N` tag. Never move a published tag or replace its packages. If App
Review rejects a build, inspect the rejection and ship a corrected candidate.

An already approved App Store version keeps its selected build. Publish a pending
initial version before promoting the next version. The first new production
release switches the public iPhone download from TestFlight to App Store.
Existing published download fallbacks remain available in the meantime.

Linux x86-64 packages include Host and CLI with their shared-library closure.
Extract the archive and run `bash install.sh` to install under `~/.local` without
restarting an existing Host. Provide Node.js and the selected provider CLIs as
for a source installation. To update a running Host, use its existing update and
continuation procedure; replacing a download alone does not update that process.

For public BEX releases, retain the configured territories (US, Japan and China,
excluding France) and existing encryption declarations in App Store Connect.
