# クライアント簡素化計画

作成: 2026-11-07。実施者: Claude（全フェーズ）。完了したらこの文書は削除する。

## 目的

UI 周りの複雑さは個々の関数ではなく、同じ事実を複数の場所で持ち、同じ判断を複数の層で繰り返す構造から来ている。この計画は、その構造を次の原則に揃えることで複雑さを取り除く。

- 一つの事実に所有者は一つ。core が持つ状態をアプリが複製しない。アプリが持つ状態を core が推測しない。
- 表示判断は core の presentation が行う。アプリはウィジェット、OS 連携、描画だけを持つ。
- 変更検出は Arc の同一性と Model の `PartialEq` だけで行う。手書きのフラグ組み合わせは置かない。
- 既存の操作・表示・受け入れ条件は変えない。未リリースなので互換処理や移行処理は残さない。

前回の「クライアント構成の再設計」で、Snapshot のグループ publication（`Changed` マクロ）と Store 側のペイン分割（`open_pane`）は完了している。この計画はその続きで、desktop のビュー所有と core の表示判断を仕上げる。

## 現状（コードで確認済み）

| 問題 | 場所 | 規模 |
| --- | --- | --- |
| 変更検出が 3 層（core フラグ → desktop の手書きゲート → Model `PartialEq`） | `crates/agent-core/src/presentation/view.rs:27`、`apps/desktop/src/app/view.rs:326` | フラグ 13、ゲート 9、ゲートを通らない直接呼び出し 23 |
| desktop が core の presentation の上に別の Model を組む | `conversation_model`、`composer_model`、`sidebar_model`、`header_model`、`workbench_model`、`publish_workspace`、`publish_projects` | 約 400 行 |
| 会話行の投影と展開状態が 3 アプリに重複 | desktop `timeline.rs:291`/`:48`、Android `ConversationProjection.kt:17`・`AndroidConversationScreen.kt:215`、iOS `ConversationHistory.swift:239` | 3 実装 |
| core の状態を影のように複製 | `Desktop.busy`、`Desktop.local_errors`、`Preferences.local_errors`/`window_error`/`snapshot`、`Desktop.connection_*` 4 フィールド | 参照 44 + 29 + α |
| 非同期完了の配管が 3 セット | `Desktop`（`Update` 12 種、`OperationCompletion` 9 種）、`Preferences`（`Completion`）、`Hosts`（`Update` 8 種） | `receive` 100 行、`operation_completed` 80 行 |
| サイドチャットが 2 個目の `Desktop` | `SurfaceResource::SideChat(Entity<Desktop>)`、`Mode`、`side_chat_mode` | 分岐 27 か所 |
| ペインをスナップショット差し替えで表現 | `crates/agent-core/src/state/panes.rs:21` `for_pane` | 呼び出し 24、reducer の `.pane` 参照 174 |
| 日本語整形済み文字列から英語を逆引き | `crates/agent-core/src/localization.rs:118` | テンプレート逆パース + 改行再帰 |
| 入力サイズで線形走査と索引を切り替える二重実装 | `client.rs:144`、`operations.rs:538`、`lists.rs` 6 か所、`list.rs:439/782`、`tree.rs:374/535`、host の `titles.rs:10`、`codex/native.rs:532`、`claude/native.rs:21`、`history.rs:743` | 約 12 か所 |
| protocol 型の uniffi ミラー | `crates/agent-core/src/bindings/protocol.rs` | 111 型、900 行 |
| 端末設定の三者マージ | `crates/agent-core/src/persistence.rs:130`、`client/persistence.rs:227` | baseline 追跡 |
| Host サービスの横断ロックと `matches!` 分類 | `crates/host-daemon/src/host_rpc/service.rs:47-62`、`:262-283` | ロック類 5、分類リスト 2 |
| 同一内容の platform ファイル | `apps/desktop/src/platform/macos.rs` と `linux.rs` | 5 行 |

## 目標構造

### core

- `Snapshot` はグループ構成のまま。ペインは `BTreeMap<PaneId, PaneState>` として保持し、「現在ペイン」フィールドと `for_pane` の差し替えはなくす。reducer と presentation は `pane_id` を引数に取る。
- `UiLanguage` は端末設定として Store が持つ。presentation は `language.format(key, args)` で最初から表示言語の文字列を作る。逆引き翻訳は削除する。
- `PaneState` が会話の展開状態（活動の展開、項目の展開）を持つ。`RenderedConversation` は展開状態を適用した「表示する行」を返す。
- 各画面の presentation（conversation、composer、sidebar、workbench、settings）が、現在 desktop が snapshot から導いているフィールドをすべて持つ。アプリ側の Model に残るのは window ローカルな値だけ。
- ローカル操作の失敗は Intent で core に通知し、`snapshot.error_notices(scope)` を唯一の通知源にする。

### desktop

