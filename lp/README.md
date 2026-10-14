# BEX landing page

Source of the existing BEX landing page at https://getbex.dev/, imported from
its currently deployed static assets so download changes can be reviewed here.

Windows buttons use the public GitHub latest-release API. When it contains the
uploaded `bex-windows-x86_64-release.zip` asset, the hero buttons, platform menu,
and download card link directly to that asset without visiting a release page.
Before publication or if the API is unavailable, the source installation link
remains usable and Windows is not described as coming soon.

Native clients publishes the ZIP after Windows tests on a release publication.
The checksum is attached alongside it. Publishing a release from a tag predating
this workflow change will not produce the new asset; use a tag containing it.

Android phones show the Android download action prominently. The same release
lookup selects `bex-android-preview.apk` after Android acceptance succeeds and
the workflow attaches it. This is the tested debug-signed preview APK, explicitly
labeled as a preview, not a production-signed/store release. GitHub-hosted builds
generate a new debug signing key; switching between preview builds may require
uninstalling the old app (which removes its local pairing keys and drafts).
Production updates need a stable maintainer-owned signing key before this preview
distribution can become a normal update channel. No signing keys are checked in.

Check download behavior with `node --test lp/download.test.cjs`.
Preview from this directory with `npx wrangler dev` and deploy with
`npx wrangler deploy`. Use the Cloudflare account that owns `bex-lp` and
`getbex.dev`. This PR does not deploy the LP or create a product release.
