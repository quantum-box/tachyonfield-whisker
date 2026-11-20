//! The queue consumes no inventory. Promotion is an explicit, non-replayed command.
use super::{model, schema::*};
use crate::api::{self, Tenant};
use chrono::DateTime;
use serde_json::{Value, json};

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
    label_keys: &["name", "email"],
};
const STATUSES: &[Choice] = &[
    Choice::new("waiting", "キャンセル待ち"),
    Choice::new("converted", "予約へ繰り上げ済み"),
    Choice::new("withdrawn", "取り下げ"),
    Choice::new("expired", "期限切れ"),
];
const CONFIRM: Field = Field::new(
    "confirmed",
    "お客様との合意・対象日時と料金を確認した",
    InputKind::Bool,
);
const CREATE: &[Field] = &[
    Field::new("reservationTypeId", "予約種別", InputKind::Reference(TYPES)).required(),
    Field::new(
        "resourceId",
        "希望する場所・設備",
        InputKind::Reference(RESOURCES),
    ),
    Field::new(
        "customerId",
        "登録済みのお客様",
        InputKind::Reference(CUSTOMERS),
    ),
    Field::text("customerName", "お名前"),
    Field::new("customerEmail", "メールアドレス", InputKind::Email),
    Field::text("customerPhone", "電話番号"),
    Field::new("desiredStartsAt", "希望する開始日時", InputKind::DateTime).required(),
    Field::new("desiredEndsAt", "希望する終了日時", InputKind::DateTime).required(),
    Field::new("quantity", "人数・利用数", InputKind::Integer)
        .required()
        .default("1"),
    Field::new("notes", "お客様のご希望", InputKind::Multiline),
];
const CONVERT: &[Field] = &[
    Field::new(
        "resourceId",
        "確保する場所・設備",
        InputKind::Reference(RESOURCES),
    ),
    Field::new("startsAt", "開始日時", InputKind::DateTime)
        .required()
        .read("desiredStartsAt"),
    Field::new("endsAt", "終了日時", InputKind::DateTime)
        .required()
        .read("desiredEndsAt"),
    Field::new("priceAmount", "予約全体の料金（円）", InputKind::Integer)
        .required()
        .help("無料の場合も0を入力してください。人数分の自動乗算はしません。"),
    Field::new("depositAmount", "前払額（円）", InputKind::Integer),
    Field::new("notes", "予約に引き継ぐご希望", InputKind::Multiline),
    CONFIRM,
];
const FILTERS: &[Field] = &[
    Field::new("status", "待ち列の状態", InputKind::Select(STATUSES))
        .required()
        .default("waiting"),
    Field::new("reservationTypeId", "予約種別", InputKind::Reference(TYPES)),
    Field::new("resourceId", "場所・設備", InputKind::Reference(RESOURCES)),
    Field::new("customerId", "お客様", InputKind::Reference(CUSTOMERS)),
    Field::new("from", "希望開始日時から", InputKind::DateTime),
    Field::new("to", "希望開始日時の上限（含まない）", InputKind::DateTime),
    Field::new(
        "seatableOnly",
        "元の希望日時に空きがあるものだけ",
        InputKind::Bool,
    ),
];
pub fn decorate(resource: Resource) -> Resource {
    if resource.key != "reservation-waitlist" {
        return resource;
    }
    WAITLIST_RESOURCE
}
const WAITLIST_RESOURCE: Resource = Resource::new("reservation-waitlist", "予約管理", "キャンセル待ち", "/v1/erp/reservation-waitlist?limit=101", "items")
        .route("reservations/waitlist").load(load).filters(FILTERS)
        .title(&["customerName", "customerEmail", "customerPhone"])
        .subtitle(&["desiredStartsAt", "_availabilityLabel"]).status("status")
        .detail("/v1/erp/reservation-waitlist/{id}", "")
        .fields(&[("待ち列の状態", "status"), ("お名前", "customerName"), ("メール", "customerEmail"), ("電話番号", "customerPhone"),
            ("希望の開始", "desiredStartsAt"), ("希望の終了", "desiredEndsAt"), ("人数・利用数", "quantity"),
            ("元の日時の空き", "_availabilityLabel"), ("待ち順", "queuePosition"), ("受付日時", "enqueuedAt"),
            ("お客様のご希望", "notes"), ("終了理由", "closedReason"), ("終了日時", "closedAt")])
        .create(Mutation::new("キャンセル待ちに登録", "POST", "/v1/erp/reservation-waitlist", CREATE)
            .prepare(prepare_create).validate_response(check_entry).result_record(entry_result)
            .explain("希望日時の在庫は確保しません。登録後の希望変更は取り下げて登録し直すため、待ち順は末尾になります。"))
        .actions(&[
            Mutation::new("空き枠を選んで予約へ繰り上げ", "POST", "/v1/erp/reservation-waitlist/{id}/convert", CONVERT)
                .when(waiting).prepare(prepare_convert).execute(convert).validate_response(check_conversion)
                .result_record(converted_entry).success_message(convert_success)
                .explain("お客様と合意した日時・設備・料金で予約を作成します。別の空き枠も選べます。予約は1組分の在庫を使用します。決済が必要な予約は返された決済状態を確認してください。"),
            Mutation::new("キャンセル待ちを終了", "POST", "/v1/erp/reservation-waitlist/{id}/close", &[
                Field::new("status", "終了区分", InputKind::Select(&[Choice::new("withdrawn", "お客様の取り下げ"), Choice::new("expired", "期限切れ")])).required(),
                Field::new("reason", "終了理由", InputKind::Multiline).required(),
            ]).when(waiting).prepare(prepare_close).preflight(check_waiting).execute(close_entry).validate_response(check_entry).result_record(entry_result).destructive()
                .explain("待ち列から外します。予約を取り消す操作ではありません。"),
        ]);
