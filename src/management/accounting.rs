//! Versioned accounting work, review requests and monthly closing.

use super::{model, schema::*};
use crate::api::{self, Tenant};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const GROUP: &str = "会計・監査";
const USERS: Lookup = Lookup {
    path: "/v1/field/iam/users?reference_only=true",
    list_key: "users",
    value_key: "id",
    label_keys: &["name", "email"],
};
const SITES: Lookup = Lookup {
    path: "/v1/erp/master-control/organization",
    list_key: "sites",
    value_key: "id",
    label_keys: &["name", "code"],
};
const WORK_STATES: &[Choice] = &[
    Choice::new("open", "作業中"),
    Choice::new("waiting", "待機中"),
    Choice::new("failed", "失敗"),
    Choice::new("completed", "完了"),
    Choice::new("cancelled", "取消"),
];
const WORK_AREAS: &[Choice] = &[
    Choice::new("sales", "販売"),
    Choice::new("expense", "経費"),
    Choice::new("procurement", "仕入"),
    Choice::new("accounting", "会計"),
    Choice::new("payment", "支払"),
    Choice::new("close", "締め"),
    Choice::new("reporting", "報告"),
];
const CASE_KINDS: &[Choice] = &[
    Choice::new("daily_report", "日報の提出・確認"),
    Choice::new("receipt_review", "証憑の確認"),
    Choice::new("payment_review", "支払の確認"),
    Choice::new("monthly_review", "月次の確認"),
];
const CONDITION_KINDS: &[Choice] = &[
    Choice::new("manual_check", "目視確認"),
    Choice::new("approved", "承認"),
    Choice::new("content_confirmed", "内容確認"),
    Choice::new("evidence_attached", "証憑提出"),
    Choice::new("inquiry_answered", "確認依頼の回答"),
    Choice::new("posted", "計上"),
    Choice::new("settled", "精算・支払"),
    Choice::new("case_completed", "依存する業務の完了"),
];
const WORK_FILTERS: &[Field] = &[
    Field::new("siteId", "拠点", InputKind::Reference(SITES)),
    Field::new("workArea", "業務領域", InputKind::Select(WORK_AREAS)),
    Field::new("status", "状態", InputKind::Select(WORK_STATES)),
    Field::text("period", "対象期間").help("例: 2026-11"),
    Field::new("assigneeId", "担当者", InputKind::Reference(USERS)),
    Field::new("overdue", "期限超過のみ", InputKind::Bool),
    Field::new("unassigned", "未割当のみ", InputKind::Bool),
];
const NOTE: &[Field] = &[Field::new("note", "作業メモ", InputKind::Multiline).empty(Empty::Null)];
const REASON_NOTE: &[Field] = &[Field::new("note", "理由", InputKind::Multiline).required()];
const CONFIRM: &[Field] =
    &[Field::new("confirmed", "対象と内容を確認しました", InputKind::Bool).required()];
const CONDITION_LINES: &[Field] = &[
    Field::new("kind", "条件", InputKind::Select(CONDITION_KINDS)),
    Field::new("required", "必須", InputKind::Bool),
    Field::new("satisfied", "確認済み", InputKind::Bool),
    Field::new("satisfiedAt", "確認日時", InputKind::DateTime),
    Field::new("satisfiedBy.id", "確認した人", InputKind::Reference(USERS)),
];
const RECURRING_FIELDS: &[Field] = &[
    Field::text("title", "業務名").required(),
    Field::new("active", "利用する", InputKind::Bool).default("true"),
    Field::new("caseKind", "業務の種類", InputKind::Select(CASE_KINDS))
        .required()
        .default("monthly_review"),
    Field::new("workArea", "業務領域", InputKind::Select(WORK_AREAS))
        .required()
        .default("accounting"),
    Field::new(
        "siteId",
        "対象拠点（空欄は全拠点）",
        InputKind::Reference(SITES),
    )
    .empty(Empty::Null),
    Field::new(
        "cadence",
        "繰り返し",
        InputKind::Select(&[
            Choice::new("daily", "毎日"),
            Choice::new("weekly", "毎週"),
            Choice::new("monthly", "毎月"),
        ]),
    )
    .required()
    .default("monthly"),
    Field::new("anchorDay", "基準日", InputKind::Integer)
        .required()
        .default("1")
        .help("毎週: 月曜=1〜日曜=7、毎月: 1〜31日。毎日の場合はこの値を使いません"),
    Field::new("leadDays", "何日前から作業を開始するか", InputKind::Integer)
        .required()
        .default("0"),
    Field::new(
        "dueOffsetDays",
        "基準日から締切までの日数",
        InputKind::Integer,
    )
    .required()
    .default("0"),
    Field::new("validFrom", "開始日", InputKind::Date)
        .required()
        .default("@today"),
    Field::new("validUntil", "終了日", InputKind::Date).empty(Empty::Null),
    Field::new("skipMon", "月曜日を除外", InputKind::Bool),
    Field::new("skipTue", "火曜日を除外", InputKind::Bool),
    Field::new("skipWed", "水曜日を除外", InputKind::Bool),
    Field::new("skipThu", "木曜日を除外", InputKind::Bool),
    Field::new("skipFri", "金曜日を除外", InputKind::Bool),
    Field::new("skipSat", "土曜日を除外", InputKind::Bool),
    Field::new("skipSun", "日曜日を除外", InputKind::Bool),
    Field::new(
        "closures",
        "作業しない期間",
        InputKind::Lines(&[
            Field::new("from", "開始日", InputKind::Date).required(),
            Field::new("until", "終了日", InputKind::Date).required(),
            Field::new("reason", "理由", InputKind::Multiline).empty(Empty::Null),
        ]),
    ),
];
const MONTH_FILTER: &[Field] = &[Field::text("yearMonth", "対象月")
    .required()
    .default("@month")
    .help("YYYY-MM形式")];
