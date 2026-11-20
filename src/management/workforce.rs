//! Counter, booking operations and workforce workflows with explicit API payloads.

use super::{model, schema::*};
use crate::api;
use chrono::{NaiveDate, NaiveTime};
use serde_json::{Value, json};

const CLIENTS: Lookup = Lookup {
    path: "/v1/erp/clients",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const CUSTOMERS: Lookup = Lookup {
    path: "/v1/erp/customers?limit=100",
    list_key: "data",
    value_key: "id",
    label_keys: &["name", "email"],
};
const STAFF: Lookup = Lookup {
    path: "/v1/erp/hrm/staff",
    list_key: "items",
    value_key: "id",
    label_keys: &["name", "jobTitle"],
};
const COURSES: Lookup = Lookup {
    path: "/v1/erp/extensions/golf-course/courses",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const PICKUP_STATUS: &[Choice] = &[
    Choice::new("placed", "注文受付"),
    Choice::new("confirmed", "確認済み"),
    Choice::new("preparing", "準備中"),
    Choice::new("ready", "準備完了"),
    Choice::new("picked_up", "受取完了"),
    Choice::new("cancelled", "取消済み"),
];
const PICKUP_FILTERS: &[Field] = &[Field::new(
    "status",
    "状態",
    InputKind::Select(PICKUP_STATUS),
)];
const CLASSIFICATION_FIELDS: &[Field] = &[
    Field::new(
        "salesChannel",
        "販売経路",
        InputKind::Select(&[
            Choice::new("direct", "直販"),
            Choice::new("partner", "パートナー"),
        ]),
    )
    .required(),
    Field::new(
        "signupStyle",
        "導入経路",
        InputKind::Select(&[
            Choice::new("selfService", "セルフサービス"),
            Choice::new("salesAssisted", "営業支援"),
        ]),
    )
    .required(),
    Field::new(
        "partnerClientId",
        "販売パートナー",
        InputKind::Reference(CLIENTS),
    )
    .empty(Empty::Null),
];
const BUDGET_VALUES: &[Field] = &[
    Field::new("targetRevenue", "目標売上（円）", InputKind::Integer)
        .required()
        .default("0"),
    Field::new("targetAverageSpend", "目標客単価（円）", InputKind::Integer)
        .required()
        .default("0"),
    Field::new(
        "targetCaddyAttachedRatio",
        "キャディ付き比率（0〜1）",
        InputKind::Decimal,
    )
    .required()
    .default("0")
    .help("0.25 は25%。小数4桁まで入力できます。"),
];
const BUDGET_CREATE: &[Field] = &[
    Field::new("golfCourseId", "コース", InputKind::Reference(COURSES)).required(),
    Field::new("date", "対象日", InputKind::Date)
        .required()
        .default("@today"),
    BUDGET_VALUES[0],
    BUDGET_VALUES[1],
    BUDGET_VALUES[2],
];
const BUDGET_FILTERS: &[Field] = &[
    Field::new("golfCourseId", "コース", InputKind::Reference(COURSES)),
    Field::new("from", "開始日", InputKind::Date).default("@monthStart"),
    Field::new("to", "終了日", InputKind::Date),
];
const SHIFT_VALUES: &[Field] = &[
    Field::text("startTime", "開始時刻")
        .required()
        .default("09:00")
        .help("24時間表記（例 09:00）"),
    Field::text("endTime", "終了時刻")
        .required()
        .default("18:00"),
    Field::text("shiftType", "シフト区分")
        .required()
        .default("regular"),
    Field::new("notes", "メモ", InputKind::Multiline).empty(Empty::Null),
];
const SHIFT_CREATE: &[Field] = &[
    Field::new("staffId", "スタッフ", InputKind::Reference(STAFF)).required(),
    Field::new("date", "勤務日", InputKind::Date)
        .required()
        .default("@today"),
    SHIFT_VALUES[0],
    SHIFT_VALUES[1],
    SHIFT_VALUES[2],
    SHIFT_VALUES[3],
];
const SHIFT_FILTERS: &[Field] = &[
    Field::new("startDate", "開始日", InputKind::Date)
        .required()
        .default("@today"),
    Field::new("endDate", "終了日", InputKind::Date)
        .required()
        .default("@today")
        .help("両端を含め42日以内で指定してください。"),
];
const MONTH_FILTERS: &[Field] = &[Field::text("month", "対象月")
    .required()
    .default("@month")
    .help("年と月を YYYY-MM で入力してください。締め・解除は全員が対象です。")];
const USAGE_FIELDS: &[Field] = &[
    Field::new("customerId", "利用者", InputKind::Reference(CUSTOMERS)).required(),
    Field::new("receivedAt", "受付日時（日本時間）", InputKind::DateTime)
        .required()
        .default("@now"),
    Field::new("note", "受付メモ", InputKind::Multiline).empty(Empty::String),
];
const CANCELLATION_FIELDS: &[Field] = &[
    Field::new("clientId", "請求先の取引先", InputKind::Reference(CLIENTS)),
    Field::text("clientName", "請求先名").required(),
    Field::new("clientEmail", "送付先メール", InputKind::Email).empty(Empty::Null),
    Field::text("clientPhone", "SMS送付先の電話番号").empty(Empty::Null),
    Field::new("amount", "キャンセル料（円）", InputKind::Integer).required(),
    Field::new("taxAmount", "税額（円）", InputKind::Integer).required().default("0"),
    Field::new("dueDate", "支払期限", InputKind::Date).required(),
    Field::text("reference", "対象の予約・注文"),
    Field::text("reason", "キャンセルの理由"),
    Field::new("notes", "請求メモ", InputKind::Multiline),
    Field::new("sendEmail", "メールで送信する", InputKind::Bool),
    Field::new("sendSms", "SMSで送信する", InputKind::Bool),
    Field::new("smsConsentConfirmed", "SMSの受信同意を確認済み", InputKind::Bool),
    Field::new("smsMessage", "SMS本文", InputKind::Multiline).default("キャンセル料 {amount}{currency} のお支払いをお願いします。\n支払期限: {dueDate}\nお支払いURL: {url}"),
];

pub const RESOURCES: &[Resource] = &[
    PLAN_RESOURCE,
    JOBCAN_SETTINGS_RESOURCE,
    JOBCAN_REVOCATION_RESOURCE,
    USAGE_SUBJECT_RESOURCE,
    USAGE_NEW_CUSTOMER_RESOURCE,
    Resource::new("tenant-sales-classifications", "販売管理", "テナント販売区分", "/v1/erp/sales-contract-tenant-classifications", "items")
        .id("tenantId").title(&["tenantName"]).subtitle(&["salesChannel", "signupStyle", "partnerClientName"])
        .fields(&[("テナント", "tenantName"), ("販売経路", "salesChannel"), ("導入経路", "signupStyle"), ("パートナー", "partnerClientName"), ("既定の分類", "isDefault"), ("更新日時", "updatedAt")])
        .edit(Mutation::new("販売区分を更新", "PUT", "/v1/erp/sales-contract-tenant-classifications/{id}", CLASSIFICATION_FIELDS).prepare(classification_payload))
        .route("tenant-sales-classifications"),
    Resource::new("store-pickup", "販売管理", "店舗受取", "/v1/storekit/orders", "items")
        .load(load_pickup).title(&["order_number", "shipping_name"]).subtitle(&["shipping_name", "totalNanodollar", "created_at"]).status("status")
        .filters(&[Field::new("status", "状態", InputKind::Select(PICKUP_STATUS))])
        .fields(&[("注文番号", "order_number"), ("受取人", "shipping_name"), ("電話番号", "shipping_phone"), ("金額", "totalNanodollar"), ("注文日時", "created_at"), ("受取期限", "pickup_deadline"), ("準備完了日時", "ready_at"), ("受取日時", "picked_up_at")])
        .actions(&[
            Mutation::new("準備完了にする", "POST", "/v1/erp/commerce/consumer-orders/{id}/ready", &[]).when(pickup_can_ready).explain("商品の準備が完了した注文を、受取可能にします。"),
            Mutation::new("受取完了にする", "POST", "/v1/erp/commerce/consumer-orders/{id}/pickup", &[]).when(pickup_can_complete).explain("本人確認と商品の引き渡しが済んでいることを確認してください。"),
            Mutation::new("注文を取り消す", "POST", "/v1/erp/commerce/consumer-orders/{id}/cancel", &[]).when(pickup_can_cancel).destructive().explain("この店舗受取注文を取り消します。"),
        ]).route("store/pickup"),
    Resource::new("usage-reception-new", "顧客管理", "利用受付を登録", "/v1/erp/usage-receptions", "items")
        .pagination(Pagination::Offset { limit: 100 }).title(&["customerName"]).subtitle(&["receivedAt", "note"])
        .fields(&[("利用者", "customerName"), ("受付日時", "receivedAt"), ("対象", "subjects"), ("受付メモ", "note"), ("取消日時", "cancelledAt")])
        .create(Mutation::new("利用受付を登録", "POST", "/v1/erp/usage-receptions", USAGE_FIELDS).prepare(usage_payload).idempotency_body("requestKey").explain("登録済みの利用者を選んで受付します。会員登録や対象者・対象物の追加を伴う手続きは別に行ってください。"))
        .route("usage-receptions/new"),
    Resource::new("reservation-daily-budgets", "予約管理", "日別予算", "/v1/erp/extensions/golf-course/daily-budgets", "items")
        .load(load_budgets).filters(BUDGET_FILTERS).title(&["date", "courseName"]).subtitle(&["courseName", "targetRevenue"])
        .fields(&[("コース", "courseName"), ("対象日", "date"), ("目標売上（円）", "targetRevenue"), ("目標客単価（円）", "targetAverageSpend"), ("キャディ付き比率（0〜1）", "targetCaddyAttachedRatio")])
        .create(Mutation::new("日別予算を登録", "POST", "/v1/erp/extensions/golf-course/daily-budgets", BUDGET_CREATE).prepare(budget_payload).execute(save_budget).explain("同じコース・日付の予算が登録済みなら、一覧で現在の金額を確認してから編集してください。"))
        .edit(Mutation::new("日別予算を更新", "POST", "/v1/erp/extensions/golf-course/daily-budgets", BUDGET_VALUES).prepare(budget_payload).execute(save_budget))
        .actions(&[Mutation::new("日別予算を削除", "DELETE", "/v1/erp/extensions/golf-course/daily-budgets/{golfCourseId}/{date}", &[]).execute(delete_budget).destructive().explain("このコース・日付の予算を削除します。")])
        .route("reservations/daily-budgets"),
    Resource::new("staff-shifts", "人事労務", "シフト管理", "/v1/erp/hrm/shift-board", "shifts")
        .load(load_shifts).filters(SHIFT_FILTERS).title(&["staffName", "date"]).subtitle(&["date", "startTime", "endTime", "shiftType"])
        .fields(&[("スタッフ", "staffName"), ("勤務日", "date"), ("開始時刻", "startTime"), ("終了時刻", "endTime"), ("シフト区分", "shiftType"), ("メモ", "notes")])
        .read_lines(&[Field::new("leaveRequests", "同日の休暇申請", InputKind::Lines(&[Field::text("requestType", "種類"), Field::text("status", "状態"), Field::text("reason", "理由")]))])
        .create(Mutation::new("シフトを追加", "POST", "/v1/erp/hrm/staff/{staffId}/shifts", SHIFT_CREATE).prepare(shift_payload).execute(create_shift).capability("hrm.manage"))
        .edit(Mutation::new("シフトを更新", "PATCH", "/v1/erp/hrm/staff/{staffId}/shifts/{id}", SHIFT_VALUES).prepare(shift_payload).capability("hrm.manage"))
        .actions(&[Mutation::new("シフトを削除", "DELETE", "/v1/erp/hrm/staff/{staffId}/shifts/{id}", &[]).destructive().capability("hrm.manage").explain("予定シフトを削除します。記録済みの勤怠は残ります。")])
        .route("hrm/shifts"),
    Resource::new("attendance-months", "人事労務", "月次勤怠", "/v1/erp/hrm/attendance-months/{month}", "")
        .load(load_attendance).id("month").filters(MONTH_FILTERS).title(&["month"]).status("status")
        .fields(&[("対象月", "month"), ("開始日", "startDate"), ("終了日", "endDate"), ("勤務日数", "totals.workedDays"), ("実働時間（分）", "totals.workedMinutes"), ("予定時間（分）", "totals.scheduledMinutes"), ("休憩時間（分）", "totals.breakMinutes"), ("締めを妨げる異常", "totals.blockingAnomalyCount"), ("警告数", "totals.warningCount"), ("締め版", "closedVersion"), ("集計日時", "asOf")])
        .read_lines(&[
            Field::new("staff", "スタッフ別集計", InputKind::Lines(&[Field::text("staffName", "スタッフ"), Field::new("totals.workedDays", "勤務日数", InputKind::Integer), Field::new("totals.workedMinutes", "実働（分）", InputKind::Integer), Field::new("totals.scheduledMinutes", "予定（分）", InputKind::Integer), Field::new("totals.breakMinutes", "休憩（分）", InputKind::Integer), Field::new("totals.blockingAnomalyCount", "異常", InputKind::Integer)])),
            Field::new("dayDetails", "日別勤怠", InputKind::Lines(&[Field::text("staffName", "スタッフ"), Field::text("date", "勤務日"), Field::new("workedMinutes", "実働（分）", InputKind::Integer), Field::new("scheduledMinutes", "予定（分）", InputKind::Integer), Field::new("breakMinutes", "休憩（分）", InputKind::Integer), Field::text("punchText", "出退勤"), Field::text("shiftText", "予定時刻"), Field::text("anomalyText", "確認事項")])),
            Field::new("blockingAnomalies", "締めを妨げる異常", InputKind::Lines(&[Field::text("staffName", "スタッフ"), Field::text("date", "日付"), Field::text("kind", "内容"), Field::text("severity", "重要度")])),
            Field::new("versions", "締め・解除履歴", InputKind::Lines(&[Field::new("version", "版", InputKind::Integer), Field::text("closedAt", "締め日時"), Field::text("reopenedAt", "解除日時"), Field::text("reopenReason", "解除理由")])),
        ])
        .actions(&[
            Mutation::new("月次勤怠を締める", "POST", "/v1/erp/hrm/attendance-months/{id}/close", &[]).when(attendance_can_close).capability("hrm.manage").explain("対象月の全スタッフの勤怠を締めます。異常や実働時間を確認してください。"),
            Mutation::new("締めを解除", "POST", "/v1/erp/hrm/attendance-months/{id}/reopen", &[Field::new("reason", "解除理由", InputKind::Multiline).required()]).when(attendance_closed).capability("hrm.manage").destructive().explain("対象月の締めを解除し、勤怠を再集計できる状態に戻します。理由は履歴に残ります。"),
            Mutation::new("保留中の外部勤怠を反映", "POST", "/v1/erp/hrm/attendance-sources/held/{id}/release", &[]).when(attendance_open).capability("hrm.manage").explain("対象月に保留されている外部勤怠を全員分反映します。反映後に月次集計を確認してください。"),
        ]).route("hrm/attendance"),
    Resource::new("jobcan-import-history", "人事労務", "ジョブカン取込履歴（最新50件）", "/v1/erp/hrm/attendance-sources/jobcan/csv-imports", "items")
        .title(&["fileName", "createdAt"]).subtitle(&["coveredFrom", "coveredTo", "createdAt"])
        .fields(&[("ファイル", "fileName"), ("対象期間の開始", "coveredFrom"), ("対象期間の終了", "coveredTo"), ("CSV出力日時", "exportedAt"), ("取込日時", "createdAt"), ("マッピング版", "mappingVersionNo"), ("勤務日数", "dayCount"), ("反映件数", "appliedCount"), ("保留件数", "heldCount"), ("変更なし", "unchangedCount"), ("古いデータ", "staleCount"), ("未対応のスタッフ", "unmappedDayCount"), ("除外行数", "rejectedRowCount"), ("取消日時", "revocation.revokedAt"), ("取消理由", "revocation.reason")])
        .route("hrm/attendance/jobcan-import"),
    Resource::new("cancellation-fees", "請求・債権管理", "キャンセル料請求", "/v1/invoices", "items")
        .load(load_cancellation_fees).title(&["invoiceNumber", "clientName"]).subtitle(&["clientName", "@money:totalAmount", "dueDate"]).status("status")
        .detail("/v1/invoices/{id}", "")
        .fields(&[("請求番号", "invoiceNumber"), ("請求先", "clientName"), ("金額", "@money:totalAmount"), ("税額", "@money:taxAmount"), ("支払期限", "dueDate"), ("メール送信", "emailDeliveryStatus"), ("SMS送信", "smsDeliveryStatus"), ("決済リンク", "paymentLinkUrl"), ("備考", "notes")])
        .read_lines(&[Field::new("lineItems", "請求明細", InputKind::Lines(&[Field::text("description", "内容"), Field::new("quantity", "数量", InputKind::Decimal), Field::new("unitPrice", "単価", InputKind::Money)]))])
        .create(Mutation::new("請求書を発行して送信", "POST", "/v1/invoices", CANCELLATION_FIELDS).prepare(cancellation_payload).idempotency_body("idempotencyKey").validate_response(invoice_delivery_result).explain("キャンセル料の請求書とStripeの決済リンクを作成し、選択したメール・SMSへ送信します。送付先と金額を確認してください。"))
        .route("cancellation-fees"),
];

const USAGE_CUSTOMER_FILTERS: &[Field] = &[Field::new(
    "customerId",
    "受付する利用者",
    InputKind::Reference(CUSTOMERS),
)
.required()];
const USAGE_SUBJECT_FIELDS: &[Field] = &[
    Field::new(
        "subjects",
        "今回の対象者・対象物",
        InputKind::Lines(&[
            Field::new("id", "", InputKind::Hidden),
            Field::new("name", "対象", InputKind::ReadOnly),
            Field::new("selected", "今回の受付対象に含める", InputKind::Bool),
        ]),
    )
    .fixed_rows(),
    USAGE_FIELDS[1],
    USAGE_FIELDS[2],
];
const USAGE_SUBJECT_RESOURCE: Resource = Resource::new(
    "usage-reception-subjects",
    "顧客管理",
    "対象者・対象物を選んで受付",
    "/v1/erp/membership/customers/{customerId}",
    "",
)
.load(load_usage_customer)
.filters(USAGE_CUSTOMER_FILTERS)
.title(&["name"])
.fields(&[("利用者", "name"), ("加入中のプラン", "activePlanName")])
.read_lines(&[Field::new(
    "subjects",
    "登録済みの対象者・対象物",
    InputKind::Lines(&[
        Field::text("name", "名前"),
        Field::text("subjectType", "種類"),
    ]),
)])
.actions(&[Mutation::new(
    "対象を選んで利用受付",
    "POST",
    "/v1/erp/usage-receptions",
    USAGE_SUBJECT_FIELDS,
)
.prepare(usage_subject_payload)
.idempotency_body("requestKey")])
.route("usage-receptions/new");
const USAGE_NEW_CUSTOMER_FIELDS: &[Field] = &[
    Field::text("newCustomer.name", "利用者名").required(),
    Field::text("newCustomer.nameKana", "フリガナ"),
    Field::text("newCustomer.phone", "電話番号"),
    Field::new("newCustomer.email", "メール", InputKind::Email),
    USAGE_FIELDS[1],
    USAGE_FIELDS[2],
];
const USAGE_NEW_CUSTOMER_RESOURCE: Resource = Resource::new("usage-reception-new-customer", "顧客管理", "初回の利用者を登録して受付", "/v1/erp/usage-receptions", "items")
    .pagination(Pagination::Offset { limit: 100 }).title(&["customerName"]).subtitle(&["receivedAt", "note"])
    .fields(&[("利用者", "customerName"), ("受付日時", "receivedAt"), ("メモ", "note")])
    .create(Mutation::new("利用者を登録して受付", "POST", "/v1/erp/usage-receptions", USAGE_NEW_CUSTOMER_FIELDS).prepare(usage_payload).idempotency_body("requestKey").explain("利用者の基本情報と受付を一緒に登録します。会員プラン加入・同意記録・証明書登録はこの操作に含まれません。"))
    .route("usage-receptions/new");

fn load_usage_customer(
    token: &str,
    tenant: &api::Tenant,
    filters: &Value,
) -> Result<Value, String> {
    let path = model::query_path(
        "/v1/erp/membership/customers/{customerId}",
        USAGE_CUSTOMER_FILTERS,
        filters,
    )?;
    let membership = api::fetch_optional_record(token, tenant, &path)?;
    let mut customer = if let Some(view) = membership {
        let mut customer = view["customer"].clone();
        if !customer.is_object() || !view["subjects"].is_array() {
            return Err("受付対象の会員情報を確認できません".into());
        }
        customer["subjects"] = json!(
            view["subjects"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|subject| subject["archived"] != true)
                .cloned()
                .collect::<Vec<_>>()
        );
        customer["activePlanName"] = view["activePlan"]["name"].clone();
        customer
    } else {
        let id = filters["customerId"]
            .as_str()
            .ok_or("利用者を選んでください")?;
        api::fetch_record(
            token,
            tenant,
            &format!("/v1/erp/customers/{}", api::encode_query(id)),
        )?
    };
    if customer["id"] != filters["customerId"] {
        return Err("受付する利用者を確認できません".into());
    }
    if !customer["subjects"].is_array() {
        customer["subjects"] = json!([]);
    }
    Ok(customer)
}

fn usage_subject_payload(body: &mut Value, original: &Value) -> Result<(), String> {
    let customer = original["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("受付する利用者を確認できません")?;
    let mut ids = Vec::new();
    for subject in body["subjects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|subject| subject["selected"] == true)
    {
        let id = subject["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("受付する対象を確認できません")?;
        if !original["subjects"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|known| known["id"] == id && known["archived"] != true)
        {
            return Err("利用者に登録されている対象を選んでください".into());
        }
        if !ids.iter().any(|known| known == id) {
            ids.push(id.to_owned());
        }
    }
    body["customerId"] = json!(customer);
    body["subjectIds"] = json!(ids);
    body.as_object_mut().unwrap().remove("subjects");
    Ok(())
}

const JOBCAN_IMPORTS: Lookup = Lookup {
    path: "/v1/erp/hrm/attendance-sources/jobcan/csv-imports",
    list_key: "items",
    value_key: "id",
    label_keys: &["fileName", "createdAt", "coveredFrom", "coveredTo"],
};
const JOBCAN_REVOCATION_FILTERS: &[Field] = &[Field::new(
    "importId",
    "取り消す取込",
    InputKind::Reference(JOBCAN_IMPORTS),
)
.required()
.help("最近50件の取込から選択できます。取消済みの取込は再度取り消せません。")];
const JOBCAN_REVOCATION_RESOURCE: Resource = Resource::new("jobcan-import-revocation", "人事労務", "ジョブカン取込の取消", "/v1/erp/hrm/attendance-sources/jobcan/csv-imports/{importId}/revocation/preview", "")
    .load(load_jobcan_revocation).filters(JOBCAN_REVOCATION_FILTERS).id("importId").title(&["fileName"])
    .fields(&[("対象ファイル", "fileName"), ("取込日時", "createdAt"), ("取消で閉じる保留記録", "revokedHeldRecords")])
    .read_lines(&[Field::new("days", "取消後の勤務日の確認", InputKind::Lines(&[
        Field::text("key.businessDate", "勤務日"), Field::text("key.staffExternalId", "ジョブカンのスタッフコード"),
        Field::new("before.content.workedMinutes", "取消前の実働（分）", InputKind::Integer),
        Field::new("after.content.workedMinutes", "取消後の実働（分）", InputKind::Integer),
        Field::text("outcome", "反映結果"), Field::new("closedVersion", "締め版", InputKind::Integer),
    ]))])
    .actions(&[Mutation::new("確認した取込を取り消す", "POST", "/v1/erp/hrm/attendance-sources/jobcan/csv-imports/{id}/revocation", &[Field::new("reason", "取消理由", InputKind::Multiline).required()]).execute(revoke_jobcan_import).capability("hrm.manage").destructive().explain("一覧の取消後の勤務日を確認してください。対象の取込を取り消し、理由を履歴に残します。締め済みの変更は保留されます。")])
    .read_capability("hrm.manage").route("hrm/attendance/jobcan-import");

fn load_jobcan_revocation(
    token: &str,
    tenant: &api::Tenant,
    filters: &Value,
) -> Result<Value, String> {
    let path = model::query_path(
        "/v1/erp/hrm/attendance-sources/jobcan/csv-imports/{importId}/revocation/preview",
        JOBCAN_REVOCATION_FILTERS,
        filters,
    )?;
    let imports = api::fetch_record(token, tenant, JOBCAN_IMPORTS.path)?;
    let items = model::extract_rows(&imports, "items")?;
    let selected = items
        .iter()
        .find(|row| row["id"] == filters["importId"])
        .ok_or("対象の取込を一覧から選び直してください")?;
    if selected["revocation"].is_object() {
        return Err("この取込は既に取り消されています".into());
    }
    let mut preview = api::mutate_record(token, tenant, "POST", &path, &json!({}), None)?;
    if preview["importId"] != filters["importId"] || !preview["days"].is_array() {
        return Err("取消の影響を確認できません".into());
    }
    preview["fileName"] = selected["fileName"].clone();
    preview["createdAt"] = selected["createdAt"].clone();
    Ok(preview)
}

fn revoke_jobcan_import(
    token: &str,
    tenant: &api::Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let id = original["importId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("取消対象を確認できません")?;
    let base = format!(
        "/v1/erp/hrm/attendance-sources/jobcan/csv-imports/{}/revocation",
        api::encode_query(id)
    );
    let latest = api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{base}/preview"),
        &json!({}),
        None,
    )?;
    if latest["importId"] != original["importId"]
        || latest["days"] != original["days"]
        || latest["revokedHeldRecords"] != original["revokedHeldRecords"]
    {
        return Err("取消の影響が変わりました。読み直して確認してください".into());
    }
    api::mutate_record(token, tenant, "POST", &base, body, None)
}

const JOBCAN_MAPPING_TARGETS: &[Choice] = &[
    Choice::new("staffCode", "スタッフコード"),
    Choice::new("staffName", "氏名"),
    Choice::new("affiliation", "所属"),
    Choice::new("businessDate", "日付"),
    Choice::new("clockIn", "出勤時刻"),
    Choice::new("clockOut", "退勤時刻"),
    Choice::new("roundedClockIn", "丸め出勤時刻"),
    Choice::new("roundedClockOut", "丸め退勤時刻"),
    Choice::new("breakMinutes", "休憩時間"),
    Choice::new("workedMinutes", "労働時間"),
    Choice::new("scheduledMinutes", "所定内労働時間"),
    Choice::new("overtimeMinutes", "残業時間"),
    Choice::new("lateNightMinutes", "深夜労働時間"),
    Choice::new("holidayMinutes", "休日労働時間"),
    Choice::new("leaveKind", "休暇区分"),
    Choice::new("leaveMinutes", "休暇時間"),
];
const JOBCAN_SETTINGS_FIELDS: &[Field] = &[
    Field::new(
        "textEncoding",
        "CSV文字コード",
        InputKind::Select(&[
            Choice::new("utf8", "UTF-8"),
            Choice::new("shift_jis", "Shift JIS / CP932"),
        ]),
    )
    .required()
    .read("format.textEncoding")
    .default("utf8"),
    Field::new("headerRow", "見出しの行番号", InputKind::Integer)
        .required()
        .read("format.headerRow")
        .default("1"),
    Field::new(
        "dateFormats",
        "日付形式（1行に1つ）",
        InputKind::StringLines,
    )
    .read("format.dateFormats")
    .help("例: %Y/%m/%d、%Y-%m-%d。空欄は既定の形式を使用します。"),
    Field::new(
        "mapping.fields",
        "CSV列の対応",
        InputKind::Lines(&[
            Field::text("source", "CSVの見出し").required(),
            Field::new(
                "target",
                "Fieldの項目",
                InputKind::Select(JOBCAN_MAPPING_TARGETS),
            )
            .required(),
            Field::new("required", "必須の列", InputKind::Bool),
            Field::new("approved", "対応を確認済み", InputKind::Bool),
        ]),
    )
    .required()
    .read("mapping.fields"),
    Field::new(
        "staffCodes",
        "スタッフコードの対応",
        InputKind::Lines(&[
            Field::text("externalCode", "ジョブカンのスタッフコード")
                .required()
                .help("先頭の0も含め、CSVと同じ表記で入力してください。"),
            Field::new(
                "internalValue",
                "Fieldのスタッフ",
                InputKind::Reference(STAFF),
            )
            .required(),
        ]),
    ),
    Field::new("reason", "設定変更の理由", InputKind::Multiline).required(),
];
const JOBCAN_SETTINGS_RESOURCE: Resource = Resource::new(
    "jobcan-import-settings",
    "人事労務",
    "ジョブカン取込設定",
    "/v1/erp/hrm/attendance-sources/jobcan/csv-settings",
    "",
)
.load(load_jobcan_settings)
.id("versionCount")
.title(&["displayName"])
.fields(&[
    ("登録済み設定の数", "versionCount"),
    ("最新の版", "latest.versionNo"),
    ("最新の作成日時", "latest.createdAt"),
    ("変更理由", "latest.reason"),
    ("文字コード", "format.textEncoding"),
    ("見出し行", "format.headerRow"),
])
.edit(
    Mutation::new(
        "新しい版として設定を保存",
        "POST",
        "/v1/erp/hrm/attendance-sources/jobcan/csv-settings",
        JOBCAN_SETTINGS_FIELDS,
    )
    .prepare(jobcan_settings_payload)
    .capability("hrm.manage")
    .explain("列とスタッフの対応を新しい設定版として保存します。過去の取込に使った版は残ります。"),
)
.route("hrm/attendance/jobcan-import");

fn load_jobcan_settings(token: &str, tenant: &api::Tenant, _: &Value) -> Result<Value, String> {
    let mut settings = api::fetch_record(
        token,
        tenant,
        "/v1/erp/hrm/attendance-sources/jobcan/csv-settings",
    )?;
    if !settings.is_object() {
        return Err("取込設定を確認できません".into());
    }
    settings["displayName"] = json!("ジョブカンCSVの取込設定");
    settings["format"] = if settings["latest"].is_object() {
        settings["latest"]["sourceFormat"].clone()
    } else {
        settings["assumedFormat"].clone()
    };
    settings["mapping"] = if settings["latest"].is_object() {
        settings["latest"]["mapping"].clone()
    } else {
        settings["assumedMapping"].clone()
    };
    settings["staffCodes"] = settings["latest"]["codeSets"]["staff"]
        .as_array()
        .cloned()
        .map(Value::Array)
        .unwrap_or_else(|| json!([]));
    Ok(settings)
}

fn jobcan_settings_payload(body: &mut Value, _: &Value) -> Result<(), String> {
    if body["headerRow"].as_u64().is_none_or(|n| n == 0) {
        return Err("見出し行は1以上で入力してください".into());
    }
    let fields = body["mapping"]["fields"]
        .as_array()
        .ok_or("CSV列の対応を設定してください")?;
    let mut targets = std::collections::HashSet::new();
    for field in fields {
        let target = field["target"].as_str().unwrap_or("");
        if !targets.insert(target) {
            return Err("同じField項目への対応を重複させないでください".into());
        }
        if field["approved"] != true {
            return Err("各列の対応を確認済みにしてください".into());
        }
    }
    for key in ["staffCode", "businessDate"] {
        if !fields
            .iter()
            .any(|field| field["target"] == key && field["required"] == true)
        {
            return Err("スタッフコードと日付を必須の列として設定してください".into());
        }
    }
    let mut codes = std::collections::HashSet::new();
    for code in body["staffCodes"].as_array().into_iter().flatten() {
        if !codes.insert(code["externalCode"].as_str().unwrap_or("")) {
            return Err("スタッフコードが重複しています".into());
        }
    }
    Ok(())
}

const PLAN_PATH: &str = "/v1/erp/reservation-products";
const PLAN_CONFIG_PATH: &str = "/v1/erp/extensions/golf_course/config";
const PLAN_MEMBERSHIPS: Lookup = Lookup {
    path: "/v1/erp/membership/plans?includeInactive=true",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const PLAN_TYPES: Lookup = Lookup {
    path: "/v1/erp/reservation-types/manage?includeInactive=true",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const PRODUCT_MASTERS: Lookup = Lookup {
    path: "/v1/order/products",
    list_key: "items",
    value_key: "id",
    label_keys: &["name", "skuCode"],
};
const PLAN_FIELDS: &[Field] = &[
    Field::text("name", "プラン名")
        .empty(Empty::String)
        .help("商品マスタを選択した場合は、商品名・内容を商品マスタから取得します。"),
    Field::new("description", "プランの内容", InputKind::Multiline).empty(Empty::String),
    Field::new("priceAmount", "料金（円）", InputKind::Integer)
        .required()
        .default("0"),
    Field::new("durationMinutes", "所要時間（分）", InputKind::Integer)
        .required()
        .default("240"),
    Field::new(
        "reservationTypeId",
        "予約種別",
        InputKind::Reference(PLAN_TYPES),
    )
    .empty(Empty::String),
    Field::new(
        "membershipAccess",
        "対象の会員種別",
        InputKind::Select(&[
            Choice::new("all", "全員に販売"),
            Choice::new("visitor", "ビジター"),
            Choice::new("plan", "会員プランを指定"),
        ]),
    )
    .required()
    .default("all"),
    Field::new(
        "membershipPlanId",
        "会員プラン",
        InputKind::Reference(PLAN_MEMBERSHIPS),
    )
    .empty(Empty::String),
    Field::new(
        "prepaymentPolicy",
        "前払い方針",
        InputKind::Select(&[
            Choice::new("none", "前払いなし"),
            Choice::new("optional", "任意"),
            Choice::new("deposit_required", "一部の前払いが必須"),
            Choice::new("full_required", "全額前払いが必須"),
        ]),
    )
    .required()
    .default("none"),
    Field::new("depositRatio", "前払い割合（0〜1）", InputKind::Decimal)
        .required()
        .default("0")
        .help("一部前払いの場合に使用します。0.3 は30%。"),
    Field::new("enabled", "公開予約ページに表示", InputKind::Bool).default("true"),
    Field::text("imageUrl", "公開画像のURL").empty(Empty::String),
];
const PLAN_CREATE_FIELDS: &[Field] = &[
    Field::new(
        "productMasterId",
        "商品マスタ（連携済みの会社では必須）",
        InputKind::Reference(PRODUCT_MASTERS),
    ),
    PLAN_FIELDS[0],
    PLAN_FIELDS[1],
    PLAN_FIELDS[2],
    PLAN_FIELDS[3],
    PLAN_FIELDS[4],
    PLAN_FIELDS[5],
    PLAN_FIELDS[6],
    PLAN_FIELDS[7],
    PLAN_FIELDS[8],
    PLAN_FIELDS[9],
    PLAN_FIELDS[10],
];
const PLAN_LINKED_FIELDS: &[Field] = &[
    Field::new("name", "商品名（商品管理で変更）", InputKind::ReadOnly),
    Field::new("description", "内容（商品管理で変更）", InputKind::ReadOnly),
    PLAN_FIELDS[2],
    PLAN_FIELDS[3],
    PLAN_FIELDS[4],
    PLAN_FIELDS[5],
    PLAN_FIELDS[6],
    PLAN_FIELDS[7],
    PLAN_FIELDS[8],
    PLAN_FIELDS[9],
    PLAN_FIELDS[10],
];
const PLAN_RESOURCE: Resource = Resource::new("reservation-plans", "予約管理", "予約プラン", PLAN_PATH, "items")
    .load(load_plans).title(&["name"]).subtitle(&["priceAmount", "durationMinutes"]).status("availabilityLabel")
    .fields(&[("プラン名", "name"), ("内容", "description"), ("料金（円）", "priceAmount"), ("所要時間（分）", "durationMinutes"), ("公開中", "available"), ("料金表への表示", "enabled"), ("前払い方針", "prepaymentPolicy"), ("前払い割合（0〜1）", "depositRatio"), ("画像", "imageUrl"), ("保存先", "sourceLabel")])
    .create(Mutation::new("予約プランを追加", "POST", PLAN_PATH, PLAN_CREATE_FIELDS).prepare(plan_payload).execute(save_plan).idempotency_body("id").explain("公開予約ページに掲載するプランを追加します。商品マスタを選んだ場合、名称・内容は商品管理の情報を使用します。"))
    .edit(Mutation::new("予約プランを編集", "PUT", "/v1/erp/reservation-products/{id}", PLAN_FIELDS).prepare(plan_payload).execute(save_plan).when(plan_unlinked))
    .actions(&[
        Mutation::new("予約プランを編集", "PUT", "/v1/erp/reservation-products/{id}", PLAN_LINKED_FIELDS).prepare(plan_payload).execute(save_plan).when(plan_linked),
        Mutation::new("予約プランを削除", "DELETE", "/v1/erp/reservation-products/{id}", &[]).execute(delete_plan).destructive().explain("公開予約ページからこのプランを削除します。実施前に販売中の予約を確認してください。"),
    ]).route("reservations/plans");

fn plan_linked(row: &Value) -> bool {
    row["productMasterId"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
}
fn plan_unlinked(row: &Value) -> bool {
    !plan_linked(row)
}

fn plan_view(raw: &Value, catalog: bool) -> Result<Value, String> {
    if !raw.is_object() {
        return Err("予約プランの応答形式を確認できません".into());
    }
    let mut view = if catalog {
        if raw["attributesJson"].is_object() {
            raw["attributesJson"].clone()
        } else if raw.get("attributesJson").is_some() {
            json!({"attributesJson":raw["attributesJson"]})
        } else {
            json!({})
        }
    } else {
        raw.clone()
    };
    if catalog {
        for key in [
            "id",
            "name",
            "durationMinutes",
            "bookingMode",
            "eligibleResourceIds",
            "ownerExtensionKey",
        ] {
            view[key] = raw[key].clone();
        }
        view["enabled"] = raw
            .get("planEnabled")
            .filter(|v| v.is_boolean())
            .unwrap_or(&raw["enabled"])
            .clone();
        view["available"] = json!(raw["enabled"] != false);
    } else {
        view["available"] = json!(raw["enabled"] != false && raw["available"] != false);
    }
    if view["id"].as_str().is_none_or(str::is_empty) {
        return Err("予約プランの識別情報を確認できません".into());
    }
    let policy = match view
        .get("prepaymentPolicy")
        .filter(|value| !value.is_null())
        .unwrap_or(&view["paymentRequirement"])
        .as_str()
    {
        Some("deposit" | "partial" | "deposit_required") => "deposit_required",
        Some("full" | "full_prepayment" | "full_required") => "full_required",
        Some("optional") => "optional",
        _ => "none",
    };
    view["prepaymentPolicy"] = json!(policy);
    for (key, fallback) in [
        ("description", json!("")),
        ("reservationTypeId", json!("")),
        ("membershipPlanId", json!("")),
        ("priceAmount", json!(0)),
        ("durationMinutes", json!(60)),
        ("prepaymentPolicy", json!("none")),
        ("depositRatio", json!(0)),
        ("imageUrl", json!("")),
        ("enabled", json!(true)),
    ] {
        if view[key].is_null() {
            view[key] = fallback;
        }
    }
    view["membershipAccess"] = json!(match view["membershipPlanId"].as_str().unwrap_or("") {
        "" => "all",
        "visitor" => "visitor",
        _ => "plan",
    });
    // The visitor is a business sentinel, not a row in the membership registry.
    if view["membershipAccess"] == "visitor" {
        view["membershipPlanId"] = json!("");
    }
    view["availabilityLabel"] = json!(if view["available"] == true {
        "公開中"
    } else {
        "停止中"
    });
    view["sourceLabel"] = json!(if catalog {
        "予約カタログ"
    } else {
        "業種別の予約設定"
    });
    view["_nativeStore"] = json!(if catalog { "catalog" } else { "config" });
    view["_nativeSource"] = raw.clone();
    Ok(view)
}

fn get_plan_catalog(token: &str, tenant: &api::Tenant) -> Result<Value, String> {
    let value = api::fetch_record(
        token,
        tenant,
        "/v1/erp/reservation-products?includeDisabled=true",
    )?;
    model::extract_rows(&value, "items")?;
    if !value.is_object() {
        return Err("予約カタログを確認できません".into());
    }
    Ok(value)
}

fn get_plan_config(token: &str, tenant: &api::Tenant) -> Result<Value, String> {
    let value = api::fetch_optional_record(
        token,
        tenant,
        "/v1/erp/extensions/golf_course/config?scopeType=tenant",
    )?
    .unwrap_or_else(|| json!({"configVersion":0,"configJson":{}}));
    if !value["configJson"].is_object() {
        return Err("予約プラン設定を確認できません".into());
    }
    if value["configJson"]
        .get("reservationProducts")
        .is_some_and(|v| !v.is_array())
    {
        return Err("予約プラン一覧の形式を確認できません".into());
    }
    Ok(value)
}

pub(crate) fn load_plans(token: &str, tenant: &api::Tenant, _: &Value) -> Result<Value, String> {
    let catalog = get_plan_catalog(token, tenant)?;
    let seeded = catalog["seeded"] == true;
    let source = if seeded {
        catalog.clone()
    } else {
        get_plan_config(token, tenant)?
    };
    let items = if seeded {
        source["items"]
            .as_array()
            .ok_or("予約カタログの一覧を確認できません")?
            .iter()
            .map(|row| plan_view(row, true))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        legacy_plan_views(&source)?
    };
    Ok(
        json!({"items":items,"seeded":seeded,"productMastersLinked":catalog["productMastersLinked"]}),
    )
}

fn legacy_plan_id(row: &Value, index: usize) -> Result<String, String> {
    if !row.is_object() {
        return Err("旧予約プランの形式を確認できません".into());
    }
    // Web uses this same positional fallback for pre-catalogue documents.
    Ok(row["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("plan_{}", index + 1)))
}

fn canonical_legacy_rows(config: &Value) -> Result<Vec<Value>, String> {
    let rows = match config.get("reservationProducts") {
        None => &[][..],
        Some(Value::Array(rows)) => rows.as_slice(),
        _ => return Err("旧予約プラン一覧を確認できません".into()),
    };
    let mut seen = std::collections::HashSet::new();
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let id = legacy_plan_id(row, index)?;
            if !seen.insert(id.clone()) {
                return Err("旧予約プランの識別情報が重複しています。設定の確認が必要です".into());
            }
            let mut row = row.clone();
            row["id"] = json!(id);
            Ok(row)
        })
        .collect()
}

fn legacy_snapshot(source: &Value) -> Value {
    json!({"configVersion":source["configVersion"],"configJson":source["configJson"]})
}

fn legacy_plan_views(source: &Value) -> Result<Vec<Value>, String> {
    let rows = canonical_legacy_rows(&source["configJson"])?;
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let mut view = plan_view(row, false)?;
            view["_nativeSource"] = source["configJson"]["reservationProducts"][index].clone();
            view["_nativeConfigSnapshot"] = legacy_snapshot(source);
            Ok(view)
        })
        .collect()
}

fn compare_legacy_snapshot(original: &Value, current: &Value) -> Result<(), String> {
    if !original.is_null() && original["_nativeConfigSnapshot"] != legacy_snapshot(current) {
        return Err("旧予約設定が変更されています。プランの並びと内容を読み直してから操作してください（409）".into());
    }
    Ok(())
}

fn plan_payload(body: &mut Value, original: &Value) -> Result<(), String> {
    if plan_linked(original) {
        body["productMasterId"] = original["productMasterId"].clone();
        body["name"] = original["name"].clone();
        body["description"] = original["description"].clone();
    }
    if body["name"].as_str().unwrap_or("").trim().is_empty() && !plan_linked(body) {
        return Err("プラン名または商品マスタを指定してください".into());
    }
    if body["name"].as_str().unwrap_or("").chars().count() > 255
        || body["description"].as_str().unwrap_or("").chars().count() > 4000
    {
        return Err("プラン名は255文字、内容は4000文字以内で入力してください".into());
    }
    if body["priceAmount"]
        .as_i64()
        .is_none_or(|n| !(0..=100_000_000).contains(&n))
    {
        return Err("料金は0〜100,000,000円の整数で入力してください".into());
    }
    if body["durationMinutes"]
        .as_i64()
        .is_none_or(|n| !(1..=1440).contains(&n))
    {
        return Err("所要時間は1〜1440分で入力してください".into());
    }
    body["depositRatio"] = if body["prepaymentPolicy"] == "deposit_required" {
        decimal_ratio(&body["depositRatio"])?
    } else {
        json!(0)
    };
    body["membershipPlanId"] = match body["membershipAccess"].as_str() {
        Some("all") => json!(""),
        Some("visitor") => json!("visitor"),
        Some("plan")
            if body["membershipPlanId"]
                .as_str()
                .is_some_and(|s| !s.is_empty()) =>
        {
            body["membershipPlanId"].clone()
        }
        _ => return Err("対象の会員プランを選択してください".into()),
    };
    body.as_object_mut().unwrap().remove("membershipAccess");
    if !original["id"].is_null() {
        body["id"] = original["id"].clone();
    }
    if let Some(url) = body["imageUrl"].as_str().filter(|s| !s.is_empty()) {
        if !url.starts_with("https://") {
            return Err("画像はHTTPSの公開URLを指定してください".into());
        }
    }
    Ok(())
}

fn compare_plan_source(
    original: &Value,
    current: Option<&Value>,
    catalog: bool,
) -> Result<(), String> {
    if original.is_null() {
        return Ok(());
    }
    let store = if catalog { "catalog" } else { "config" };
    if original["_nativeStore"] != store {
        return Err("予約プランの保存先が変更されました。一覧を読み直して編集してください".into());
    }
    if current != Some(&original["_nativeSource"]) {
        return Err(
            "予約プランが更新されています。一覧を読み直して変更内容を確認してください".into(),
        );
    }
    Ok(())
}

fn merge_plan(source: Option<&Value>, body: &Value) -> Value {
    let mut plan = source
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    for key in [
        "id",
        "name",
        "description",
        "priceAmount",
        "durationMinutes",
        "prepaymentPolicy",
        "depositRatio",
        "enabled",
    ] {
        plan[key] = body[key].clone();
    }
    for key in [
        "membershipPlanId",
        "reservationTypeId",
        "imageUrl",
        "productMasterId",
    ] {
        if body[key].as_str().is_some_and(|s| !s.is_empty()) {
            plan[key] = body[key].clone();
        } else {
            plan.as_object_mut().unwrap().remove(key);
        }
    }
    // The canonical policy replaces its legacy spelling. Retaining both could
    // restore an obsolete deposit/full-prepayment requirement on a later read.
    plan.as_object_mut().unwrap().remove("paymentRequirement");
    plan
}

fn catalog_plan_payload(source: Option<&Value>, body: &Value) -> Value {
    let attributes = source.map(|s| &s["attributesJson"]);
    let mut attrs = match attributes {
        Some(v) if v.is_object() => v.clone(),
        Some(v) => json!({"attributesJson":v}),
        None => json!({}),
    };
    attrs = merge_plan(Some(&attrs), body);
    for key in [
        "id",
        "name",
        "enabled",
        "durationMinutes",
        "bookingMode",
        "eligibleResourceIds",
        "ownerExtensionKey",
        "slots",
    ] {
        attrs.as_object_mut().unwrap().remove(key);
    }
    json!({
        "attributesJson":attrs,
        "name":body["name"], "enabled":body["enabled"], "durationMinutes":body["durationMinutes"],
        "bookingMode":source.and_then(|s| s["bookingMode"].as_str()).filter(|s| *s == "time").unwrap_or("slot"),
        "ownerExtensionKey":source.and_then(|s| s["ownerExtensionKey"].as_str()).filter(|s| !s.is_empty()).unwrap_or("golf_course"),
        "eligibleResourceIds":source.map(|s| s["eligibleResourceIds"].clone()).unwrap_or(Value::Null)
    })
}

fn merge_plan_config(mut config: Value, plan: &Value, delete: bool) -> Result<Value, String> {
    let id = plan["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("予約プランの識別情報を確認できません")?;
    // Assign missing positional IDs before deletion can shift the remaining
    // positions. Non-identity values, including unknown fields, stay untouched.
    let mut plans = canonical_legacy_rows(&config)?;
    let index = plans.iter().position(|row| row["id"] == id);
    if delete {
        if let Some(index) = index {
            plans.remove(index);
        } else {
            return Err("削除対象の予約プランを確認できません".into());
        }
    } else if let Some(index) = index {
        plans[index] = merge_plan(Some(&plans[index]), plan);
    } else {
        plans.push(merge_plan(None, plan));
    }
    config["reservationProducts"] = json!(plans);
    for (key, value) in [
        ("cartPolicy", json!("optional")),
        ("defaultHoles", json!(18)),
        ("maxPlayersPerTeeTime", json!(4)),
    ] {
        if config.get(key).is_none() {
            config[key] = value;
        }
    }
    Ok(json!({"scopeType":"tenant","configJson":config}))
}

fn save_plan(
    token: &str,
    tenant: &api::Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let mut body = body.clone();
    let catalog = get_plan_catalog(token, tenant)?;
    let seeded = catalog["seeded"] == true;
    let id = body["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("予約プランの登録準備ができませんでした")?
        .to_owned();
    if catalog["productMastersLinked"] == true && !plan_linked(&body) {
        return Err("この会社では商品マスタの選択が必要です".into());
    }
    if let Some(master) = body["productMasterId"].as_str().filter(|s| !s.is_empty()) {
        let product = api::fetch_record(
            token,
            tenant,
            &format!("/v1/order/products/{}", api::encode_query(master)),
        )?;
        if product["name"].as_str().is_none_or(str::is_empty) {
            return Err("商品マスタの名称を確認できません".into());
        }
        body["name"] = product["name"].clone();
        body["description"] = product["description"]
            .as_str()
            .map(|s| json!(s))
            .unwrap_or_else(|| json!(""));
    }
    if seeded {
        let current = catalog["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id);
        compare_plan_source(original, current, true)?;
        let mut payload = catalog_plan_payload(current, &body);
        if original.is_null() {
            payload["id"] = json!(id);
            if let Some(current) = current {
                let comparable = catalog_plan_payload(Some(current), &body);
                let mut stored = plan_view(current, true)?;
                if stored["membershipAccess"] == "visitor" {
                    stored["membershipPlanId"] = json!("visitor");
                }
                if comparable == catalog_plan_payload(Some(current), &stored) {
                    return Ok(current.clone());
                }
                return Err(
                    "同じ登録番号のプランが既にあります。一覧で登録結果を確認してください".into(),
                );
            }
        }
        let path = if original.is_null() {
            PLAN_PATH.to_owned()
        } else {
            format!("{PLAN_PATH}/{}", api::encode_query(&id))
        };
        api::mutate_record(
            token,
            tenant,
            if original.is_null() { "POST" } else { "PUT" },
            &path,
            &payload,
            None,
        )
    } else {
        let source = get_plan_config(token, tenant)?;
        compare_legacy_snapshot(original, &source)?;
        let views = legacy_plan_views(&source)?;
        let current = views
            .iter()
            .find(|row| row["id"] == id)
            .map(|view| &view["_nativeSource"]);
        compare_plan_source(original, current, false)?;
        if original.is_null() && current.is_some() {
            if current == Some(&merge_plan(current, &body)) {
                return Ok(source);
            }
            return Err(
                "同じ登録番号のプランが既にあります。一覧で登録結果を確認してください".into(),
            );
        }
        let payload = merge_plan_config(source["configJson"].clone(), &body, false)?;
        api::mutate_record(token, tenant, "PATCH", PLAN_CONFIG_PATH, &payload, None)
    }
}

fn delete_plan(
    token: &str,
    tenant: &api::Tenant,
    _: &Value,
    original: &Value,
) -> Result<Value, String> {
    let catalog = get_plan_catalog(token, tenant)?;
    let seeded = catalog["seeded"] == true;
    let id = original["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("削除対象の予約プランを確認できません")?;
    if seeded {
        let current = catalog["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id);
        compare_plan_source(original, current, true)?;
        api::mutate_record(
            token,
            tenant,
            "DELETE",
            &format!("{PLAN_PATH}/{}", api::encode_query(id)),
            &json!({}),
            None,
        )
    } else {
        let source = get_plan_config(token, tenant)?;
        compare_legacy_snapshot(original, &source)?;
        let views = legacy_plan_views(&source)?;
        let current = views
            .iter()
            .find(|row| row["id"] == id)
            .map(|view| &view["_nativeSource"]);
        compare_plan_source(original, current, false)?;
        let payload = merge_plan_config(source["configJson"].clone(), &json!({"id":id}), true)?;
        api::mutate_record(token, tenant, "PATCH", PLAN_CONFIG_PATH, &payload, None)
    }
}

fn classification_payload(body: &mut Value, _: &Value) -> Result<(), String> {
    match body["salesChannel"].as_str() {
        Some("direct") => body["partnerClientId"] = Value::Null,
        Some("partner") => {
            if body["partnerClientId"].as_str().is_none_or(str::is_empty) {
                return Err("販売パートナーを選んでください".into());
            }
            if body["signupStyle"] != "salesAssisted" {
                return Err("パートナー販売では導入経路を営業支援にしてください".into());
            }
        }
        _ => return Err("販売経路を確認してください".into()),
    }
    Ok(())
}

fn pickup_can_ready(row: &Value) -> bool {
    row["fulfillment_method"] == "pickup"
        && matches!(row["status"].as_str(), Some("placed" | "preparing"))
}
fn pickup_can_complete(row: &Value) -> bool {
    row["fulfillment_method"] == "pickup" && row["status"] == "ready"
}
fn pickup_can_cancel(row: &Value) -> bool {
    row["fulfillment_method"] == "pickup"
        && matches!(
            row["status"].as_str(),
            Some("placed" | "confirmed" | "preparing" | "ready")
        )
}

fn filtered_collection(mut collection: model::Collection, keep: impl Fn(&Value) -> bool) -> Value {
    collection.rows.retain(keep);
    json!({"items": collection.rows, "truncated": collection.truncated})
}

fn load_pickup(token: &str, tenant: &api::Tenant, filters: &Value) -> Result<Value, String> {
    let raw = Resource::new("pickup-raw", "", "", "/v1/storekit/orders", "items")
        .pagination(Pagination::Offset { limit: 100 })
        .filters(PICKUP_FILTERS);
    let mut response = filtered_collection(
        model::fetch_collection(token, tenant, raw, filters)?,
        |row| row["fulfillment_method"] == "pickup",
    );
    for row in response["items"].as_array_mut().into_iter().flatten() {
        row["totalNanodollar"] = row["total_nanodollar"].clone();
    }
    Ok(response)
}

fn usage_payload(body: &mut Value, _: &Value) -> Result<(), String> {
    body["subjectIds"] = json!([]);
    Ok(())
}

fn nonnegative(body: &Value, key: &str, label: &str) -> Result<(), String> {
    if body[key].as_i64().is_none_or(|n| n < 0) {
        return Err(format!("{label}は0以上の整数で入力してください"));
    }
    Ok(())
}

fn decimal_ratio(value: &Value) -> Result<Value, String> {
    let text = model::input_text(value);
    let (whole, fractional) = text.split_once('.').unwrap_or((&text, ""));
    if !matches!(whole, "0" | "1")
        || fractional.len() > 4
        || !fractional.bytes().all(|b| b.is_ascii_digit())
        || (whole == "1" && fractional.bytes().any(|b| b != b'0'))
    {
        return Err("比率は0〜1の小数4桁以内で入力してください".into());
    }
    serde_json::from_str(&text).map_err(|_| "比率を数値で入力してください".into())
}

fn budget_payload(body: &mut Value, original: &Value) -> Result<(), String> {
    for key in ["golfCourseId", "date"] {
        if !original[key].is_null() {
            body[key] = original[key].clone();
        }
        if body[key].as_str().is_none_or(str::is_empty) {
            return Err("コースと対象日を指定してください".into());
        }
    }
    NaiveDate::parse_from_str(body["date"].as_str().unwrap_or_default(), "%Y-%m-%d")
        .map_err(|_| "対象日は有効な YYYY-MM-DD で入力してください")?;
    nonnegative(body, "targetRevenue", "目標売上")?;
    nonnegative(body, "targetAverageSpend", "目標客単価")?;
    body["targetCaddyAttachedRatio"] = decimal_ratio(&body["targetCaddyAttachedRatio"])?;
    Ok(())
}

fn load_budgets(token: &str, tenant: &api::Tenant, filters: &Value) -> Result<Value, String> {
    budget_range(filters)?;
    let path = model::query_path(
        "/v1/erp/extensions/golf-course/daily-budgets",
        BUDGET_FILTERS,
        filters,
    )?;
    let mut response = api::fetch_record(token, tenant, &path)?;
    let courses = api::fetch_record(token, tenant, COURSES.path)?;
    let names = model::extract_rows(&courses, "items")?;
    for row in response["items"]
        .as_array_mut()
        .ok_or("日別予算の応答形式を確認できません")?
    {
        row["courseName"] = names
            .iter()
            .find(|course| course["id"] == row["golfCourseId"])
            .map(|course| course["name"].clone())
            .unwrap_or_else(|| json!("コース名未設定"));
    }
    Ok(response)
}

fn budget_range(filters: &Value) -> Result<(), String> {
    let parse = |key: &str| -> Result<Option<NaiveDate>, String> {
        filters[key]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(|value| {
                NaiveDate::parse_from_str(value, "%Y-%m-%d")
                    .map_err(|_| "予算の対象日を確認してください".to_owned())
            })
            .transpose()
    };
    if matches!((parse("from")?, parse("to")?), (Some(from), Some(to)) if from > to) {
        return Err("終了日は開始日以降にしてください".into());
    }
    Ok(())
}

fn budget_current(
    token: &str,
    tenant: &api::Tenant,
    scope: &Value,
) -> Result<Option<Value>, String> {
    let course = scope["golfCourseId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("コースを確認してください")?;
    let day = scope["date"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("対象日を確認してください")?;
    NaiveDate::parse_from_str(day, "%Y-%m-%d").map_err(|_| "対象日を確認してください")?;
    let response = api::fetch_record(
        token,
        tenant,
        &format!(
            "/v1/erp/extensions/golf-course/daily-budgets?golfCourseId={}&from={}&to={}",
            api::encode_query(course),
            api::encode_query(day),
            api::encode_query(day)
        ),
    )?;
    let rows = response["items"]
        .as_array()
        .ok_or("日別予算の応答形式を確認できません")?;
    if rows.len() > 1
        || rows.iter().any(|row| {
            row["golfCourseId"].as_str() != Some(course) || row["date"].as_str() != Some(day)
        })
    {
        return Err("対象のコース・日付の予算を確認できません".into());
    }
    Ok(rows.first().cloned())
}

fn compare_budget(original: &Value, current: Option<&Value>) -> Result<(), String> {
    if original.is_null() {
        if current.is_some() {
            return Err("このコース・日付の予算は登録済みです。一覧から現在の金額を確認して編集してください".into());
        }
    } else {
        let current = current.ok_or("予算が削除されています。一覧を読み直してください")?;
        for key in [
            "id",
            "golfCourseId",
            "date",
            "targetRevenue",
            "targetAverageSpend",
            "targetCaddyAttachedRatio",
            "updatedAt",
        ] {
            if original[key] != current[key] {
                return Err(
                    "予算が変更されています。一覧を読み直してから編集してください（409）".into(),
                );
            }
        }
    }
    Ok(())
}

fn save_budget(
    token: &str,
    tenant: &api::Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let courses = api::fetch_record(token, tenant, COURSES.path)?;
    let course = courses["items"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["id"] == body["golfCourseId"]))
        .ok_or("コースを確認できません。候補を読み直してください")?;
    if original.is_null() && course["isActive"] != true {
        return Err("新しい予算は有効なコースを選択してください".into());
    }
    let current = budget_current(token, tenant, body)?;
    compare_budget(original, current.as_ref())?;
    let result = api::mutate_record(
        token,
        tenant,
        "POST",
        "/v1/erp/extensions/golf-course/daily-budgets",
        body,
        None,
    )?;
    check_saved_budget(body, &result)?;
    Ok(result)
}

fn check_saved_budget(body: &Value, result: &Value) -> Result<(), String> {
    for key in [
        "golfCourseId",
        "date",
        "targetRevenue",
        "targetAverageSpend",
    ] {
        if result[key] != body[key] {
            return Err("予算の保存結果を確認できません。一覧を読み直してください".into());
        }
    }
    // The API serializes this bounded fractional rate as f64 (including 0.0).
    // Currency amounts above stay integers and never pass through float conversion.
    if result["targetCaddyAttachedRatio"].as_f64().is_none()
        || result["targetCaddyAttachedRatio"].as_f64() != body["targetCaddyAttachedRatio"].as_f64()
    {
        return Err("予算の保存結果を確認できません。一覧を読み直してください".into());
    }
    Ok(())
}

fn delete_budget(
    token: &str,
    tenant: &api::Tenant,
    _: &Value,
    original: &Value,
) -> Result<Value, String> {
    let current = budget_current(token, tenant, original)?;
    compare_budget(original, current.as_ref())?;
    let path = model::record_path(
        Resource::new(
            "budget",
            "",
            "",
            "/v1/erp/extensions/golf-course/daily-budgets",
            "items",
        ),
        "/v1/erp/extensions/golf-course/daily-budgets/{golfCourseId}/{date}",
        original,
    )?;
    api::mutate_record(token, tenant, "DELETE", &path, &json!({}), None)
}

fn shift_payload(body: &mut Value, _: &Value) -> Result<(), String> {
    let parse = |key: &str| -> Result<NaiveTime, String> {
        let value = body[key].as_str().unwrap_or("");
        NaiveTime::parse_from_str(value, "%H:%M")
            .or_else(|_| NaiveTime::parse_from_str(value, "%H:%M:%S"))
            .map_err(|_| "勤務時刻は HH:MM で入力してください".into())
    };
    let start = parse("startTime")?;
    let end = parse("endTime")?;
    if end <= start {
        return Err("終了時刻は開始時刻より後にしてください".into());
    }
    if body["shiftType"].as_str().unwrap_or("").chars().count() > 50 {
        return Err("シフト区分は50文字以内にしてください".into());
    }
    if body["notes"].as_str().unwrap_or("").chars().count() > 1000 {
        return Err("メモは1000文字以内にしてください".into());
    }
    body["startTime"] = json!(start.format("%H:%M").to_string());
    body["endTime"] = json!(end.format("%H:%M").to_string());
    Ok(())
}

fn create_shift(
    token: &str,
    tenant: &api::Tenant,
    body: &Value,
    _: &Value,
) -> Result<Value, String> {
    let staff = body["staffId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("スタッフを選んでください")?;
    let path = format!("/v1/erp/hrm/staff/{}/shifts", api::encode_query(staff));
    let mut payload = body.clone();
    payload
        .as_object_mut()
        .ok_or("入力を確認してください")?
        .remove("staffId");
    api::mutate_record(token, tenant, "POST", &path, &payload, None)
}

fn shift_range(filters: &Value) -> Result<(), String> {
    let start = NaiveDate::parse_from_str(filters["startDate"].as_str().unwrap_or(""), "%Y-%m-%d")
        .map_err(|_| "開始日を指定してください")?;
    let end = NaiveDate::parse_from_str(filters["endDate"].as_str().unwrap_or(""), "%Y-%m-%d")
        .map_err(|_| "終了日を指定してください")?;
    if !(0..42).contains(&(end - start).num_days()) {
        return Err("シフトの期間は両端を含め42日以内で指定してください".into());
    }
    Ok(())
}

fn load_shifts(token: &str, tenant: &api::Tenant, filters: &Value) -> Result<Value, String> {
    shift_range(filters)?;
    let path = model::query_path("/v1/erp/hrm/shift-board", SHIFT_FILTERS, filters)?;
    let mut board = api::fetch_record(token, tenant, &path)?;
    let staff = api::fetch_record(token, tenant, STAFF.path)?;
    let names = model::extract_rows(&staff, "items")?;
    let leave = model::extract_rows(&board, "leaveRequests")?;
    for shift in board["shifts"]
        .as_array_mut()
        .ok_or("シフトの応答形式を確認できません")?
    {
        shift["staffName"] = names
            .iter()
            .find(|person| person["id"] == shift["staffId"])
            .map(|person| person["name"].clone())
            .unwrap_or_else(|| json!("スタッフ名未設定"));
        shift["leaveRequests"] = json!(
            leave
                .iter()
                .filter(|request| request["staffId"] == shift["staffId"]
                    && request["date"] == shift["date"])
                .collect::<Vec<_>>()
        );
    }
    Ok(board)
}

fn attendance_closed(value: &Value) -> bool {
    value["status"] == "closed"
}
fn attendance_open(value: &Value) -> bool {
    value["status"] == "open"
}
fn attendance_can_close(value: &Value) -> bool {
    attendance_open(value) && value["closable"] == true
}

fn attendance_details(mut response: Value) -> Result<Value, String> {
    let staff = response["staff"]
        .as_array()
        .ok_or("月次勤怠の応答形式を確認できません")?;
    let mut days = Vec::new();
    for person in staff {
        for day in person["days"].as_array().into_iter().flatten() {
            let mut detail = day.clone();
            detail["staffName"] = person["staffName"].clone();
            detail["punchText"] = json!(
                day["punches"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|punch| format!(
                        "{} 〜 {}",
                        model::display(&punch["clockIn"]),
                        punch["clockOut"].as_str().unwrap_or("退勤未記録")
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            detail["shiftText"] = json!(
                day["shifts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|shift| format!(
                        "{} 〜 {}",
                        model::display(&shift["startTime"]),
                        model::display(&shift["endTime"])
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            detail["anomalyText"] = json!(
                day["anomalies"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|anomaly| model::display(&anomaly["kind"]))
                    .collect::<Vec<_>>()
                    .join("、")
            );
            days.push(detail);
        }
    }
    response["dayDetails"] = json!(days);
    Ok(response)
}

fn load_attendance(token: &str, tenant: &api::Tenant, filters: &Value) -> Result<Value, String> {
    let month = filters["month"].as_str().unwrap_or("");
    if month.len() != 7 || NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").is_err() {
        return Err("対象月は YYYY-MM で入力してください".into());
    }
    let path = model::query_path(
        "/v1/erp/hrm/attendance-months/{month}",
        MONTH_FILTERS,
        filters,
    )?;
    attendance_details(api::fetch_record(token, tenant, &path)?)
}

fn cancellation_invoice(row: &Value) -> bool {
    row["notes"]
        .as_str()
        .is_some_and(|text| text.contains("キャンセル料のご請求です。"))
        || row["lineItems"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|line| {
                line["description"]
                    .as_str()
                    .is_some_and(|text| text.trim().starts_with("キャンセル料"))
            })
}

fn load_cancellation_fees(token: &str, tenant: &api::Tenant, _: &Value) -> Result<Value, String> {
    let raw = Resource::new("cancellation-invoices-raw", "", "", "/v1/invoices", "items")
        .pagination(Pagination::Offset { limit: 100 });
    Ok(filtered_collection(
        model::fetch_collection(token, tenant, raw, &json!({}))?,
        cancellation_invoice,
    ))
}

fn normalize_phone(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let digits: String = trimmed
        .chars()
        .filter_map(|c| {
            if c.is_ascii_digit() {
                Some(c)
            } else if ('０'..='９').contains(&c) {
                char::from_u32(c as u32 - '０' as u32 + '0' as u32)
            } else {
                None
            }
        })
        .collect();
    if digits.is_empty() {
        return None;
    }
    let number = if trimmed.starts_with('+') || digits.starts_with("81") {
        format!("+{digits}")
    } else {
        format!("+81{}", digits.strip_prefix('0').unwrap_or(&digits))
    };
    (number.len() >= 9 && number.len() <= 16).then_some(number)
}

fn cancellation_payload(body: &mut Value, _: &Value) -> Result<(), String> {
    let source = body.clone();
    let amount = source["amount"]
        .as_i64()
        .filter(|n| *n >= 1)
        .ok_or("キャンセル料は1円以上の整数で入力してください")?;
    nonnegative(&source, "taxAmount", "税額")?;
    amount
        .checked_add(source["taxAmount"].as_i64().unwrap())
        .ok_or("請求金額が大きすぎます")?;
    let email = source["sendEmail"].as_bool().unwrap_or(false);
    let sms = source["sendSms"].as_bool().unwrap_or(false);
    if !email && !sms {
        return Err("メールまたはSMSの送信方法を選んでください".into());
    }
    if email
        && source["clientEmail"]
            .as_str()
            .is_none_or(|s| !s.contains('@'))
    {
        return Err("送付先メールを入力してください".into());
    }
    let phone = normalize_phone(source["clientPhone"].as_str().unwrap_or(""));
    if sms && (phone.is_none() || source["smsConsentConfirmed"] != true) {
        return Err("SMSの電話番号と受信同意を確認してください".into());
    }
    for (key, maximum, label) in [
        ("reference", 255, "対象"),
        ("reason", 255, "理由"),
        ("notes", 2000, "メモ"),
        ("smsMessage", 1000, "SMS本文"),
    ] {
        if source[key].as_str().unwrap_or("").chars().count() > maximum {
            return Err(format!("{label}は{maximum}文字以内で入力してください"));
        }
    }
    let reference = source["reference"].as_str().unwrap_or("").trim();
    let mut notes = vec!["キャンセル料のご請求です。".to_owned()];
    if !reference.is_empty() {
        notes.push(format!("対象: {reference}"));
    }
    if let Some(reason) = source["reason"].as_str().filter(|s| !s.trim().is_empty()) {
        notes.push(format!("理由: {}", reason.trim()));
    }
    if let Some(note) = source["notes"].as_str().filter(|s| !s.trim().is_empty()) {
        notes.push(note.trim().to_owned());
    }
    *body = json!({
        "clientId": source["clientId"].as_str().filter(|s| !s.is_empty()).unwrap_or(source["clientName"].as_str().unwrap_or("")),
        "clientName": source["clientName"], "clientEmail": source["clientEmail"], "clientPhone": phone,
        "currency": "JPY", "dueDate": source["dueDate"], "taxAmount": source["taxAmount"],
        "lineItems": [{"description": if reference.is_empty() { "キャンセル料".to_owned() } else { format!("キャンセル料 ({reference})") }, "quantity": 1, "unitPrice": amount}],
        "notes": notes.join("\n"), "sendEmail": email, "sendSms": sms,
        "smsMessage": if sms { source["smsMessage"].clone() } else { Value::Null },
        "createPaymentLink": true, "paymentLinkProvider": "stripe"
    });
    Ok(())
}

fn invoice_delivery_result(response: &Value) -> Result<(), String> {
    if [
        "emailDeliveryStatus",
        "smsDeliveryStatus",
        "paymentLinkStatus",
    ]
    .iter()
    .any(|key| response[*key] == "Failed")
    {
        return Err("請求書は作成されましたが、送信または決済リンク作成が失敗しました。一覧で送信状態を確認してください。新しい請求書を重複作成しないでください".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn resource(key: &str) -> Resource {
        *RESOURCES.iter().find(|r| r.key == key).unwrap()
    }

    #[test]
    fn subject_reception_submits_only_selected_owned_active_subjects() {
        let op = USAGE_SUBJECT_RESOURCE.actions[0];
        let original = json!({"id":"customer-a","subjects":[{"id":"subject-a","name":"対象A","archived":false},{"id":"subject-b","name":"対象B","archived":false}]});
        let mut draft = model::draft(op.fields, &original);
        draft["subjects"][1]["selected"] = json!(true);
        draft["receivedAt"] = json!("2026-11-20 10:00");
        let body = model::request_body(op, &draft, &original, "stable-reception").unwrap();
        assert_eq!(body["subjectIds"], json!(["subject-b"]));
        assert_eq!(body["customerId"], "customer-a");
        assert_eq!(body["requestKey"], "stable-reception");
        assert!(body.get("subjects").is_none());
        draft["subjects"][1]["id"] = json!("another-customers-subject");
        assert!(model::request_body(op, &draft, &original, "stable-reception").is_err());
    }

    #[test]
    fn first_reception_registers_basic_customer_atomically_without_both_identity_modes() {
        let op = USAGE_NEW_CUSTOMER_RESOURCE.create.unwrap();
        let mut draft = model::draft(op.fields, &Value::Null);
        draft["newCustomer"]["name"] = json!("初回の利用者");
        draft["receivedAt"] = json!("2026-11-20 10:00");
        let body = model::request_body(op, &draft, &Value::Null, "new-reception").unwrap();
        assert_eq!(body["newCustomer"]["name"], "初回の利用者");
        assert!(body.get("customerId").is_none());
        assert_eq!(body["subjectIds"], json!([]));
    }

    #[test]
    fn filtering_mixed_orders_keeps_scan_limit_visibility() {
        let collection = model::Collection {
            rows: vec![
                json!({"id":"a","fulfillment_method":"shipping"}),
                json!({"id":"b","fulfillment_method":"pickup"}),
            ],
            meta: json!({}),
            truncated: true,
        };
        let filtered = filtered_collection(collection, |row| row["fulfillment_method"] == "pickup");
        assert_eq!(filtered["items"].as_array().unwrap().len(), 1);
        assert_eq!(filtered["items"][0]["id"], "b");
        assert_eq!(filtered["truncated"], true);
    }

    #[test]
    fn catalog_plan_round_trip_preserves_owner_eligibility_and_unknown_attributes() {
        let raw = json!({"id":"plan-a","name":"朝のプラン","enabled":false,"planEnabled":true,"durationMinutes":90,"bookingMode":"time","eligibleResourceIds":[],"ownerExtensionKey":"another_extension","attributesJson":{"description":"内容","priceAmount":5000,"membershipPlanId":"visitor","reservationTypeId":"type-a","prepaymentPolicy":"deposit_required","depositRatio":0.125,"imageUrl":"https://example.com/photo.jpg","eligibility":{"minimumAge":18},"slots":[{"server":"child-table"}],"eligibleResourceIds":["shadow"]}});
        let original = plan_view(&raw, true).unwrap();
        assert_eq!(original["enabled"], true);
        assert_eq!(original["available"], false);
        assert_eq!(original["membershipAccess"], "visitor");
        let op = PLAN_RESOURCE.edit.unwrap();
        let mut draft = model::draft(op.fields, &original);
        draft["description"] = json!("更新した内容");
        let body = model::request_body(op, &draft, &original, "").unwrap();
        let request = catalog_plan_payload(Some(&raw), &body);
        assert_eq!(request["ownerExtensionKey"], "another_extension");
        assert_eq!(request["eligibleResourceIds"], json!([]));
        assert_eq!(request["bookingMode"], "time");
        assert_eq!(
            request["attributesJson"]["eligibility"],
            raw["attributesJson"]["eligibility"]
        );
        assert_eq!(request["attributesJson"]["membershipPlanId"], "visitor");
        assert!(request["attributesJson"].get("slots").is_none());
        assert!(
            request["attributesJson"]
                .get("eligibleResourceIds")
                .is_none()
        );
    }

    #[test]
    fn extension_plan_edit_preserves_other_plans_and_settings_and_detects_migration() {
        let raw = json!({"id":"plan-a","name":"昼","enabled":true,"description":"旧","priceAmount":2000,"durationMinutes":60,"bookingMode":"slot","custom":{"keep":true}});
        let original = plan_view(&raw, false).unwrap();
        let op = PLAN_RESOURCE.edit.unwrap();
        let mut draft = model::draft(op.fields, &original);
        draft["description"] = json!("更新");
        let body = model::request_body(op, &draft, &original, "").unwrap();
        let other = json!({"id":"other","unknown":[1,2,3]});
        let config = json!({"timezone":"Asia/Tokyo","maxPlayersPerTeeTime":6,"customExtensionSetting":true,"reservationProducts":[raw,other]});
        let payload = merge_plan_config(config.clone(), &body, false).unwrap();
        assert_eq!(payload["configJson"]["reservationProducts"][1], other);
        assert_eq!(payload["configJson"]["timezone"], "Asia/Tokyo");
        assert_eq!(payload["configJson"]["maxPlayersPerTeeTime"], 6);
        assert_eq!(
            payload["configJson"]["reservationProducts"][0]["custom"],
            json!({"keep":true})
        );
        assert!(compare_plan_source(&original, Some(&raw), true).is_err());
        assert!(
            compare_plan_source(
                &original,
                Some(&json!({"id":"plan-a","name":"他の更新"})),
                false
            )
            .is_err()
        );
        assert!(compare_plan_source(&original, Some(&raw), false).is_ok());
    }

    #[test]
    fn linked_plan_form_cannot_overwrite_product_master_identity() {
        let raw = json!({"id":"plan-a","name":"商品名","enabled":true,"durationMinutes":60,"attributesJson":{"productMasterId":"product-a","description":"商品マスタの内容","priceAmount":1234}});
        let original = plan_view(&raw, true).unwrap();
        assert!(!PLAN_RESOURCE.edit.unwrap().is_available(&original));
        let op = PLAN_RESOURCE.actions[0];
        assert!(op.is_available(&original));
        let draft = model::draft(op.fields, &original);
        let body = model::request_body(op, &draft, &original, "").unwrap();
        assert_eq!(body["name"], "商品名");
        assert_eq!(body["productMasterId"], "product-a");
        assert_eq!(body["description"], "商品マスタの内容");
    }

    #[test]
    fn jobcan_mapping_preserves_leading_zero_codes_and_requires_review() {
        let mut body = json!({"headerRow":1,"mapping":{"fields":[{"source":"番号","target":"staffCode","required":true,"approved":true},{"source":"日付","target":"businessDate","required":true,"approved":true}]},"staffCodes":[{"externalCode":"0012","internalValue":"staff-a"}]});
        jobcan_settings_payload(&mut body, &Value::Null).unwrap();
        assert_eq!(body["staffCodes"][0]["externalCode"], "0012");
        body["mapping"]["fields"][0]["approved"] = json!(false);
        assert!(jobcan_settings_payload(&mut body, &Value::Null).is_err());
    }

    #[test]
    fn classification_clears_direct_partner_and_rejects_incompatible_signup() {
        let mut body =
            json!({"salesChannel":"direct","signupStyle":"selfService","partnerClientId":"old"});
        classification_payload(&mut body, &Value::Null).unwrap();
        assert!(body["partnerClientId"].is_null());
        body = json!({"salesChannel":"partner","signupStyle":"selfService","partnerClientId":"partner"});
        assert!(classification_payload(&mut body, &Value::Null).is_err());
    }

    #[test]
    fn budget_edit_keeps_identity_and_large_integer_amount() {
        let op = resource("reservation-daily-budgets").edit.unwrap();
        let original = json!({"golfCourseId":"course/1","date":"2026-11-20","targetRevenue":9007199254740993_i64,"targetAverageSpend":2500,"targetCaddyAttachedRatio":0.125});
        let draft = model::draft(op.fields, &original);
        let body = model::request_body(op, &draft, &original, "").unwrap();
        assert_eq!(body["targetRevenue"], 9007199254740993_i64);
        assert_eq!(body["golfCourseId"], "course/1");
        assert_eq!(body["targetCaddyAttachedRatio"], 0.125);
        assert!(decimal_ratio(&json!("1.0001")).is_err());
        assert!(decimal_ratio(&json!("0.12345")).is_err());
        assert!(decimal_ratio(&json!("-0.1")).is_err());
    }

    #[test]
    fn legacy_plan_payment_policy_is_normalized_and_old_spelling_removed() {
        for (legacy, expected) in [
            ("deposit", "deposit_required"),
            ("partial", "deposit_required"),
            ("full", "full_required"),
            ("full_prepayment", "full_required"),
            ("optional", "optional"),
        ] {
            let raw = json!({"id":"legacy","name":"既存プラン","priceAmount":2500,"durationMinutes":60,"paymentRequirement":legacy,"depositRatio":0.125,"custom":{"retain":true},"available":false});
            let original = plan_view(&raw, false).unwrap();
            assert_eq!(original["prepaymentPolicy"], expected);
            assert_eq!(original["available"], false);
            let op = PLAN_RESOURCE.edit.unwrap();
            let draft = model::draft(op.fields, &original);
            let body = model::request_body(op, &draft, &original, "").unwrap();
            let merged = merge_plan(Some(&raw), &body);
            assert_eq!(merged["prepaymentPolicy"], expected);
            assert!(merged.get("paymentRequirement").is_none());
            assert_eq!(merged["custom"], raw["custom"]);
            assert_eq!(
                merged["depositRatio"],
                if expected == "deposit_required" {
                    json!(0.125)
                } else {
                    json!(0)
                }
            );
        }
        let raw = json!({"id":"canonical","prepaymentPolicy":"none","paymentRequirement":"full"});
        assert_eq!(plan_view(&raw, false).unwrap()["prepaymentPolicy"], "none");
    }

    #[test]
    fn legacy_plans_without_ids_load_and_only_the_selected_plan_is_edited() {
        let first = json!({"name":"午前","priceAmount":3500,"durationMinutes":90,"paymentRequirement":"deposit","depositRatio":0.125,"unknown":{"first":true}});
        let second = json!({"id":"existing-plan","name":"午後","priceAmount":4500,"durationMinutes":120,"unknown":{"second":true}});
        let third =
            json!({"name":"夕方","priceAmount":2500,"durationMinutes":60,"unknown":[1,2,3]});
        let source = json!({"configVersion":3,"configJson":{"timezone":"Asia/Tokyo","unknownTenantPolicy":["keep"],"reservationProducts":[first,second,third]}});
        let views = legacy_plan_views(&source).unwrap();
        assert_eq!(views[0]["id"], "plan_1");
        assert_eq!(views[1]["id"], "existing-plan");
        assert_eq!(views[2]["id"], "plan_3");
        compare_legacy_snapshot(&views[0], &source).unwrap();
        compare_plan_source(&views[0], Some(&first), false).unwrap();
        let op = PLAN_RESOURCE.edit.unwrap();
        let mut draft = model::draft(op.fields, &views[0]);
        draft["name"] = json!("午前・更新");
        let body = model::request_body(op, &draft, &views[0], "").unwrap();
        let payload = merge_plan_config(source["configJson"].clone(), &body, false).unwrap();
        let rows = &payload["configJson"]["reservationProducts"];
        assert_eq!(rows[0]["id"], "plan_1");
        assert_eq!(rows[0]["name"], "午前・更新");
        assert_eq!(rows[0]["unknown"], first["unknown"]);
        assert_eq!(rows[0]["prepaymentPolicy"], "deposit_required");
        assert_eq!(rows[0]["depositRatio"], 0.125);
        assert_eq!(rows[1], second);
        let mut expected_third = third.clone();
        expected_third["id"] = json!("plan_3");
        assert_eq!(rows[2], expected_third);
        assert_eq!(
            payload["configJson"]["unknownTenantPolicy"],
            source["configJson"]["unknownTenantPolicy"]
        );
    }

    #[test]
    fn legacy_positional_identity_cannot_retarget_after_reorder_or_deletion() {
        let source = json!({"configVersion":1,"configJson":{"reservationProducts":[{"name":"午前","priceAmount":1000},{"name":"午後","priceAmount":2000}]}});
        let original = legacy_plan_views(&source).unwrap()[0].clone();
        let mut reordered = source.clone();
        reordered["configJson"]["reservationProducts"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        assert!(compare_legacy_snapshot(&original, &reordered).is_err());
        let mut other_setting = source.clone();
        other_setting["configJson"]["unrelatedSetting"] = json!(true);
        assert!(compare_legacy_snapshot(&original, &other_setting).is_err());
        let mut newer = source.clone();
        newer["configVersion"] = json!(2);
        assert!(compare_legacy_snapshot(&original, &newer).is_err());
        let payload =
            merge_plan_config(source["configJson"].clone(), &json!({"id":"plan_1"}), true).unwrap();
        let remaining =
            legacy_plan_views(&json!({"configVersion":1,"configJson":payload["configJson"]}))
                .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0]["id"], "plan_2");
        assert_eq!(remaining[0]["name"], "午後");
        assert!(
            compare_legacy_snapshot(
                &original,
                &json!({"configVersion":1,"configJson":payload["configJson"]})
            )
            .is_err()
        );
    }

    #[test]
    fn ambiguous_legacy_ids_are_never_silently_assigned_to_multiple_plans() {
        let source = json!({"configVersion":1,"configJson":{"reservationProducts":[{"name":"一件目"},{"id":"plan_1","name":"二件目"}]}});
        assert!(legacy_plan_views(&source).is_err());
        assert!(
            merge_plan_config(source["configJson"].clone(), &json!({"id":"plan_1"}), true).is_err()
        );
        let empty = json!({"configVersion":0,"configJson":{}});
        assert!(legacy_plan_views(&empty).unwrap().is_empty());
    }

    #[test]
    fn budget_preflight_rejects_unseen_upsert_stale_amounts_and_invalid_dates() {
        let original = json!({"id":"budget1","golfCourseId":"course1","date":"2026-11-20","targetRevenue":9007199254740993_i64,"targetAverageSpend":2500,"targetCaddyAttachedRatio":0.125,"updatedAt":"first"});
        assert!(compare_budget(&Value::Null, Some(&original)).is_err());
        compare_budget(&Value::Null, None).unwrap();
        compare_budget(&original, Some(&original)).unwrap();
        let mut current = original.clone();
        current["targetRevenue"] = json!(9007199254740994_i64);
        assert!(compare_budget(&original, Some(&current)).is_err());
        assert!(compare_budget(&original, None).is_err());
        assert!(budget_range(&json!({"from":"2026-11-20","to":"2026-11-19"})).is_err());
        assert!(budget_range(&json!({"from":"2026-02-30"})).is_err());
        let mut invalid = json!({"golfCourseId":"course1","date":"2026-02-30","targetRevenue":100,"targetAverageSpend":10,"targetCaddyAttachedRatio":"0.1"});
        assert!(budget_payload(&mut invalid, &Value::Null).is_err());
        let mut saved = original.clone();
        let mut body = original.clone();
        body["targetCaddyAttachedRatio"] = json!(0);
        saved["targetCaddyAttachedRatio"] = json!(0.0);
        check_saved_budget(&body, &saved).unwrap();
        saved["targetRevenue"] = json!(9007199254740994_i64);
        assert!(check_saved_budget(&body, &saved).is_err());
    }

    #[test]
    fn shift_edit_keeps_person_and_day_immutable_and_rejects_invalid_time() {
        let op = resource("staff-shifts").edit.unwrap();
        let original = json!({"id":"shift1","staffId":"person1","date":"2026-11-20","startTime":"09:00:00","endTime":"18:00:00","shiftType":"regular","notes":null});
        let draft = model::draft(op.fields, &original);
        let body = model::request_body(op, &draft, &original, "").unwrap();
        assert_eq!(body["startTime"], "09:00");
        assert!(body.get("staffId").is_none());
        assert!(body.get("date").is_none());
        let mut bad = body;
        bad["endTime"] = json!("08:59");
        assert!(shift_payload(&mut bad, &original).is_err());
        assert!(shift_range(&json!({"startDate":"2026-01-01","endDate":"2026-02-11"})).is_ok());
        assert!(shift_range(&json!({"startDate":"2026-01-01","endDate":"2026-02-12"})).is_err());
    }

    #[test]
    fn pickup_actions_do_not_apply_to_shipping_or_final_orders() {
        assert!(pickup_can_ready(
            &json!({"fulfillment_method":"pickup","status":"placed"})
        ));
        assert!(!pickup_can_ready(
            &json!({"fulfillment_method":"shipping","status":"placed"})
        ));
        assert!(!pickup_can_ready(
            &json!({"fulfillment_method":"pickup","status":"confirmed"})
        ));
        assert!(!pickup_can_cancel(
            &json!({"fulfillment_method":"pickup","status":"picked_up"})
        ));
    }

    #[test]
    fn reception_generates_stable_request_key_and_no_foreign_subject_ids() {
        let op = resource("usage-reception-new").create.unwrap();
        let mut draft = model::draft(op.fields, &Value::Null);
        draft["customerId"] = json!("customer-1");
        draft["receivedAt"] = json!("2026-11-20 09:30");
        let body = model::request_body(op, &draft, &Value::Null, "retry-key").unwrap();
        assert_eq!(body["subjectIds"], json!([]));
        assert_eq!(body["requestKey"], "retry-key");
        assert!(body["receivedAt"].as_str().unwrap().ends_with("+09:00"));
    }

    #[test]
    fn cancellation_invoice_requires_consent_and_preserves_integer_precision() {
        let op = resource("cancellation-fees").create.unwrap();
        let mut draft = model::draft(op.fields, &Value::Null);
        for (key, value) in [
            ("clientName", "請求先"),
            ("amount", "9007199254740993"),
            ("dueDate", "2026-12-01"),
            ("clientPhone", "０９０-１２３４-５６７８"),
        ] {
            draft[key] = json!(value);
        }
        draft["sendSms"] = json!(true);
        assert!(model::request_body(op, &draft, &Value::Null, "invoice-key").is_err());
        draft["smsConsentConfirmed"] = json!(true);
        let body = model::request_body(op, &draft, &Value::Null, "invoice-key").unwrap();
        assert_eq!(body["lineItems"][0]["unitPrice"], 9007199254740993_i64);
        assert_eq!(body["clientPhone"], "+819012345678");
        assert_eq!(body["idempotencyKey"], "invoice-key");
        assert!(body.get("smsConsentConfirmed").is_none());
        assert!(cancellation_invoice(&body));
        assert!(
            invoice_delivery_result(&json!({"id":"saved","smsDeliveryStatus":"Failed"})).is_err()
        );
    }

    #[test]
    fn attendance_flattening_keeps_multiple_punches_and_close_gate() {
        let response = attendance_details(json!({"staff":[{"staffName":"担当者","days":[{"date":"2026-11-20","punches":[{"clockIn":"09:00","clockOut":"12:00"},{"clockIn":"13:00","clockOut":null}],"shifts":[],"anomalies":[{"kind":"missing_clock_out"}]}]}]})).unwrap();
        let text = response["dayDetails"][0]["punchText"].as_str().unwrap();
        assert!(text.contains("09:00"));
        assert!(text.contains("退勤未記録"));
        assert!(!attendance_can_close(
            &json!({"status":"open","closable":false})
        ));
        assert!(!attendance_can_close(
            &json!({"status":"closed","closable":true})
        ));
        assert!(attendance_can_close(
            &json!({"status":"open","closable":true})
        ));
    }
}
