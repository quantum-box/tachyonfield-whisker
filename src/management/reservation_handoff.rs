//! Append-only handoff notes, bound to the open reservation or waitlist entry.
use super::{model, schema::*};
use crate::api::{self, Tenant};
use serde_json::{Value, json};

const BODY: Field = Field::new("body", "引き継ぐ内容", InputKind::Multiline).required();
const CONFIRM: Field = Field::new("confirmed", "対象と内容を確認した", InputKind::Bool);
const CREATE_FIELDS: &[Field] = &[
    BODY,
    Field::text("category", "分類").default("general"),
    CONFIRM,
];
pub const CREATE_RESERVATION: Mutation = Mutation::new("引き継ぎメモを追加", "POST", "/v1/erp/handoff-notes", CREATE_FIELDS)
    .prefill(blank_note).prepare(prepare_reservation).validate_response(check_note).result_record(subject_record)
    .explain("スタッフ間の引き継ぎとして履歴に残します。お客様には送信しません。訂正は撤回と新しいメモの追加で行います。");
pub const CREATE_WAITLIST: Mutation = Mutation::new(
    "引き継ぎメモを追加",
    "POST",
    "/v1/erp/handoff-notes",
    CREATE_FIELDS,
)
.prefill(blank_note)
.prepare(prepare_waitlist)
.validate_response(check_note)
.result_record(subject_record)
.explain("このキャンセル待ちの引き継ぎとして履歴に残します。お客様には送信しません。");
pub const ACKNOWLEDGE: Mutation = Mutation::new(
    "このメモを引き継いだ",
    "POST",
    "/v1/erp/handoff-notes/{id}/acknowledge",
    &[
        Field::new("body", "引き継ぐ内容", InputKind::ReadOnly),
        CONFIRM,
    ],
)
.when(outstanding)
.prepare(prepare_acknowledge)
.preflight(check_current)
.validate_response(check_acknowledged)
.result_record(subject_record)
.explain("自分が引き継いだことを記録します。先に引き継いだ人の記録は上書きされません。");
pub const RETRACT: Mutation = Mutation::new(
    "このメモを撤回",
    "POST",
    "/v1/erp/handoff-notes/{id}/retract",
    &[
        Field::new("body", "撤回する内容", InputKind::ReadOnly),
        Field::new("reason", "撤回理由", InputKind::Multiline).required(),
        CONFIRM,
    ],
)
.when(active)
.prepare(prepare_retract)
.preflight(check_current)
.validate_response(check_retracted)
.result_record(subject_record)
.destructive()
.explain("本文を残したまま撤回済みにします。履歴は削除されません。");