const CLOSE_FIELDS: &[Field] = &[
    Field::new("reason", "実行理由", InputKind::Multiline).required(),
    Field::new(
        "confirmed",
        "対象月・未完了事項・貸借を確認しました",
        InputKind::Bool,
    )
    .required(),
];
const REVIEW_STATES: &[Choice] = &[
    Choice::new("open", "未割当"),
    Choice::new("assigned", "回答待ち"),
    Choice::new("answered", "回答済み"),
    Choice::new("returned", "差戻し"),
    Choice::new("confirmed", "確認済み"),
    Choice::new("expired", "期限切れ"),
    Choice::new("cancelled", "取消"),
];

pub const RESOURCES: &[Resource] = &[
    Resource::new("work-cases", GROUP, "業務ケース", "/v1/erp/work-cases", "items")
        .route("accounting/work-cases").title(&["nativeTitle"]).subtitle(&["period", "scopeLabel"]).status("status").filters(WORK_FILTERS).load(load_work_cases)
        .detail("/v1/erp/work-cases/{id}", "")
        .fields(&[("対象の会社・拠点", "scopeLabel"), ("業務", "nativeTitle"), ("対象期間", "period"), ("状態", "status"), ("期限", "dueAt"), ("期限超過", "overdue"), ("開始日時", "openedAt"), ("作業権の期限", "lease.until"), ("待機理由", "wait.reason"), ("待機メモ", "wait.note"), ("待機期限", "wait.until"), ("直近の失敗", "lastFailureReason"), ("完了メモ", "closeNote"), ("更新日時", "updatedAt")])
        .read_lines(&[
            Field::new("conditions", "完了条件", InputKind::Lines(CONDITION_LINES)),
            Field::new("blockers", "未完了の事項", InputKind::Lines(&[Field::new("kind", "種類", InputKind::Select(&[Choice::new("failed", "処理失敗"), Choice::new("overdue", "期限超過"), Choice::new("unassigned", "担当なし"), Choice::new("unmet_condition", "未完了の条件"), Choice::new("waiting", "待機") ])), Field::new("at", "日時", InputKind::DateTime)])),
            Field::new("history", "操作履歴", InputKind::Lines(&[Field::new("occurredAt", "日時", InputKind::DateTime), Field::text("command", "操作"), Field::new("actor.id", "実行者", InputKind::Reference(USERS)), Field::text("note", "メモ"), Field::text("handoverNote", "引継ぎメモ")])),
        ])
        .actions(&[
            Mutation::new("自分が引き受ける", "POST", "/v1/erp/work-cases/{id}/claim", &[]).prepare(version).when(claim_unassigned),
            Mutation::new("作業を開始する", "POST", "/v1/erp/work-cases/{id}/claim", &[]).prepare(version).when(claim_assigned).actor_match("assignee.id"),
            Mutation::new("作業権を解放", "POST", "/v1/erp/work-cases/{id}/release", &[]).prepare(version).when(work_open).actor_match("lease.holder.id"),
            Mutation::new("作業内容を報告", "POST", "/v1/erp/work-cases/{id}/report", NOTE).prepare(version).when(work_open).explain("業務の実施内容を報告し、自分を担当者として記録します。未完了の条件は自動では満たされません。"),
            Mutation::new("待機にする", "POST", "/v1/erp/work-cases/{id}/wait", &[
                Field::new("reason", "待機理由", InputKind::Select(&[Choice::new("awaiting_approval", "承認待ち"), Choice::new("awaiting_evidence", "証憑待ち"), Choice::new("awaiting_external_reply", "回答待ち"), Choice::new("awaiting_upstream", "上流処理待ち"), Choice::new("period_closed", "期間締め済み"), Choice::new("returned", "差戻し"), Choice::new("scheduled", "予定待ち"), Choice::new("other", "その他")])).required(),
                Field::new("note", "メモ", InputKind::Multiline).empty(Empty::Null), Field::new("until", "待機期限", InputKind::DateTime).empty(Empty::Null),
            ]).prepare(version).when(|r| work_open(r) && !matches!(r["status"].as_str(), Some("waiting"|"failed"))),
            Mutation::new("作業を再開", "POST", "/v1/erp/work-cases/{id}/resume", &[]).prepare(version).when(|r| matches!(r["status"].as_str(), Some("waiting"|"failed"))),
            Mutation::new("担当を引き継ぐ", "POST", "/v1/erp/work-cases/{id}/handover", &[
                Field::new("nextAssignee", "次の担当者（空欄は未割当）", InputKind::Reference(USERS)).empty(Empty::Null), Field::new("note", "引継ぎメモ", InputKind::Multiline).required(),
            ]).prepare(handover).when(work_open),
            Mutation::new("完了として記録", "POST", "/v1/erp/work-cases/{id}/complete", &[
                Field::new("note", "完了内容", InputKind::Multiline).required(), Field::new("confirmed", "必須条件と作業内容を確認しました", InputKind::Bool).required(),
            ]).prepare(complete_payload).when(can_complete).actor_match("lease.holder.id").execute(complete_case),
            Mutation::new("失敗を記録", "POST", "/v1/erp/work-cases/{id}/failures", REASON_NOTE).prepare(version).when(work_open),
            Mutation::new("業務ケースを取り消す", "POST", "/v1/erp/work-cases/{id}/cancel", REASON_NOTE).prepare(version).when(work_open).destructive(),
        ]),
    Resource::new("work-case-manual-checks", GROUP, "業務ケースの目視確認", "/v1/erp/work-cases", "items")
        .route("accounting/work-cases").id("workCaseId|key").title(&["nativeTitle"]).subtitle(&["period", "scopeLabel"]).filters(WORK_FILTERS).load(load_manual_checks)
        .fields(&[("業務", "nativeTitle"), ("対象の会社・拠点", "scopeLabel"), ("期間", "period"), ("必須", "required"), ("確認済み", "satisfied"), ("確認日時", "satisfiedAt"), ("業務の状態", "status")])
        .actions(&[
            Mutation::new("目視確認を済みにする", "POST", "/v1/erp/work-cases/{workCaseId}/conditions/{key}/satisfy", CONFIRM).prepare(condition_payload).when(|r| work_open(r) && r["satisfied"] == false),
            Mutation::new("目視確認を戻す", "POST", "/v1/erp/work-cases/{workCaseId}/conditions/{key}/withdraw", CONFIRM).prepare(condition_payload).when(|r| work_open(r) && r["satisfied"] == true).destructive(),
        ]),
    Resource::new("work-case-summary", GROUP, "業務の進捗集計", "/v1/erp/work-cases/summary", "")
        .route("accounting/work-cases").id("generatedAt").title(&["generatedAt"])
        .filters(&[Field::new("siteId", "拠点", InputKind::Reference(SITES)), Field::new("workArea", "業務領域", InputKind::Select(WORK_AREAS)), Field::new("completedDays", "完了件数の対象日数", InputKind::Integer).default("7"), Field::new("stagnantHours", "停滞とみなす時間数", InputKind::Integer).default("48")])
        .fields(&[("作業中", "totals.open"), ("完了", "totals.completed"), ("失敗", "totals.failed"), ("期限超過", "totals.overdue"), ("担当なし", "totals.unowned"), ("停滞", "totals.stagnant"), ("定期業務の未着", "totals.recurringMissing"), ("定期業務の遅延", "totals.recurringLate"), ("手戻り", "totals.reworked"), ("完了の集計開始", "completedSince"), ("一部のみ集計", "truncated")]),
    Resource::new("recurring-work-definitions", GROUP, "定期業務の設定", "/v1/erp/work-cases/recurring-definitions", "items")
        .route("accounting/work-cases").title(&["title"]).subtitle(&["nextPeriod", "nextDueAt"]).load(load_recurring)
        .fields(&[("業務名", "title"), ("利用中", "active"), ("次回の対象期間", "nextPeriod"), ("次回の期限", "nextDueAt"), ("開始日", "validFrom"), ("終了日", "validUntil")])
        .read_lines(&[Field::new("conditions", "完了条件", InputKind::Lines(CONDITION_LINES))])
        .create(Mutation::new("定期業務を登録", "POST", "/v1/erp/work-cases/recurring-definitions", RECURRING_FIELDS).prepare(recurring_payload).explain("新しい定期業務には必須の目視確認を1つ設定します。開始日・基準日・締切を確認してください。"))
        .edit(Mutation::new("定期業務を編集", "PUT", "/v1/erp/work-cases/recurring-definitions/{id}", RECURRING_FIELDS).prepare(recurring_payload))
        .actions(&[Mutation::new("対象期間の業務ケースを生成", "POST", "/v1/erp/work-cases/recurring-definitions/generate", CONFIRM).prepare(generate_payload).when(|r| r["active"] == true).validate_response(generation_result)]),
    Resource::new("review-requests", GROUP, "確認依頼・回答", "/v1/erp/reconciliation/review-requests", "items")
        .route("accounting/review-requests").pagination(Pagination::Offset { limit: 500 }).title(&["questionText"]).subtitle(&["occurredOn", "dueOn"]).status("status")
        .detail("/v1/erp/reconciliation/review-requests/{id}", "")
        .filters(&[Field::new("status", "状態", InputKind::Select(REVIEW_STATES)), Field::new("assigneeId", "担当者", InputKind::Reference(USERS)), Field::new("unassigned", "未割当のみ", InputKind::Bool), Field::new("overdue", "期限超過のみ", InputKind::Bool)])
        .fields(&[("質問", "questionText"), ("状態", "status"), ("発生日", "occurredOn"), ("回答期限", "dueOn"), ("回答", "answer.response"), ("回答日時", "answeredAt"), ("更新日時", "updatedAt")])
        .actions(&[
            Mutation::new("自分が引き受ける", "POST", "/v1/erp/reconciliation/review-requests/{id}/assign", &[]).actor_body("assigneeId").when(|r| matches!(r["status"].as_str(), Some("open"|"returned")) && r["assigneeId"].is_null()).execute(assign_review),
            Mutation::new("担当者を割り当てる", "POST", "/v1/erp/reconciliation/review-requests/{id}/assign", &[Field::new("assigneeId", "担当者", InputKind::Reference(USERS)).required()]).when(|r| matches!(r["status"].as_str(), Some("open"|"returned"|"assigned"))).execute(assign_review),
            Mutation::new("回答を保存", "POST", "/v1/erp/reconciliation/review-requests/{id}/answer", &[Field::new("answer.response", "事実についての回答", InputKind::Multiline).required()]).when(|r| r["status"] == "assigned").actor_match("assigneeId").execute(answer_review).explain("取引の事実について回答します。勘定科目・税区分の確定は別の確認操作です。"),
            Mutation::new("回答を差し戻す", "POST", "/v1/erp/reconciliation/review-requests/{id}/return", &[Field::new("reason", "差戻し理由", InputKind::Multiline).required()]).when(|r| r["status"] == "answered").execute(return_review),
            Mutation::new("取引事実を確認済みにする", "POST", "/v1/erp/reconciliation/review-requests/{id}/confirm", CONFIRM).prepare(confirm_fact).when(|r| r["status"] == "answered").execute(confirm_review),
            Mutation::new("税務の確認を確定", "POST", "/v1/erp/reconciliation/review-requests/{id}/tax-confirm", CONFIRM).prepare(confirm_tax).capability("accounting.confirmTax").when(|r| matches!(r["status"].as_str(), Some("answered"|"confirmed"))).execute(tax_confirm_review).explain("権限のある担当者が対象の版について税務確認を確定します。"),
            Mutation::new("確認依頼を取り消す", "POST", "/v1/erp/reconciliation/review-requests/{id}/cancel", &[Field::new("reason", "取消理由", InputKind::Multiline).required()]).when(|r| matches!(r["status"].as_str(), Some("open"|"assigned"|"answered"|"returned"))).execute(cancel_review).destructive(),
        ]),
    Resource::new("monthly-closing", GROUP, "月次締め", "/v1/erp/monthly-closes/{yearMonth}", "")
        .route("accounting/monthly-closing").id("yearMonth").title(&["yearMonth"]).status("status").filters(MONTH_FILTER).load(load_closing)
        .fields(&[("対象月", "yearMonth"), ("状態", "status"), ("期間開始", "periodStart"), ("期間終了", "periodEnd"), ("締める前に解消が必要", "readiness.criticalCount"), ("警告", "readiness.warningCount"), ("請求書", "invoiceCount"), ("未処理請求書", "draftInvoiceCount"), ("未転記仕訳", "unpostedJournalEntryCount"), ("未仕訳売上", "unjournalizedSalesEventCount"), ("未照合決済", "unreconciledPaymentCount"), ("証憑なし", "missingEvidenceCount"), ("売掛残高", "receivableOutstanding"), ("買掛残高", "payableOutstanding"), ("借方合計", "trialBalance.debitTotal"), ("貸方合計", "trialBalance.creditTotal"), ("貸借一致", "trialBalance.balanced"), ("締め日時", "closedAt"), ("再開日時", "reopenedAt")])
        .read_lines(&[
            Field::new("readiness.blockers", "締め前の対応事項", InputKind::Lines(&[Field::text("label", "項目"), Field::new("count", "件数", InputKind::Integer), Field::text("description", "内容"), Field::text("remediationHint", "対応方法"), Field::new("severity", "重要度", InputKind::Select(&[Choice::new("critical", "解消が必要"), Choice::new("warning", "警告")]))])),
            Field::new("issues", "対象明細", InputKind::Lines(&[Field::text("description", "内容"), Field::new("occurredOn", "発生日", InputKind::Date), Field::text("status", "状態"), Field::new("amount", "金額", InputKind::Decimal)])),
            Field::new("unresolved", "未完了事項の詳細", InputKind::Lines(&[Field::text("categoryLabel", "項目"), Field::new("occurredOn", "発生日", InputKind::Date), Field::text("reason", "理由"), Field::text("remediationHint", "対応方法")])),
        ])
        .actions(&[
            Mutation::new("月次を締める", "POST", "/v1/erp/monthly-closes/{id}/close", CLOSE_FIELDS).capability("accounting.closePeriod").prepare(close_payload).when(can_close).execute(close_period).explain("対象月の会計期間を締めます。最新の未完了事項と貸借を再確認してから実行します。"),
            Mutation::new("締めた月を再開", "POST", "/v1/erp/monthly-closes/{id}/reopen", CLOSE_FIELDS).capability("accounting.reopenPeriod").prepare(close_payload).when(|r| r["status"] == "closed").execute(reopen_period).destructive(),
            Mutation::new("会計出力を作成", "POST", "/v1/erp/accounting/exports", CONFIRM).prepare(export_payload),
        ]),
    Resource::new("accounting-export-history", GROUP, "会計出力の作成履歴", "/v1/erp/accounting/exports", "items")
        .route("accounting/monthly-closing").title(&["periodStart"]).subtitle(&["periodEnd", "generatedAt"]).status("status").filters(MONTH_FILTER)
        .fields(&[("期間開始", "periodStart"), ("期間終了", "periodEnd"), ("状態", "status"), ("作成日時", "generatedAt"), ("帳簿の版", "ledgerVersion"), ("科目対応の版", "accountMappingVersion"), ("税区分対応の版", "taxMappingVersion"), ("確認状態の版", "confirmationStateVersion")]),
];

