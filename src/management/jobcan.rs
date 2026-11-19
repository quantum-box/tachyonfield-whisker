//! Native CSV import: preview and commit share the same immutable file bytes and settings.

use super::{form::InputFields, model, schema::*};
use crate::cash_book_screen::{Button, spawn_in};
use crate::management::files;
use crate::*;
use serde_json::{Value, json};
use std::sync::Arc;
use whisker::Owner;

const SETTINGS_PATH: &str = "/v1/erp/hrm/attendance-sources/jobcan/csv-settings";
const PREVIEW_PATH: &str = "/v1/erp/hrm/attendance-sources/jobcan/csv-imports/preview";
const IMPORT_PATH: &str = "/v1/erp/hrm/attendance-sources/jobcan/csv-imports";
const MAX_CSV_BYTES: u64 = 4 * 1024 * 1024;
const PAGE_SIZE: usize = 30;
const UPLOAD_FIELDS: &[Field] = &[
    Field::new("exportedAt", "CSVを書き出した日時（日本時間）", InputKind::DateTime).required().help("取込時刻ではなく、ジョブカンから書き出した日時を指定してください。"),
    Field::new("periodFrom", "対象期間の開始日", InputKind::Date),
    Field::new("periodTo", "対象期間の終了日", InputKind::Date).help("開始日・終了日は両方指定するか、両方空欄にします。空欄の場合はファイルから期間を読み取ります。"),
    Field::new("coverage", "取り込み方", InputKind::Select(&[Choice::new("partial", "一部として取り込む"), Choice::new("complete", "対象期間の全件として取り込む")])).required().default("partial").help("全件として取り込むと、対象期間のCSVにない勤務日は削除の対象になります。プレビューで確認してください。"),
];

