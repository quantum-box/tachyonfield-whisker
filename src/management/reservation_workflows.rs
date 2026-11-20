//! Reservation lifecycle commands from the verified admin REST contracts.
//! Monetary operations keep provider-owned payment state out of editable fields.

use super::{model, schema::*, workforce};
use crate::api::{self, Tenant};
use chrono::{DateTime, FixedOffset, Utc};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

const PATH: &str = "/v1/erp/reservations";
const TEMPLATES_PATH: &str = "/v1/erp/reservation-notification-templates";
const TYPES: Lookup = Lookup {
    path: "/v1/erp/reservation-types",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const RESOURCES: Lookup = Lookup {
    path: "/v1/erp/reservation-resources",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const CUSTOMERS: Lookup = Lookup {
    path: "/v1/erp/customers?limit=100",
    list_key: "data",
    value_key: "id",
    label_keys: &["name", "email", "phone"],
};
const STAFF: Lookup = Lookup {
    path: "/v1/erp/staff",
    list_key: "items",
    value_key: "id",
    label_keys: &["name", "jobTitle"],
};
const PLANS: Lookup = Lookup {
    path: "/v1/erp/reservation-products?includeDisabled=true",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};

const STAFF_ROWS: &[Field] =
    &[Field::new("staffId", "担当スタッフ", InputKind::Reference(STAFF)).required()];
const PREPAYMENT: &[Choice] = &[
    Choice::new("inherit", "予約種別の規定に従う"),
    Choice::new("none", "事前決済なし"),
    Choice::new("optional", "事前決済は任意"),
    Choice::new("deposit_required", "前払い金"),
    Choice::new("full_prepayment", "全額事前決済"),
];

pub const CREATE_FIELDS: &[Field] = &[
    Field::new("reservationTypeId", "予約種別", InputKind::Reference(TYPES)).required(),
    // The reservation-specific picker supplies this ID from the normalized plan catalogue.
    Field::new("serviceId", "予約プラン", InputKind::Reference(PLANS)),
    Field::new("resourceId", "場所・設備", InputKind::Reference(RESOURCES)),
    Field::new("customerId", "登録済みのお客様", InputKind::Reference(CUSTOMERS)),
    Field::text("customerName", "お客様の氏名").required(),
    Field::new("customerEmail", "メールアドレス", InputKind::Email).empty(Empty::String),
    Field::text("customerPhone", "電話番号").empty(Empty::String),
    Field::new("startsAt", "開始日時（日本時間）", InputKind::DateTime).required(),
    Field::new("endsAt", "終了日時（日本時間）", InputKind::DateTime).required(),
    Field::new("quantity", "人数・数量", InputKind::Integer).required().default("1"),
    Field::new("currency", "通貨", InputKind::ReadOnly).default("JPY"),
    Field::new("priceAmount", "予約全体の料金（円）", InputKind::Money).required().default("0")
        .help("人数・数量を掛ける前の単価ではなく、予約全体の金額です。従来サービスの価格設定がある場合はその金額が優先されるため、登録後の確定料金も確認してください。"),
    Field::new("prepaymentPolicy", "事前決済", InputKind::Select(PREPAYMENT)).required().default("inherit"),
    Field::new("depositAmount", "前払い金（円）", InputKind::Money).required().default("0")
        .help("前払い金の方式を選んだ場合に指定します。種別に金額・割合の規定がある場合はそちらが優先されます。規定もなく0のときは全額が事前決済の対象になります。"),
    Field::new("assignedStaffIds", "担当スタッフ", InputKind::Lines(STAFF_ROWS)),
    Field::new("notes", "お客様に関する備考", InputKind::Multiline).empty(Empty::String),
];

pub const EDIT_FIELDS: &[Field] = &[
    Field::new("serviceId", "予約プラン", InputKind::Reference(PLANS)).empty(Empty::String)
        .help("請求書作成後はプランを変更できません。プランを変更しても予約済みの料金と人数は保持されます。"),
    Field::new("customerId", "登録済みのお客様", InputKind::Reference(CUSTOMERS)).empty(Empty::Null)
        .help("空欄にするとお客様台帳との紐付けを解除します。請求書作成後は紐付けを変更できません。"),
    Field::text("customerName", "お客様の氏名").required(),
    Field::new("customerEmail", "メールアドレス", InputKind::Email).empty(Empty::String),
    Field::text("customerPhone", "電話番号").empty(Empty::String),
    Field::new("startsAt", "開始日時（日本時間）", InputKind::DateTime).required(),
    Field::new("endsAt", "終了日時（日本時間）", InputKind::DateTime).required(),
    Field::new("resourceId", "場所・設備", InputKind::Reference(RESOURCES)).empty(Empty::String)
        .help("別の場所・設備へ変更できます。設定済みの割当を空欄にして解除する操作はAPIにありません。"),
    Field::new("assignedStaffIds", "担当スタッフ", InputKind::Lines(STAFF_ROWS)),
    Field::new("notes", "お客様に関する備考", InputKind::Multiline).empty(Empty::String),
    Field::new("internalNote", "社内メモを追記", InputKind::Multiline).read("_newInternalNote").empty(Empty::String)
        .help("既存の社内メモ・通知記録は保持して追記します。"),
    Field::new("quantity", "人数・数量（変更不可）", InputKind::ReadOnly),
    Field::new("priceAmount", "予約全体の料金（変更不可）", InputKind::ReadOnly).read("@money:priceAmount"),
];

pub const CANCEL_FIELDS: &[Field] = &[
    Field::new("cancellationPolicySummary", "取消規定", InputKind::ReadOnly),
    Field::new("currency", "通貨", InputKind::ReadOnly),
    Field::new("paidAmount", "支払済み", InputKind::ReadOnly).read("@money:paidAmount"),
    Field::new("reason", "取消理由", InputKind::Multiline).required().read("_newCancellationReason"),
    Field::new("cancellationFeeAmount", "キャンセル料", InputKind::Money).required()
        .help("規定から算出した初期値です。確定時にこの金額を送信します。必要な場合だけ変更してください。"),
    Field::new("refundAmount", "返金する金額", InputKind::Money).required()
        .help("0は返金なし。支払済みの金額以下で指定してください。"),
];

const REFUND_FIELDS: &[Field] = &[Field::new(
    "_reservationRefundEvents",
    "返金を再実行する取消履歴",
    InputKind::Lines(&[
        Field::new("id", "", InputKind::Hidden),
        Field::new("selected", "この履歴の返金を再実行", InputKind::Bool).default("false"),
        Field::new("createdAt", "取消日時", InputKind::ReadOnly),
        Field::new("reason", "取消理由", InputKind::ReadOnly),
        Field::new("refundAmount", "依頼済みの返金額", InputKind::ReadOnly)
            .read("@money:refundAmount"),
    ]),
)
.fixed_rows()
.required()];

const UNMATCHED_FIELDS: &[Field] = &[
    Field::new(
        "_reservationReviewPayments",
        "確認する未割当入金",
        InputKind::Lines(&[
            Field::new("_key", "", InputKind::Hidden),
            Field::new("selected", "この入金を確認済みにする", InputKind::Bool).default("false"),
            Field::new("providerLabel", "決済事業者", InputKind::ReadOnly),
            Field::new("recordedAt", "記録日時", InputKind::ReadOnly),
            Field::new("amount", "入金額", InputKind::ReadOnly).read("@money:amount"),
        ]),
    )
    .fixed_rows()
    .required(),
    Field::new("note", "確認内容", InputKind::Multiline)
        .required()
        .read("_newReviewNote"),
];

const NOTICE_FIELDS: &[Field] = &[
    Field::new("customerEmail", "送信先", InputKind::ReadOnly),
    Field::text("subjectTemplate", "件名").required(),
    Field::new("bodyTemplate", "本文", InputKind::Multiline).required()
        .help("{{customerName}}・{{reservationNumber}}・{{startsAt}}・{{endsAt}}・{{status}}・{{checkoutUrl}}を差し込みます。"),
];

const SNAPSHOT: &[&str] = &[
    "id",
    "updatedAt",
    "reservationTypeId",
    "serviceId",
    "resourceId",
    "assignedStaffIds",
    "storeId",
    "customerId",
    "customerName",
    "customerEmail",
    "customerPhone",
    "billTo",
    "status",
    "paymentStatus",
    "startsAt",
    "endsAt",
    "timezone",
    "quantity",
    "priceAmount",
    "depositAmount",
    "paidAmount",
    "currency",
    "checkoutUrl",
    "policySnapshotJson",
    "customFieldsJson",
    "notes",
    "internalNotes",
    "invoiceId",
    "cancelledAt",
];

const FILTERS: &[Field] = &[
    Field::text("search", "氏名・予約番号・連絡先で検索"),
    Field::new("from", "開始日時から（日本時間）", InputKind::DateTime),
    Field::new("to", "開始日時まで・未満（日本時間）", InputKind::DateTime),
    Field::new("reservationTypeId", "予約種別", InputKind::Reference(TYPES)),
    Field::new("resourceId", "場所・設備", InputKind::Reference(RESOURCES)),
    Field::new("staffId", "担当スタッフ", InputKind::Reference(STAFF)),
    Field::new(
        "status",
        "予約状況",
        InputKind::Select(&[
            Choice::new("requested", "受付済み"),
            Choice::new("payment_pending", "事前決済待ち"),
            Choice::new("confirmed", "予約確定"),
            Choice::new("change_requested", "変更希望"),
            Choice::new("cancel_requested", "取消希望"),
            Choice::new("waiting", "待機中"),
            Choice::new("admin_review", "管理者確認待ち"),
            Choice::new("suspended", "保留"),
            Choice::new("completed", "利用完了"),
            Choice::new("no_show", "来店なし"),
            Choice::new("cancelled", "取消済み"),
            Choice::new("rejected", "受付不可"),
        ]),
    ),
    Field::new(
        "paymentStatus",
        "支払状況",
        InputKind::Select(&[
            Choice::new("not_required", "事前決済不要"),
            Choice::new("unpaid", "未払い"),
            Choice::new("partial", "一部支払済み"),
            Choice::new("paid", "支払済み"),
            Choice::new("refund_pending", "返金処理中"),
            Choice::new("refunded", "返金済み"),
            Choice::new("fee_due", "キャンセル料未払い"),
            Choice::new("fee_paid", "キャンセル料支払済み"),
        ]),
    ),
];

pub fn decorate(resource: Resource) -> Resource {
    if resource.key != "reservations" {
        return resource;
    }
    resource
        .detail_enrichment(load_context)
        .filters(FILTERS)
        .fields(&[("予約番号", "reservationNumber"), ("予約の状態", "status"), ("お客様", "customerName"), ("メール", "customerEmail"),
            ("電話", "customerPhone"), ("開始", "startsAt"), ("終了", "endsAt"), ("人数・数量", "quantity"),
            ("料金", "@money:priceAmount"), ("前払い金", "@money:depositAmount"), ("支払済み", "@money:paidAmount"),
            ("通貨", "currency"), ("決済状況", "paymentStatus"), ("担当スタッフ", "_reservationStaffNames"),
            ("支払リンク", "checkoutUrl"), ("備考", "notes"), ("社内メモ", "_reservationInternalNotes"),
            ("取消日時", "cancelledAt"), ("取消理由", "cancelReason"), ("更新日時", "updatedAt")])
        .create(Mutation::new("予約を追加", "POST", PATH, CREATE_FIELDS)
            .prepare(prepare_create).preflight(preflight_create).idempotency_body("idempotencyKey")
            .validate_response(validate_result).result_record(result_record).success_message(created_message)
            .explain("予約を登録し、必要な場合は事前決済待ちになります。担当者への通知が設定されている場合は通知されます。"))
        .edit(Mutation::new("予約内容・日時・割当を変更", "PATCH", "/v1/erp/reservations/{id}", EDIT_FIELDS)
            .when(editable).prefill(edit_prefill).prepare(prepare_edit).preflight(preflight_edit)
            .validate_response(validate_result).result_record(result_record).success_message(saved_message)
            .explain("日時と場所の変更は空き枠を確認して保存します。人数・料金・支払状況は変更しません。設定に従って担当者へ変更が通知されます。"))
        .actions(ACTIONS)
}

pub const ACTIONS: &[Mutation] = &[
    Mutation::new("予約を確定", "PATCH", "/v1/erp/reservations/{id}", &[])
        .when(can_confirm).prepare(prepare_confirm).preflight(preflight_existing).validate_response(validate_result)
        .result_record(result_record).success_message(saved_message)
        .explain("予約を確定します。支払状況は変えません。確認メールと担当者通知が設定されている場合は送信されます。"),
    Mutation::new("利用完了にする", "PATCH", "/v1/erp/reservations/{id}", &[])
        .when(can_complete).prepare(prepare_complete).preflight(preflight_existing).validate_response(validate_result)
        .result_record(result_record).success_message(saved_message).explain("利用が終了した予約を完了にします。支払状況は変えません。"),
    Mutation::new("来店なしにする", "PATCH", "/v1/erp/reservations/{id}", &[])
        .when(can_no_show).prepare(prepare_no_show).preflight(preflight_existing).validate_response(validate_result)
        .result_record(result_record).success_message(saved_message).destructive()
        .explain("予約を来店なしとして記録します。この操作では返金やキャンセル料の請求は行いません。"),
    Mutation::new("予約を取り消す", "POST", "/v1/erp/reservations/{id}/cancel", CANCEL_FIELDS)
        .when(cancellable).prefill(cancel_prefill).prepare(prepare_cancel).preflight(preflight_cancel).idempotency_header()
        .validate_response(validate_result).result_record(result_record).success_message(cancelled_message).destructive()
        .explain("予約を取り消し、確認した金額で返金またはキャンセル料の処理を依頼します。キャンセル料の請求書・支払案内や担当者への通知が作成・送信される場合があります。"),
    Mutation::new("取消済みの返金を再実行", "POST", "/v1/erp/reservations/{id}/cancellations/{cancellationEventId}/refund-retry", REFUND_FIELDS)
        .when(can_retry_refund).prepare(prepare_refund_retry).preflight(preflight_refund_retry).execute(retry_refund)
        .validate_response(validate_result).result_record(result_record).success_message(refund_message).destructive()
        .explain("選択した取消履歴に記録済みの返金処理を再開します。新しい取消や返金額の追加は行いません。"),
    Mutation::new("請求書と支払リンクを作成・再取得", "POST", "/v1/erp/reservations/{id}/billing-invoice", &[])
        .when(billable).preflight(preflight_existing).validate_response(validate_billing_result)
        .result_record(result_record).success_message(billing_message)
        .explain("既存の支払待ち内容から請求書と支払リンクを作成、または再取得します。設定された請求先に支払案内メールが送信される場合があります。"),
    Mutation::new("支払リンクのみ作成・再取得", "POST", "/v1/erp/reservations/{id}/billing-link", &[])
        .when(billable).preflight(preflight_existing).validate_response(validate_billing_result)
        .result_record(result_record).success_message(billing_message)
        .explain("支払待ちの決済リンクを作成、または既存のリンクを再取得します。この操作だけではメールを送信しません。"),
    Mutation::new("未割当入金を確認済みにする", "POST", "/v1/erp/reservations/{id}/unmatched-payments/review", UNMATCHED_FIELDS)
        .when(can_review_payment).prepare(prepare_review).preflight(preflight_review).execute(review_payment)
        .validate_response(validate_result).result_record(result_record).success_message(review_message)
        .explain("決済事業者の入金を調査した結果を記録します。予約の復活、入金の振替、返金を自動で行う操作ではありません。"),
    Mutation::new("予約確認メールを送る", "POST", "/v1/erp/reservations/{id}/notifications", NOTICE_FIELDS)
        .when(can_notify_confirmation).prefill(prefill_confirmation).prepare(prepare_confirmation_notice).preflight(preflight_notice)
        .validate_response(validate_result).result_record(result_record).success_message(notification_message)
        .explain("表示した宛先へ予約確認メールを送信します。公開予約の場合はお客様用の予約管理リンクが付くことがあります。"),
    Mutation::new("リマインダーメールを送る", "POST", "/v1/erp/reservations/{id}/notifications", NOTICE_FIELDS)
        .when(can_notify_reminder).prefill(prefill_reminder).prepare(prepare_reminder_notice).preflight(preflight_notice)
        .validate_response(validate_result).result_record(result_record).success_message(notification_message)
        .explain("表示した宛先へ予約のリマインダーメールを送信します。"),
    Mutation::new("変更・取消メールを送る", "POST", "/v1/erp/reservations/{id}/notifications", NOTICE_FIELDS)
        .when(can_notify_change).prefill(prefill_change).prepare(prepare_change_notice).preflight(preflight_notice)
        .validate_response(validate_result).result_record(result_record).success_message(notification_message)
        .explain("表示した宛先へ予約変更・取消のお知らせを送信します。"),
];

fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn is_status(value: &Value, state: &str) -> bool {
    string(value, "status") == state
}
fn date(value: &Value, key: &str) -> Result<DateTime<FixedOffset>, String> {
    DateTime::parse_from_rfc3339(string(value, key))
        .map_err(|_| format!("{key}の日時を確認してください。"))
}
fn amount(value: &Value, key: &str) -> Result<i64, String> {
    value[key]
        .as_i64()
        .filter(|n| *n >= 0)
        .ok_or_else(|| "料金・支払済み額を正確に読み取れません。再読み込みしてください。".into())
}
fn required_text(value: &Value, key: &str, label: &str, max: usize) -> Result<(), String> {
    let text = string(value, key).trim();
    if text.is_empty() || text.chars().count() > max {
        Err(format!("{label}は1〜{max}文字で入力してください。"))
    } else {
        Ok(())
    }
}
fn id(original: &Value) -> Result<&str, String> {
    let value = string(original, "id");
    if value.starts_with("rsv_") && value.len() > 4 {
        Ok(value)
    } else {
        Err("対象の予約を確認できません。詳細を再読み込みしてください。".into())
    }
}
fn record_path(original: &Value) -> Result<String, String> {
    Ok(format!("{PATH}/{}", api::encode_query(id(original)?)))
}

pub fn editable(record: &Value) -> bool {
    matches!(
        string(record, "status"),
        "requested"
            | "payment_pending"
            | "confirmed"
            | "change_requested"
            | "cancel_requested"
            | "waiting"
            | "admin_review"
            | "suspended"
            | "completed"
            | "no_show"
            | "cancelled"
            | "rejected"
    )
}
fn cancellable(record: &Value) -> bool {
    matches!(
        string(record, "status"),
        "requested"
            | "payment_pending"
            | "confirmed"
            | "change_requested"
            | "cancel_requested"
            | "waiting"
            | "admin_review"
            | "suspended"
            | "completed"
            | "no_show"
            | "rejected"
    ) && record["cancelledAt"].is_null()
        && amount(record, "paidAmount").is_ok()
        && !string(record, "currency").is_empty()
        && !matches!(
            string(record, "paymentStatus"),
            "refund_pending" | "refunded" | "fee_due" | "fee_paid"
        )
}
fn can_confirm(record: &Value) -> bool {
    status_change_allowed(record) && !is_status(record, "confirmed")
}
fn can_complete(record: &Value) -> bool {
    status_change_allowed(record) && !is_status(record, "completed")
}
fn can_no_show(record: &Value) -> bool {
    status_change_allowed(record) && !is_status(record, "no_show")
}
fn status_change_allowed(record: &Value) -> bool {
    matches!(
        string(record, "status"),
        "requested"
            | "payment_pending"
            | "confirmed"
            | "change_requested"
            | "cancel_requested"
            | "waiting"
            | "admin_review"
            | "suspended"
            | "completed"
            | "no_show"
    ) && record["cancelledAt"].is_null()
}
fn billable(record: &Value) -> bool {
    string(record, "paymentStatus") == "fee_due"
        || (!matches!(string(record, "status"), "cancelled" | "rejected")
            && matches!(string(record, "paymentStatus"), "unpaid" | "partial"))
}

pub fn edit_prefill(original: &Value) -> Result<Value, String> {
    let mut visible = original.clone();
    visible["assignedStaffIds"] = Value::Array(
        original["assignedStaffIds"]
            .as_array()
            .ok_or("担当スタッフ情報を読み取れません。")?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .map(|id| json!({"staffId":id}))
                    .ok_or("担当スタッフ情報を読み取れません。")
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(visible)
}
fn prepare_staff(body: &mut Value) -> Result<(), String> {
    let rows = body["assignedStaffIds"]
        .as_array()
        .ok_or("担当スタッフの入力を確認してください。")?;
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for row in rows {
        let staff_id = string(row, "staffId");
        if staff_id.is_empty() || !seen.insert(staff_id) {
            return Err("担当スタッフを選択し、重複を取り除いてください。".into());
        }
        ids.push(json!(staff_id));
    }
    body["assignedStaffIds"] = Value::Array(ids);
    Ok(())
}
fn validate_contact(body: &Value) -> Result<(), String> {
    required_text(body, "customerName", "お客様の氏名", 255)?;
    for (key, max) in [
        ("customerEmail", 255),
        ("customerPhone", 64),
        ("notes", 10_000),
    ] {
        if string(body, key).chars().count() > max {
            return Err("お客様情報または備考が長すぎます。".into());
        }
    }
    if date(body, "endsAt")? <= date(body, "startsAt")? {
        return Err("終了日時は開始日時より後にしてください。".into());
    }
    Ok(())
}
fn prepare_create(body: &mut Value, _: &Value) -> Result<(), String> {
    validate_contact(body)?;
    required_text(body, "reservationTypeId", "予約種別", 255)?;
    let count = body["quantity"]
        .as_i64()
        .filter(|n| (1..=i32::MAX as i64).contains(n))
        .ok_or("人数・数量は1以上の整数で入力してください。")?;
    let _ = count;
    let price = amount(body, "priceAmount")?;
    let deposit = amount(body, "depositAmount")?;
    match string(body, "prepaymentPolicy") {
        "inherit" => {
            body.as_object_mut().unwrap().remove("prepaymentPolicy");
        }
        "none" | "optional" => {}
        "deposit_required" => {
            if deposit > price {
                return Err("前払い金は予約料金以下で指定してください。".into());
            }
            body["prepaymentPolicy"] = json!("deposit");
        }
        "full_prepayment" => {
            body["prepaymentPolicy"] = json!("full");
        }
        _ => return Err("事前決済の方式を選択してください。".into()),
    }
    prepare_staff(body)?;
    body["currency"] = json!("JPY");
    body["timezone"] = json!("Asia/Tokyo");
    Ok(())
}
fn prepare_edit(body: &mut Value, original: &Value) -> Result<(), String> {
    if !editable(original) {
        return Err("この予約の状態を確認できません。詳細を再読み込みしてください。".into());
    }
    validate_contact(body)?;
    prepare_staff(body)?;
    if body["customerId"] != original["customerId"] && !original["invoiceId"].is_null() {
        return Err("請求書作成後はお客様台帳との紐付けを変更できません。".into());
    }
    if string(body, "serviceId").is_empty() {
        if !string(original, "serviceId").is_empty() {
            return Err(
                "設定済みの予約プランは空欄で解除できません。変更先を選択してください。".into(),
            );
        }
        body.as_object_mut().unwrap().remove("serviceId");
    } else if body["serviceId"] != original["serviceId"] && !original["invoiceId"].is_null() {
        return Err("請求書作成後は予約プランを変更できません。".into());
    }
    if string(body, "resourceId").is_empty() {
        if !original["resourceId"].is_null() && !string(original, "resourceId").is_empty() {
            return Err(
                "設定済みの場所・設備は空欄にして解除できません。変更先を選択してください。".into(),
            );
        }
        body.as_object_mut().unwrap().remove("resourceId");
    }
    let note = string(body, "internalNote").trim().to_owned();
    if note.chars().count() > 4000 {
        return Err("追記する社内メモは4000文字以内で入力してください。".into());
    }
    body.as_object_mut().unwrap().remove("internalNote");
    if !note.is_empty() {
        let previous = string(original, "internalNotes");
        body["internalNotes"] = json!(if previous.is_empty() {
            note
        } else {
            format!("{previous}\n{note}")
        });
    }
    for key in [
        "customerId",
        "serviceId",
        "assignedStaffIds",
        "resourceId",
        "startsAt",
        "endsAt",
        "customerName",
        "customerEmail",
        "customerPhone",
        "notes",
    ] {
        if body.get(key).is_some_and(|value| {
            value == &original[key]
                || (value == "" && original[key].is_null())
                || (matches!(key, "startsAt" | "endsAt")
                    && date(body, key)
                        .ok()
                        .zip(date(original, key).ok())
                        .is_some_and(|(a, b)| a == b))
        }) {
            body.as_object_mut().unwrap().remove(key);
        }
    }
    Ok(())
}
fn prepare_confirm(body: &mut Value, original: &Value) -> Result<(), String> {
    if !can_confirm(original) {
        return Err("この予約の受付状態は確定へ変更できません。".into());
    }
    *body = json!({"status":"confirmed"});
    Ok(())
}
fn prepare_complete(body: &mut Value, original: &Value) -> Result<(), String> {
    if !can_complete(original) {
        return Err("この予約の受付状態は完了へ変更できません。".into());
    }
    *body = json!({"status":"completed"});
    Ok(())
}
fn prepare_no_show(body: &mut Value, original: &Value) -> Result<(), String> {
    if !can_no_show(original) {
        return Err("この予約の受付状態は来店なしへ変更できません。".into());
    }
    *body = json!({"status":"no_show"});
    Ok(())
}

pub fn now_jst() -> DateTime<FixedOffset> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    DateTime::<Utc>::from_timestamp(seconds.min(i64::MAX as u64) as i64, 0)
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
        .with_timezone(&FixedOffset::east_opt(9 * 3600).unwrap())
}
fn policy_amounts(original: &Value, now: DateTime<FixedOffset>) -> Result<Value, String> {
    let price = amount(original, "priceAmount")?;
    let paid = amount(original, "paidAmount")?;
    let deposit = amount(original, "depositAmount")?;
    let snapshot = &original["policySnapshotJson"];
    let policy = snapshot["cancellationPolicy"]
        .as_str()
        .unwrap_or("full_refund");
    if !matches!(policy, "full_refund" | "no_refund" | "fee_deducted") {
        return Err("この取消規定の金額を確認できません。Web管理画面で確認してください。".into());
    }
    let cutoff = snapshot["cancellationPolicyJson"]["cutoffHours"]
        .as_i64()
        .unwrap_or(0);
    let duration = chrono::Duration::try_hours(cutoff)
        .filter(|_| cutoff >= 0)
        .ok_or("取消期限の設定を確認してください。")?;
    let boundary = date(original, "startsAt")?
        .checked_sub_signed(duration)
        .ok_or("取消期限を読み取れません。")?;
    let within = cutoff == 0 || now >= boundary;
    let fee = if within {
        match policy {
            "no_refund" => price.saturating_sub(paid).max(0),
            "fee_deducted" => deposit.saturating_sub(paid).max(0),
            _ => 0,
        }
    } else {
        0
    };
    let refund = match policy {
        "no_refund" => 0,
        "fee_deducted" => paid.saturating_sub(fee).max(0),
        _ => paid,
    };
    Ok(
        json!({"currency":original["currency"],"paidAmount":paid,"cancellationFeeAmount":fee,"refundAmount":refund,
        "policy":policy,"withinCutoff":within,"cutoffAt":boundary.to_rfc3339(),"automatic":true}),
    )
}
pub fn cancellation_preview(
    original: &Value,
    draft: &Value,
    now: DateTime<FixedOffset>,
) -> Result<Value, String> {
    let mut preview = policy_amounts(original, now)?;
    let mut editable = draft.clone();
    if !editable.is_object() {
        editable = json!({});
    }
    editable["currency"] = original["currency"].clone();
    let fields = [
        Field::new("cancellationFeeAmount", "キャンセル料", InputKind::Money),
        Field::new("refundAmount", "返金額", InputKind::Money),
    ];
    let amounts = model::body(&fields, &editable)?;
    for key in ["cancellationFeeAmount", "refundAmount"] {
        if amounts.get(key).is_some() {
            preview[key] = amounts[key].clone();
            preview["automatic"] = json!(false);
        }
    }
    if amount(&preview, "refundAmount")? > amount(original, "paidAmount")? {
        return Err("返金額は支払済みの金額以下にしてください。".into());
    }
    amount(&preview, "cancellationFeeAmount")?;
    Ok(preview)
}
fn cancel_prefill(original: &Value) -> Result<Value, String> {
    let defaults = policy_amounts(original, now_jst())?;
    let mut visible = original.clone();
    visible["cancellationFeeAmount"] = defaults["cancellationFeeAmount"].clone();
    visible["refundAmount"] = defaults["refundAmount"].clone();
    visible["cancellationPolicySummary"] = json!(match string(&defaults, "policy") {
        "no_refund" => "返金なし（キャンセル料は規定に従う）",
        "fee_deducted" => "規定の費用を差し引いて返金",
        _ => "全額返金",
    });
    Ok(visible)
}
fn prepare_cancel(body: &mut Value, original: &Value) -> Result<(), String> {
    if !cancellable(original) {
        return Err("この予約は取消できません。取消履歴と返金状況を確認してください。".into());
    }
    required_text(body, "reason", "取消理由", 1000)?;
    amount(body, "cancellationFeeAmount")?;
    if amount(body, "refundAmount")? > amount(original, "paidAmount")? {
        return Err("返金額は支払済みの金額以下にしてください。".into());
    }
    Ok(())
}

fn unchanged(original: &Value, fresh: &Value) -> Result<(), String> {
    if string(original, "updatedAt").is_empty() {
        return Err("予約の更新日時を確認できません。詳細を再読み込みしてください。".into());
    }
    if SNAPSHOT.iter().any(|key| original[*key] != fresh[*key]) {
        return Err("予約・入金状況が更新されています。詳細を再読み込みして、日時と金額を確認してください。（409）".into());
    }
    Ok(())
}
fn fetch_fresh(token: &str, tenant: &Tenant, original: &Value) -> Result<Value, String> {
    let fresh = api::fetch_record(token, tenant, &record_path(original)?)?;
    unchanged(original, &fresh)?;
    Ok(fresh)
}
fn preflight_existing(
    token: &str,
    tenant: &Tenant,
    _: &Value,
    original: &Value,
) -> Result<(), String> {
    fetch_fresh(token, tenant, original).map(|_| ())
}
fn preflight_cancel(
    token: &str,
    tenant: &Tenant,
    _: &Value,
    original: &Value,
) -> Result<(), String> {
    let fresh = api::fetch_record(token, tenant, &record_path(original)?)?;
    if is_status(&fresh, "cancelled") {
        return Err("予約の取消は既に記録されています。詳細を再読み込みし、取消履歴から必要な返金を再実行してください。（409）".into());
    }
    unchanged(original, &fresh)
}

fn preflight_create(token: &str, tenant: &Tenant, body: &Value, _: &Value) -> Result<(), String> {
    validate_selection(token, tenant, body, &Value::Null)
}
fn preflight_edit(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<(), String> {
    let fresh = fetch_fresh(token, tenant, original)?;
    if !editable(&fresh) {
        return Err("この予約は変更できません。".into());
    }
    if [
        "serviceId",
        "resourceId",
        "assignedStaffIds",
        "startsAt",
        "endsAt",
    ]
    .iter()
    .any(|key| body.get(*key).is_some())
    {
        validate_selection(token, tenant, body, &fresh)?;
    }
    Ok(())
}
fn validate_selection(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<(), String> {
    validate_staff_selection(token, tenant, body, original)?;
    // A staff-only change must remain possible for an old booking whose type
    // or plan is no longer offered. Slot changes use the server's inventory check.
    if !original.is_null()
        && !["serviceId", "resourceId", "startsAt", "endsAt"]
            .iter()
            .any(|key| body.get(*key).is_some())
    {
        return Ok(());
    }
    let effective = |key: &str| {
        body.get(key)
            .filter(|value| !value.is_null())
            .unwrap_or(&original[key])
            .clone()
    };
    let type_id = effective("reservationTypeId");
    let types = api::fetch_record(token, tenant, TYPES.path)?;
    let rows = model::extract_rows(&types, "items")?;
    let kind = rows
        .iter()
        .find(|row| row["id"] == type_id)
        .ok_or("予約種別を確認できません。選び直してください。")?;
    if original.is_null() && kind["active"] == false {
        return Err("この予約種別は受付を停止しています。".into());
    }
    let quantity = effective("quantity")
        .as_i64()
        .ok_or("人数・数量を確認してください。")?;
    if kind["quantityPolicy"]["maxCount"]
        .as_i64()
        .is_some_and(|max| quantity > max)
    {
        return Err("人数・数量が予約種別の上限を超えています。".into());
    }
    let resource_id = effective("resourceId");
    if let Some(resource_id) = resource_id.as_str().filter(|value| !value.is_empty()) {
        let resources = api::fetch_record(token, tenant, RESOURCES.path)?;
        let rows = model::extract_rows(&resources, "items")?;
        let resource = rows
            .iter()
            .find(|row| string(row, "id") == resource_id)
            .ok_or("選択した場所・設備を確認できません。")?;
        if resource["active"] == false {
            return Err("この場所・設備は利用を停止しています。".into());
        }
        if !string(kind, "resourceModel").is_empty()
            && resource["resourceModel"] != kind["resourceModel"]
        {
            return Err("場所・設備が予約種別に対応していません。選び直してください。".into());
        }
    }
    let service = effective("serviceId");
    if let Some(service_id) = service.as_str().filter(|id| !id.is_empty()) {
        if original.is_null() || body.get("serviceId").is_some() || body.get("resourceId").is_some()
        {
            let catalogue = workforce::load_plans(token, tenant, &json!({}))?;
            let rows = model::extract_rows(&catalogue, "items")?;
            let plan = rows
                .iter()
                .find(|plan| string(plan, "id") == service_id)
                .ok_or("選択した予約プランを確認できません。")?;
            validate_plan_selection(plan, body, original, resource_id.as_str())?;
        }
    }
    Ok(())
}

fn validate_staff_selection(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<(), String> {
    if let Some(ids) = body["assignedStaffIds"]
        .as_array()
        .filter(|ids| !ids.is_empty())
    {
        let staff = api::fetch_record(token, tenant, STAFF.path)?;
        let rows = model::extract_rows(&staff, "items")?;
        for selected in ids {
            if original["assignedStaffIds"]
                .as_array()
                .is_some_and(|ids| ids.contains(selected))
            {
                continue;
            }
            if !rows
                .iter()
                .any(|row| &row["id"] == selected && row["employmentStatus"] != "retired")
            {
                return Err("選択した担当スタッフを確認できません。選び直してください。".into());
            }
        }
    }
    Ok(())
}

pub fn validate_plan_selection(
    plan: &Value,
    body: &Value,
    original: &Value,
    resource_id: Option<&str>,
) -> Result<(), String> {
    let changed = original.is_null()
        || body
            .get("serviceId")
            .is_some_and(|id| id != &original["serviceId"]);
    if changed && (plan["available"] == false || plan["enabled"] == false) {
        return Err("選択した予約プランは受付を停止しています。".into());
    }
    let type_id = body
        .get("reservationTypeId")
        .unwrap_or(&original["reservationTypeId"]);
    if !string(plan, "reservationTypeId").is_empty() && &plan["reservationTypeId"] != type_id {
        return Err("予約プランと予約種別が一致しません。".into());
    }
    if let Some(resources) = plan["eligibleResourceIds"].as_array() {
        if !resource_id.is_some_and(|id| resources.iter().any(|value| value.as_str() == Some(id))) {
            return Err("このプランで利用できる場所・設備を選択してください。".into());
        }
    }
    Ok(())
}

/// Form defaults only. Staff may adjust a booking's end and requested price;
/// the server applies any legacy service and reservation-type policy overrides.
pub fn plan_form_defaults(plan: &Value, kind: Option<&Value>) -> Result<Value, String> {
    let price = amount(plan, "priceAmount")?;
    let type_policy = kind
        .map(|kind| &kind["paymentPolicyJson"])
        .unwrap_or(&Value::Null);
    let policy = plan
        .get("prepaymentPolicy")
        .filter(|value| !value.is_null())
        .or_else(|| type_policy.get("prepaymentPolicy"))
        .or_else(|| type_policy.get("prepaymentType"))
        .and_then(Value::as_str)
        .unwrap_or("full");
    let policy = match policy {
        "none" | "not_required" => "none",
        "optional" => "optional",
        "deposit" | "partial" | "deposit_required" => "deposit_required",
        "full" | "full_required" | "full_prepayment" => "full_prepayment",
        _ => return Err("プランの事前決済方式を確認できません。設定を確認してください。".into()),
    };
    let deposit = if matches!(policy, "none" | "full_prepayment") {
        0
    } else {
        let type_amount = ["depositAmount", "deposit_amount", "amount"]
            .iter()
            .find_map(|key| type_policy.get(*key).and_then(Value::as_i64));
        let type_ratio = ["depositRatio", "deposit_ratio", "ratio"]
            .iter()
            .find_map(|key| type_policy.get(*key).filter(|value| value.is_number()));
        let requested_ratio = plan
            .get("depositRatio")
            .filter(|value| !value.is_null())
            .or(type_ratio)
            .or_else(|| type_policy.get("requiredPaymentRatio"));
        let requested_amount = match type_policy["requiredPaymentAmount"]
            .as_i64()
            .filter(|amount| *amount > 0)
        {
            Some(amount) => amount,
            None => requested_ratio
                .map(|ratio| rounded_ratio_amount(price, ratio))
                .transpose()?
                .unwrap_or(0),
        };
        let amount = type_amount.unwrap_or(requested_amount);
        if amount < 0 {
            return Err("種別の前払い金を確認できません。".into());
        }
        // A type's explicit zero overrides the form input, then the server
        // computes its own ratio. Do not keep zero or substitute a plan ratio.
        if amount == 0 {
            type_ratio
                .map(|ratio| rounded_ratio_amount(price, ratio))
                .transpose()?
                .unwrap_or(0)
        } else {
            amount
        }
    };
    Ok(
        json!({"priceAmount":price.to_string(), "prepaymentPolicy":policy, "depositAmount":deposit.to_string()}),
    )
}

fn rounded_ratio_amount(amount: i64, ratio: &Value) -> Result<i64, String> {
    let text = ratio
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| ratio.to_string());
    let text = text.trim();
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (
            mantissa,
            exponent
                .parse::<i32>()
                .map_err(|_| "前払い割合を確認してください。")?,
        ),
        None => (text, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
        || whole.len() + fraction.len() > 20
        || !(-18..=18).contains(&exponent)
    {
        return Err("前払い割合は0〜1の小数で設定してください。".into());
    }
    let digits = format!("{whole}{fraction}")
        .parse::<i128>()
        .map_err(|_| "前払い割合を確認してください。")?;
    let scale =
        i32::try_from(fraction.len()).map_err(|_| "前払い割合を確認してください。")? - exponent;
    if !(-18..=18).contains(&scale) {
        return Err("前払い割合の桁数が大きすぎます。".into());
    }
    let numerator = digits
        .checked_mul(10i128.pow((-scale).max(0) as u32))
        .ok_or("前払い割合が大きすぎます。")?;
    let denominator = 10i128.pow(scale.max(0) as u32);
    if !(0..=denominator).contains(&numerator) {
        return Err("前払い割合は0〜1で設定してください。".into());
    }
    let result = (i128::from(amount) * numerator + denominator / 2) / denominator;
    i64::try_from(result).map_err(|_| "前払い金が大きすぎます。".into())
}

/// Supplemental failures stay visible without hiding the booking itself. Each
/// command tests its own readiness flag; missing history is never an empty history.
pub fn load_context(token: &str, tenant: &Tenant, original: &Value) -> Result<Value, String> {
    let path = record_path(original)?;
    let failed = || Err("補足情報の取得処理が完了しませんでした。".to_owned());
    let (cancellations, unmatched, templates, staff) = std::thread::scope(|scope| {
        let cancellations =
            scope.spawn(|| api::fetch_record(token, tenant, &format!("{path}/cancellations")));
        let unmatched =
            scope.spawn(|| api::fetch_record(token, tenant, &format!("{path}/unmatched-payments")));
        let templates = scope.spawn(|| api::fetch_record(token, tenant, TEMPLATES_PATH));
        let staff = scope.spawn(|| api::fetch_record(token, tenant, STAFF.path));
        (
            cancellations.join().unwrap_or_else(|_| failed()),
            unmatched.join().unwrap_or_else(|_| failed()),
            templates.join().unwrap_or_else(|_| failed()),
            staff.join().unwrap_or_else(|_| failed()),
        )
    });
    enrich_context(original, cancellations, unmatched, templates, staff)
}

fn enrich_context(
    original: &Value,
    cancellations: Result<Value, String>,
    unmatched: Result<Value, String>,
    templates: Result<Value, String>,
    staff: Result<Value, String>,
) -> Result<Value, String> {
    id(original)?;
    let mut value = original.clone();
    let mut warnings = Vec::new();
    value["_reservationContextReady"] =
        json!({"cancellations":false,"unmatchedPayments":false,"templates":false,"staff":false});
    for key in [
        "_reservationCancellations",
        "_reservationRefundEvents",
        "_reservationUnmatchedPayments",
        "_reservationReviewPayments",
        "_reservationStaffNames",
    ] {
        value[key] = json!([]);
    }
    value["_reservationTemplates"] = Value::Null;
    let cancellations = cancellations.and_then(|response| {
        let mut rows = model::extract_rows(&response, "items")?;
        let mut ids = HashSet::new();
        for row in &mut rows {
            if string(row, "id").is_empty()
                || !ids.insert(string(row, "id").to_owned())
                || row["reservationId"] != original["id"]
            {
                return Err("取消履歴の識別情報が一致しません。".into());
            }
            row["currency"] = original["currency"].clone();
        }
        Ok(rows)
    });
    match cancellations {
        Ok(rows) => {
            value["_reservationRefundEvents"] = json!(
                rows.iter()
                    .filter(|row| string(row, "source") == "reservation.cancel"
                        && row["refundAmount"]
                            .as_i64()
                            .is_some_and(|amount| amount > 0))
                    .cloned()
                    .collect::<Vec<_>>()
            );
            value["_reservationCancellations"] = json!(rows);
            value["_reservationContextReady"]["cancellations"] = json!(true);
        }
        Err(error) => warnings.push(format!("取消履歴を取得できません: {error}")),
    }
    let unmatched = unmatched.and_then(|response| {
        let mut rows = model::extract_rows(&response, "items")?;
        let mut ids = HashSet::new();
        for row in &mut rows {
            let provider = string(row, "provider");
            let payment_id = string(row, "paymentId");
            if provider.is_empty()
                || payment_id.is_empty()
                || amount(row, "amount").is_err()
                || string(row, "currency").is_empty()
            {
                return Err("未割当入金の内容を確認できません。".into());
            }
            let key = serde_json::to_string(&(provider, payment_id))
                .map_err(|_| "入金を識別できません。")?;
            if !ids.insert(key.clone()) {
                return Err("未割当入金が重複しています。".into());
            }
            let label = match provider {
                "square" => "Square",
                "stripe" => "Stripe",
                _ => "決済事業者",
            };
            row["providerLabel"] = json!(label);
            row["_key"] = json!(key);
        }
        Ok(rows)
    });
    match unmatched {
        Ok(rows) => {
            value["_reservationReviewPayments"] = json!(
                rows.iter()
                    .filter(|row| row["reviewedAt"].is_null())
                    .cloned()
                    .collect::<Vec<_>>()
            );
            value["_reservationUnmatchedPayments"] = json!(rows);
            value["_reservationContextReady"]["unmatchedPayments"] = json!(true);
        }
        Err(error) => warnings.push(format!("未割当入金を取得できません: {error}")),
    }
    match templates.and_then(|response| validate_templates(&response["items"])) {
        Ok(templates) => {
            value["_reservationTemplates"] = templates;
            value["_reservationContextReady"]["templates"] = json!(true);
        }
        Err(error) => warnings.push(format!("通知設定を取得できません: {error}")),
    }
    match staff.and_then(|response| model::extract_rows(&response, "items")) {
        Ok(rows) => {
            value["_reservationStaffNames"] = json!(
                original["assignedStaffIds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|assigned| {
                        rows.iter()
                            .find(|row| &row["id"] == assigned)
                            .and_then(|row| row["name"].as_str())
                            .unwrap_or("氏名を確認できないスタッフ")
                            .to_owned()
                    })
                    .collect::<Vec<_>>()
            );
            value["_reservationContextReady"]["staff"] = json!(true);
        }
        Err(error) => warnings.push(format!("担当スタッフ名を取得できません: {error}")),
    }
    value["_reservationContextWarning"] = json!(warnings.join("\n"));
    value["_reservationInternalNotes"] = json!(
        string(original, "internalNotes")
            .lines()
            .filter(|line| !line.starts_with("[reservation-notification "))
            .collect::<Vec<_>>()
            .join("\n")
    );
    Ok(value)
}

fn can_retry_refund(record: &Value) -> bool {
    is_status(record, "cancelled")
        && string(record, "paymentStatus") == "refund_pending"
        && record["_reservationContextReady"]["cancellations"] == true
        && record["_reservationRefundEvents"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
}
fn selected_row<'a>(
    body: &Value,
    original: &'a Value,
    field: &str,
    identity: &str,
) -> Result<&'a Value, String> {
    let selected: Vec<_> = body[field]
        .as_array()
        .ok_or("対象を読み取れません。")?
        .iter()
        .filter(|row| row["selected"] == true)
        .collect();
    if selected.len() != 1 {
        return Err("対象を1件選択してください。".into());
    }
    original[field]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row[identity] == selected[0][identity])
        })
        .ok_or_else(|| "選択した対象の識別情報を確認できません。再読み込みしてください。".into())
}
fn prepare_refund_retry(body: &mut Value, original: &Value) -> Result<(), String> {
    if !can_retry_refund(original) {
        return Err("再実行できる返金処理を確認できません。".into());
    }
    let event = selected_row(body, original, "_reservationRefundEvents", "id")?;
    if event["reservationId"] != original["id"]
        || string(event, "source") != "reservation.cancel"
        || amount(event, "refundAmount")? <= 0
    {
        return Err("この取消履歴は返金の再実行対象ではありません。".into());
    }
    *body = json!({"cancellationEventId":event["id"]});
    Ok(())
}
fn preflight_refund_retry(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<(), String> {
    let fresh = fetch_fresh(token, tenant, original)?;
    if string(&fresh, "paymentStatus") != "refund_pending" {
        return Err("返金状況が変わっています。詳細を再読み込みしてください。".into());
    }
    let response = api::fetch_record(
        token,
        tenant,
        &format!("{}/cancellations", record_path(original)?),
    )?;
    let rows = model::extract_rows(&response, "items")?;
    let prior = original["_reservationRefundEvents"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["id"] == body["cancellationEventId"])
        })
        .ok_or("確認した取消履歴を取得できません。")?;
    let fresh_event = rows
        .iter()
        .find(|row| row["id"] == body["cancellationEventId"])
        .ok_or("取消履歴を再読み込みしてください。")?;
    for key in [
        "id",
        "reservationId",
        "source",
        "reason",
        "refundAmount",
        "cancellationFeeAmount",
        "createdAt",
    ] {
        if prior[key] != fresh_event[key] {
            return Err("取消履歴が変わっています。再読み込みしてください。（409）".into());
        }
    }
    Ok(())
}
fn retry_refund(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let event = string(body, "cancellationEventId");
    if event.is_empty() {
        return Err("返金する取消履歴を選択してください。".into());
    }
    api::mutate_record(
        token,
        tenant,
        "POST",
        &format!(
            "{}/cancellations/{}/refund-retry",
            record_path(original)?,
            api::encode_query(event)
        ),
        &json!({}),
        None,
    )
}