fn work_open(record: &Value) -> bool {
    matches!(
        record["status"].as_str(),
        Some("open" | "waiting" | "failed")
    )
}
fn claim_unassigned(record: &Value) -> bool {
    work_open(record) && record["lease"].is_null() && record["assignee"].is_null()
}
fn claim_assigned(record: &Value) -> bool {
    work_open(record) && record["lease"].is_null() && record["assignee"].is_object()
}
fn version(body: &mut Value, original: &Value) -> Result<(), String> {
    let value = original["version"]
        .as_u64()
        .ok_or("業務ケースの版を確認できません。読み直してください")?;
    body["expectedVersion"] = json!(value);
    Ok(())
}
fn handover(body: &mut Value, original: &Value) -> Result<(), String> {
    let next = body["nextAssignee"].as_str().map(str::to_owned);
    body.as_object_mut()
        .ok_or("入力を確認できません")?
        .remove("nextAssignee");
    body["to"] = next.map_or(Value::Null, |id| json!({"id":id,"kind":"user"}));
    version(body, original)
}
fn confirmed(body: &mut Value) -> Result<(), String> {
    if body["confirmed"] != true {
        return Err("対象と実行内容を確認してください".into());
    }
    body.as_object_mut()
        .ok_or("入力を確認できません")?
        .remove("confirmed");
    Ok(())
}
fn condition_payload(body: &mut Value, original: &Value) -> Result<(), String> {
    if original["kind"] != "manual_check" {
        return Err("この条件は関連業務で確認してください".into());
    }
    confirmed(body)?;
    version(body, original)
}
fn can_complete(record: &Value) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(i64::MAX, |duration| {
            duration.as_millis().min(i64::MAX as u128) as i64
        });
    complete_at(record, now)
}
fn complete_at(record: &Value, now: i64) -> bool {
    record["status"] == "open"
        && record["lease"]["holder"]["kind"] == "user"
        && record["lease"]["until"]
            .as_str()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|date| date.timestamp_millis() > now)
        && record["conditions"].as_array().is_some_and(|rows| {
            rows.iter()
                .all(|row| row["required"] == false || row["satisfied"] == true)
        })
}
fn complete_payload(body: &mut Value, original: &Value) -> Result<(), String> {
    confirmed(body)?;
    version(body, original)
}
fn complete_case(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let id = original["id"]
        .as_str()
        .ok_or("業務ケースを読み直してください")?;
    let path = format!("/v1/erp/work-cases/{}", api::encode_query(id));
    let fresh = api::fetch_record(token, tenant, &path)?;
    if fresh["version"] != original["version"]
        || fresh["lease"]["holder"] != original["lease"]["holder"]
        || !can_complete(&fresh)
    {
        return Err("条件または作業権が変わっています。業務ケースを読み直してください".into());
    }
    api::send_json(
        token,
        tenant,
        "POST",
        &format!("{path}/complete"),
        body,
        Duration::from_secs(30),
    )
}
fn case_name(record: &Value) -> &'static str {
    if record["caseKind"] == "monthly_close_checklist" {
        return match record["subjectScope"].as_str() {
            Some("readiness_review") => "締め前の未完了事項の確認",
            Some("trial_balance_review") => "試算表・残高の確認",
            Some("package_review") => "月次成果物の確認",
            Some("customer_delivery") => "顧客への共有と受領確認",
            _ => "月次締めチェック",
        };
    }
    CASE_KINDS
        .iter()
        .find(|choice| record["caseKind"] == choice.value)
        .map_or("業務ケース", |choice| choice.label)
}
fn load_work_cases(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let resource = Resource::new("work-source", "", "", "/v1/erp/work-cases", "items")
        .pagination(Pagination::Offset { limit: 500 })
        .filters(WORK_FILTERS);
    let mut result = model::fetch_collection(token, tenant, resource, filters)?;
    let organization = api::fetch_record(token, tenant, SITES.path).unwrap_or(Value::Null);
    let entity = &organization["accountingEntity"];
    for row in &mut result.rows {
        row["nativeTitle"] = json!(case_name(row));
        let entity_name = if row["accountingEntityId"] == entity["id"]
            || row["accountingEntityId"] == tenant.id
        {
            entity["legalName"]
                .as_str()
                .or_else(|| entity["code"].as_str())
                .unwrap_or("会社（名称未取得）")
        } else {
            "会社（名称未取得）"
        };
        let site_name = if row["siteId"].is_null() {
            "全拠点"
        } else {
            organization["sites"]
                .as_array()
                .and_then(|sites| sites.iter().find(|site| site["id"] == row["siteId"]))
                .and_then(|site| site["name"].as_str())
                .unwrap_or("拠点（名称未取得）")
        };
        row["scopeLabel"] = json!(format!("{entity_name} / {site_name}"));
    }
    Ok(json!({"items":result.rows,"truncated":result.truncated}))
}
fn load_manual_checks(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let cases = load_work_cases(token, tenant, filters)?;
    let mut rows = Vec::new();
    for case in cases["items"].as_array().into_iter().flatten() {
        for condition in case["conditions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|condition| condition["kind"] == "manual_check")
        {
            let mut row = condition.clone();
            for key in ["version", "period", "scopeLabel", "nativeTitle", "status"] {
                row[key] = case[key].clone();
            }
            row["workCaseId"] = case["id"].clone();
            rows.push(row);
        }
    }
    Ok(json!({"items":rows,"truncated":cases["truncated"]}))
}
const SKIP_FIELDS: &[&str] = &[
    "skipMon", "skipTue", "skipWed", "skipThu", "skipFri", "skipSat", "skipSun",
];
fn load_recurring(token: &str, tenant: &Tenant, _: &Value) -> Result<Value, String> {
    let mut result = api::fetch_record(token, tenant, "/v1/erp/work-cases/recurring-definitions")?;
    let rows = result["items"]
        .as_array_mut()
        .ok_or("定期業務の一覧を確認できません")?;
    for row in rows {
        for (index, key) in SKIP_FIELDS.iter().enumerate() {
            row[*key] = json!(row["skipWeekdays"].as_array().is_some_and(|days| {
                days.iter()
                    .any(|day| day.as_u64() == Some(index as u64 + 1))
            }));
        }
    }
    Ok(result)
}
fn recurring_payload(body: &mut Value, original: &Value) -> Result<(), String> {
    let anchor = body["anchorDay"]
        .as_u64()
        .ok_or("基準日は0以上の整数を入力してください")?;
    let max = match body["cadence"].as_str() {
        Some("daily") => 0,
        Some("weekly") => 7,
        Some("monthly") => 31,
        _ => return Err("繰り返しを選んでください".into()),
    };
    if max == 0 {
        body["anchorDay"] = json!(0);
    } else if anchor == 0 || anchor > max {
        return Err(format!("この繰り返しの基準日は1〜{max}で入力してください"));
    }
    for key in ["leadDays", "dueOffsetDays"] {
        if body[key].as_u64().is_none() {
            return Err("作業開始と締切までの日数は0以上の整数を入力してください".into());
        }
    }
    if body["validUntil"]
        .as_str()
        .zip(body["validFrom"].as_str())
        .is_some_and(|(end, start)| end < start)
    {
        return Err("終了日は開始日以降を選んでください".into());
    }
    for closure in body["closures"].as_array().into_iter().flatten() {
        if closure["from"]
            .as_str()
            .zip(closure["until"].as_str())
            .is_some_and(|(start, end)| start > end)
        {
            return Err("作業しない期間の終了日を確認してください".into());
        }
    }
    let skips = if body["cadence"] == "daily" {
        SKIP_FIELDS
            .iter()
            .enumerate()
            .filter_map(|(index, key)| (body[*key] == true).then_some(index + 1))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if skips.len() == 7 {
        return Err("すべての曜日を除外することはできません".into());
    }
    for key in SKIP_FIELDS {
        body.as_object_mut()
            .ok_or("入力を確認できません")?
            .remove(*key);
    }
    body["skipWeekdays"] = json!(skips);
    if original["id"].is_string() {
        for key in [
            "accountingEntityId",
            "assignee",
            "conditions",
            "subjectScope",
        ] {
            body[key] = original[key].clone();
        }
        version(body, original)?;
    } else {
        body["accountingEntityId"] = Value::Null;
        body["assignee"] = Value::Null;
        body["subjectScope"] = Value::Null;
        body["conditions"] =
            json!([{"key":"manual_review","kind":"manual_check","required":true,"target":null}]);
    }
    Ok(())
}
fn generate_payload(body: &mut Value, original: &Value) -> Result<(), String> {
    confirmed(body)?;
    body["definitionId"] = original["id"]
        .as_str()
        .map(|s| json!(s))
        .ok_or("定期業務を読み直してください")?;
    Ok(())
}
fn generation_result(result: &Value) -> Result<(), String> {
    let failures = result["failures"]
        .as_array()
        .ok_or("生成結果を確認できません。業務一覧を読み直してください")?;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{}件を生成しましたが、{}件の生成に失敗しました。業務一覧を確認してください",
            model::input_text(&result["created"]),
            failures.len()
        ))
    }
}

