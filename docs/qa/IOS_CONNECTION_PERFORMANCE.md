# iPhone 接続のパフォーマンス計測

対象は接続・復帰から初回一覧まで。Core、Host、iOS の境界を同じ試行に対応づける。
Host とクライアントを同じリビジョンでビルドする。iPhone 側にコピー・共有・計測開始の操作を求めない。

## 記録と回収

Core が単調時計による数値イベントを最大768件のメモリ内リングへ記録する。
記録は接続開始と接続成功から各30秒。失敗・キャンセル・アプリ状態変更は期間外でも残す。
上限を超えた件数は `dropped` に残り、解析で欠落を0msに置き換えない。
接続成功後31秒でスナップショットを確定し、各キャプチャ1要求・10秒の期限で Host へ送る。
新しい接続試行は前の回収を取り消す。切断・終了も保留中の送信を取り消すが、端末の未送信記録は保持する。
解析結果の `attempts` は同じ回収に含まれる全試行を列挙する。`--attempt ID` で過去の試行を選べる。
iOS / Android は接続先ごとのアプリ専用ディレクトリにも数値イベントを保存する。
接続開始・成功・失敗・キャンセル・アプリ状態変更時と、計測中の250ms周期でスナップショットを専用workerへ渡す。
workerは最新の未保存スナップショットへまとめ、原子的に置換する。接続処理はディスクI/Oを待たない。
1キャプチャ256 KiB、最大16キャプチャ（4 MiB、置換中の一時ファイルを除く）。古いものから削除する。
再起動後も同じ接続先への接続が成功すれば、31秒後の回収で未送信分も送る。
Hostから成功応答を受け取った範囲だけ削除し、送信失敗・切断・キャンセル時は残す。
送信中に追加されたイベントは削除しない。応答喪失時には重複送信され得るためtraceとsequenceで対応づける。
強制終了・OSによる停止では、workerがまだ書き込んでいない最後のイベントは残らない場合がある。
途中までの記録を成功・0msと扱わない。端末時計は発生時期の目安であり区間計測には使わない。

診断転送を計測期間中に行わず、転送自身の要求は記録しない。
Host の詳細ファイル出力は blocking worker へ渡す。
通常の非公開・サイズ制限付き `logs/host.jsonl` を使う。
アドレス、端末鍵、認証ヘッダー、会話本文、任意の依存ライブラリのメッセージは保存しない。

```sh
cargo xtask connection-diagnostics
# ローカル検証用の Host を解析する場合
cargo xtask connection-diagnostics --log /path/to/logs/host.jsonl --platform Macos
# 過去のアプリ起動から回収したキャプチャ
cargo xtask connection-diagnostics --trace TRACE_ID --attempt ATTEMPT_ID
```

## 計測点

| 区間 | 観測する境界・値 |
| --- | --- |
| iOS準備 | 保存状態の読込・復元、Keychain読込、Core呼出しと復帰、build番号 |
| 接続先準備 | Endpoint生成、アドレス探索、Host DNS lookup、ネットワーク調査 |
| Relay接続 | dial開始、TCP試行・成功、TLS開始・完了、Relay認証開始・完了 |
| QUIC接続 | 接続開始・完了・失敗、接続再利用の有無 |
| 各要求 | slot待ち、stream確保、エンコード、送信、readのpoll/pending/wake、最初の読取、応答完了、デコード、先行応答の採用 |
| Host | 同じstreamの受理、読取・デコード、実行待ち、処理・エンコード、書込、応答bytes |
| 経路 | 直接/Relayの選択と変更、各応答時の推定RTT、QUIC損失packets/bytes、送受信packets |
| 停止・UI | 250ms周期の処理系の実測間隔、active/inactive/background、一覧公開とSwiftUI状態更新 |

Relay はレジストリ版 `iroh-relay 1.1.0` の既存の固定メッセージだけを数値の段階へ変換する。
ライブラリのコピー、qlog、パケット全件記録、TCP socketの差替え、kernel samplerは使わない。
固定されたライブラリの更新時は公開Relayテストで境界が残っていることを確認する。
ネットワーク調査開始→完了も記録するため、Relayへdialする前の待ちを区別できる。

