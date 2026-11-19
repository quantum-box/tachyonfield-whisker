# 利益試算・AIチャット・支出・会計レポートのネイティブ対応

比較元は `quantum-box/tachyonfield` のコミット `73e645adebe3b21dddb83b6d4556ce58b6bee08f`。
実装は [analytics.rs](../src/management/analytics.rs)、[analytics_engine.rs](../src/management/analytics_engine.rs)、[analytics_screen.rs](../src/management/analytics_screen.rs)。11個の画面定義が、Webの6メニューに対応する。

## 実装した範囲

| Webメニュー | ネイティブで使える機能 | 残る差分 |
| --- | --- | --- |
| レジ明細の分析 | 日付・拠点の指定、期間合計、時間帯・商品・部門・支払手段・曜日の内訳 | グラフ、CSV保存。期間の終了日は含まない |
| 利益改善シナリオ | JPYの全社実績取得、価格と販売数の整数割合・月の販促費を最大10案で比較、利益と施策の寄与を表示、新しい下書きとして保存。保存済みの一覧・詳細・前提・相談履歴、期間/利益定義/メモの更新、削除 | 保存済みの案を計算フォームへ戻す操作、既存案の前提再編集、セット販売/廃棄削減/追加労働の入力、AIによる案編集、実業務への採用 |
| AIチャット | データセットと公開版を名称で選択、列の確認、版を固定した質問と継続会話、実行状況確認・停止、集計結果を最大200行表示。既存Agent画面で利用できるデータや分析方針を相談 | 複数データセットの結合、SQLの直接入力/編集、グラフ、結果ファイル保存。公開版の発行や元ファイル取込は別のデータ管理機能として未対応 |
| ベンダー支出 | 使用月の支出一覧・明細・履歴、手入力登録、計上済みの手入力に対する訂正/取消、CSV内容の貼付取込、カード明細、AWS/GCP/GitHub/Anthropic/OpenAIの接続設定の登録/更新/削除 | CSVファイル選択と文字コード変換、プロバイダー同期の手動実行、複合的な明細照合 |
| 会計レポート | 対象月の現金/発生主義の橋渡し、売掛買掛と元帳の突合、現在の販売契約指標 | グラフ、帳票保存/共有、元伝票への遷移。販売契約指標は指定月のスナップショットではない |
| Agent書類 | Agent見積書/請求書リクエストの一覧、金額・相手先・期日の確認、権限と状態に応じた送付 | 新規書類生成、PDF、メール内容の詳細編集 |

## 利益試算の契約と計算

実績を `/v1/erp/profit-summary/consultation-reference` から読み、期間と通貨を確認して `/v1/erp/profit-summary/scenarios/calculate` に計算を依頼する。価格・販売数・販促費の入力が有効な場合だけ計算を行う。価格変更は-100%より大きい値、販売数変化は-100%以上、販促費は0円以上とする。月の販促費を指定する場合は同じ月の月初から月末を対象とする。

金額は十進数文字列のまま扱い、割合の適用も整数演算で行う。`9007199254740993.01` のような大きい値や細かい原価を浮動小数点へ変換しない。計算できる桁数を超えた場合はエラーにする。仕入・人件費・固定費などの未取得値は `null` を維持し、必要な実績のない結果を0円と表示しない。食品ロス実績がない場合も「未算出」と表示する。

新規保存は `/v1/field/profit-scenarios/workspaces`、更新は `/{id}/versions` を使う。更新時は取得済みの `expectedVersion`、新規・更新・削除では冪等キーを利用する。結果未確認の保存を内容を変えて再送しない。保存済みの期間・メモ編集ではシナリオ、会話、Agentセッションを保持し、以前の計算結果の参照は外す。保存済み一覧はWebと同じ `before` によるページングを行う。

## AI分析の契約と結果表示