pub fn subject_type(resource: &str) -> Option<&'static str> {
    match resource {
        "reservations" => Some("reservation"),
        "reservation-waitlist" => Some("reservation_waitlist_entry"),
        _ => None,
    }
}
fn id(value: &Value, key: &str) -> Result<String, String> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .map(str::to_owned)
        .ok_or_else(|| "引き継ぎの対象を確認できません。詳細を読み直してください".into())
}
pub fn outstanding(value: &Value) -> bool {
    active(value) && value["acknowledgedAt"].is_null()
}
fn active(value: &Value) -> bool {
    value["retractedAt"].is_null() && value["id"].is_string()
}
pub fn label(value: &Value) -> &'static str {
    if !value["retractedAt"].is_null() {
        "撤回済み"
    } else if !value["acknowledgedAt"].is_null() {
        "引き継ぎ済み"
    } else {
        "未引き継ぎ"
    }
}
fn blank_note(_: &Value) -> Result<Value, String> {
    Ok(json!({}))
}
fn confirmed(body: &mut Value) -> Result<(), String> {
    if body["confirmed"] != true {
        return Err("対象と内容を確認してチェックしてください".into());
    }
    body.as_object_mut().unwrap().remove("confirmed");
    Ok(())
}
fn prepare_create(body: &mut Value, original: &Value, subject: &str) -> Result<(), String> {
    confirmed(body)?;
    if body["body"].as_str().is_none_or(|s| s.trim().is_empty()) {
        return Err("引き継ぐ内容を入力してください".into());
    }
    if body["body"]
        .as_str()
        .is_some_and(|s| s.chars().count() > 4000)
    {
        return Err("引き継ぐ内容は4000文字以内で入力してください".into());
    }
    if body["category"]
        .as_str()
        .is_some_and(|s| s.chars().count() > 64)
    {
        return Err("分類は64文字以内で入力してください".into());
    }
    body["subjectId"] = json!(id(original, "id")?);
    body["subjectType"] = json!(subject);
    Ok(())
}
fn prepare_reservation(body: &mut Value, original: &Value) -> Result<(), String> {
    prepare_create(body, original, "reservation")
}
fn prepare_waitlist(body: &mut Value, original: &Value) -> Result<(), String> {
    prepare_create(body, original, "reservation_waitlist_entry")
}
fn prepare_acknowledge(body: &mut Value, original: &Value) -> Result<(), String> {
    if !outstanding(original) {
        return Err("このメモはすでに処理されています".into());
    }
    confirmed(body)
}
fn prepare_retract(body: &mut Value, original: &Value) -> Result<(), String> {
    if !active(original) {
        return Err("このメモはすでに撤回されています".into());
    }
    if body["reason"]
        .as_str()
        .is_some_and(|s| s.chars().count() > 1000)
    {
        return Err("撤回理由は1000文字以内で入力してください".into());
    }
    confirmed(body)
}
fn check_note(value: &Value) -> Result<(), String> {
    id(value, "id")?;
    id(value, "subjectId")?;
    if !matches!(
        value["subjectType"].as_str(),
        Some("reservation" | "reservation_waitlist_entry")
    ) || !value["body"].is_string()
    {
        return Err("引き継ぎの更新結果を確認できません。詳細を読み直してください".into());
    }
    Ok(())
}
fn check_acknowledged(value: &Value) -> Result<(), String> {
    check_note(value)?;
    if value["acknowledgedAt"].is_null() {
        return Err("引き継ぎの記録を確認できません".into());
    }
    Ok(())
}
fn check_retracted(value: &Value) -> Result<(), String> {
    check_note(value)?;
    if value["retractedAt"].is_null() {
        return Err("撤回の記録を確認できません".into());
    }
    Ok(())
}
fn subject_record(value: &Value) -> Option<Value> {
    check_note(value).ok()?;
    Some(json!({"id":value["subjectId"]}))
}
pub fn load(
    token: &str,
    tenant: &Tenant,
    subject: &str,
    subject_id: &str,
) -> Result<Value, String> {
    if !matches!(subject, "reservation" | "reservation_waitlist_entry") {
        return Err("引き継ぎの対象を確認できません".into());
    }
    let path = format!(
        "/v1/erp/handoff-notes?subjectType={}&subjectId={}&includeRetracted=true&limit=200",
        api::encode_query(subject),
        api::encode_query(subject_id)
    );
    let result = api::fetch_record(token, tenant, &path)?;
    normalize(result, subject, subject_id)
}
fn normalize(result: Value, subject: &str, subject_id: &str) -> Result<Value, String> {
    let mut rows = model::extract_rows(&result, "items")?;
    for row in &rows {
        check_note(row)?;
        if row["subjectType"] != subject || row["subjectId"] != subject_id {
            return Err("別の対象の引き継ぎメモが含まれています".into());
        }
    }
    let truncated = rows.len() >= 200;
    rows.truncate(200);
    Ok(json!({"items":rows,"truncated":truncated}))
}
fn check_current(token: &str, tenant: &Tenant, _: &Value, original: &Value) -> Result<(), String> {
    let rows = load(
        token,
        tenant,
        &id(original, "subjectType")?,
        &id(original, "subjectId")?,
    )?;
    let fresh = rows["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == original["id"])
        .ok_or("メモを確認できません。詳細を読み直してください")?;
    if [
        "body",
        "subjectType",
        "subjectId",
        "acknowledgedAt",
        "retractedAt",
    ]
    .iter()
    .any(|key| fresh[key] != original[key])
    {
        return Err("引き継ぎメモが変更されています。詳細を読み直してください（409）".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn note() -> Value {
        json!({"id":"note-one","subjectType":"reservation","subjectId":"booking-one","body":"アレルギーは受付で再確認","createdAt":"2026-11-20T00:00:00Z"})
    }
    #[test]
    fn subject_is_bound_to_the_open_record_and_not_editable() {
        let original = json!({"id":"booking-one"});
        let mut draft = model::mutation_draft(CREATE_RESERVATION, &original).unwrap();
        draft["body"] = json!("次の担当へ");
        draft["subjectId"] = json!("forged");
        draft["confirmed"] = json!(true);
        let body = model::request_body(CREATE_RESERVATION, &draft, &original, "").unwrap();
        assert_eq!(body["subjectId"], "booking-one");
        assert_eq!(body["subjectType"], "reservation");
        assert!(body.get("confirmed").is_none());
    }
    #[test]
    fn append_only_actions_preserve_the_original_text() {
        let original = note();
        assert!(outstanding(&original));
        let mut draft = model::mutation_draft(RETRACT, &original).unwrap();
        draft["reason"] = json!("誤記を訂正するため");
        draft["confirmed"] = json!(true);
        let body = model::request_body(RETRACT, &draft, &original, "").unwrap();
        assert!(body.get("body").is_none());
        assert!(body.get("id").is_none());
        let mut retracted = original.clone();
        retracted["retractedAt"] = json!("2026-11-20T01:00:00Z");
        assert!(!outstanding(&retracted));
        assert!(!RETRACT.is_available(&retracted));
    }
    #[test]
    fn response_and_read_scope_are_checked() {
        assert!(normalize(json!({"items":[note()]}), "reservation", "other").is_err());
        assert_eq!(subject_record(&note()), Some(json!({"id":"booking-one"})));
        assert!(check_acknowledged(&note()).is_err());
        assert!(check_retracted(&note()).is_err());
    }
    #[test]
    fn hitting_the_server_cap_is_never_treated_as_complete_history() {
        let response = normalize(
            json!({"items":vec![note();200]}),
            "reservation",
            "booking-one",
        )
        .unwrap();
        assert_eq!(response["truncated"], true);
    }
}
