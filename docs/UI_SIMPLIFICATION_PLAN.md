# クライアント簡素化計画

既存の操作・表示を維持し、重複した判断と不要な同期経路を取り除く。実施者はこの会話のエージェント。完了した項目は文書から除き、全体の完了時に削除する。

## 判断の基準

- 可変な事実の所有者を一つにする。共有する不変 Snapshot や導出済み presentation の参照は、それだけでは状態の二重管理ではない。
- 共通の判断は core、ウィジェット・フォーカス・IME・OS 操作・ネイティブ資源の寿命は各アプリが持つ。単にファイルや型を移して層を増やさない。
- 必要な値を明示して純粋な投影に渡す。設定の保存や接続などの副作用は資源の所有者が行う。
- Arc の同一性と `PartialEq` で導出結果を再利用する。フラグ・モデル・channel・フィールドの数を減らすこと自体を受け入れ条件にしない。
- テストが検証する製品の動作と、現在の実装方法を数える検査を区別する。後者を理由に既存構造を固定せず、期待する動作・性能を確かめる検査に置き換える。実装を通すためだけに期待値を緩めない。

## 先に維持すべき契約

- 入力・ストリーミング・アニメーションで無関係な画面を再描画しない。会話の長さに比例する投影を入力や無関係な通知のたびに実行しない。
- `view.rs::cache_tests` の描画回数・通知回数の検査を維持する。`projections` は現状では `set_model` / `sync` の呼び出し回数であり、実際の計算量ではない。その回数だけを性能要件として扱わない。
- native 入力の未保存 revision、IME の編集中テキスト、送信直前の内容を保持する。core の保存済み draft と入力ウィジェットの値が一時的に異なることは正常。
- 同じ draft key の Host 切替、再接続、ペインの閉鎖、遅れた完了通知でも、新しい入力・エラー・録音状態を古い結果で上書きしない。
- 会話行の順序、繰り返す native ID、完了時の操作ボタン、fork 先、時刻・進捗・最終行の更新を維持する。
- ドッキング表示の設定と一時的な details popover は別の事実として保持する。画面幅の変更で一方の設定を失わない。
- 接続前・接続失敗時にも設定と言語・エラー表示を利用できる。複数ウィンドウ・複数 Host の設定更新を失わない。

## 段階 1: 表示モデルの入力と所有者を揃える

対象: `apps/desktop/src/app.rs`、`app/preferences.rs`、`app/connections.rs`、`app/view/` と `crates/agent-core/src/presentation/`。

1. desktop の各 Model が参照する値を調べ、共通判断と native ローカル値を分ける。共通判断は既存の `SettingsPresentation`、`ComposerPresentation`、会話 context などへ追加し、呼び出し側の計算を同じ変更で削除する。並行する独自の presentation 層は増やさない。追加する入力は投影の再利用判定にも反映する。現在の `SettingsPresentation::project` の早期 return は workspace 設定・worktrees・cwd を見ないため、これらを移す際は条件とテストも同時に更新する。
2. `Preferences` の呼び出し側を presentation に移した後で `snapshot` と `DisplayInputs` を削除する。単に Snapshot の複製フィールドへ置き換えない。設定完了と publication の前後関係を検証してから `sync_from_pane` の独自読み直しをなくす。再接続は、渡す presentation に含めた connection epoch と既存の bound pane の同一性から判定する。削除前と同様に polling / saving / removing / save_pending をリセットし、古い完了を拒否する。これは段階 6 まで延期しない。
3. `connection_list` / `connection_names` / `connected_hosts` は既存の `Hosts` を入力として sidebar に渡し、Desktop の再構築用状態を除く。合成した一覧の不要な再計算も避ける。
4. `busy` の使用箇所を操作単位で置き換える。core が知る操作は受付中・待機中・実行中を含む既存状態から導く。OS ダイアログや複数操作にまたがる native ワークフローはローカルで所有し、汎用の二重カウンターを残さない。
5. core の操作エラーは引き続き core が所有し、OS 操作や接続準備など native のローカル失敗はウィンドウの通知所有者に集める。`Preferences` の別保存と `window_error` の再合流をなくし、通知の発生をウィンドウへ渡す。共通の ErrorNotice を使って描画し、発生元・Host / ペイン / 画面の scope・通知 ID から適切な所有者へ dismiss を返す。native の失敗を Store に入れるだけの Intent は追加しない。接続前にも表示でき、同じ文面や遅れた dismiss で別の通知を消さないようにし、ペイン閉鎖など対応する scope の終了時に破棄する。