- `Desktop` を Window と Pane の 2 種類のエンティティに分ける。Window は sidebar、header、通知、設定、ペイン一覧を持つ。Pane は conversation、composer、workbench、workspace を持ち、Store の pane presentation を購読する。サイドチャットは Window 内の 2 個目の Pane。
- Tokio から GPUI への橋は「pane の presentation が変わった」の 1 本だけ。操作完了で desktop が触るのは `DraftText::saved` と dictation の状態だけ。
- 各サブビューは Pane の presentation と window ローカル状態から Model を作り、`PartialEq` で再描画を判定する。publish 側のゲートは持たない。

### mobile

- core の「表示する行」をそのまま描く。行の絞り込み、`unchanged` 判定、展開状態の保持はアプリから消える。
- 言語は snapshot から読む。アプリ独自の言語状態は消える。

## フェーズ

各フェーズは 1 ブランチで実施し、`scripts/dev-env.sh cargo nextest run --locked -p <crate>` と `scripts/dev-env.sh just complexity` を通した後、統合前に `scripts/dev-env.sh just unit-tests` を 1 回実行する。分岐や状態遷移を変えたフェーズは対象を絞って cargo-mutants を回す。統合は毎回報告して承認を得てから行う。

### Phase 0: 独立した即時修正

依存なし。他フェーズと並行可。

- 線形走査と索引の切り替えを索引版だけにする（上表の約 12 か所）。外部入力に対するバイト上限（`64 * 1024` など）は正当な制限なので対象外。
- `Receipt::join` の `MAX_WAITERS` を削除する（`crates/agent-core/src/store.rs:28`）。
- `platform/macos.rs` と `linux.rs` を `#[cfg(unix)]` の 1 ファイルにする。
- `service.rs:117` の `account_request` を `Call → Body` の 1 回変換にする。

受け入れ: 既存テストがそのまま通る。削除した分岐のテストも削除する。

### Phase 1: desktop の publish ゲートと影の状態を消す

依存なし。Phase 2 以降の土台。

- `publish_presentation` を削除し、`accept_snapshot` は常に `publish_views` を呼ぶ。`PresentationChanges` のうち desktop が使っていたフラグは、mobile が使うもの以外を削除する。
- `Desktop.busy` を Store 経由の操作（送信、fork、ファイル保存、エディタで開く）から外し、`snapshot.operation_running` を見る。core の `OperationKey` に足りないキーがあれば追加する。ローカル処理（フォルダ選択、添付、プレビュー取得）だけを `local_busy` として残す。
- `Desktop.local_errors` と `Preferences.local_errors`/`window_error` を、core への `Intent::RaiseNotice { scope, message }` に置き換える。`errors::error_rows` は `snapshot.error_notices` だけを返す。
- `Preferences.snapshot` と `sync_from_pane` を削除し、Preferences は `publish_views` から渡される presentation だけを受け取る。
- `Desktop.connection_list`/`connection_names`/`connected_hosts` を削除し、`sidebar_model` が `Hosts` から直接導く。
- `details`/`details_popover` を `Details { Hidden, Docked, Popover }` の 1 つにする。

受け入れ: `view.rs` の `cache_tests`（再描画回数の検証）がそのまま通る。`just complexity` の上限内。desktop の挙動差なし。

### Phase 2: 言語を状態に持たせる

Phase 3 の前提。core が整形する文字列が増える前に入れる。

- `UiLanguage` を Store の端末設定にする。`Intent::SetLanguage` で変更し、`DevicePreferences` に保存する。Snapshot に `language` を持つグループを足す（既存グループに同居させない。言語変更時に presentation 全体を再投影するため）。
- presentation の整形はすべて `language.format` 経由にする。`localized_ui_text` の逆引き（テンプレート順位付け、引数復元、改行分割）を削除する。`localized_ui_format` は残す。
- desktop の `Language` グローバルと `observe_global::<Language>` 8 か所、Android の `language` 状態、iOS の相当物を snapshot からの読み取りに置き換える。アプリ自身の文言（メニュー、ツールチップ）は引き続き `language.text` を使う。

受け入れ: 言語切替で全画面が切り替わる。英語表示で誤一致由来の表記が出ない。`every_catalog_entry_has_both_languages` を維持する。

### Phase 3: 表示判断を core に移す

Phase 2 の後。

- `PaneState` に `expanded_activities: BTreeMap<ActivityId, Status>` と `expanded_items: BTreeSet<ItemId>` を追加し、`Intent::SetActivityExpanded`/`SetItemExpanded` で更新する。
- `RenderedConversation` に「表示する行」を追加する。折りたたみ中の活動行の除外、`PendingRequest` の除外、`InProgress` の重複除外、`last` 印をここで行う。desktop の `conversation_rows`/`ConversationRow::unchanged`、Android の `ConversationProjection`、iOS の `ConversationHistory` の絞り込みを削除する。行の同一性判定は Arc の同一性だけにする。
- `conversation_model`/`composer_model`/`sidebar_model`/`workbench_model`/`publish_workspace`/`publish_projects` の snapshot 由来フィールドを core の presentation に移す。desktop の Model に残すのは、dictation の phase と levels、`Hosts` エンティティ、`review_expanded`、補完インデックス、ウィジェットのエンティティ、フォーカスハンドルだけ。
- `ComposerModel.empty` はウィジェットの値ではなく core の draft から判定する。

