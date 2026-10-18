# BEX landing page

Source of the existing BEX landing page at https://getbex.dev/, imported from
its currently deployed static assets so download changes can be reviewed here.

Windows buttons download the published `bex-windows-x86_64-setup.exe` installer directly, including when the GitHub release API is unavailable. Run the EXE to install BEX and all companion Host/CLI/Claude SDK files into the current user's Programs folder and create a Start menu shortcut. No ZIP extraction or Rust compilation is needed. Git, Node.js, the chosen agents and WebView2 are separate runtime prerequisites.

The latest-release API may replace the pinned Windows preview only with an uploaded `bex-windows-x86_64-setup.exe` from the upstream repository. Old ZIP-only releases never override the EXE download. Native clients builds the installer with Inno Setup after Windows tests, retains it as an artifact, and uploads the EXE/checksum on release publication. The ZIP remains an optional diagnostic artifact, not a site download.

For every platform update, **including macOS**, the developer publishing it must keep the corresponding getbex.dev download current in the same release task. Follow [the release checklist](../docs/RELEASING.md): publish and verify the replacement or distribution channel first, update the pinned URL and metadata in `public/download.js`, run `node --test lp/download.test.cjs`, deploy this Worker with `wrangler deploy`, and verify the live https://getbex.dev/download.js and its actual download buttons deliver the latest published version. This applies to all developers, including those who do not normally maintain the site. Do not finish at a local build, PR or GitHub release alone. Keep workers.dev and preview URLs enabled.

Android phones show the Android download action prominently. Android buttons link
directly to the fork's verified Android preview APK release asset,
including when the upstream release API is unavailable. The manual
`android-download.yml` workflow builds the APK before
publishing the APK and SHA-256 checksum. Android 8.0 (API 26) or newer and an
ARM64 or x86-64 device are required.
This is a debuggable preview APK, not a store release. The download workflow now
requires a persistent PKCS#12 signing key in the repository's
`BEX_ANDROID_PREVIEW_KEYSTORE_BASE64` secret; no signing keys are checked in.
The preview key uses alias `androiddebugkey` and password `android`; its private
key must remain secret despite that conventional debug password. The workflow
increments the Android version code using its run number, so later previews from
the same repository/key can update in place. Keep a private backup of this key.
Older previews used disposable keys: replacing one removes its local pairing keys
and drafts, so save unsent drafts first and pair again afterward. PC conversation
history stays on the Host. A production/store release needs its own signing policy.

Check download behavior with `node --test lp/download.test.cjs`.
Preview from this directory with `npx wrangler dev` and deploy with
`npx wrangler deploy`. Use the Cloudflare account that owns `bex-lp` and
`getbex.dev`. Publish and verify the APK asset before deploying a changed Android URL.


## Usage demos

The LP includes three silent ten-second recreated BEX usage demos, with fictional
project data and an English interface. The dark desktop shell follows the supplied
BEX screenshot, including its sidebar, composer and workspace card. These are labeled recreations, not recordings of a real session.
Tabs support arrow keys, Home/End and accessible selected states. Inactive and
out-of-view clips pause. Reduced-motion preferences disable automatic playback;
native video controls remain available. Only the selected clip is loaded.

Reproducible HyperFrames source lives in `demo-source/` (Node 22 and FFmpeg).
Run `npm run check` there, then
`npm run render -- --output ../bex-demo.mp4 --fps 30 --quality standard`.
Split the 30-second render at 0/10/20 seconds into `public/demos/agents.mp4`,
`changes.mp4`, `mobile.mp4` using FFmpeg with libx264, yuv420p, CRF 23 and
`-movflags +faststart`. Posters use full-render timestamps 3/17/27 seconds.
The shipped clips are about 120–170 KB each, 1280×720, H.264, without audio.


The published APK was found to require API 37 after an installation failure was reported.
The replacement has minimum API 26 (Android 8.0), while compile/target SDK remain
37. It supports ARM64 and x86-64. Android 8–16 bypass the Android 17-only local
network permission; Android 17+ keeps the permission check. The download workflow
runs unit tests and Android lint, then checks the packaged minimum SDK and
verifies the APK signature before publishing. The reporting phone can now install
and connect; its follow-up runtime failures were traced to an older running PC Host
and an existing conversation owned by the Codex desktop app.

## Android connection recovery

Build and restart the PC Host and desktop client together when updating the APK.
Replacing source or binaries alone does not update the already running Host.
A mismatched Host can load projects successfully while model-list decoding fails.
Keep pairing credentials when restarting; reinstalling Android is unnecessary for
a PC-only Host update.

Codex permits one app-server writer per conversation. BEX never steals an active
writer or silently forks. If the PC Codex app owns a conversation, choose the
message's **分岐** action to continue in a separate conversation; the unsent draft
is copied while the original draft and conversation remain intact. New mobile
builds present a readable recovery message instead of a raw RPC envelope.

A model usable in the Codex desktop app may still be rejected by the separately
connected app-server account. Choose an available catalog model when that happens;
BEX does not silently substitute another model. The live smoke test completed a
reply on an explicit fork using GPT-6-Astra, while this account rejected
gpt-6.1-sol through the BEX connection.

## Mobile composer

