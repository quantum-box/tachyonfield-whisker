# 予約業務の受け入れ観点

比較元は `quantum-box/tachyonfield` のcommit `27b3a5296f735b6811bcb9d49b4de615c07ac9e7`。比較開始時のnativeはPR #38取り込み後の `e0fc53176b078e59c194efc9f54efffa8b3ae48c`。この文書はメニュー数ではなく、担当者が日常業務を完了できるかを確認するための観点であり、全項目の実機確認済みを意味しない。

## Webの導線と開始時点の差分

Webの予約一覧は、検索・受付状態・決済状態から予約を選び、詳細に進む。新規追加は種別・設備・顧客連絡先・日時・人数・金額を入力し、保存した予約の詳細に戻る。種別詳細には、別に月別の空き状況と日別の枠・予約一覧がある。Webの空き状況から新規予約フォームへの連続遷移はなく、今回のnative専用画面ではそのつながりも確認する。

開始時点のnativeは予約の汎用一覧、新規作成、連絡先・日時編集、取消、待機登録・希望枠での予約化・終了、種別・設備・プラン・日別予算の基本CRUDが中心だった。実枠の確認、営業時間からの枠生成、担当・状態操作、決済再開、取消履歴からの返金再開、通知、引継ぎなどが業務の穴として残っていた。

## 一連の操作で確認すること

| 業務 | 完了の条件 | 代表的な失敗・境界条件 |
| --- | --- | --- |
| 当日の予約を確認 | 日付・種別・設備で予約と空き枠を確認し、予約番号・顧客・時刻・担当・受付状態・決済状態・金額が分かる。日別一覧と詳細を往復できる | 日付をまたぐUTC/JST変換、一覧のページ上限、取得失敗を「予約なし」と扱わない |
| 空き枠から新規予約 | 選んだ種別・設備・開始／終了が新規フォームへ引き継がれ、顧客・サービス・人数・料金を確認して作成する。作成した予約を開き、一覧と残枠が更新される | 非稼働設備・異なるresourceModel・サービス対象外設備、人数上限、終了≦開始、直前の満席、連打、通信切断 |
| 顧客と料金を確認 | 既存顧客を選ぶか、新規の顧客連絡先を入力できる。サービス・プラン由来の条件、総額、前払条件を確認する | 顧客選択変更後に古いIDを送らない。非JPYは通貨の最小単位に従う。請求後の顧客／service変更を成功扱いしない |
| 予約を変更 | 日時・設備・担当・連絡先・備考を変更し、詳細と予約枠の両方に反映する | 旧枠と新枠の残数、満席競合、決済保留中の制約、取得できなかった担当候補を空配列で上書きしない |
| 受付状態を進める | 確定・完了・無断不参加を明示的に実行し、サーバーの状態を再表示する | 支払済みや返金待ちをクライアントだけで書き換えない。一般PATCH取消で専用の返金処理を迂回しない |
| 取消・返金を完了 | 理由・適用するキャンセル料・返金額を確認し、取消履歴を残す。返金待ちは同じ取消イベントの再実行へ進める | 同一取消のIdempotency-Keyを保持。取消記録後の決済事業者エラーでは再取得し、二度目の取消による重複返金を防ぐ |
| 決済の再開と入金確認 | 支払リンクを発行・再利用し、決済画面を開ける。対応が必要な入金の内容を見て確認メモを記録する | paid／refund_pending等の条件、二重請求、通知・決済応答の部分成功。表示用の金額だけで入金完了としない |
| キャンセル待ちを予約化 | 希望条件と順番を見て、希望枠または確認した代替枠を選び、予約化後に作成した予約を開ける。辞退・期限終了も記録できる | 枠未作成・満席・空きありを区別。競合では待機を消さない。通信失敗後はconvertedReservationIdを確認し、無条件に再送しない |
| 引継ぎを完了 | 予約・待機のそれぞれでメモを追加し、確認・撤回を記録する | 本文の書き換えによる履歴消失、別レコードへのメモ混在、未取得を空の履歴として扱うことを防ぐ |
| 営業時間を予約可能枠へ反映 | 設備の曜日・時間・間隔・定員を保存し、生成の試算→生成→実際の枠確認まで行える。休業・再開も再生成まで進める | 営業時間保存だけで反映済みと表示しない。月曜=0の営業時間と日曜=0のプラン曜日を混同しない。休業revision競合、既予約枠の扱い、最大366日 |
| 種別・設備・プランを管理 | 作成・編集・利用停止・削除、数量表示・電話受付、プラン料金・前払条件・対象設備・公開状態を維持する。日別予算を対象日／コース別に扱う | 使用中削除409、catalog移行前後の保存先、商品連携プランの派生値、非公開プラン、未編集の拡張項目の保持 |
| 通知を扱う | 顧客向け3種のテンプレートを設定し、予約詳細で宛先と内容を確認して送信する。社内通知はメール・スタッフ・Slackと3イベントを管理する | 顧客向けと社内向けを混同しない。無効な通知、設定取得失敗、管理権限なし、他の通知宛先・文面の消失を防ぐ |
| 予約CSVを持ち出す | 明示的な取得後、iOSの保存／共有画面でCSVを保存する。取消は成功扱いにせず、エラーを表示する | 出力APIにない検索・状態・設備フィルタを適用済みと表示しない。文字コード・CSV引用・数式対策を保持。別テナントへ遅延応答を出さない |

