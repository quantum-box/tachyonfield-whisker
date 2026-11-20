//! Customer email templates, adapted from the verified reservation API contract.
//! The server resolves legacy configuration; a missing template is never invented here.

use super::schema::{Field, InputKind, Mutation, Resource};
use crate::api::{self, Tenant};
use serde_json::{Value, json};

const PATH: &str = "/v1/erp/reservation-notification-templates";
const SNAPSHOT: &str = "_notificationTemplates";
const KINDS: &[(&str, &str)] = &[
    ("confirmation", "予約確認"),
    ("reminder", "リマインダー"),
    ("change_cancellation", "変更・取消のご案内"),
];

const FIELDS: &[Field] = &[
    Field::new("enabled", "この通知を利用する", InputKind::Bool).required(),
    Field::text("subject", "件名")
        .required()
        .help("255文字まで。{{customerName}}、{{reservationNumber}}、{{startsAt}}、{{status}}を利用できます。"),
    Field::new("body", "本文", InputKind::Multiline)
        .required()
        .help("10,000文字まで。無効にしても件名と本文は保持されます。"),
];

pub const RESOURCES: &[Resource] = &[
    Resource::new(
        "reservation-notification-templates",
        "予約管理",
        "お客様への予約通知設定",
        PATH,
        "items",
    )
    .load(load_templates)
    .title(&["name"])
    .subtitle(&["subject"])
    .status("enabled")
    .fields(&[
        ("通知", "name"),
        ("利用する", "enabled"),
        ("件名", "subject"),
        ("本文", "body"),
        ("差し込み項目", "variables"),
    ])
    .edit(
        Mutation::new("通知テンプレートを保存", "PUT", PATH, FIELDS)
            .prefill(prefill_template)
            .prepare(prepare_template)
            .execute(save_template)
            .explain("お客様へ送る予約メールの件名・本文を保存します。実際のメール送信は予約詳細から行えます。"),
    )
    .admin_only()
    .route("reservations/settings"),
];

/// Strict shape checking prevents an unavailable or newer response from becoming
/// a destructive set of client defaults. Unknown template variables remain text.
fn template(value: &Value) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or("通知テンプレートを読み取れませんでした")?;
    if object.len() != 3
        || object
            .keys()
            .any(|key| !matches!(key.as_str(), "enabled" | "subject" | "body"))
    {
        return Err(
            "未対応の通知設定が含まれています。設定を変更せず、Web管理画面で確認してください"
                .into(),
        );
    }
    let enabled = value["enabled"]
        .as_bool()
        .ok_or("通知の有効・無効を確認できませんでした")?;
    let subject = value["subject"]
        .as_str()
        .ok_or("通知の件名を確認できませんでした")?;
    let body = value["body"]
        .as_str()
        .ok_or("通知の本文を確認できませんでした")?;
    if subject.trim().is_empty() || body.trim().is_empty() {
        return Err("通知を無効にする場合も、件名と本文を入力してください".into());
    }
    if subject.trim().chars().count() > 255 {
        return Err("通知の件名は255文字以内で入力してください".into());
    }
    if body.trim().chars().count() > 10_000 {
        return Err("通知の本文は10,000文字以内で入力してください".into());
    }
    Ok(json!({"enabled": enabled, "subject": subject, "body": body}))
}

fn templates(value: &Value) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or("通知設定を取得できませんでした。再読み込みしてください")?;
    if object.len() != KINDS.len()
        || object
            .keys()
            .any(|key| !KINDS.iter().any(|(kind, _)| key == kind))
    {
        return Err(
            "3種類の通知設定を確認できませんでした。設定を変更せず、再読み込みしてください".into(),
        );
    }
    let mut result = serde_json::Map::new();
    for (kind, _) in KINDS {
        result.insert((*kind).into(), template(&value[*kind])?);
    }
    Ok(Value::Object(result))
}

fn normalized_templates(value: &Value) -> Result<Value, String> {
    let mut result = templates(value)?;
    for (kind, _) in KINDS {
        for field in ["subject", "body"] {
            let trimmed = result[*kind][field].as_str().unwrap().trim().to_owned();
            result[*kind][field] = json!(trimmed);
        }
    }
    Ok(result)
}