The Android chat list keeps the BEX wordmark, Host selector, connection dot,
search and refresh actions in one header row, including at 320dp width.
There is no workspace heading or separate Host row. Search opens in place;
closing it clears the filter and restores the header controls. Host names
truncate to one line, and the connection dot retains a spoken status label.
The selector uses a small connection dot, PC icon and machine name without a
filled button background or chevron. Hosts default to the machine hostname
(Windows computer name); an explicit `--name` overrides it. Paired Android
clients refresh the saved name when reconnecting, without pairing again.

The microphone button opens the phone's speech recognition activity in the device
language and appends the result to the original draft through the shared core.
Review or edit the text before sending. Cancellation preserves the draft; a phone
without a speech recognition app shows setup guidance. The speech activity owns
microphone access, so BEX does not require a separate recording permission.

The Android UI uses a dark conversation screen and a rounded bottom composer.
Tap the model/effort label to search models and change the provider-advertised
reasoning effort or speed tier. Existing conversations stay on their provider;
new chats can choose either provider. Speed descriptions are shown when available.

The plus button opens Android's multiple-document picker. Picked files upload to
the paired Host through authenticated binary transfers; sending stays disabled
until uploads finish. Attachment cards open image, PDF, text/code and Markdown
previews. Conversation images and existing Markdown file links also open previews.
Binary or oversized text files show an explicit unavailable-preview message.
Draft attachments can be removed without deleting the original phone file.

Conversation website and PR links open Android's browser or matching app. Links
inside tables behave the same way. If no app can handle a link, BEX displays its
URL with a copy action. Workspace file links keep the authenticated BEX preview.


The landing page uses a short hero followed immediately by the tabbed usage demo.
Repeated mock screens, feature catalogues, platform lists, decorative illustrations
and release announcements are omitted. Three short benefits, pairing steps and
downloads provide the remaining hierarchy; detailed specifications live in Docs.
The demo retains a short recreation label. Supporting text is at least 14px,
with body copy at 16px or larger; the video contains its own recorded UI text.

The three benefits and three pairing steps each have a distinct generated UI
illustration in `public/assets/ui/`. These illustrate agent selection, mobile
approval, local execution, opening the app, QR pairing and resuming a conversation.
They use fictional data and are labeled as UI illustrations, not live screenshots.
Images reserve a 3:2 layout box, load lazily and stack with the cards on phones.

The hero and three technical benefits highlight Rust + GPUI rendering, QUIC / iroh
streams and Host-side execution with reconnect synchronization. Sources are
README.ja.md (native clients and Host execution), ADR 0005 (reused presentation
objects, independent request/subscription/blob streams, reconnect reconciliation),
and `crates/agent-transport` (iroh endpoint and stream implementation).
QUIC stream independence is described in RFC 9000 sections 2 and 2.2:
https://www.rfc-editor.org/rfc/rfc9000.html#section-2.2
These implementation and resilience claims do not establish reliability advantages
over Codex or Claude mobile apps. Do not infer a performance ratio from using Rust,
promise uninterrupted networking or claim automatic replay of user input.

## Windows baseline measurements

`public/benchmarks-windows.html` explains the October 18, 2026 Windows baseline.
`public/benchmarks/windows-20261018.json` retains every sample from five BEX/Claude launches
and five Codex reference launches plus three excluded warm-ups, including process-level memory and binary
hashes. The LP's Japanese and English numbers use the same validated medians.

This uses the downloadable BEX Windows preview 6 dev-profile build, Codex package
26.1013.4894.0 and Claude package 2.42304.0.0. App profiles and provider state
directories are isolated, with no project or generation. BEX and Claude are signed
out; Codex automatically restores Windows login despite its isolated profile.
Codex is labeled a signed-in reference, not used for multiplier or superiority
claims. The Claude percentage compares signed-out initial states. BEX includes its Host;
all apps include their discovered descendants. The primary memory metric is USS
(private resident RAM), excluding shared pages and GPU memory. RSS and private
commit are also published to make the choice of metric inspectable.

Window timing is process launch to the first visible OS window, not content
readiness or time to first answer. Five launches follow one excluded warm-up,
using warm OS caches and rotating app order. Memory uses 11 samples at 20–30
seconds, a median for each launch, then a median across valid launches (five each).
The original Codex fifth result is retained. Additional failed diagnostic attempts
and a BEX resume missing its preexisting Host are excluded explicitly; BEX final
valid run restarts its test Host. The
single-PC test retains unrelated background apps and tasks. Do not generalize
these numbers to authenticated coding workloads, mobile clients, all hardware,
or a causal Rust-versus-Electron claim.

Reproduce with Windows, Python 3.10+ and psutil 7.2.2. Provide a JSON array of
`name`, `version`, `executable` objects named BEX, Codex and Claude, then run:

```text
python scripts/benchmark-desktop.py --config config.json --output results --runs 5
python scripts/summarize-desktop-benchmark.py results lp/public/benchmarks/windows-20261018.json
```

Only harness-launched processes are terminated. Never publish profile directories
or credentials. The summarizer requires complete runs, validates sampling times,
process totals and BEX Host inclusion, and refuses incomplete datasets. Update the
LP and report medians/ranges together when replacing a dataset.