fn review_command(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
    operation: &str,
) -> Result<Value, String> {
    let id = original["id"]
        .as_str()
        .ok_or("確認依頼を読み直してください")?;
    let path = format!(
        "/v1/erp/reconciliation/review-requests/{}",
        api::encode_query(id)
    );
    let fresh = api::fetch_record(token, tenant, &path)?;
    for key in [
        "status",
        "updatedAt",
        "subjectVersion",
        "subjectFingerprint",
        "targetId",
        "targetKind",
        "assigneeId",
        "answer",
    ] {
        if fresh[key] != original[key] {
            return Err(
                "確認依頼または回答が変わっています。読み直してから確認してください".into(),
            );
        }
    }
    api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{path}/{operation}"),
        body,
        None,
    )
}
fn assign_review(t: &str, n: &Tenant, b: &Value, o: &Value) -> Result<Value, String> {
    review_command(t, n, b, o, "assign")
}
fn answer_review(t: &str, n: &Tenant, b: &Value, o: &Value) -> Result<Value, String> {
    review_command(t, n, b, o, "answer")
}
fn return_review(t: &str, n: &Tenant, b: &Value, o: &Value) -> Result<Value, String> {
    review_command(t, n, b, o, "return")
}
fn confirm_review(t: &str, n: &Tenant, b: &Value, o: &Value) -> Result<Value, String> {
    review_command(t, n, b, o, "confirm")
}
fn tax_confirm_review(t: &str, n: &Tenant, b: &Value, o: &Value) -> Result<Value, String> {
    review_command(t, n, b, o, "tax-confirm")
}
fn cancel_review(t: &str, n: &Tenant, b: &Value, o: &Value) -> Result<Value, String> {
    review_command(t, n, b, o, "cancel")
}
fn confirm_fact(body: &mut Value, _: &Value) -> Result<(), String> {
    confirmed(body)?;
    body["confirmationKind"] = json!("business_fact");
    Ok(())
}
fn confirm_tax(body: &mut Value, _: &Value) -> Result<(), String> {
    confirmed(body)?;
    body["confirmationKind"] = json!("tax");
    Ok(())
}