### 在庫数の解釈

予約枠の `capacity` / `reservedQuantity` / `heldQuantity` / `availableQuantity` と待機候補の `availableSlotGroups` は組数であり、予約の `quantity` は人数または対象数である。人数が4名でも1予約が消費するのは1組。`availableSlotGroups = null` は該当枠が未作成、`0` は満席で、1以上なら1予約を受けられる。画面の表示・選択可否・テストfixtureでこの違いを保つ。

### 共通の受け入れ条件

- 一覧権限がない場合は予約データを出さず、管理設定はIAMの管理者確認が成功してから表示する。書き込みの最終認可は各APIが行う。
- テナント・日付・選択レコードの切替後に、前の取得結果・候補・CSV・送信内容が戻ってこない。
- 部分取得エラーを空の一覧へ変換しない。取得できなかった候補や設定を、空の更新データとして保存しない。
- 空き枠・service・顧客などの依存選択が変わったら、以前の選択を再検証する。
- 書き込み中の連打、戻ってからの再送、同じキーと異なるpayload、取得・保存後の遅延応答を区別する。
- 保存後の詳細値は同じレコードIDのままでも更新される。操作後に確認したい予約や枠へ戻れる。
- fixtureによるUI操作と契約テストでは業務APIへアクセスしない。実際のメール送信・予約作成・返金をテスト目的で実行しない。

## APIの境界として残すもの

| 項目 | 最新契約で確認できた境界 |
| --- | --- |
| 作成後の人数・直接の金額変更 | `UpdateReservationRequest` にquantity / priceAmount / depositAmount / currencyの変更フィールドはない。service変更は別途存在するが、任意の再料金設定と同一ではない |
| 請求後の顧客・service・請求先変更 | 顧客リンクやservice等には請求後の変更拒否がある。native側だけで回避しない |
| 独立した任意返金 | 確認した予約決済APIは専用取消と取消イベントに対するrefund-retry。任意額の単独返金APIがあるとは扱わない |
| 待機客への空き提案通知・承諾管理 | 確認したWeb待機画面・APIに専用の提案／承諾状態はない。スタッフの代替枠選択→convertとは区別する |
| 待機の安全な再送 | convertは予約作成後に待機をconvertedへ更新する。admin createにあるpayload hash／claimの保護はsource_channel=waitlistに同じ形では適用されない。予約だけ作成済みの不確実な失敗では再取得と人による確認が必要 |
| 種別の決済・取消ポリシー編集 | Web種別画面でもpaymentPolicyJson / cancellationPolicyJsonは閲覧項目。現在の種別CRUD契約へ推測で追加しない |
| CSVの絞り込み | exportは日付from/toのみで、`DATE(starts_at)`の両端を含む比較。予約一覧のRFC3339範囲や検索・状態等とは別の契約 |
| テンプレートの同時更新 | 設定APIにversion／ETagなし。直前再取得で既存の競合は防げるが、確認からPUTの間の競合を完全には排除できない |

## 主な根拠ファイル

- `apps/client/src/features/reservations/reservations-page.tsx`
- `apps/client/src/features/reservations/reservation-editor-page.tsx`
- `apps/client/src/features/reservations/reservation-detail-page.tsx`
- `apps/client/src/features/reservations/waitlist-page.tsx`
- `apps/client/src/features/reservations/{reservation-api,waitlist-api,handoff-notes-api}.ts`
- `apps/client/src/features/reservation-types/occupancy-calendar.ts` と `{occupancy-calendar-section,staff-notification-section}.tsx`
- `apps/client/src/features/reservation-schedule/{reservation-schedule-api.ts,reservation-schedule-page.tsx}`
- `apps/client/src/features/reservation-plans/reservation-plans-page.tsx`
- `apps/api/src/reservation_api.rs` と `reservation_api/{requests,notification_templates,report_csv}.rs`
- `apps/api/src/reservation_payment_api.rs`、`reservation_waitlist_api.rs`、`handoff_note_api.rs`
- `packages/reservation/src/sqlx_repository.rs`、`sqlx_repository/reschedule.rs`