`RelayDialStart`→最初の`RelayTcpStart`にはDNS等の接続準備が含まれる。
TLS完了→認証開始にはWebSocket upgrade等が含まれる。どちらも特定の処理だけの時間とはしない。
TCPは成功した試行のspan同士を対応づけ、並行した試行を合算しない。
完了のない試行は失敗・キャンセル・欠落の候補であり、0msではない。
公開Relayのリージョンは既知のn0ドメインを1=aps1、2=usw1、3=use1、4=euw1へ分類するだけで、ホスト名は保存しない。

## 相関と解釈

`client.connection` は接続全体のサマリー。
`client.connection.timeline` の `trace` / `attempt` / `connection` と
`host.connection.link` の `session` を使い、同じQUIC `stream` のHost処理を対応づける。
サマリーはCoreの接続時間で、iOS準備や一覧・描画を含まない。
永続化から回収したキャプチャは `recovered=true` とし、未計測の集約サマリーや今回の接続への `host.connection.link` は生成しない。
`started_at_ms` はキャプチャ開始時の端末のUNIX時刻。解析は送信順ではなくこの時刻で最新キャプチャを選ぶ。
再利用時にはEndpoint生成や新規QUIC接続のイベントがない。

各 `at_us` はそのTrace内の単調時計。異なる端末の時刻を直接引いて片道時間を算出しない。
`host.rpc.performance` の `accepted_at_us` / `write_started_at_us` / `write_finished_at_us` は
Host endpointのTrace時計で、HostのRuntimePulseと比較できる。
stream確保から応答完了までの時間からHost内の時間を引いた残りには、クライアント処理・通信・未観測のスケジューリングが含まれる。
異なる観測区間が重なり残差が負になる場合は不整合として明示する。
全残差をRelay遅延と断定しない。

`RequestOpened.value` は要求開始からstream確保までのus、`RequestSlotWait.value`はその内のslot待ちus。
`RequestEncoded` / `ResponseDecoded`はエンコード/デコードus。
`RequestSent` / `ResponseFirstRead` / `ResponseReceived`はbytes。
RTT・準備・接続所要時間・RuntimePulseのvalueはus、通信統計は累積値。
readのWake→次のPollは通知を受けた後にアプリが再開するまでの待ちで、kernel受信時刻ではない。
先行一覧の読取タスクは採用前から応答を読み、期限・キャンセル・request slotの所有権を保つ。

RuntimePulseの遅延は処理系が予定時刻に動けなかった証拠であり、原因となったスレッドやCPU処理を特定するものではない。
アプリ状態を併せて見て、background停止とactive中の遅延を区別する。
一覧状態の更新はGPUのフレーム提示完了ではない。
QUICの損失数はRelayが使うTCPの再送回数ではない。
Relay内部の待ち、他社網のhop、TCP内の再送、kernelでのパケット受信時刻は未観測として扱う。

`BEX_CONNECTION_DIAGNOSTICS=off`を起動前に指定すれば、記録・sampler・read waker・回収を止めて同一バイナリで負荷比較できる。
通常のアプリログはこのスイッチの対象外。ローカルの比較を実機の国際回線の改善幅とは扱わない。

## 検証

TransportとCoreの実通信テストで、先行応答、期限、キャンセル、wakerの転送と破棄、
計測なし、バッファ上限、秘密情報の除外、処理系停止の検出を確認する。
診断の受信待ちが復帰完了と終了を妨げないことは、接続完了をモックで制御し、
診断キューを未読にしたCoreの単体テストで確認する。接続速度の合否とは分離する。
解析テストはプロセス・接続・streamの誤対応と、欠落を0msにしないことを確認する。

```sh
nix develop . --command cargo test -p agent-transport -p agent-core --features agent-core/bindings --lib
nix develop . --command cargo test -p agent-transport --lib \
  real_relay_records_upstream_connection_boundaries -- --ignored --nocapture
nix develop . --command cargo test --locked -p xtask --lib connection_diagnostics
```

## 履歴取得の診断

Host の `history.open` は `native_ms`、`project_ms`、`total_ms`、`bytes`、
`limit` を記録する。本文や会話タイトルは記録しない。`native_ms` は provider
からの取得、`project_ms` はプロジェクト情報の付与、`total_ms` は応答作成まで。
端末への転送時間や描画完了時間とは区別する。

0-RTT の先行要求は製品経路へ採用していない。再検討時は現行の実装で、認証・
拒否時の再送を保ち、実際の一覧応答と実機で通常接続との差を測る。