#[derive(Clone, Debug, PartialEq)]
struct CsvFile {
    name: String,
    bytes: Arc<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
struct ReviewedImport {
    file: CsvFile,
    fields: Value,
    preview: Value,
}

fn upload_fields(draft: &Value, settings: &Value) -> Result<Value, String> {
    let mut fields = model::body(UPLOAD_FIELDS, draft)?;
    let mapping = settings["latest"]["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("先に「ジョブカン取込設定」で列・スタッフの対応を保存してください")?;
    let from = fields["periodFrom"].as_str();
    let to = fields["periodTo"].as_str();
    if from.is_some() != to.is_some() {
        return Err("対象期間の開始日と終了日を両方指定してください".into());
    }
    if let (Some(from), Some(to)) = (from, to) {
        if from > to {
            return Err("対象期間の終了日は開始日以降にしてください".into());
        }
    }
    fields["mappingVersionId"] = json!(mapping);
    Ok(fields)
}

fn validate_preview(value: &Value) -> Result<(), String> {
    let sha = value["file"]["fileSha256"]
        .as_str()
        .ok_or("プレビューしたファイルを確認できません")?;
    if sha.len() != 64
        || !sha.bytes().all(|b| b.is_ascii_hexdigit())
        || !value["blockers"].is_array()
        || !value["days"].is_array()
        || !value["counts"].is_object()
    {
        return Err("取込プレビューの応答形式を確認できません".into());
    }
    Ok(())
}

fn commit_fields(review: &ReviewedImport, confirmed: bool) -> Result<Value, String> {
    validate_preview(&review.preview)?;
    if review.preview["blockers"]
        .as_array()
        .is_none_or(|v| !v.is_empty())
    {
        return Err("取込を妨げる項目を修正して、もう一度プレビューしてください".into());
    }
    if !confirmed {
        return Err("プレビューの変更内容を確認してください".into());
    }
    let mut fields = review.fields.clone();
    fields["fileSha256"] = review.preview["file"]["fileSha256"].clone();
    Ok(fields)
}

fn status_name(value: &str) -> &str {
    match value {
        "new" => "新規",
        "changed" => "更新",
        "unchanged" => "変更なし",
        "stale" => "古いデータ",
        "held" => "保留",
        "unmapped" => "スタッフ未対応",
        "rejected" => "取込不可",
        "deletes" => "削除",
        "kept_partial" => "部分取込のため保持",
        "kept_newer" => "新しい記録を保持",
        "period_total" => "期間合計",
        "no_attendance" => "勤務なし",
        _ => value,
    }
}

fn preview_rows(preview: &Value) -> Vec<(usize, String, String)> {
    let mut rows = Vec::new();
    for day in preview["days"].as_array().into_iter().flatten() {
        let title = format!(
            "{}　{}　{}",
            model::display(&day["businessDate"]),
            day["fieldStaffName"]
                .as_str()
                .or_else(|| day["staffName"].as_str())
                .unwrap_or("未対応のスタッフ"),
            model::display(&day["staffCode"])
        );
        let text = format!(
            "{}　実働 {} 分 / 休憩 {} 分\n{} 〜 {}\n{}",
            status_name(day["status"].as_str().unwrap_or("")),
            model::display(&day["workedMinutes"]),
            model::display(&day["breakMinutes"]),
            model::display(&day["clockIn"]),
            model::display(&day["clockOut"]),
            model::display(&day["notes"])
        );
        rows.push((rows.len(), title, text));
    }
    for candidate in preview["cancelCandidates"].as_array().into_iter().flatten() {
        rows.push((
            rows.len(),
            format!(
                "勤務日の取消候補　{}　{}",
                model::display(&candidate["businessDate"]),
                model::display(&candidate["staffCode"])
            ),
            format!(
                "{}　現在の実働 {} 分",
                status_name(candidate["effect"].as_str().unwrap_or("")),
                model::display(&candidate["currentWorkedMinutes"])
            ),
        ));
    }
    for issue in preview["issues"].as_array().into_iter().flatten() {
        rows.push((
            rows.len(),
            format!(
                "CSV {} 行目　{}",
                model::display(&issue["sourceRowNumber"]),
                status_name(issue["kind"].as_str().unwrap_or(""))
            ),
            model::display(&issue["messages"]),
        ));
    }
    for total in preview["fileTotals"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["matches"] == false)
    {
        rows.push((
            rows.len(),
            format!(
                "合計が一致しません　CSV {} 行目",
                model::display(&total["sourceRowNumber"])
            ),
            format!(
                "CSVの合計 {} 分 / 日別の合計 {} 分",
                model::display(&total["fileWorkedMinutes"]),
                model::display(&total["dayWorkedMinutes"])
            ),
        ));
    }
    rows
}

#[component]
pub(crate) fn jobcan_import_panel(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    can_manage: ReadSignal<bool>,
    on_imported: Callback<()>,
) -> Element {
    let owner = Owner::current();
    let draft = signal(model::draft(UPLOAD_FIELDS, &Value::Null));
    let settings = signal(Value::Null);
    let file = signal(None::<CsvFile>);
    let reviewed = signal(None::<ReviewedImport>);
    let result = signal(Value::Null);
    let error = signal(String::new());
    let busy = signal(false);
    let confirmed = signal(false);
    let submitted = signal(false);
    let page = signal(0_usize);
    let refresh_settings = move || {
        if busy.get_untracked() || !can_manage.get_untracked() {
            return;
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        busy.set(true);
        error.set(String::new());
        spawn_in(owner, async move {
            let fetched = authed(token, move |access| {
                api::fetch_record(access, &current, SETTINGS_PATH)
            })
            .await;
            if !owner.is_none_or(Owner::is_alive)
                || tenant.get_untracked().as_ref() != Some(&context)
                || token.with_untracked(Option::is_none)
            {
                return;
            }
            busy.set(false);
            match fetched {
                Ok(value) => {
                    settings.set(value);
                    reviewed.set(None);
                    confirmed.set(false);
                }
                Err(message) => error.set(message),
            }
        });
    };
    refresh_settings();
    effect(move || {
        let _ = draft.get();
        let _ = file.get();
        reviewed.set(None);
        confirmed.set(false);
        submitted.set(false);
        result.set(Value::Null);
        page.set(0);
    });
    let pick = move |picked: field_glass::PickedFile| {
        if busy.get_untracked() || !can_manage.get_untracked() {
            return;
        }
        let extension = picked
            .name
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "csv" | "txt") {
            error.set("ジョブカンから書き出したCSVまたはTXTファイルを選んでください".into());
            return;
        }
        match files::read(&picked, MAX_CSV_BYTES) {
            Ok(bytes) => {
                file.set(Some(CsvFile {
                    name: picked.name.clone(),
                    bytes: Arc::new(bytes),
                }));
                error.set(String::new());
            }
            Err(message) => error.set(message),
        }
    };
    let preview = move || {
        if busy.get_untracked() || !can_manage.get_untracked() {
            return;
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let Some(csv) = file.get_untracked() else {
            error.set("CSVファイルを選んでください".into());
            return;
        };
        let fields = match upload_fields(&draft.get_untracked(), &settings.get_untracked()) {
            Ok(fields) => fields,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let context = current.clone();
        let snapshot = csv.clone();
        let field_snapshot = fields.clone();
        busy.set(true);
        error.set(String::new());
        reviewed.set(None);
        confirmed.set(false);
        result.set(Value::Null);
        submitted.set(false);
        page.set(0);
        spawn_in(owner, async move {
            let response = authed(token, move |access| {
                api::upload_management_csv(
                    access,
                    &current,
                    PREVIEW_PATH,
                    &fields,
                    &csv.name,
                    &csv.bytes,
                )
            })
            .await;
            if !owner.is_none_or(Owner::is_alive)
                || tenant.get_untracked().as_ref() != Some(&context)
                || token.with_untracked(Option::is_none)
            {
                return;
            }
            busy.set(false);
            match response.and_then(|value| {
                validate_preview(&value)?;
                Ok(value)
            }) {
                Ok(value) => reviewed.set(Some(ReviewedImport {
                    file: snapshot,
                    fields: field_snapshot,
                    preview: value,
                })),
                Err(message) => error.set(message),
            }
        });
    };
    let commit = move || {
        if busy.get_untracked() || submitted.get_untracked() || !can_manage.get_untracked() {
            return;
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let Some(review) = reviewed.get_untracked() else {
            return;
        };
        if file.get_untracked().as_ref() != Some(&review.file)
            || upload_fields(&draft.get_untracked(), &settings.get_untracked()).as_ref()
                != Ok(&review.fields)
        {
            error
                .set("ファイルまたは取込条件が変わりました。もう一度プレビューしてください".into());
            reviewed.set(None);
            return;
        }
        let fields = match commit_fields(&review, confirmed.get_untracked()) {
            Ok(fields) => fields,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let context = current.clone();
        submitted.set(true);
        busy.set(true);
        error.set(String::new());
        spawn_in(owner, async move {
            let response = authed(token, move |access| {
                api::upload_management_csv(
                    access,
                    &current,
                    IMPORT_PATH,
                    &fields,
                    &review.file.name,
                    &review.file.bytes,
                )
            })
            .await;
            if !owner.is_none_or(Owner::is_alive)
                || tenant.get_untracked().as_ref() != Some(&context)
                || token.with_untracked(Option::is_none)
            {
                return;
            }
            busy.set(false);
            match response {
                Ok(value) if value["run"]["id"].as_str().is_some_and(|s| !s.is_empty()) => { result.set(value); on_imported.call(); }
                Ok(_) => error.set("取込結果の形式を確認できませんでした。再実行する前に取込履歴を確認してください".into()),
                Err(message) => error.set(format!("{message}\n結果が確定していない可能性があります。取込履歴を確認してから、必要な場合だけ再度プレビューしてください。")),
            }
        });
    };
    let preview_value = computed(move || {
        reviewed.with(|review| {
            review
                .as_ref()
                .map(|review| review.preview.clone())
                .unwrap_or(Value::Null)
        })
    });
    let rows = computed(move || preview_rows(&preview_value.get()));
    render! {
        View(style: computed(move || group_style().margin_top(px(12)))) {
            SectionHeader(title: "ジョブカンCSVを取り込む")
            Text(value: "外部から取り込んだ勤怠は、月次勤怠の集計・締めに含まれません。外部側の勤怠も確認してください。", style: computed(move || css!(font_size: px(14), color: hex(SECONDARY_LABEL), padding: px(16))))
            Show(when: move || !can_manage.get()) { Notice(message: "取込を管理する権限がありません") }
            Show(when: move || can_manage.get()) {
                Show(when: move || busy.get()) { Notice(message: "ファイルと取込結果を確認しています…") }
                Show(when: move || !error.with(String::is_empty)) { ErrorNotice(message: computed(move || error.get())) }
                Text(value: computed(move || settings.with(|value| if value["latest"]["id"].is_string() { format!("取込設定 第{}版", model::display(&value["latest"]["versionNo"])) } else { "取込設定が未登録です。「ジョブカン取込設定」で対応表を保存してください。".into() })), style: computed(move || css!(font_size: px(14), color: hex(SECONDARY_LABEL), padding: px(16))))
                Show(when: move || !busy.get()) {
                    Button(label: "取込設定を読み直す", on_tap: move |_| refresh_settings())
                    InputFields(fields: UPLOAD_FIELDS.to_vec(), draft: draft, prefix: String::new(), token: token, tenant: tenant)
                    View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Row, align_items: AlignItems::Center, gap: px(12), padding: px(16))) {
                        AttachButton(on_pick: pick, style: css!(width: px(40), height: px(40)))
                        Text(value: computed(move || file.with(|file| file.as_ref().map(|file| format!("{}（{} KB）", file.name, file.bytes.len().div_ceil(1024))).unwrap_or_else(|| "CSV・TXTを選択（4MBまで）".into()))), style: computed(move || css!(font_size: px(15), color: hex(LABEL), flex_grow: 1.0)))
                    }
                    Button(label: "取込前のプレビュー", on_tap: move |_| preview())
                }
                Show(when: move || reviewed.with(Option::is_some)) {
                    SectionHeader(title: "変更内容の確認")
                    Text(value: computed(move || { let p = preview_value.get(); format!("新規 {} 件 / 更新 {} 件 / 変更なし {} 件\n保留 {} 件 / 古いデータ {} 件 / 未対応 {} 件\n取消候補 {} 件 / 除外行 {} 件", model::display(&p["counts"]["new"]), model::display(&p["counts"]["changed"]), model::display(&p["counts"]["unchanged"]), model::display(&p["counts"]["held"]), model::display(&p["counts"]["stale"]), model::display(&p["counts"]["unmapped"]), model::display(&p["counts"]["cancelCandidates"]), model::display(&p["counts"]["rejectedRows"])) }), style: computed(move || css!(font_size: px(15), color: hex(LABEL), padding: px(16))))
                    Text(value: computed(move || { let p = preview_value.get(); format!("実働時間の照合（分）\nCSV: {} / 反映前: {} / 反映後: {}", model::display(&p["reconciliation"]["file"]["workedMinutes"]), model::display(&p["reconciliation"]["before"]["workedMinutes"]), model::display(&p["reconciliation"]["after"]["workedMinutes"])) }), style: computed(move || css!(font_size: px(14), color: hex(LABEL), padding: px(16))))
                    Show(when: move || preview_value.with(|p| p["file"]["alreadyImported"].as_array().is_some_and(|rows| !rows.is_empty()))) { Notice(message: "同じファイルの取込履歴があります。重複や設定変更の意図を確認してください。") }
                    Show(when: move || preview_value.with(|p| p["blockers"].as_array().is_some_and(|rows| !rows.is_empty()))) { ErrorNotice(message: computed(move || preview_value.with(|p| model::display(&p["blockers"])))) }
                    ForEach(
                        each: move || rows.with(|rows| rows.iter().skip(page.get() * PAGE_SIZE).take(PAGE_SIZE).cloned().collect::<Vec<_>>()),
                        key: |row: &(usize, String, String)| row.0,
                        children: move |row: ReadSignal<(usize, String, String)>| render! {
                            View(style: computed(move || css!(padding: px(16), display: Display::Flex, flex_direction: FlexDirection::Column, gap: px(8)).border_bottom(hairline()))) {
                                Text(value: computed(move || row.get().1), style: computed(move || css!(font_size: px(15), color: hex(LABEL), font_weight: FontWeight::Numeric(600))))
                                Text(value: computed(move || row.get().2), style: computed(move || css!(font_size: px(14), color: hex(SECONDARY_LABEL))))
                            }
                        },
                    )
                    Text(value: computed(move || format!("確認明細 {} / {} ページ（{}件）", page.get() + 1, rows.with(|rows| rows.len().div_ceil(PAGE_SIZE).max(1)), rows.with(Vec::len))), style: computed(move || css!(font_size: px(13), color: hex(SECONDARY_LABEL), padding: px(16))))
                    Show(when: move || page.get() > 0) { Button(label: "前の明細", on_tap: move |_| page.update(|page| *page = page.saturating_sub(1))) }
                    Show(when: move || (page.get() + 1) * PAGE_SIZE < rows.with(Vec::len)) { Button(label: "次の明細", on_tap: move |_| page.update(|page| *page += 1)) }
                    Show(when: move || !busy.get() && !submitted.get() && preview_value.with(|p| p["blockers"].as_array().is_some_and(Vec::is_empty))) {
                        View(style: css!(padding: px(16)), on_tap: move |_| confirmed.update(|value| *value = !*value)) {
                            Text(value: computed(move || if confirmed.get() { "✓ 変更・保留・取消候補を確認しました".to_owned() } else { "□ 変更・保留・取消候補を確認しました".to_owned() }), style: computed(move || css!(font_size: px(15), color: hex(TINT))))
                        }
                        Show(when: move || confirmed.get()) { Button(label: "確認したファイルを取り込む", on_tap: move |_| commit()) }
                    }
                    Show(when: move || result.with(|value| value["run"]["id"].is_string())) {
                        Text(value: computed(move || result.with(|value| format!("取込が完了しました。反映 {} 件、保留 {} 件です。", model::display(&value["run"]["appliedCount"]), model::display(&value["run"]["heldCount"])))), style: computed(move || css!(font_size: px(16), color: hex(TINT), padding: px(16))))
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn review(blockers: Value) -> ReviewedImport {
        ReviewedImport {
            file: CsvFile {
                name: "attendance.csv".into(),
                bytes: Arc::new(vec![1, 2, 3]),
            },
            fields: json!({"mappingVersionId":"mapping-2","exportedAt":"2026-11-20T09:00:00+09:00","coverage":"partial"}),
            preview: json!({"file":{"fileSha256":"a".repeat(64)},"blockers":blockers,"days":[],"counts":{}}),
        }
    }
    #[test]
    fn commit_uses_reviewed_sha_mapping_and_requires_explicit_confirmation() {
        let reviewed = review(json!([]));
        assert!(commit_fields(&reviewed, false).is_err());
        let fields = commit_fields(&reviewed, true).unwrap();
        assert_eq!(fields["mappingVersionId"], "mapping-2");
        assert_eq!(fields["fileSha256"], "a".repeat(64));
        assert_eq!(reviewed.file.bytes.as_slice(), &[1, 2, 3]);
        assert!(commit_fields(&review(json!(["締め済み"])), true).is_err());
    }
    #[test]
    fn upload_requires_paired_dates_and_a_saved_mapping() {
        let mut draft = model::draft(UPLOAD_FIELDS, &Value::Null);
        draft["exportedAt"] = json!("2026-11-20 09:00");
        draft["periodFrom"] = json!("2026-11-01");
        assert!(upload_fields(&draft, &json!({"latest":{"id":"mapping-2"}})).is_err());
        draft["periodTo"] = json!("2026-11-30");
        let fields = upload_fields(&draft, &json!({"latest":{"id":"mapping-2"}})).unwrap();
        assert_eq!(fields["exportedAt"], "2026-11-20T09:00:00+09:00");
        assert!(upload_fields(&draft, &Value::Null).is_err());
    }
    #[test]
    fn preview_rows_include_delete_candidates_and_total_mismatches() {
        let rows = preview_rows(
            &json!({"days":[],"cancelCandidates":[{"businessDate":"2026-11-20","staffCode":"001","effect":"deletes","currentWorkedMinutes":480}],"fileTotals":[{"sourceRowNumber":2,"matches":false,"fileWorkedMinutes":500,"dayWorkedMinutes":480}]}),
        );
        assert_eq!(rows.len(), 2);
        assert!(rows[0].2.contains("削除"));
        assert!(rows[1].1.contains("一致しません"));
    }
}
