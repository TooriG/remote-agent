# Publishing platform updates / 各プラットフォームの更新公開

Every developer who publishes a BEX update is responsible for keeping that
platform's download on **https://getbex.dev** current. This includes **macOS**,
Windows, Android and iOS. It is part of releasing the update, even when the
developer does not normally work on the website. A build, merge or GitHub Release
alone does not complete publication.

## Release checklist

1. Build and verify the affected client and its companion Host from the same
   revision. Publish the verified artifact or update its distribution channel.
   Use the macOS app bundle archive, the Windows EXE installer with companion
   files, Android APK, or the iOS TestFlight/App Store channel as appropriate.
2. Update the platform entry in [`lp/public/download.js`](../lp/public/download.js),
   including the pinned URL and any changed version, requirements, format or size
   labels. For a stable channel URL such as TestFlight, verify that the new build
   is available through that channel. A moving "latest release" lookup does not
   replace updating and checking the pinned fallback URL.
3. Run `node --test lp/download.test.cjs` from the repository root. Update relevant
   assertions when changing pinned download metadata.
4. Deploy the website with `npx wrangler deploy` from `lp/`, using the Cloudflare
   account that owns `bex-lp` and `getbex.dev`. See [`lp/README.md`](../lp/README.md).
5. Check the live `https://getbex.dev/download.js` and the platform's actual
   download buttons. Follow the resulting link and verify that the downloaded
   artifact matches the published replacement (version/revision and checksum),
   or that the distribution channel offers the new build. Also check the pinned
   fallback when a release API lookup is used.
6. Record the source revision, release/channel URL, verification results and site
   deployment in the PR or release notes. If publishing or deployment is blocked,
   record the blocker and handoff owner; do not report the release as complete.

## 日本語

**macOSを含む全プラットフォームで、更新版を公開した開発者が、同じ公開作業内で
getbex.devの該当ダウンロードも最新版に更新してください。** サイト担当者やWindows
開発者への依頼待ちにせず、各プラットフォームの公開担当者の作業に含めます。

検証済みの成果物・配布チャンネルを公開した後、`lp/public/download.js`の固定URLと
必要な表示情報を更新し、ダウンロードテスト、サイトのデプロイ、公開サイトのボタン
から最新版を取得できることの確認まで行います。macOSも対象です。iOSのようにURLが
変わらない場合も、そのリンク先で新ビルドを取得できることを確認してください。
APIによる最新版検索があっても、固定の代替リンクを古いままにしないでください。

PRまたはリリースノートに、リビジョン・公開先・検証結果・サイトの反映結果を残します。
権限などで公開できない場合は、未完了の内容と引き継ぎ担当を明記してください。