メニューの `feature.field.external-data-analysis` に加えて、対話の `feature.field.external-data-dialogue` を確認する。データセット、公開版、列を取得し、選択した版を質問本文へ固定する。質問はストリーミング用の `/v1/erp/analytics/external-data/dialogue` を使い、応答から許可された継続会話だけを引き継ぐ。

質問が中断した場合は自動再送しない。実行IDを保持して `/dialogue/status` で状態を確認し、明示的な停止は `/dialogue/cancel` へ送る。結果は `/dialogue/jobs/{id}/result` から取得し、ジョブID、参照版、列、行の幅、件数と省略情報を確認する。ページが進まない、版や列が変わる、形式を確認できない場合は表示を止める。

結果には最大200行・8ページの表示上限がある。読み残しは明示し、値の十進数文字列を保持する。描画済みの同じ行に新しい値が届いたときも表示を更新する。テナント切替、データ/版の切替、画面終了後の古い応答を表示しない。

## 支出・書類送付の操作条件

- ベンダー支出の金額は通常の通貨金額から最小通貨単位へ正確に変換する。使用月は `YYYY-MM` で検証する。
- 訂正/取消は未取消の手入力で、元の仕訳を確認できる記録だけを対象にする。`expectedJournalEntryId` と冪等キー、効力発生日、理由を送る。為替レートは正の十進数文字列・小数6桁以内とし、通貨変更時などに必要なレートを省略しない。
- CSVは内容を貼り付ける形式で5MB以内。必要な列を確認して既存の取込APIへ送る。ファイルアップロード画面を実装済みとは扱わない。
- 接続の更新では元の契約とプロバイダーを保持する。AWS以外の接続に必要な資格情報参照を確認する。資格情報の発行/保管フローは含まない。
- Agent書類は `agentDocuments.quotations.list/send` と `agentDocuments.invoices.list/send` の能力を個別に確認する。送付済み等の状態では操作を提示しない。
- 請求書送付はHTTPが成功しても `SendFailed` やメール配信失敗をエラーとして表示する。不明な送付結果を自動再送しない。

## 確認したWeb契約

以下は比較元リポジトリの `apps/client/src/features/` 以下のパス。参照ソースはこの公開リポジトリに含めていない。

| 分野 | 比較元 |
| --- | --- |
| レジ分析 | `work-reports/work-report-api.ts`、`work-reports/register-analysis-page.tsx` |
| 利益試算 | `profit-scenario/profit-scenario-api.ts`、`profit-scenario/profit-scenario-schema.ts`、`profit-scenario/profit-scenario-calculator.ts`、`profit-scenario/profit-scenario-state.ts`、`profit-scenario/profit-scenario-page.tsx` |
| AI分析 | `external-data-analysis/analysis-api.ts`、`external-data-analysis/analysis-dialogue.tsx`、`external-data-analysis/exploration-chat.tsx` |
| ベンダー支出 | `vendor-spend/vendor-spend-api.ts`、`vendor-spend/vendor-connection-api.ts`、`vendor-spend/vendor-spend-manual-actions.tsx`、`vendor-spend/vendor-spend-page.tsx` |
| 会計レポート | `finance-reports/finance-reports-page.tsx` と参照先のAPI契約 |
| Agent書類 | `agent/agent-api.ts`、`agent/agent-schema.ts`、`agent/agent-page.tsx` |

## 検証

7件の契約/計算テストで、大きい金額、精密な原価、税の基準不一致、未取得原価、月の販促費、食品ロス未算出、保存版の保持、訂正時の仕訳/為替、HTTP成功時の配信失敗、分析結果の版と形を検証した。さらにネイティブUI回帰テストで、利益試算の期間とJPY小数入力の拒否、集計結果の十進数表示と更新、レジ分析の全5内訳と金額表示を確認した。

これらはAPI実アクセスなしで実行する。実機での操作やテスト用テナントでのAPI往復を実施済みとは扱わない。全113メニューの対応範囲と他領域の残りは [全体対応表](native-management-parity.md) を参照する。