fn project(response: &Value) -> Result<Value, String> {
    let source = templates(&response["items"])?;
    let items: Vec<Value> = KINDS.iter().map(|(kind, name)| {
        let mut row = source[*kind].clone();
        row["id"] = json!(kind);
        row["name"] = json!(name);
        row["variables"] = json!("{{customerName}} お客様名 / {{reservationNumber}} 予約番号 / {{startsAt}} 開始日時 / {{status}} 予約状態");
        row[SNAPSHOT] = source.clone();
        row
    }).collect();
    Ok(json!({"items": items}))
}

fn load_templates(token: &str, tenant: &Tenant, _: &Value) -> Result<Value, String> {
    project(&api::fetch_record(token, tenant, PATH)?)
}

fn original_template(original: &Value) -> Result<(&str, Value), String> {
    let kind = original["id"]
        .as_str()
        .filter(|kind| KINDS.iter().any(|(known, _)| known == kind))
        .ok_or("通知の種類を確認できませんでした。再読み込みしてください")?;
    let source = templates(&original[SNAPSHOT])?;
    for field in ["enabled", "subject", "body"] {
        if original[field] != source[kind][field] {
            return Err("表示中の通知設定が一致しません。再読み込みしてください".into());
        }
    }
    Ok((kind, source))
}

fn prefill_template(original: &Value) -> Result<Value, String> {
    let (kind, source) = original_template(original)?;
    Ok(source[kind].clone())
}

fn prepare_template(body: &mut Value, original: &Value) -> Result<(), String> {
    let (kind, mut source) = original_template(original)?;
    source[kind] = template(body)?;
    // The first row persisted migrates a legacy configuration. Sending all three
    // loaded templates preserves the other two even before any row was saved.
    *body = json!({"items": normalized_templates(&source)?});
    Ok(())
}

/// None means a previous identical save already reached the server. There is no
/// revision token on this endpoint, so re-read before replacing all three kinds.
fn checked_update(
    body: &Value,
    original: &Value,
    current: &Value,
) -> Result<Option<Value>, String> {
    let (_, old) = original_template(original)?;
    let current = templates(&current["items"])?;
    let desired = normalized_templates(&body["items"])?;
    if normalized_templates(&current)? == desired {
        return Ok(None);
    }
    if current != old {
        return Err(
            "通知設定が別の操作で更新されています。一覧を読み直してから編集してください".into(),
        );
    }
    Ok(Some(json!({"items": desired})))
}