publish ゲートもこの段階で見直す。純粋な Model の再構築が安価なら `PartialEq` に任せ、重い共通投影は core の Arc を再利用する。`publish_views` には `sync_surfaces` など資源操作も含まれるので、無条件呼び出しで監視・接続・録音停止などの副作用が繰り返されないことを確認する。不要なゲートと、その利用者がなくなった変更フラグを削除する。

受け入れ:

- 各 Model の共通判断に実装が一つだけあり、snapshot と presentation の両方を参照する過渡実装を残さない。
- 入力・ストリーミング時の描画・通知の検査を維持し、重い投影の再利用は core の `RenderedConversation` と表示行の `Arc::ptr_eq`、必要な行だけの再構築・再計測で検証する。単なるモデル受け渡し回数と区別する。
- presentation に追加した入力ごとに、他の入力を固定してその値だけを変えるテストを用意する。workspace 設定・worktrees・cwd の単独変更を取りこぼさず、無関係な更新では元の Arc を再利用する。
- 同じ Pane で再接続が起きた場合も、アカウント polling 中・worktree 保存中のフラグが戻り、再接続前の完了が状態を変更しない。
- 接続失敗、ローカル操作失敗、遅れた dismiss、操作の連打、設定更新失敗と再試行が既存どおり動く。

## 段階 2: 会話の表示行を共通化する

対象: core の `presentation/conversation.rs`、desktop の `timeline.rs`、Android の `ConversationProjection` / 会話画面、iOS の `ConversationHistory` / `ConversationPresentation` / `ConversationItem`。

1. 活動行の折りたたみ、要求・進捗行の表示、最終行の判定を core の純粋な投影にまとめる。要求・進捗の配置方針を明示的な入力にし、OS 名による分岐は core に持ち込まない。現在の配置は以下を保持する。

   | クライアント | 要求行 | InProgress 行 |
   | --- | --- | --- |
   | Android | 履歴内で表示し、その場で回答できる | 履歴内の停止ボタンとして表示する |
   | iOS | composer dock で扱うため履歴から除外 | composer で扱うため履歴から除外 |
   | desktop | composer dock で扱うため履歴から除外 | 直前に実際に表示した行が進行中の活動 header のときだけ除外 |

   同じ入力と配置方針から同じ行を得る。native 固有の配置と仮想化は保持する。
2. 展開状態は各アプリの native ペインが所有し、不変の入力として core の純粋な行投影に明示的に渡す。前回の投影結果も渡し、core が行の Arc を再利用する。`Snapshot` のペイン状態や Store Intent は追加しない。展開の判断規則は core にまとめる。活動の status 変更時の判断は共通化する。会話切替時は iOS / Android のリセットと desktop の保持という現在の寿命を各 native 所有者で維持し、別会話や繰り返す ID を混同しないキーを使う。ペイン閉鎖時は破棄する。この段階の共通化対象は活動行を表示するかという展開であり、項目本文・詳細表示の展開は native に保持する。
3. 行の再利用判定を core にまとめる。本文の Arc だけでなく、timestamp、最後の行か、操作ボタン、fork 先、進捗など描画に影響する値を含めて再利用を決める。その結果の Arc 同一性を native が利用する。
4. desktop / Android / iOS の置き換えられたフィルター・内容比較と、統一された展開状態のほかに残る古い状態を同じ変更で削除する。native に残るのはペインごとの展開値の所有・入力の受け渡し・描画であり、同じ展開判断を各言語で再実装しない。

この段階は既存の表示内容の投影を移すもので、新しい表示文言を追加しない。言語移動は前提にせず、新しい文言の整形が必要になった場合は段階 3 で扱う。