pub fn waiting(value: &Value) -> bool {
    value["status"] == "waiting"
}
pub fn availability_label(value: &Value) -> String {
    match value["availableSlotGroups"].as_i64() {
        Some(0) => "満枠".into(),
        Some(n) if n > 0 => format!("空き {n}組"),
        _ => "予約枠が未開放".into(),
    }
}
fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .filter(|s| !s.trim().is_empty() && *s != "." && *s != "..")
        .ok_or_else(|| "対象の情報を確認できません。詳細を読み直してください".into())
}
fn interval(body: &Value, start: &str, end: &str) -> Result<(), String> {
    let start = DateTime::parse_from_rfc3339(required(body, start)?)
        .map_err(|_| "開始日時を確認してください")?;
    let end = DateTime::parse_from_rfc3339(required(body, end)?)
        .map_err(|_| "終了日時を確認してください")?;
    if end <= start {
        return Err("終了日時は開始日時より後にしてください".into());
    }
    Ok(())
}
fn prepare_create(body: &mut Value, _: &Value) -> Result<(), String> {
    interval(body, "desiredStartsAt", "desiredEndsAt")?;
    if body["quantity"]
        .as_i64()
        .is_none_or(|n| !(1..=1000).contains(&n))
    {
        return Err("人数・利用数は1〜1000で入力してください".into());
    }
    if [
        "customerId",
        "customerName",
        "customerEmail",
        "customerPhone",
    ]
    .iter()
    .all(|key| body[key].as_str().is_none_or(|s| s.trim().is_empty()))
    {
        return Err("お客様を選択するか、お名前・連絡先を入力してください".into());
    }
    text_limit(body, "notes", 1000)
}
fn text_limit(value: &Value, key: &str, limit: usize) -> Result<(), String> {
    if value[key]
        .as_str()
        .is_some_and(|text| text.chars().count() > limit)
    {
        return Err(format!("備考・理由は{limit}文字以内で入力してください"));
    }
    Ok(())
}
fn prepare_close(body: &mut Value, _: &Value) -> Result<(), String> {
    text_limit(body, "reason", 1000)
}
fn prepare_convert(body: &mut Value, original: &Value) -> Result<(), String> {
    if !waiting(original) {
        return Err("待ち列の状態が変わっています。読み直してください".into());
    }
    interval(body, "startsAt", "endsAt")?;
    let amount = body["priceAmount"]
        .as_i64()
        .filter(|n| *n >= 0)
        .ok_or("予約全体の料金を0以上で入力してください")?;
    if body
        .get("depositAmount")
        .is_some_and(|v| v.as_i64().is_none_or(|n| n < 0 || n > amount))
    {
        return Err("前払額は予約料金の範囲で入力してください".into());
    }
    if body["confirmed"] != true {
        return Err("お客様との合意・日時・料金を確認してチェックしてください".into());
    }
    body.as_object_mut().unwrap().remove("confirmed");
    body["currency"] = json!("JPY");
    text_limit(body, "notes", 1000)
}
fn load(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let path = model::query_path("/v1/erp/reservation-waitlist?limit=101", FILTERS, filters)?;
    let response = api::fetch_record(token, tenant, &path)?;
    let mut rows = model::extract_rows(&response, "items")?;
    let truncated = rows.len() > 100;
    rows.truncate(100);
    for row in &mut rows {
        check_entry(row)?;
        row["_availabilityLabel"] = json!(availability_label(row));
    }
    Ok(json!({"items":rows,"truncated":truncated}))
}
fn check_entry(value: &Value) -> Result<(), String> {
    required(value, "id")?;
    if !STATUSES.iter().any(|s| value["status"] == s.value) {
        return Err("待ち列の更新結果を確認できません。詳細を読み直してください".into());
    }
    Ok(())
}
fn fresh_entry(token: &str, tenant: &Tenant, original: &Value) -> Result<Value, String> {
    let id = required(original, "id")?;
    let value = api::fetch_record(
        token,
        tenant,
        &format!("/v1/erp/reservation-waitlist/{}", api::encode_query(id)),
    )?;
    check_entry(&value)?;
    if value["id"] != original["id"] {
        return Err("待ち列の対象が一致しません".into());
    }
    Ok(value)
}
fn check_waiting(token: &str, tenant: &Tenant, _: &Value, original: &Value) -> Result<(), String> {
    let fresh = fresh_entry(token, tenant, original)?;
    unchanged(original, &fresh)
}
fn unchanged(original: &Value, fresh: &Value) -> Result<(), String> {
    if !waiting(fresh)
        || [
            "id",
            "reservationTypeId",
            "resourceId",
            "customerId",
            "desiredStartsAt",
            "desiredEndsAt",
            "quantity",
        ]
        .iter()
        .any(|key| original[key] != fresh[key])
    {
        return Err("待ち列が変更されています。詳細を読み直してください（409）".into());
    }
    Ok(())
}
fn convert(token: &str, tenant: &Tenant, body: &Value, original: &Value) -> Result<Value, String> {
    check_waiting(token, tenant, body, original)?;
    let path = format!(
        "/v1/erp/reservation-waitlist/{}/convert",
        api::encode_query(required(original, "id")?)
    );
    match api::mutate_record(token, tenant, "POST", &path, body, None) {
        Ok(value) => {
            check_conversion(&value)?;
            if value["entry"]["id"] != original["id"] {
                return Err("繰り上げた待ち列の対象を確認できません".into());
            }
            Ok(value)
        }
        Err(error) => {
            // Conversion is create-then-stamp and has no atomic replay contract.
            // Read only: never repeat the POST after an uncertain response.
            if let Ok(entry) = fresh_entry(token, tenant, original) {
                if entry["status"] == "converted" {
                    return Err("待ち列は繰り上げ済みですが、今回の操作による予約か確認できません。別の予約も作成された可能性があるため、予約一覧と繰り上げ先を確認してください。この画面からは再送しません。".into());
                }
            }
            Err(format!(
                "{error}\n予約が作成済みの可能性があります。予約一覧と待ち列を読み直し、結果を確認してください。この画面からは再送しません。"
            ))
        }
    }
}
fn close_entry(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let path = format!(
        "/v1/erp/reservation-waitlist/{}/close",
        api::encode_query(required(original, "id")?)
    );
    let result = api::mutate_record(token, tenant, "POST", &path, body, None)?;
    validate_close(&result, original, body)?;
    Ok(result)
}
fn validate_close(result: &Value, original: &Value, body: &Value) -> Result<(), String> {
    check_entry(result)?;
    if result["id"] != original["id"] || result["status"] != body["status"] {
        return Err("待ち列は別の操作で終了しています。詳細を読み直して予約への繰り上げや終了状態を確認してください（409）".into());
    }
    Ok(())
}
fn check_conversion(value: &Value) -> Result<(), String> {
    let entry = &value["entry"];
    check_entry(entry)?;
    let id = required(&value["reservation"]["reservation"], "id")?;
    if entry["status"] != "converted" || entry["convertedReservationId"] != id {
        return Err("繰り上げ結果を確認できません。予約一覧と待ち列を読み直してください".into());
    }
    Ok(())
}
fn entry_result(value: &Value) -> Option<Value> {
    check_entry(value).ok().map(|_| value.clone())
}
fn converted_entry(value: &Value) -> Option<Value> {
    entry_result(&value["entry"])
}
pub fn converted_record(value: &Value) -> Option<Value> {
    let entry = if value["entry"].is_object() {
        &value["entry"]
    } else {
        value
    };
    if entry["status"] != "converted" {
        return None;
    }
    let id = entry["convertedReservationId"]
        .as_str()
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")?;
    Some(json!({"id":id}))
}
fn convert_success(_: &Value) -> String {
    "予約を作成しました。予約と決済の状態をご確認ください".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry() -> Value {
        json!({"id":"queue-1","status":"waiting","reservationTypeId":"type-1","desiredStartsAt":"2026-11-20T09:00:00+09:00","desiredEndsAt":"2026-11-20T10:00:00+09:00","quantity":4,"customerName":"お客様","availableSlotGroups":1})
    }
    #[test]
    fn group_capacity_is_not_party_size_and_unopened_is_not_full() {
        assert_eq!(availability_label(&entry()), "空き 1組");
        assert_eq!(
            availability_label(&json!({"availableSlotGroups":0})),
            "満枠"
        );
        assert_eq!(availability_label(&json!({})), "予約枠が未開放");
        assert_eq!(
            availability_label(&json!({"availableSlotGroups":null})),
            "予約枠が未開放"
        );
    }
    #[test]
    fn conversion_requires_agreed_price_and_explicit_confirmation_without_replay() {
        let r = decorate(Resource::new("reservation-waitlist", "", "", "/", ""));
        let op = r.actions[0];
        assert_eq!(op.idempotency, Idempotency::None);
        let original = entry();
        let mut draft = model::mutation_draft(op, &original).unwrap();
        assert!(model::request_body(op, &draft, &original, "").is_err());
        draft["priceAmount"] = json!("8000");
        draft["depositAmount"] = json!("2000");
        draft["confirmed"] = json!(true);
        let body = model::request_body(op, &draft, &original, "").unwrap();
        assert_eq!(body["priceAmount"], 8000);
        assert_eq!(body["currency"], "JPY");
        assert!(body.get("quantity").is_none());
        assert!(body.get("confirmed").is_none());
        draft["depositAmount"] = json!("8001");
        assert!(model::request_body(op, &draft, &original, "").is_err());
    }
    #[test]
    fn uncertain_conversion_opens_only_a_verified_result() {
        let response = json!({"entry":{"id":"queue-1","status":"converted","convertedReservationId":"booking-1"},"reservation":{"reservation":{"id":"booking-1"}}});
        assert!(check_conversion(&response).is_ok());
        assert_eq!(converted_record(&response), Some(json!({"id":"booking-1"})));
        let mut invalid = response.clone();
        invalid["reservation"]["reservation"]["id"] = json!("other");
        assert!(check_conversion(&invalid).is_err());
        assert!(converted_record(&entry()).is_none());
    }
    #[test]
    fn preflight_rejects_closed_or_reassigned_entries() {
        let original = entry();
        let mut fresh = original.clone();
        fresh["quantity"] = json!(2);
        assert!(unchanged(&original, &fresh).is_err());
        fresh = original.clone();
        fresh["status"] = json!("converted");
        assert!(unchanged(&original, &fresh).is_err());
    }
    #[test]
    fn a_competing_conversion_is_not_reported_as_a_successful_withdrawal() {
        let original = entry();
        let request = json!({"status":"withdrawn","reason":"辞退"});
        assert!(
            validate_close(
                &json!({"id":"queue-1","status":"converted","convertedReservationId":"rsv_other"}),
                &original,
                &request
            )
            .is_err()
        );
        assert!(
            validate_close(
                &json!({"id":"queue-1","status":"withdrawn"}),
                &original,
                &request
            )
            .is_ok()
        );
    }
    #[test]
    fn filter_dates_are_half_open_and_reserved_characters_are_encoded() {
        let filters = json!({"status":"waiting","resourceId":"a/b","from":"2026-11-20 00:00","seatableOnly":false});
        let path =
            model::query_path("/v1/erp/reservation-waitlist?limit=101", FILTERS, &filters).unwrap();
        assert!(path.contains("resourceId=a%2Fb"));
        assert!(path.contains("%2B09%3A00"));
        assert!(!path.contains("offset"));
    }
}