fn can_review_payment(record: &Value) -> bool {
    record["_reservationContextReady"]["unmatchedPayments"] == true
        && record["_reservationReviewPayments"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
}
fn prepare_review(body: &mut Value, original: &Value) -> Result<(), String> {
    required_text(body, "note", "確認内容", 1000)?;
    let payment = selected_row(body, original, "_reservationReviewPayments", "_key")?;
    if !payment["reviewedAt"].is_null() {
        return Err("この入金は既に確認済みです。".into());
    }
    *body = json!({"provider":payment["provider"],"paymentId":payment["paymentId"],"note":body["note"]});
    Ok(())
}
fn preflight_review(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<(), String> {
    fetch_fresh(token, tenant, original)?;
    let response = api::fetch_record(
        token,
        tenant,
        &format!("{}/unmatched-payments", record_path(original)?),
    )?;
    let rows = model::extract_rows(&response, "items")?;
    let matches = |payment: &&Value| {
        payment["provider"] == body["provider"] && payment["paymentId"] == body["paymentId"]
    };
    let prior = original["_reservationReviewPayments"]
        .as_array()
        .and_then(|rows| rows.iter().find(matches))
        .ok_or("確認した入金を読み取れません。")?;
    let fresh = rows
        .iter()
        .find(matches)
        .ok_or("確認する入金を再読み込みしてください。")?;
    if !fresh["reviewedAt"].is_null()
        || ["provider", "paymentId", "amount", "currency", "recordedAt"]
            .iter()
            .any(|key| prior[*key] != fresh[*key])
    {
        return Err("入金の確認状況が変わっています。再読み込みしてください。（409）".into());
    }
    Ok(())
}
fn review_payment(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let path = record_path(original)?;
    let reviewed = api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{path}/unmatched-payments/review"),
        body,
        None,
    )?;
    if reviewed["reviewedAt"].is_null()
        || reviewed["provider"] != body["provider"]
        || reviewed["paymentId"] != body["paymentId"]
    {
        return Err(
            "入金の確認結果を読み取れません。再送せず、詳細を再読み込みしてください。".into(),
        );
    }
    Ok(json!({"reservation":api::fetch_record(token, tenant, &path)?,"reviewedPayment":reviewed}))
}

fn validate_templates(value: &Value) -> Result<Value, String> {
    for kind in ["confirmation", "reminder", "change_cancellation"] {
        if value[kind]["enabled"].as_bool().is_none()
            || string(&value[kind], "subject").trim().is_empty()
            || string(&value[kind], "body").trim().is_empty()
        {
            return Err("予約メールの設定を読み取れません。".into());
        }
    }
    Ok(value.clone())
}
fn can_notify(record: &Value, kind: &str) -> bool {
    let email = string(record, "customerEmail");
    record["_reservationContextReady"]["templates"] == true
        && record["_reservationTemplates"][kind]["enabled"] == true
        && email.contains('@')
        && !email.chars().any(char::is_whitespace)
}
fn can_notify_confirmation(record: &Value) -> bool {
    can_notify(record, "confirmation")
}
fn can_notify_reminder(record: &Value) -> bool {
    can_notify(record, "reminder")
}
fn can_notify_change(record: &Value) -> bool {
    can_notify(record, "change_cancellation")
}
fn notice_prefill(original: &Value, kind: &str) -> Result<Value, String> {
    if !can_notify(original, kind) {
        return Err("送信先または有効な通知設定を確認できません。".into());
    }
    let mut visible = original.clone();
    visible["subjectTemplate"] = original["_reservationTemplates"][kind]["subject"].clone();
    visible["bodyTemplate"] = original["_reservationTemplates"][kind]["body"].clone();
    Ok(visible)
}
fn prefill_confirmation(original: &Value) -> Result<Value, String> {
    notice_prefill(original, "confirmation")
}
fn prefill_reminder(original: &Value) -> Result<Value, String> {
    notice_prefill(original, "reminder")
}
fn prefill_change(original: &Value) -> Result<Value, String> {
    notice_prefill(original, "change_cancellation")
}
fn prepare_notice(body: &mut Value, original: &Value, kind: &str) -> Result<(), String> {
    if !can_notify(original, kind) {
        return Err("送信先または有効な通知設定を確認できません。".into());
    }
    required_text(body, "subjectTemplate", "件名", 255)?;
    required_text(body, "bodyTemplate", "本文", 10_000)?;
    if string(original, "customerEmail").contains(['\n', '\r']) {
        return Err("送信先メールアドレスを確認してください。".into());
    }
    body["kind"] = json!(kind);
    Ok(())
}
fn prepare_confirmation_notice(body: &mut Value, original: &Value) -> Result<(), String> {
    prepare_notice(body, original, "confirmation")
}
fn prepare_reminder_notice(body: &mut Value, original: &Value) -> Result<(), String> {
    prepare_notice(body, original, "reminder")
}
fn prepare_change_notice(body: &mut Value, original: &Value) -> Result<(), String> {
    prepare_notice(body, original, "change_cancellation")
}
fn preflight_notice(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<(), String> {
    fetch_fresh(token, tenant, original)?;
    let fresh = api::fetch_record(token, tenant, TEMPLATES_PATH)?;
    let kind = string(body, "kind");
    let fresh = validate_templates(&fresh["items"])?;
    if fresh[kind] != original["_reservationTemplates"][kind] || fresh[kind]["enabled"] != true {
        return Err(
            "通知設定が変更されています。詳細を再読み込みして文面を確認してください。（409）"
                .into(),
        );
    }
    Ok(())
}

pub fn notification_preview(original: &Value, draft: &Value) -> Result<Value, String> {
    let zone = string(original, "timezone")
        .trim()
        .parse::<chrono_tz::Tz>()
        .unwrap_or(chrono_tz::Asia::Tokyo);
    let starts = date(original, "startsAt")?
        .with_timezone(&zone)
        .format("%Y/%m/%d %H:%M")
        .to_string();
    let ends = date(original, "endsAt")?
        .with_timezone(&zone)
        .format("%Y/%m/%d %H:%M")
        .to_string();
    let replace = |text: &str| {
        text.trim()
            .replace(
                "{{customerName}}",
                original["customerName"].as_str().unwrap_or("お客様"),
            )
            .replace(
                "{{reservationNumber}}",
                string(original, "reservationNumber"),
            )
            .replace("{{reservationId}}", string(original, "id"))
            .replace("{{startsAt}}", &starts)
            .replace("{{endsAt}}", &ends)
            .replace("{{status}}", string(original, "status"))
            .replace("{{checkoutUrl}}", string(original, "checkoutUrl"))
    };
    required_text(draft, "subjectTemplate", "件名", 255)?;
    required_text(draft, "bodyTemplate", "本文", 10_000)?;
    Ok(
        json!({"to":original["customerEmail"],"subject":replace(string(draft,"subjectTemplate")),"body":replace(string(draft,"bodyTemplate")),
        "notice":"公開予約の確認メールには、お客様用の予約管理リンクが追加される場合があります。"}),
    )
}

pub fn checkout_url(value: &Value) -> Option<&str> {
    let url = value["checkoutUrl"].as_str()?.trim();
    let host = url.strip_prefix("https://")?.split('/').next()?;
    (!host.is_empty() && !host.contains('@') && !url.chars().any(char::is_whitespace))
        .then_some(url)
}
pub fn result_record(response: &Value) -> Option<Value> {
    let source = if response["reservation"].is_object() {
        &response["reservation"]
    } else {
        response
    };
    if id(source).is_err() || source["status"].as_str().is_none() {
        return None;
    }
    let mut record = source.clone();
    if let Some(url) = checkout_url(response) {
        record["checkoutUrl"] = json!(url);
    }
    Some(record)
}
fn validate_result(response: &Value) -> Result<(), String> {
    result_record(response).map(|_| ()).ok_or_else(|| {
        "予約の処理結果を確認できません。再送せず、一覧から予約と履歴を確認してください。".into()
    })
}
fn validate_billing_result(response: &Value) -> Result<(), String> {
    validate_result(response)?;
    if checkout_url(response).is_none() {
        return Err(
            "請求処理の支払リンクを確認できません。再送せず、予約詳細と請求書を確認してください。"
                .into(),
        );
    }
    Ok(())
}
fn created_message(response: &Value) -> String {
    let record = result_record(response).unwrap_or(Value::Null);
    match string(&record, "status") {
        "payment_pending" => "予約を登録しました。事前決済待ちです。支払リンクを確認してください。",
        "admin_review" => "予約を登録しました。管理者による確認が必要です。",
        "requested" => "予約を受け付けました。内容を確認して予約を確定してください。",
        _ => "予約を登録しました。現在の予約・支払状況を確認してください。",
    }
    .into()
}
fn saved_message(_: &Value) -> String {
    "予約を更新しました。現在の予約・支払状況をご確認ください。".into()
}
fn cancelled_message(response: &Value) -> String {
    let record = result_record(response).unwrap_or(Value::Null);
    match string(&record, "paymentStatus") {
        "refund_pending" => {
            "予約の取消を記録しました。返金は処理中です。取消履歴と返金状況を確認してください。"
        }
        "refunded" => "予約の取消と返金が反映されました。取消履歴をご確認ください。",
        "fee_due" => {
            "予約の取消を記録しました。キャンセル料は未払いです。請求・支払リンクをご確認ください。"
        }
        _ => "予約の取消を記録しました。取消履歴と支払状況をご確認ください。",
    }
    .into()
}
fn refund_message(response: &Value) -> String {
    let record = result_record(response).unwrap_or(Value::Null);
    if string(&record, "paymentStatus") == "refund_pending" {
        "返金処理を再開しました。まだ処理中のため、決済事業者からの結果を確認してください。".into()
    } else {
        "返金処理の結果を取得しました。支払状況と取消履歴をご確認ください。".into()
    }
}
fn billing_message(response: &Value) -> String {
    if response["reusedExistingInvoice"] == true || response["reusedExistingLink"] == true {
        "既存の支払リンクを取得しました。予約詳細から開けます。".into()
    } else {
        "支払リンクを作成しました。予約詳細で請求内容とリンクを確認してください。".into()
    }
}
fn review_message(_: &Value) -> String {
    "未割当入金の確認内容を記録しました。".into()
}
fn notification_message(_: &Value) -> String {
    "予約メールを送信しました。".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource() -> Resource {
        decorate(Resource::new(
            "reservations",
            "予約管理",
            "予約",
            PATH,
            "items",
        ))
    }
    fn action(label: &str) -> Mutation {
        *ACTIONS
            .iter()
            .find(|operation| operation.label == label)
            .unwrap()
    }
    fn booking() -> Value {
        json!({
            "id":"rsv_fixture", "reservationNumber":"R-2026-001", "reservationTypeId":"type_visit",
            "serviceId":null, "resourceId":"room_one", "assignedStaffIds":["staff_old", "staff_current"],
            "storeId":null, "customerId":"customer_one", "customerName":"テスト利用者",
            "customerEmail":"reservation@example.test", "customerPhone":"0000000000", "billTo":null,
            "status":"confirmed", "paymentStatus":"partial", "startsAt":"2026-12-03T01:00:00Z",
            "endsAt":"2026-12-03T02:00:00Z", "timezone":"Asia/Tokyo", "quantity":4,
            "priceAmount":12000, "depositAmount":3000, "paidAmount":3000, "currency":"JPY",
            "checkoutUrl":null, "policySnapshotJson":{"cancellationPolicy":"full_refund", "cancellationPolicyJson":{"cutoffHours":24}, "requiredPaymentAmount":3000},
            "customFieldsJson":{"accessibility":"段差なし"}, "notes":"到着時に連絡",
            "internalNotes":"準備を確認\n[reservation-notification kind=confirmation status=sent at=2026-11-20T10:00:00Z actor=fixture]",
            "invoiceId":null, "cancelledAt":null, "updatedAt":"2026-11-20T10:00:00Z"
        })
    }
    fn create_draft() -> Value {
        let mut value = model::mutation_draft(resource().create.unwrap(), &Value::Null).unwrap();
        for (key, field) in [
            ("reservationTypeId", "type_visit"),
            ("customerName", "テスト利用者"),
            ("customerEmail", "reservation@example.test"),
            ("startsAt", "2026-12-03 10:00"),
            ("endsAt", "2026-12-03 11:00"),
            ("quantity", "4"),
            ("priceAmount", "12000"),
        ] {
            value[key] = json!(field);
        }
        value
    }
    fn templates() -> Value {
        json!({"items":{
            "confirmation":{"enabled":true,"subject":"予約 {{reservationNumber}}","body":"{{customerName}} 様\n{{startsAt}}〜{{endsAt}} {{status}}"},
            "reminder":{"enabled":true,"subject":"ご来店のお知らせ","body":"{{startsAt}} にお待ちしています。"},
            "change_cancellation":{"enabled":true,"subject":"予約の変更","body":"{{reservationNumber}} の状況: {{status}}"}
        }})
    }
    fn context(original: &Value) -> Value {
        enrich_context(original, Ok(json!({"items":[
            {"id":"event/first","reservationId":"rsv_fixture","source":"reservation.cancel","refundAmount":2000,"cancellationFeeAmount":0,"reason":"顧客都合","createdAt":"2026-11-21T01:00:00Z"},
            {"id":"event_second","reservationId":"rsv_fixture","source":"reservation.cancel","refundAmount":1000,"cancellationFeeAmount":0,"reason":"変更分","createdAt":"2026-11-21T02:00:00Z"},
            {"id":"event_other","reservationId":"rsv_fixture","source":"reservation.update","refundAmount":100,"createdAt":"2026-11-20T23:00:00Z"}
        ]})), Ok(json!({"items":[
            {"provider":"square","paymentId":"pay_same","amount":1200,"currency":"JPY","recordedAt":"2026-11-21T03:00:00Z","reviewedAt":null},
            {"provider":"stripe","paymentId":"pay_same","amount":1300,"currency":"JPY","recordedAt":"2026-11-21T03:30:00Z","reviewedAt":null},
            {"provider":"square","paymentId":"pay_reviewed","amount":100,"currency":"JPY","recordedAt":"2026-11-21T02:00:00Z","reviewedAt":"2026-11-21T04:00:00Z"}
        ]})), Ok(templates()), Ok(json!({"items":[
            {"id":"staff_old","name":"旧担当者"}, {"id":"staff_current","name":"現在の担当者"}
        ]}))).unwrap()
    }
    fn at(value: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(value).unwrap()
    }

    #[test]
    fn create_form_keeps_total_money_exact_and_server_owned_fields_out() {
        let op = resource().create.unwrap();
        let mut draft = create_draft();
        draft["priceAmount"] = json!("9007199254740993");
        draft["assignedStaffIds"] = json!([{"staffId":"staff_current"}]);
        draft["status"] = json!("confirmed");
        draft["paymentStatus"] = json!("paid");
        let body = model::request_body(op, &draft, &Value::Null, "same-booking-key").unwrap();
        assert_eq!(body["priceAmount"], json!(9_007_199_254_740_993i64));
        assert_eq!(body["quantity"], 4);
        assert_eq!(body["assignedStaffIds"], json!(["staff_current"]));
        assert_eq!(body["startsAt"], "2026-12-03T10:00:00+09:00");
        assert_eq!(body["timezone"], "Asia/Tokyo");
        assert_eq!(body["currency"], "JPY");
        assert_eq!(body["idempotencyKey"], "same-booking-key");
        for key in [
            "status",
            "paymentStatus",
            "customFields",
            "billTo",
            "prepaymentPolicy",
        ] {
            assert!(body.get(key).is_none(), "unexpected {key}");
        }
        assert_eq!(
            body,
            model::request_body(op, &draft, &Value::Null, "same-booking-key").unwrap()
        );
    }

    #[test]
    fn create_validates_dates_counts_staff_and_explicit_prepayment() {
        let op = resource().create.unwrap();
        let mut draft = create_draft();
        for (key, invalid) in [
            ("quantity", "0"),
            ("quantity", "2147483648"),
            ("priceAmount", "-1"),
            ("endsAt", "2026-12-03 09:00"),
        ] {
            let mut bad = draft.clone();
            bad[key] = json!(invalid);
            assert!(
                model::request_body(op, &bad, &Value::Null, "key").is_err(),
                "{key}"
            );
        }
        draft["assignedStaffIds"] =
            json!([{"staffId":"staff_current"},{"staffId":"staff_current"}]);
        assert!(model::request_body(op, &draft, &Value::Null, "key").is_err());
        draft["assignedStaffIds"] = json!([]);
        draft["prepaymentPolicy"] = json!("deposit_required");
        draft["depositAmount"] = json!("3000");
        let body = model::request_body(op, &draft, &Value::Null, "key").unwrap();
        assert_eq!(body["prepaymentPolicy"], "deposit");
        assert_eq!(body["depositAmount"], 3000);
        draft["depositAmount"] = json!("12001");
        assert!(model::request_body(op, &draft, &Value::Null, "key").is_err());
        draft["depositAmount"] = json!("0");
        draft["prepaymentPolicy"] = json!("full_prepayment");
        assert_eq!(
            model::request_body(op, &draft, &Value::Null, "key").unwrap()["prepaymentPolicy"],
            "full"
        );
    }

    #[test]
    fn plan_defaults_use_exact_total_amount_and_policy_without_multiplying_people() {
        let plan =
            json!({"priceAmount":101,"prepaymentPolicy":"deposit_required","depositRatio":"0.5"});
        assert_eq!(
            plan_form_defaults(&plan, None).unwrap(),
            json!({"priceAmount":"101","prepaymentPolicy":"deposit_required","depositAmount":"51"})
        );
        let mut large = plan.clone();
        large["priceAmount"] = json!(9_007_199_254_740_993i64);
        large["depositRatio"] = json!("0.1");
        assert_eq!(
            plan_form_defaults(&large, None).unwrap()["depositAmount"],
            "900719925474099"
        );
        assert_eq!(rounded_ratio_amount(1_000_000, &json!("1e-6")).unwrap(), 1);
        assert!(rounded_ratio_amount(1000, &json!("1.01")).is_err());
        assert!(rounded_ratio_amount(1000, &json!("-0.1")).is_err());
        let kind = json!({"paymentPolicyJson":{"depositAmount":30}});
        assert_eq!(
            plan_form_defaults(&plan, Some(&kind)).unwrap()["depositAmount"],
            "30"
        );
        let full =
            json!({"priceAmount":15000,"prepaymentPolicy":"full_required","depositRatio":0.2});
        assert_eq!(
            plan_form_defaults(&full, None).unwrap()["prepaymentPolicy"],
            "full_prepayment"
        );
        assert_eq!(
            plan_form_defaults(&full, None).unwrap()["depositAmount"],
            "0"
        );
    }

    #[test]
    fn zero_type_deposit_falls_back_to_the_server_type_ratio() {
        let mut plan =
            json!({"priceAmount":10000,"prepaymentPolicy":"deposit_required","depositRatio":0.2});
        let kind = json!({"paymentPolicyJson":{"depositAmount":0,"depositRatio":0.2}});
        assert_eq!(
            plan_form_defaults(&plan, Some(&kind)).unwrap()["depositAmount"],
            "2000"
        );
        plan["depositRatio"] = json!(0.3);
        assert_eq!(
            plan_form_defaults(&plan, Some(&kind)).unwrap()["depositAmount"],
            "2000"
        );
        plan["depositRatio"] = json!(0);
        let ratio_only = json!({"paymentPolicyJson":{"depositRatio":0.2}});
        assert_eq!(
            plan_form_defaults(&plan, Some(&ratio_only)).unwrap()["depositAmount"],
            "2000"
        );
        let explicit = json!({"paymentPolicyJson":{"depositAmount":1500,"depositRatio":0.2}});
        assert_eq!(
            plan_form_defaults(&plan, Some(&explicit)).unwrap()["depositAmount"],
            "1500"
        );
    }

    #[test]
    fn plan_selection_checks_scope_but_allows_staff_time_and_price_adjustments() {
        let mut plan = json!({"id":"plan_one","available":true,"enabled":true,"reservationTypeId":"type_visit","eligibleResourceIds":["room_one"],"priceAmount":12000,"durationMinutes":60});
        let body = json!({"reservationTypeId":"type_visit","serviceId":"plan_one","priceAmount":10000,"startsAt":"2026-12-03T10:00:00+09:00","endsAt":"2026-12-03T10:45:00+09:00"});
        assert!(validate_plan_selection(&plan, &body, &Value::Null, Some("room_one")).is_ok());
        assert!(validate_plan_selection(&plan, &body, &Value::Null, Some("room_other")).is_err());
        plan["eligibleResourceIds"] = json!([]);
        assert!(validate_plan_selection(&plan, &body, &Value::Null, None).is_err());
        plan["eligibleResourceIds"] = Value::Null;
        assert!(validate_plan_selection(&plan, &body, &Value::Null, None).is_ok());
        plan["available"] = json!(false);
        assert!(validate_plan_selection(&plan, &body, &Value::Null, None).is_err());
        assert!(validate_plan_selection(&plan, &body, &booking(), None).is_err());
    }

    #[test]
    fn notes_edit_omits_unchanged_timestamps_and_preserves_staff_and_internal_audit() {
        let original = booking();
        let before = original.clone();
        let op = resource().edit.unwrap();
        let mut draft = model::mutation_draft(op, &original).unwrap();
        assert_eq!(
            draft["assignedStaffIds"],
            json!([{"staffId":"staff_old"},{"staffId":"staff_current"}])
        );
        draft["notes"] = json!("到着は5分前");
        let body = model::request_body(op, &draft, &original, "").unwrap();
        assert_eq!(body, json!({"notes":"到着は5分前"}));
        draft["notes"] = original["notes"].clone();
        draft["internalNote"] = json!("備品を準備");
        let body = model::request_body(op, &draft, &original, "").unwrap();
        assert_eq!(
            body,
            json!({"internalNotes":format!("{}\n備品を準備",original["internalNotes"].as_str().unwrap())})
        );
        assert_eq!(original, before);
    }

    #[test]
    fn customer_link_plan_and_resource_changes_keep_invoice_and_nullable_contracts() {
        let op = resource().edit.unwrap();
        let mut original = booking();
        original["serviceId"] = json!("plan_old");
        original["invoiceId"] = json!("invoice_existing");
        let mut draft = model::mutation_draft(op, &original).unwrap();
        draft["notes"] = json!("請求後のメモ");
        assert_eq!(
            model::request_body(op, &draft, &original, "").unwrap(),
            json!({"notes":"請求後のメモ"})
        );
        draft["customerId"] = json!("");
        assert!(
            model::request_body(op, &draft, &original, "")
                .unwrap_err()
                .contains("請求書")
        );
        draft["customerId"] = original["customerId"].clone();
        draft["serviceId"] = json!("plan_new");
        assert!(
            model::request_body(op, &draft, &original, "")
                .unwrap_err()
                .contains("請求書")
        );
        original["invoiceId"] = Value::Null;
        let changed = model::request_body(op, &draft, &original, "").unwrap();
        assert_eq!(changed["serviceId"], "plan_new");
        assert!(changed.get("priceAmount").is_none());
        assert!(changed.get("quantity").is_none());
        draft["customerId"] = json!("");
        assert!(
            model::request_body(op, &draft, &original, "")
                .unwrap()
                .get("customerId")
                .unwrap()
                .is_null()
        );
        draft["resourceId"] = json!("");
        assert!(
            model::request_body(op, &draft, &original, "")
                .unwrap_err()
                .contains("場所・設備")
        );
        draft["resourceId"] = original["resourceId"].clone();
        draft["serviceId"] = json!("");
        assert!(
            model::request_body(op, &draft, &original, "")
                .unwrap_err()
                .contains("プラン")
        );
    }

    #[test]
    fn staff_clear_and_date_change_do_not_send_immutable_money_or_quantity() {
        let original = booking();
        let op = resource().edit.unwrap();
        let mut draft = model::mutation_draft(op, &original).unwrap();
        draft["assignedStaffIds"] = json!([]);
        draft["startsAt"] = json!("2026-12-04 11:30");
        draft["endsAt"] = json!("2026-12-04 13:00");
        draft["priceAmount"] = json!("1");
        draft["quantity"] = json!("99");
        assert_eq!(
            model::request_body(op, &draft, &original, "").unwrap(),
            json!({"assignedStaffIds":[],"startsAt":"2026-12-04T11:30:00+09:00","endsAt":"2026-12-04T13:00:00+09:00"})
        );
        let mut terminal = original.clone();
        terminal["status"] = json!("cancelled");
        terminal["cancelledAt"] = json!("2026-11-21T10:00:00Z");
        let mut note = model::mutation_draft(op, &terminal).unwrap();
        note["notes"] = json!("取消後の問い合わせ");
        assert_eq!(
            model::request_body(op, &note, &terminal, "").unwrap(),
            json!({"notes":"取消後の問い合わせ"})
        );
    }

    #[test]
    fn status_commands_do_not_mark_money_paid_and_cannot_restore_released_inventory() {
        let mut original = booking();
        original["status"] = json!("payment_pending");
        original["paymentStatus"] = json!("unpaid");
        original["paidAmount"] = json!(0);
        for (label, status) in [
            ("予約を確定", "confirmed"),
            ("利用完了にする", "completed"),
            ("来店なしにする", "no_show"),
        ] {
            let op = action(label);
            assert_eq!(
                model::request_body(op, &json!({}), &original, "").unwrap(),
                json!({"status":status})
            );
            for terminal in ["cancelled", "rejected"] {
                let mut stopped = original.clone();
                stopped["status"] = json!(terminal);
                assert!(model::request_body(op, &json!({}), &stopped, "").is_err());
            }
        }
        original["status"] = json!("no_show");
        assert!(cancellable(&original));
        original["status"] = json!("completed");
        assert!(cancellable(&original));
    }

    #[test]
    fn cancellation_policy_cutoff_and_refund_match_the_server_for_all_policies() {
        let mut original = booking();
        original["paidAmount"] = json!(1000);
        original["depositAmount"] = json!(5000);
        for (policy, before_fee, before_refund, after_fee, after_refund) in [
            ("full_refund", 0, 1000, 0, 1000),
            ("no_refund", 0, 0, 11000, 0),
            ("fee_deducted", 0, 1000, 4000, 0),
        ] {
            original["policySnapshotJson"]["cancellationPolicy"] = json!(policy);
            let before = policy_amounts(&original, at("2026-12-02T00:59:59Z")).unwrap();
            let boundary = policy_amounts(&original, at("2026-12-02T01:00:00Z")).unwrap();
            assert_eq!(before["cancellationFeeAmount"], before_fee);
            assert_eq!(before["refundAmount"], before_refund);
            assert_eq!(boundary["cancellationFeeAmount"], after_fee);
            assert_eq!(boundary["refundAmount"], after_refund);
            assert_eq!(before["withinCutoff"], false);
            assert_eq!(boundary["withinCutoff"], true);
        }
        original["policySnapshotJson"]["cancellationPolicy"] = json!("fee_deducted");
        original["paidAmount"] = json!(7000);
        assert_eq!(
            policy_amounts(&original, at("2026-12-03T01:00:00Z")).unwrap()["refundAmount"],
            7000
        );
    }

    #[test]
    fn cancellation_form_freezes_confirmed_minor_amounts_and_uses_retry_header() {
        let original = booking();
        let op = action("予約を取り消す");
        assert!(matches!(op.idempotency, Idempotency::Header));
        let mut draft = model::mutation_draft(op, &original).unwrap();
        draft["reason"] = json!("ご本人からの取消依頼");
        draft["cancellationFeeAmount"] = json!("250");
        draft["refundAmount"] = json!("2750");
        let expected = json!({"reason":"ご本人からの取消依頼","cancellationFeeAmount":250,"refundAmount":2750});
        assert_eq!(
            model::request_body(op, &draft, &original, "retry-key").unwrap(),
            expected
        );
        let before = cancellation_preview(&original, &draft, at("2026-12-01T00:00:00Z")).unwrap();
        let after = cancellation_preview(&original, &draft, at("2026-12-03T02:00:00Z")).unwrap();
        assert_eq!(before["refundAmount"], after["refundAmount"]);
        assert_eq!(before["cancellationFeeAmount"], 250);
        for invalid in ["3001", "-1"] {
            draft["refundAmount"] = json!(invalid);
            assert!(model::request_body(op, &draft, &original, "retry-key").is_err());
        }
        draft["refundAmount"] = json!("0");
        draft["reason"] = json!("あ".repeat(1001));
        assert!(model::request_body(op, &draft, &original, "retry-key").is_err());
    }

    #[test]
    fn cancellation_currency_conversion_does_not_float_round_or_overrefund() {
        let mut original = booking();
        original["currency"] = json!("USD");
        original["paidAmount"] = json!(1234);
        let op = action("予約を取り消す");
        let mut draft = model::mutation_draft(op, &original).unwrap();
        assert_eq!(draft["refundAmount"], "12.34");
        draft["reason"] = json!("取消");
        draft["cancellationFeeAmount"] = json!("0.01");
        let body = model::request_body(op, &draft, &original, "key").unwrap();
        assert_eq!(body["refundAmount"], 1234);
        assert_eq!(body["cancellationFeeAmount"], 1);
        draft["refundAmount"] = json!("12.35");
        assert!(model::request_body(op, &draft, &original, "key").is_err());
    }

    #[test]
    fn fresh_snapshot_conflicts_include_money_assignment_and_notification_markers() {
        let original = booking();
        assert!(unchanged(&original, &original).is_ok());
        for (key, value) in [
            ("paidAmount", json!(4000)),
            ("resourceId", json!("room_other")),
            ("internalNotes", json!("new marker")),
            ("invoiceId", json!("new_invoice")),
            ("status", json!("cancelled")),
        ] {
            let mut fresh = original.clone();
            fresh[key] = value;
            assert!(
                unchanged(&original, &fresh).unwrap_err().contains("409"),
                "{key}"
            );
        }
        let mut invalid = original.clone();
        invalid["updatedAt"] = Value::Null;
        assert!(unchanged(&invalid, &invalid).is_err());
    }

    #[test]
    fn context_failures_keep_the_reservation_visible_but_disable_dependent_commands() {
        let original = booking();
        let failed = enrich_context(
            &original,
            Err("history denied".into()),
            Err("payments unavailable".into()),
            Err("templates denied".into()),
            Err("staff unavailable".into()),
        )
        .unwrap();
        assert_eq!(failed["id"], original["id"]);
        assert_eq!(failed["notes"], original["notes"]);
        assert!(!can_retry_refund(&failed));
        assert!(!can_review_payment(&failed));
        assert!(!can_notify_confirmation(&failed));
        assert!(
            failed["_reservationContextWarning"]
                .as_str()
                .unwrap()
                .contains("取消履歴")
        );
        assert_eq!(failed["_reservationInternalNotes"], "準備を確認");
        assert_eq!(failed["internalNotes"], original["internalNotes"]);
        let valid = context(&original);
        assert_eq!(
            valid["_reservationRefundEvents"].as_array().unwrap().len(),
            2
        );
        assert_eq!(
            valid["_reservationReviewPayments"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            valid["_reservationStaffNames"],
            json!(["旧担当者", "現在の担当者"])
        );
    }

    #[test]
    fn history_for_another_reservation_is_not_a_refund_candidate() {
        let original = booking();
        let value = enrich_context(&original,Ok(json!({"items":[{"id":"event","reservationId":"rsv_other","source":"reservation.cancel","refundAmount":1000}]})),Ok(json!({"items":[]})),Ok(templates()),Ok(json!({"items":[]}))).unwrap();
        assert_eq!(value["_reservationContextReady"]["cancellations"], false);
        assert_eq!(value["_reservationRefundEvents"], json!([]));
    }

    #[test]
    fn refund_retry_uses_one_existing_event_and_cannot_inject_new_refund_money() {
        let mut original = booking();
        original["status"] = json!("cancelled");
        original["paymentStatus"] = json!("refund_pending");
        original["cancelledAt"] = json!("2026-11-21T01:00:00Z");
        let original = context(&original);
        let op = action("取消済みの返金を再実行");
        let mut draft = model::mutation_draft(op, &original).unwrap();
        assert!(model::request_body(op, &draft, &original, "").is_err());
        draft["_reservationRefundEvents"][0]["selected"] = json!(true);
        draft["_reservationRefundEvents"][0]["refundAmount"] = json!("999999");
        let body = model::request_body(op, &draft, &original, "").unwrap();
        assert_eq!(body, json!({"cancellationEventId":"event/first"}));
        assert_eq!(
            model::request_path(resource(), op.path, &original, &body).unwrap(),
            "/v1/erp/reservations/rsv_fixture/cancellations/event%2Ffirst/refund-retry"
        );
        draft["_reservationRefundEvents"][1]["selected"] = json!(true);
        assert!(model::request_body(op, &draft, &original, "").is_err());
        draft["_reservationRefundEvents"][1]["selected"] = json!(false);
        draft["_reservationRefundEvents"][0]["id"] = json!("forged_event");
        assert!(model::request_body(op, &draft, &original, "").is_err());
    }

    #[test]
    fn unmatched_payment_review_keeps_provider_identity_and_server_note_limit() {
        let original = context(&booking());
        let op = action("未割当入金を確認済みにする");
        let mut draft = model::mutation_draft(op, &original).unwrap();
        draft["_reservationReviewPayments"][1]["selected"] = json!(true);
        draft["note"] = json!("決済事業者で確認済み");
        let body = model::request_body(op, &draft, &original, "").unwrap();
        assert_eq!(
            body,
            json!({"provider":"stripe","paymentId":"pay_same","note":"決済事業者で確認済み"})
        );
        draft["note"] = json!("あ".repeat(1001));
        assert!(model::request_body(op, &draft, &original, "").is_err());
        draft["note"] = json!("あ".repeat(1000));
        assert!(model::request_body(op, &draft, &original, "").is_ok());
        draft["_reservationReviewPayments"][1]["_key"] = json!("forged-key");
        assert!(model::request_body(op, &draft, &original, "").is_err());
    }

    #[test]
    fn notification_prefill_preview_and_send_preserve_templates_and_exact_local_time() {
        let original = context(&booking());
        let before = original.clone();
        let op = action("予約確認メールを送る");
        let mut draft = model::mutation_draft(op, &original).unwrap();
        draft["bodyTemplate"] = json!(
            "{{customerName}} {{startsAt}} / {{endsAt}}\n{{status}} {{checkoutUrl}} {{unknown}}"
        );
        let preview = notification_preview(&original, &draft).unwrap();
        assert_eq!(preview["to"], "reservation@example.test");
        assert_eq!(preview["subject"], "予約 R-2026-001");
        assert_eq!(
            preview["body"],
            "テスト利用者 2026/12/03 10:00 / 2026/12/03 11:00\nconfirmed  {{unknown}}"
        );
        let body = model::request_body(op, &draft, &original, "").unwrap();
        assert_eq!(body["kind"], "confirmation");
        assert!(body.get("customerEmail").is_none());
        assert_eq!(body["bodyTemplate"], draft["bodyTemplate"]);
        assert_eq!(original, before);
        assert!(matches!(op.idempotency, Idempotency::None));
    }

    #[test]
    fn notification_timezone_preview_observes_daylight_saving_and_template_availability() {
        let mut original = context(&booking());
        original["timezone"] = json!("America/New_York");
        original["startsAt"] = json!("2026-11-01T05:30:00Z");
        original["endsAt"] = json!("2026-11-01T06:30:00Z");
        let draft = json!({"subjectTemplate":"時間","bodyTemplate":"{{startsAt}} - {{endsAt}}"});
        assert_eq!(
            notification_preview(&original, &draft).unwrap()["body"],
            "2026/11/01 01:30 - 2026/11/01 01:30"
        );
        original["status"] = json!("payment_pending");
        assert!(can_notify_confirmation(&original));
        original["_reservationTemplates"]["confirmation"]["enabled"] = json!(false);
        assert!(!can_notify_confirmation(&original));
        original["customerEmail"] = json!("a@example.test\nwrong");
        assert!(!can_notify_change(&original));
    }

    #[test]
    fn returned_links_and_messages_do_not_claim_pending_refunds_are_finished() {
        let original = booking();
        let billing = json!({"reservation":original,"checkoutUrl":"https://payments.example.test/checkout/one","reusedExistingLink":true});
        assert!(validate_billing_result(&billing).is_ok());
        assert_eq!(
            result_record(&billing).unwrap()["checkoutUrl"],
            billing["checkoutUrl"]
        );
        for unsafe_url in [
            "javascript:alert(1)",
            "http://payments.example.test",
            "https://user@payments.example.test",
            "https://payments.example.test/with space",
        ] {
            assert!(checkout_url(&json!({"checkoutUrl":unsafe_url})).is_none());
        }
        let mut pending = original.clone();
        pending["status"] = json!("cancelled");
        pending["paymentStatus"] = json!("refund_pending");
        assert!(cancelled_message(&pending).contains("処理中"));
        assert!(refund_message(&pending).contains("処理中"));
        assert!(billing_message(&billing).contains("既存"));
        pending["status"] = json!("payment_pending");
        assert!(created_message(&json!({"reservation":pending})).contains("事前決済待ち"));
    }
}
