# 予約のお客様向け通知設定

## 対応範囲

予約管理の「お客様への予約通知設定」で、予約確認・リマインダー・変更／取消の3種類を一覧表示し、それぞれの利用可否、件名、本文を編集できる。

この画面はテンプレートの設定を保存する。お客様への実際の送信操作は予約詳細に属し、スタッフ向けのEmail／Slack通知設定とは別の契約である。

| 項目 | 実装 |
| --- | --- |
| 読み取り | `GET /v1/erp/reservation-notification-templates` の `items` を3行に展開 |
| 保存 | `PUT /v1/erp/reservation-notification-templates`、`items` に3種類の `enabled` / `subject` / `body` を指定 |
| 画面の管理者制限 | Web予約設定と同じIAM usersの確認。読み取り時は共通 `Resource.admin_only`、保存直前にも再確認 |
| API側の権限 | 読み取りは `field:ListReservations`、保存は `field:ManageReservations`。現在のclient-capabilitiesに予約書き込みの独立boolはない |
| 件名 | 前後空白を除いて1〜255文字。Unicodeの文字数で検証 |
| 本文 | 前後空白を除いて1〜10,000文字。改行と差し込み記法を保持 |
| 無効な通知 | 利用可否だけをオフにでき、件名と本文は保持。API同様、無効時も空の件名・本文は保存しない |
| 取得できない設定 | デフォルトへ置き換えず、保存を停止 |
| 未対応の応答 | 3種の欠損、未知の通知種別や未知のテンプレート項目、型違いを検出したら保存を停止 |

## 既存設定を保持する理由

APIは、まだ新しいテンプレート行が保存されていないテナントでは旧extension設定から読み取る。1種類だけを初めて保存すると、未保存の残り2種類が既定文へ戻る可能性がある。そのためnativeでは、サーバーから読み取った3種類を保持し、編集した1種類だけを差し替えて3種類とも保存する。クライアント独自の既定文は持たない。

保存前に最新の3種類を再取得し、画面を開いた時点の値と比較する。別の担当者が他の種類を編集していた場合も上書きせず、再読み込みを求める。同じ保存内容がすでに反映されている場合は追加の書き込みをしない。

このAPIにはversionやETagの条件付き更新がない。直前確認からPUTまでの間に起きる同時更新を完全に排除することはできない。保存の応答も内容まで照合し、確認できない結果を成功扱いにしない。

## 検証

`src/management/reservation_notifications.rs` の契約テストで、以下を確認する。テナントの実データ取得や通知メールの送信は行わない。

- 保存済み3種類と、無効の設定・改行・任意の差し込み記法が読み込みで保持される。
- 欠損した応答、未知の種類／項目、型違いから書き込み用の既定値を作らない。
- 1種類の編集で残り2種類と旧設定由来の文面を維持する。
- 件名／本文のUnicode文字数と、無効時も必要な入力を検証する。
- 読み込んだ種類と値が不整合なら編集を開始しない。
- 未編集の種類を含め、最新値との競合を検出する。
- 保存済みの同一内容を再度書き込まない。
- 管理者用Resourceとして登録し、設定画面から送信操作を提供しない。

## 比較元

`quantum-box/tachyonfield` のcommit `27b3a5296f735b6811bcb9d49b4de615c07ac9e7` を参照した。実装をコピーせず、API契約に基づいてnative側へ適用している。

- `apps/client/src/features/reservations/reservation-settings-page.tsx`
- `apps/client/src/features/reservations/reservation-api.ts`
- `apps/client/src/features/reservations/reservation-schema.ts`
- `apps/api/src/reservation_api/notification_templates.rs`
- `apps/api/src/client_capabilities_api.rs`