受け入れ: 3 アプリで同じ会話が同じ行を出す。`timeline.rs` のテストは core 側に移す。cargo-mutants で行の絞り込みと展開を検査する。

### Phase 4: ペインを core と desktop の両側で一次元にする

Phase 1、3 の後。

core:

- `Snapshot.pane`/`pane_id` を削除し、`panes: BTreeMap<PaneId, Arc<PaneState>>` だけにする。reducer の `.pane` 参照 174 か所を `panes[pane_id]` に変える。
- `Snapshot::for_pane`、`PaneStates::same`、`Scheduled::new`/`finish`/`Scheduler::admit` の差し替えを削除する。
- `Presentation` は `pane_id` を受けて投影する。`observe` は pane ごとに 1 本。

desktop:

- `Desktop` を `Window` と `Pane` に分ける。`Window` は sidebar、header、通知、`Preferences`、`Hosts`、`Updates`、ペイン一覧。`Pane` は conversation、composer、workbench、workspace、selection、media、dictation、ファイル編集。
- `Mode`、`side_chat_mode`、`SurfaceResource::SideChat(Entity<Desktop>)` を削除し、サイドチャットは `Entity<Pane>` にする。`#[cfg(test)] Fixture` は `Window::fixture` のテスト専用コンストラクタにする。
- `hosts`/`app_updates`/`sidebar_view` の `Option` を外す。

受け入れ: サイドチャットの開閉、ペイン間のナビゲーション、ペインを閉じたときの購読解除が既存テストどおり。`view.rs` の `cache_tests` を Window/Pane 構成に書き直す。

### Phase 5: 完了配管を 1 本にする

Phase 4 の後。

- `Update` と `OperationCompletion` を削除する。Tokio 側は Store の receipt を await し、GPUI 側に `Box<dyn FnOnce(&mut Pane, &mut Window, &mut Context<Pane>)>` を 1 本の channel で渡す。古い結果の無効化は `epoch` ではなく、クロージャが対象の pane と draft key を照合して捨てる。
- 完了時に desktop が触るのは `DraftText::saved`（composer、question、editor）と dictation の状態だけ。それ以外は presentation の購読で反映される。
- `Preferences` と `Hosts` の channel/`Completion`/`Update` も同じ仕組みに揃える。

受け入れ: `receive`/`operation_completed` が消える。`snapshot_bursts_adopt_the_latest_state_once_per_frame` などの既存テストが通る。

### Phase 6: core と Host の周辺

Phase 0〜5 と独立。判断待ちの項目を含む。

- 【判断待ち】`agent-protocol` に optional feature `bindings` を追加し、型に `cfg_attr(derive(uniffi::Record))` を付けて `bindings/protocol.rs` の 900 行を削除する。AGENTS.md の「protocol はネイティブバインディングに依存しない」を「既定ビルドで依存しない」に改める必要がある。
- `DevicePreferences` を設定の唯一の書き手にし、モデル選択の Intent はここに直接書く。Store は最新値を購読する。`merge_model_preferences` と baseline 追跡を削除する。
- `Call` の隣に `fn class(&self) -> CallClass` を置き、`service.rs` の `matches!` 2 本を置き換える。`worktree_access` と `permission_settings_access` はそれぞれの資源の所有者に移す。

### Phase 7: ツールの集約（任意）

最後。UI とは無関係なので、他フェーズ完了後に改めて要否を確認する。

- `scripts/*.ts` と `scripts/*.test.mjs` のうち CI・リリース補助を `crates/xtask` に移し、bash は環境準備だけに限定する。`dev-env.test.mjs` は削除する。

## 判断をお願いしたい点

1. Phase 6 の protocol feature 追加（AGENTS.md の規則変更）。
2. `presentation/files/icons.rs` と `mobile_icons.rs` の統一。T3 準拠の意図的な差異なら対象外にする。
3. Phase 7 を実施するか。

## 進め方

- フェーズごとにブランチを切り、完了ごとに差分と検証結果を報告し、承認後に統合する。
- Phase 0 と Phase 1 は今すぐ始められる。Phase 2 → 3 → 4 → 5 は順序依存。Phase 6 は並行可。
- 各フェーズで削除対象を列挙し、置き換えた実装・不要になった状態・テストを同じブランチで消す。後回しにしない。
- 完了したフェーズの記述はこの文書から消し、全フェーズ完了時に文書を削除する。