fn month(value: &Value) -> Result<&str, String> {
    let month = value.as_str().ok_or("対象月を入力してください")?;
    let date = format!("{month}-01");
    if month.len() != 7 || chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
        return Err("対象月はYYYY-MM形式で入力してください".into());
    }
    Ok(month)
}
fn normalized_decimal(value: &Value) -> Result<String, String> {
    let raw = value.as_str().ok_or("金額の形式を確認できません")?;
    let (negative, raw) = raw.strip_prefix('-').map_or((false, raw), |s| (true, s));
    let mut parts = raw.split('.');
    let integer = parts.next().unwrap_or("");
    let fraction = parts.next().unwrap_or("");
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|c| c.is_ascii_digit())
        || !fraction.bytes().all(|c| c.is_ascii_digit())
    {
        return Err("金額の形式を確認できません".into());
    }
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let fraction = fraction.trim_end_matches('0');
    let sign = if negative && (integer != "0" || !fraction.is_empty()) {
        "-"
    } else {
        ""
    };
    Ok(if fraction.is_empty() {
        format!("{sign}{integer}")
    } else {
        format!("{sign}{integer}.{fraction}")
    })
}
fn combine_closing(
    summary: Value,
    readiness: Value,
    drilldown: Value,
    requested: &str,
) -> Result<Value, String> {
    for context in [&summary, &readiness, &drilldown] {
        if context["yearMonth"] != requested {
            return Err("月次締めの対象月が一致しません。読み直してください".into());
        }
        for key in ["companyId", "periodStart", "periodEnd", "status"] {
            if !summary[key].is_string() || summary[key] != context[key] {
                return Err("月次締めの会社・期間・状態が一致しません。読み直してください".into());
            }
        }
    }
    let balanced = normalized_decimal(&summary["trialBalance"]["debitTotal"])?
        == normalized_decimal(&summary["trialBalance"]["creditTotal"])?;
    if summary["trialBalance"]["balanced"].as_bool() != Some(balanced) {
        return Err("試算表の合計と貸借一致の判定が一致しません".into());
    }
    let blockers = readiness["blockers"]
        .as_array()
        .ok_or("締め前の対応事項を確認できません")?;
    let mut total = 0u64;
    let mut critical = 0u64;
    let mut warning = 0u64;
    for blocker in blockers {
        let count = blocker["count"]
            .as_u64()
            .ok_or("対応事項の件数を確認できません")?;
        total = total.checked_add(count).ok_or("件数が上限を超えています")?;
        if blocker["severity"] == "critical" {
            critical = critical
                .checked_add(count)
                .ok_or("件数が上限を超えています")?;
        }
        if blocker["severity"] == "warning" {
            warning = warning
                .checked_add(count)
                .ok_or("件数が上限を超えています")?;
        }
    }
    if readiness["blockerCount"].as_u64() != Some(total)
        || readiness["criticalCount"].as_u64() != Some(critical)
        || readiness["warningCount"].as_u64() != Some(warning)
    {
        return Err("締め前の対応事項と件数が一致しません".into());
    }
    let mut result = summary;
    let mut unresolved = Vec::new();
    for category in drilldown["categories"]
        .as_array()
        .ok_or("未完了事項の詳細を確認できません")?
    {
        for source in category["sources"]
            .as_array()
            .ok_or("未完了事項の明細を確認できません")?
        {
            let mut source = source.clone();
            source["categoryLabel"] = category["label"].clone();
            unresolved.push(source);
        }
    }
    result["readiness"] = readiness;
    result["unresolved"] = json!(unresolved);
    Ok(result)
}
fn load_closing(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let requested = month(&filters["yearMonth"])?;
    let path = format!("/v1/erp/monthly-closes/{}", api::encode_query(requested));
    let (summary, readiness, drilldown) = std::thread::scope(|scope| {
        let summary = scope.spawn(|| api::fetch_record(token, tenant, &path));
        let readiness =
            scope.spawn(|| api::fetch_record(token, tenant, &format!("{path}/close-readiness")));
        let drilldown = scope.spawn(|| {
            api::fetch_record(token, tenant, &format!("{path}/close-readiness/drilldown"))
        });
        let failed = || Err("月次締めの状態を取得できませんでした".to_owned());
        (
            summary.join().unwrap_or_else(|_| failed()),
            readiness.join().unwrap_or_else(|_| failed()),
            drilldown.join().unwrap_or_else(|_| failed()),
        )
    });
    combine_closing(
        summary?["summary"].clone(),
        readiness?["summary"].clone(),
        drilldown?["drilldown"].clone(),
        requested,
    )
}
fn can_close(record: &Value) -> bool {
    record["status"]
        .as_str()
        .is_some_and(|status| status != "closed")
        && record["readiness"]["criticalCount"].as_u64() == Some(0)
        && record["trialBalance"]["balanced"] == true
        && [
            "draftInvoiceCount",
            "unpostedJournalEntryCount",
            "unjournalizedSalesEventCount",
            "unreconciledPaymentCount",
        ]
        .iter()
        .all(|key| record[*key].as_u64() == Some(0))
}
fn close_payload(body: &mut Value, _: &Value) -> Result<(), String> {
    confirmed(body)?;
    let reason = body["reason"]
        .as_str()
        .map(str::trim)
        .ok_or("実行理由を入力してください")?;
    if reason.is_empty() || reason.chars().count() > 1000 {
        return Err("実行理由は1〜1000文字で入力してください".into());
    }
    body["reason"] = json!(reason);
    Ok(())
}
fn closing_command(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
    reopen: bool,
) -> Result<Value, String> {
    let requested = month(&original["yearMonth"])?;
    let fresh = load_closing(token, tenant, &json!({"yearMonth":requested}))?;
    if fresh["companyId"] != original["companyId"]
        || fresh["status"] != original["status"]
        || (reopen && fresh["status"] != "closed")
        || (!reopen && !can_close(&fresh))
    {
        return Err("月次締めの状態が変わりました。対象月を読み直してください".into());
    }
    let operation = if reopen { "reopen" } else { "close" };
    api::send_json(
        token,
        tenant,
        "POST",
        &format!(
            "/v1/erp/monthly-closes/{}/{operation}",
            api::encode_query(requested)
        ),
        body,
        Duration::from_secs(30),
    )
}
fn close_period(t: &str, n: &Tenant, b: &Value, o: &Value) -> Result<Value, String> {
    closing_command(t, n, b, o, false)
}
fn reopen_period(t: &str, n: &Tenant, b: &Value, o: &Value) -> Result<Value, String> {
    closing_command(t, n, b, o, true)
}
fn export_payload(body: &mut Value, original: &Value) -> Result<(), String> {
    confirmed(body)?;
    body["yearMonth"] = json!(month(&original["yearMonth"])?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn work_commands_keep_versions_and_do_not_complete_expired_or_incomplete_work() {
        let mut record = json!({"version":29,"status":"open","lease":{"holder":{"id":"us_a","kind":"user"},"until":"2030-01-01T00:00:00Z"},"conditions":[{"required":true,"satisfied":true}]});
        assert!(complete_at(&record, 0));
        record["conditions"][0]["satisfied"] = json!(false);
        assert!(!complete_at(&record, 0));
        record["conditions"][0]["satisfied"] = json!(true);
        assert!(!complete_at(&record, i64::MAX));
        let mut body = json!({"confirmed":true,"note":"確認済み"});
        complete_payload(&mut body, &record).unwrap();
        assert_eq!(body, json!({"note":"確認済み","expectedVersion":29}));
        assert!(version(&mut json!({}), &json!({})).is_err());
    }
    #[test]
    fn manual_conditions_cannot_replace_automatic_approval_conditions() {
        assert!(
            condition_payload(
                &mut json!({"confirmed":true}),
                &json!({"kind":"approved","version":3})
            )
            .is_err()
        );
        let mut body = json!({"confirmed":true});
        condition_payload(&mut body, &json!({"kind":"manual_check","version":3})).unwrap();
        assert_eq!(body, json!({"expectedVersion":3}));
    }
    fn snapshots() -> (Value, Value, Value) {
        let context = json!({"companyId":"company","yearMonth":"2026-10","periodStart":"2026-10-01","periodEnd":"2026-10-31","status":"open"});
        let mut summary = context.clone();
        summary["trialBalance"] = json!({"balanced":true,"debitTotal":"9007199254740993.00010","creditTotal":"09007199254740993.0001"});
        for key in [
            "draftInvoiceCount",
            "unpostedJournalEntryCount",
            "unjournalizedSalesEventCount",
            "unreconciledPaymentCount",
        ] {
            summary[key] = json!(0);
        }
        let mut readiness = context.clone();
        readiness["blockers"] = json!([]);
        for key in ["criticalCount", "warningCount", "blockerCount"] {
            readiness[key] = json!(0);
        }
        let mut drilldown = context;
        drilldown["categories"] = json!([]);
        (summary, readiness, drilldown)
    }
    #[test]
    fn closing_requires_matching_snapshots_exact_balances_and_complete_blocker_counts() {
        let (summary, readiness, drilldown) = snapshots();
        let result = combine_closing(
            summary.clone(),
            readiness.clone(),
            drilldown.clone(),
            "2026-10",
        )
        .unwrap();
        assert!(can_close(&result));
        let mut wrong = drilldown.clone();
        wrong["companyId"] = json!("another");
        assert!(combine_closing(summary.clone(), readiness.clone(), wrong, "2026-10").is_err());
        let mut wrong = readiness.clone();
        wrong["blockerCount"] = json!(1);
        assert!(combine_closing(summary.clone(), wrong, drilldown.clone(), "2026-10").is_err());
        let mut wrong = summary;
        wrong["trialBalance"]["creditTotal"] = json!("9007199254740993.00011");
        assert!(combine_closing(wrong, readiness, drilldown, "2026-10").is_err());
    }
    #[test]
    fn recurring_edits_preserve_existing_condition_targets_and_assignee() {
        let original = json!({"id":"rec_1","version":8,"title":"定期確認","active":true,"caseKind":"monthly_review","workArea":"accounting","cadence":"monthly","anchorDay":1,"leadDays":2,"dueOffsetDays":3,"validFrom":"2026-10-01","accountingEntityId":"company","assignee":{"id":"agent_a","kind":"ai_agent"},"conditions":[{"key":"prior","kind":"case_completed","required":true,"target":{"id":"work_1","kind":"work_case"}}],"subjectScope":"scope","closures":[],"skipSat":true});
        let operation = RESOURCES
            .iter()
            .find(|resource| resource.key == "recurring-work-definitions")
            .unwrap()
            .edit
            .unwrap();
        let body = model::request_body(
            operation,
            &model::draft(operation.fields, &original),
            &original,
            "",
        )
        .unwrap();
        assert_eq!(body["conditions"], original["conditions"]);
        assert_eq!(body["assignee"], original["assignee"]);
        assert_eq!(body["expectedVersion"], 8);
        assert_eq!(body["skipWeekdays"], json!([]));
    }
    #[test]
    fn tax_confirmation_uses_its_dedicated_contract_and_permission() {
        let operation = RESOURCES
            .iter()
            .find(|resource| resource.key == "review-requests")
            .unwrap()
            .actions
            .iter()
            .find(|operation| operation.path.ends_with("/tax-confirm"))
            .unwrap();
        assert_eq!(operation.capability, Some("accounting.confirmTax"));
        let body = model::request_body(
            *operation,
            &json!({"confirmed":true}),
            &json!({"status":"answered"}),
            "",
        )
        .unwrap();
        assert_eq!(body, json!({"confirmationKind":"tax"}));
    }
}
