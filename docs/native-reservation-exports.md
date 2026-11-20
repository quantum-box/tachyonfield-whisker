# 予約CSVと支払リンクの端末操作

## 予約CSV

`api::export_reservations_csv` はサーバーの予約CSVを取得し、検証済みのファイル名とUTF-8文字列を返す。呼び出しは画面上の明示的な取得操作から行う。

| 項目 | 契約・実装 |
| --- | --- |
| API | `GET /v1/erp/reservations/export.csv` |
| 権限 | `field:ListReservations`。既存の認証・テナントヘッダーを利用 |
| フィルター | 任意の `from` / `to`、形式は `YYYY-MM-DD`。開始日≤終了日を検証 |
| 日付の意味 | APIのSQLは `DATE(starts_at)` を両端inclusiveで比較する。画面のJST/RFC3339範囲とは別の契約 |
| 一覧の検索条件 | 検索語・状態・決済状態・種別・設備による絞り込みはCSV APIにない |
| 応答 | `text/csv; charset=utf-8`。Content-Dispositionのファイル名は安全なbasenameのみ受け付ける |
| 内容保持 | CSVの引用符・改行・日本語・BOM・サーバーが付けた表計算数式への対策を変更しない |
| 容量 | アプリ側で20MBまで受け取る。超えた場合は範囲を短くするよう表示。途中切断・HTML・JSONエラー・不正UTF-8をCSVとして保存しない |

`field_glass::CsvExportButton` はiOSのネイティブ部品で、「ファイルに保存」または「共有」を利用者が選んだ時点で一時CSVを作る。Filesへの保存は `UIDocumentPickerViewController`、共有は `UIActivityViewController` を利用する。iPadの共有シートにも表示位置を指定する。

保存・共有・取消の後は一時ファイルを削除し、画面のデータ差し替えや部品の破棄でもシートと一時ファイルを片付ける。データ取得だけでは端末ファイルの作成や外部への送信を行わない。

`FileExportDetail` の `ok` がtrueの時だけ完了と扱う。取消は `ok=false` かつ空の `error`。OSの共有完了は予約内容の変更や決済完了を表さない。

## 支払リンク

`field_glass::PaymentLinkButton` は予約の `checkoutUrl` を受け取り、HTTPS・hostあり・userinfoなし・空白や制御文字なしのURLだけを扱う。

手動メニューから「支払リンクを開く」「リンクをコピー」「リンクを共有」を選べる。メニューは表示時のURLを保持し、予約やURLが変わった後に古いメニューから別のリンクを実行しない。共有画面もURL変更や部品破棄で閉じる。

`PaymentLinkDetail` は `ok`、操作名 `open` / `copy` / `share`、`error` を返す。開く操作の成功はURLを開けたことを示し、支払いの完了を示さない。コピーや共有をしただけで予約の決済状態を変更しない。

## 対応プラットフォームと検証

CSVの端末保存・共有と支払リンクの端末操作はiOS向け。`CSV_EXPORT_SUPPORTED` / `PAYMENT_LINK_SUPPORTED` で画面側が対応状況を表示できる。非iOSで利用できたように見せる代替処理は行わない。

CSVのRust契約テストは日付・応答型・容量・文字列の保持・ファイル名を確認する。通知設定と合わせた独立harnessでは12件が成功した。取得・送信APIはテストで呼ばない。UIKitの保存／共有シートはLinux環境では実行できないため、iOSビルドと端末確認を別途行う必要がある。

比較元は `quantum-box/tachyonfield` のcommit `27b3a5296f735b6811bcb9d49b4de615c07ac9e7`。

- `apps/client/src/features/reservations/reservations-page.tsx`
- `apps/client/src/features/reservations/reservation-detail-page.tsx`
- `apps/api/src/reservation_api.rs`
- `apps/api/src/reservation_api/report_csv.rs`
- `packages/reservation/src/sqlx_repository.rs`