受け入れ:

- 同じ入力・展開状態・配置方針から同じ意味・順序の行が得られる。Android の要求カードから回答できることと停止ボタンの配置を維持し、iOS / desktop に不要な要求行を追加しない。
- `timeline.rs` の行投影・完了時の操作更新のテストを core の振る舞いの検査として移す。desktop には仮想化と native 描画の検査を残す。
- 同一本文のまま完了した場合の操作更新、繰り返す ID、折りたたみ中の更新、別ペインの独立性を検証し、対象を絞った mutation 検査を行う。desktop の InProgress は、折りたたみ中には直前に残る進行中 header と重複して隠れ、展開中に活動行が間に表示されれば残る両方を検証する。
- iOS の accessibility の `items=` は従来どおりフィルター前の source rows から数え、表示行数へ置き換えない。元データの件数と表示行を区別し、Pagination の検査もこの契約を保持する。

## 段階 3: 端末設定の単一書き手と言語を揃える

対象: core の `client/persistence.rs`、`persistence.rs`、`localization.rs`、各 native の設定アダプター。

1. 既存の `DevicePreferences` を、設定を復元・更新・保存・購読する所有者にする。各アプリは Host 接続の前に、端末専用のストレージアダプターを渡してこの所有者を初期化する。Host / window ごとの `ClientStorage` で遅延初期化する現在の方式から切り離し、Host 固有の LocalState の保存と区別する。各アプリの起動経路で一つの所有者を生成し、Host セッション側にはそのインスタンスを注入する。`ClientSessions` と desktop の `Connections` がそれぞれ使用する経路を更新し、後から別の既定インスタンスを作って購読先を分裂させない。Store の reducer に I/O を持たせず、明示的な設定変更を所有者へ渡す。
2. すべての設定変更 Intent と各 Store の購読を移した後で、`merge_model_preferences` とセッション別 baseline を削除する。保存失敗後の未保存状態と再試行に必要な情報は残す。
3. モデル・effort・tier の独立した更新、別モデルへの変更、複数ウィンドウの編集順序を既存の `client/persistence` テストどおり保持する。単純な設定全体の上書きに置き換えない。
4. 言語も同じ端末単位の所有者に持たせ、接続前と全 Store に反映する。presentation は明示された言語でキーと引数から整形する。最初のウィンドウ・composition より前に端末設定の最初の読込を完了し、既定言語が一瞬表示される変更を避ける。保存形式・既定値・起動直後の適用を各アプリで揃える。保存失敗時の契約は設定種別ごとに維持する。モデル / editor 設定は適用済みの値を未保存として保持して再試行する。desktop の言語変更は保存成功後にだけ表示へ適用し、保存失敗時は元の言語を保ってエラーを通知する。ユーザーの再選択で再試行できるようにする。mobile は現在のネイティブ設定ストレージの保存・通知契約を保持し、UI が扱わない保存失敗を勝手に新設しない。復元失敗で元のファイルを既定値で上書きしない。モデル / editor 設定の破損時は従来どおり Host の復元を失敗させる。言語だけの破損は従来どおり既定言語で表示し、Host 接続を妨げない。設定種別ごとの失敗・復旧の契約を保つ。後から Host セッションが開いても、復元済み・変更済みの端末設定をそのセッションの値で上書きしない。
5. GPUI のウィジェット locale など native 側に必要な読み取り用アダプターは残せる。別の設定保存や独立した書き手は残さない。
6. 言語を含め、追加した入力をすべて presentation の早期 return / equivalent に反映する。言語だけを変更した場合の再投影と、未変更の言語での再利用をテストする。呼び出し元をすべて更新してから、整形済み日本語のテンプレート順位付け・引数復元・改行再帰を削除する。ユーザー文、ファイル名、外部エラーなどの引数を再翻訳しない。

受け入れ:

- 現在の同時編集・逆順終了・保存失敗からの復旧テストが通る。
- 接続前の復元・変更、保存失敗後の再試行、後発 Host に上書きされないことを検証する。desktop の言語保存に失敗しても表示言語が変わらず、再選択して保存できた時点で切り替わることを検証する。接続前の設定画面、同時に開いた画面、再起動後、送受信中の言語変更が反映される。
- カタログの両言語検査と、引数をそのまま保持する翻訳テストを維持する。

## 段階 4: core のペイン差し替えをなくす

この段階では desktop のエンティティ構成を同時に変更しない。

- `Snapshot.pane` と他ペインの map を一つの map に揃え、reducer・operation・presentation に対象ペイン ID を明示する。`for_pane` と差し替え・復元の配管を削除する。
- Host 共通の資源更新とペインの表示更新を区別する。閉じたペインへの結果で map を添字アクセスして panic したり、ペインを復活させたりしない。
- 主ペイン `""` は map に必ず残し、閉鎖できない契約を保つ。閉じたペインへの結果を root や別ペインの表示更新へフォールバックさせない。遅れた完了でも必要な Host 資源の更新と receipt の解決を行い、別ペインの購読や入力は保持する。
- 更新していないペインの Arc と presentation を再利用する。単に全ペインを毎回投影する構造に置き換えない。

受け入れ: 既存の pane / scheduler / subscription / stale-result テストを維持し、ペイン閉鎖中の作成・保存・ファイル読込、共通会話の購読、再接続を検証する。

## 段階 5: desktop のウィンドウと会話ペインを分離する

段階 1・2 の共通 presentation を利用する。core の正規化とは別の変更として検証する。

- sidebar、接続一覧、アプリ更新と設定画面のワークフローはウィンドウが所有する。conversation、composer とその draft に依存するモデル選択・quick controls、workbench、selection、media、dictation、ファイル編集は会話ペインが所有する。現在の `Preferences` に混在する設定画面と composer の責務を分離する。
- ウィンドウの設定画面は明示的な Host / 設定対象に固定して bind し、会話ペインのフォーカス移動だけでは bind し直さない。アカウント操作の開始時の draft など必要な発生元を保持する。各 composer のモデル選択は自身のペインの presentation を読み、自身の draft に書く。native の `account.busy` による無効化は操作の発生元ペインに紐づく workflow を入力にし、主ペインの操作中という理由だけでサイドチャットを無効化する Host 全体の busy は追加しない。core の共有 account login 状態による無効化は既存どおり保持する。
- サイドチャットも同じ会話ペインを使い、2 個目の Desktop と不要な `side_chat_mode` 分岐を削除する。
- header のような選択中ペインに依存する表示は、その参照先を明示する。フォーカス、ショートカット、引用・説明、表示中ペインの切替を保持する。
- テスト fixture は `#[cfg(test)]` の専用コンストラクターから背景の Hosts / 更新監視を開始しない依存を注入する。side-chat mode を兼用して起動を抑止する構造は除く。型や `Option` をなくすために実際の任意性まで消さない。

受け入れ: サイドチャットの開閉、別ペイン間の入力・ナビゲーション・設定、非表示時の録音停止、閉鎖時の購読・資源解放と既存の描画キャッシュ検査が通る。worktree 保存中・account polling 中に主ペインとサイドチャットのフォーカスを切り替えても、未保存の入力と処理のフラグを失わず完了を受け取る。サイドチャットのモデル選択が主ペインの draft を読み書きしない。

## 段階 6: native の非同期完了を必要なものに絞る

段階 5 の所有者に揃え、GPUI の既存の task / entity 更新機構を優先する。

- Store の結果は presentation の購読で表示する。完了通知がその内容を再配信するだけなら削除する。
- 入力 revision の確認、録音、OS ダイアログ、接続・設定保存など、native の状態更新に必要な完了は所有者で扱う。
- enum の代わりに boxed closure を独自 channel に流す設計を前提にしない。型消去、Send 制約、別のイベントループを新たに増やさない。
- epoch を削除すること自体を目的にしない。Host / 接続 / ペイン / draft / operation generation / 入力 revision のうち必要な識別を保ち、別 Host の同じ draft key や再接続前の結果を拒否する。
- `Preferences` と `Hosts` はそれぞれの資源の寿命を持つ。すべてを一つの channel にまとめるための中継層を作らない。