fn save_template(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    // Repeat the Web's administrator check at commit time as well as list load.
    let admin = api::fetch_record(token, tenant, "/v1/field/iam/users")?;
    if !admin["users"].is_array() {
        return Err("予約通知設定を変更する管理権限を確認できませんでした".into());
    }
    let current = api::fetch_record(token, tenant, PATH)?;
    let Some(payload) = checked_update(body, original, &current)? else {
        return Ok(current);
    };
    let response = api::mutate_record(token, tenant, "PUT", PATH, &payload, None)?;
    if normalized_templates(&response["items"])? != payload["items"] {
        return Err("通知設定の保存結果を確認できませんでした。一覧を読み直してください".into());
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        json!({"items":{
            "confirmation":{"enabled":true,"subject":"{{customerName}} 様の予約","body":"予約番号: {{reservationNumber}}\n\n{{ customToken }}"},
            "reminder":{"enabled":false,"subject":"以前に保存した件名","body":"以前に保存した本文\n二行目"},
            "change_cancellation":{"enabled":true,"subject":"変更のご案内","body":"状態: {{status}}"}
        }})
    }
    fn original() -> Value {
        project(&fixture()).unwrap()["items"][0].clone()
    }
    fn update(original: &Value) -> Value {
        let mut value = json!({"enabled":false,"subject":"  新しい件名  ","body":"  新しい本文\n\n{{customerName}}  "});
        prepare_template(&mut value, original).unwrap();
        value
    }

    #[test]
    fn reads_all_three_server_templates_without_inventing_defaults() {
        let projected = project(&fixture()).unwrap();
        assert_eq!(projected["items"].as_array().unwrap().len(), 3);
        assert_eq!(projected["items"][0]["id"], "confirmation");
        assert_eq!(projected["items"][1]["enabled"], false);
        assert_eq!(
            prefill_template(&projected["items"][0]).unwrap(),
            fixture()["items"]["confirmation"]
        );
        assert!(
            projected["items"][0]["body"]
                .as_str()
                .unwrap()
                .contains("{{ customToken }}")
        );
    }

    #[test]
    fn partial_or_unknown_settings_never_become_writable_defaults() {
        for malformed in [
            json!({}),
            json!({"items":[]}),
            json!({"items":{"confirmation":{"enabled":true,"subject":"件名","body":"本文"}}}),
        ] {
            assert!(project(&malformed).is_err());
        }
        let mut missing = fixture();
        missing["items"]["reminder"]
            .as_object_mut()
            .unwrap()
            .remove("enabled");
        assert!(project(&missing).is_err());
        let mut unknown = fixture();
        unknown["items"]["future_kind"] = unknown["items"]["confirmation"].clone();
        assert!(project(&unknown).is_err());
        let mut extension = fixture();
        extension["items"]["confirmation"]["futureDelivery"] = json!(true);
        assert!(project(&extension).is_err());
    }

    #[test]
    fn editing_one_template_preserves_both_other_templates_and_legacy_text() {
        let old = original();
        let body = update(&old);
        assert_eq!(
            body["items"]["confirmation"],
            json!({"enabled":false,"subject":"新しい件名","body":"新しい本文\n\n{{customerName}}"})
        );
        for kind in ["reminder", "change_cancellation"] {
            assert_eq!(body["items"][kind], fixture()["items"][kind]);
        }
        assert_eq!(old[SNAPSHOT], fixture()["items"]);
        assert_eq!(body.as_object().unwrap().len(), 1);
        assert!(!body.to_string().contains(SNAPSHOT));
    }

    #[test]
    fn template_limits_count_unicode_characters_and_apply_when_disabled() {
        let valid = json!({"enabled":false,"subject":"件".repeat(255),"body":"文".repeat(10_000)});
        assert!(template(&valid).is_ok());
        let mut invalid = valid.clone();
        invalid["subject"] = json!("件".repeat(256));
        assert!(template(&invalid).is_err());
        invalid = valid.clone();
        invalid["body"] = json!("文".repeat(10_001));
        assert!(template(&invalid).is_err());
        for key in ["subject", "body"] {
            invalid = valid.clone();
            invalid[key] = json!(" \n ");
            assert!(template(&invalid).is_err());
        }
        invalid = valid;
        invalid["enabled"] = json!("false");
        assert!(template(&invalid).is_err());
    }

    #[test]
    fn template_identity_and_loaded_values_cannot_be_rebound() {
        let mut record = original();
        record["id"] = json!("reminder");
        assert!(prefill_template(&record).is_err());
        record = original();
        record[SNAPSHOT] = Value::Null;
        assert!(prefill_template(&record).is_err());
        record = original();
        record["id"] = json!("future_kind");
        assert!(prepare_template(&mut json!({}), &record).is_err());
    }

    #[test]
    fn concurrent_changes_to_an_unedited_kind_are_not_overwritten() {
        let old = original();
        let body = update(&old);
        assert_eq!(
            checked_update(&body, &old, &fixture()).unwrap(),
            Some(body.clone())
        );
        let mut changed = fixture();
        changed["items"]["reminder"]["body"] = json!("別の担当者の編集");
        assert!(checked_update(&body, &old, &changed).is_err());
        assert!(checked_update(&body, &old, &json!({})).is_err());
    }

    #[test]
    fn an_already_committed_identical_update_is_recognized_without_rewriting() {
        let old = original();
        let body = update(&old);
        assert_eq!(checked_update(&body, &old, &body).unwrap(), None);
    }

    #[test]
    fn settings_keep_administrator_gate_and_offer_no_send_operation() {
        let resource = RESOURCES[0];
        assert!(resource.admin_only);
        assert_eq!(resource.web_route, "reservations/settings");
        assert_eq!(resource.path, PATH);
        assert!(resource.loader.is_some());
        assert!(resource.create.is_none());
        assert!(resource.actions.is_empty());
        let operation = resource.edit.unwrap();
        assert_eq!(operation.method, "PUT");
        assert_eq!(operation.path, PATH);
        assert!(operation.prefill.is_some());
        assert!(operation.executor.is_some());
        assert_eq!(operation.fields.len(), 3);
    }
}