受け入れ: 古い完了が新しい入力を変更しない検査と、snapshot のバーストをフレーム単位に集約する検査、接続失敗・保存失敗・閉鎖後の完了の検査が通る。

## 段階 7: iOS の view model を会話ペインと Host 一覧に分ける

対象: `apps/mobile/iosApp/Bex/AppViewModel.swift`、`AppViewConnection.swift`、`AppViewActions.swift`、`ConversationModel.swift`、`ConversationPresentation.swift`、`AppPresentationData.swift`、`WorkspaceScreen.swift`、`ConversationSideChat.swift` と core の `client/session.rs`、`client/session/hosts.rs`。

現状（確認済み）:

- `AppViewModel` と `ConversationModel` が同じ `Presentation` を別々に `changes(previous:)` で差分し、`publication` / `snapshot` / `settings` / `isConnected` を両方に複製している（`AppViewModel.swift:248`、`ConversationModel.swift:96`）。
- `ConversationModel` への publish 経路が 3 つある。`AppViewModel` の Host 購読からの `chat?.publish`、`perform` 直後と receipt 後の再 publish、サイドチャット用 pane の購読 Task。`AppViewModel.perform` も receipt 後に再 publish する。段階 6 の desktop と同じ「完了通知による再配信」。
- 主会話は session レベルの presentation、サイドチャットは `AgentPane` を使うため、`pane.map { dispatch } ?? session.dispatch`、`currentPresentation()`、`pane?.prepareDictation() ?? store?.prepareDictation()` の二重経路がある。core の `ClientSession` は主 pane を公開していない（desktop は `store.primary_pane()` を使う）。
- `ConversationPresentation.project`、`ConversationItem` の再利用判定、`recentConversations`（3 件）、detached Task の投影と `ProjectionInput` の鍵照合は、段階 2 の core 表示行で置き換わる iOS 側の実装。
- ワークベンチの `panels`、`migrateWorkbench`（`WorkspaceScreen.swift:69`）、`workbenchSideChats`、`workbenchTerminalHandle` を `ConversationModel` が所有し、draft → session の rekey を desktop `app.rs:1036` と同じ規則で再実装している。
- `notice: String?` が `AppViewModel` と `ConversationModel` にあり、代入が 35 か所。接続失敗・一覧失敗・永続化失敗・操作失敗の catch-all で、`threadLoadState == .failed` の判定にも使われる。desktop の `local_errors` と同じ影の通知経路。
- `connections` / `observations` / `connectionStates` / `pairingConnection` の Task 辞書と、`connect(_:afterForeground:)` の再接続方針（`AppViewConnection.swift:20-56`）は Android `App.kt:393` と同じ内容。`HostConnectionState` は snapshot から導ける値のキャッシュ。
- `HostProfile`（id / name / ticket）を UserDefaults に保存し、publish ごとに `updateProfile` で name / ticket を snapshot から書き戻す。Android も同じ。core の `ClientSessions` は Host ごとのセッション map を既に持つ。desktop は local Host の `management.remotes` を使うので対象外。
- `ConversationTextDiagnostics.swift`（120 行）は Dev 構成限定の計測コードで、`ConversationTextLayout.swift` と UI テスト fixture から参照されている。

手順:

1. `ClientSession` に主 pane を公開し、`ConversationModel` は常に `AgentPane` を持つ。session を直接 dispatch する経路と `??` の fallback を削除し、pane の購読 Task を唯一の publish 経路にする。
2. `Presentation` の差分と複製を、`ConversationModel`（会話ペインの表示）と `AppViewModel`（Host 一覧・接続・設定・ナビゲーション）で重ならないよう分ける。`settings` / `isConnected` / `snapshot` は一方だけが持つ。`perform` 直後と receipt 後の再 publish は、入力の即時反映に必要なもの（`BufferedTextInput` が既に保持）を確認してから削除する。段階 6 と同じ基準。
3. 段階 2 の core 表示行を使い、`ConversationPresentation.project`、`ConversationItem` の再利用判定、`recentConversations`、detached 投影を削除する。native に残るのは展開値の所有、`ConversationMarkdownContent.Part` へのレイアウト分割、`ConversationList` の仮想化。
4. ワークベンチの `Panels` 状態と draft → session の rekey は desktop と iOS が同じ規則を持つので、所有者を一つにする。段階 4・5 と同じ変更で、ペインの presentation に含めるか、core の純粋関数に両アプリから渡すかを決め、両アプリの rekey 実装を削除する。
5. `notice` を段階 1 の通知モデルに揃える。接続失敗（Host 単位）、一覧失敗、永続化失敗、操作失敗をそれぞれの scope の通知にし、`threadLoadState` は一覧操作の状態から導く。
6. Host プロファイルの保存と再接続方針（起動時・ネットワーク復帰・フォアグラウンド復帰・接続有効化）を core の `ClientSessions` に移す。iOS と Android は保存アダプターと UI だけを持つ。`HostConnectionState` は `sessions.session(host).snapshot()` から都度導く。
7. `ConversationTextDiagnostics` は、Native clients の夜間実行で参照している計測かを UI テスト fixture から確認し、不要なら Dev 構成の条件ごと削除する。

受け入れ:

- 既存の XCUITest（`SynchronizationUITests`、`HistoryUITests`、`NavigationUITests` ほか）が Native clients CI で通る。
- 会話を開く・戻る・サイドチャット・フォアグラウンド復帰・再接続で、入力中の draft と質問回答が保持される。
- 同じ `Presentation` を 2 回差分しない。`ConversationModel` の observed プロパティは会話ペインの表示だけが使う。
- Android に同じ構造（`AndroidAppModel` の `connect` / `publish` / `HostProfile`）があるので、6 の core 移動は Android も同じ変更で切り替える。

## この計画に含めないもの

- iOS の `TaskLiveActivities` の token 管理と ActivityKit 連携、`ConversationList` の UIKit 仮想化、`ConversationTextLayout` のテキスト計測は OS 連携なので native に残す。

- `account_request` の `AccountCommand` / `AccountReply` は provider の業務境界なので維持する。RPC の Call / Body を provider に渡して変換数だけを減らさない。
- protocol の optional uniffi feature は追加しない。ネイティブバインディングに依存しない現行の規則を維持する。
- desktop / mobile のアイコン表は T3 の別カタログに由来する意図的な差異なので統一しない。
- Host 全体の操作分類は実行・認可・更新中の資源保護を含む。単一の CallClass を protocol に持ち込んだり、横断的な排他を個別資源へ動かしたりする変更は、UI 簡素化と分離する。
- CI・リリース補助ツールの言語統一と `dev-env.test.mjs` の削除は行わない。環境準備の契約を検証するテストは、使用言語を理由に削除しない。

## 実施と検証

- 現在の作業ツリーで、各段階を独立して検証・統合できる変更に分ける。他セッションの変更を保持する。
- 各段階で置き換えられた実装と全利用者を確認し、不要な状態・転送・重複を削除する。削除後に影響先を再確認し、不要なものがなくなるまで繰り返す。
- 変更中は `scripts/dev-env.sh cargo nextest run --locked -p <crate>` と `scripts/dev-env.sh just complexity` を実行する。Host の検証に必要な supervisor と SDK は既定のビルド手順で用意する。
- 分岐・境界・状態遷移の変更は対象を絞った cargo-mutants で検査する。無関係な mutant の追及や、mutant 用の回帰シード保存は行わない。
- 統合前に `scripts/dev-env.sh just unit-tests` を一度実行し、指定の統合前レビューを通す。共有 Rust の実行は設定済みビルダーを使う。
- main への統合・push はリポジトリの手順に従う。Apple のビルド・受け入れ確認は Native clients CI が担当し、ローカルの重い QA は追加しない。
- 完全な検証の報告には、対象コミットの Portable checks、対象コミットを含む main の Native clients、作業ツリーの状態を確認する。
