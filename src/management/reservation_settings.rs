//! Booking masters and internal notification settings. Existing destinations are
//! retained independently of directory availability; only added identities need
//! to be resolved again before saving.

use super::{model, schema::*};
use crate::api::{self, Tenant};
use serde_json::{Value, json};
use std::collections::HashSet;

const TYPES_PATH: &str = "/v1/erp/reservation-types/manage?includeInactive=true";
const TYPES: Lookup = Lookup {
    path: TYPES_PATH,
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const STAFF: Lookup = Lookup {
    path: "/v1/erp/hrm/staff",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const FLAGS: &[&str] = &[
    "enabled",
    "notifyOnCreated",
    "notifyOnChanged",
    "notifyOnCancelled",
];
const NOTIFICATION_FIELDS: &[Field] = &[
    Field::new("reservationTypeName", "予約種別", InputKind::ReadOnly),
    Field::new("enabled", "スタッフに知らせる", InputKind::Bool),
    Field::new("notifyOnCreated", "新しい予約を通知", InputKind::Bool),
    Field::new("notifyOnChanged", "日時・場所の変更を通知", InputKind::Bool),
    Field::new("notifyOnCancelled", "キャンセルを通知", InputKind::Bool),
    Field::new("recipients", "メールの送信先", InputKind::StringLines)
        .help("1行に1アドレス。カンマ区切りも使えます。最大10件。"),
    Field::new(
        "staff",
        "通知するスタッフ",
        InputKind::Lines(&[
            Field::new("staffId", "スタッフ", InputKind::Reference(STAFF)).required(),
        ]),
    )
    .help(
        "最大20人。ログインアカウントと連携済みならそのメール、未連携なら人事のメールへ送ります。",
    ),
    Field::new("_slackConnections", "", InputKind::Hidden),
    Field::new("slackDirectoryNotice", "Slack接続", InputKind::ReadOnly),
    Field::new(
        "slackTargets",
        "Slackの通知先",
        InputKind::Lines(&[
            Field::new(
                "connectionId",
                "Slack接続",
                InputKind::RecordReference {
                    rows_key: "_slackConnections",
                    value_key: "id",
                    label_keys: &["displayName"],
                },
            )
            .empty(Empty::Null)
            .help("未選択の場合は会社の既定のSlack接続を使用します。"),
            Field::text("channel", "チャンネル")
                .required()
                .help("例 #front-desk。最大5件。Slackにはお客様の名前や連絡先は載りません。"),
        ]),
    ),
];

pub const RESOURCES: &[Resource] = &[
    Resource::new("reservation-staff-notifications", "予約管理", "予約の社内通知", "/v1/erp/reservation-types/{reservationTypeId}/staff-notification", "items")
        .id("reservationTypeId").route("reservations/types").load(load_notifications)
        .filters(&[Field::new("reservationTypeId", "予約種別", InputKind::Reference(TYPES)).required()])
        .title(&["reservationTypeName"]).subtitle(&["destinationsSummary"]).status("notificationState")
        .fields(&[("予約種別", "reservationTypeName"), ("社内通知", "enabled"),
            ("新しい予約", "notifyOnCreated"), ("日時・場所の変更", "notifyOnChanged"), ("キャンセル", "notifyOnCancelled"),
            ("メールの送信先", "recipients"), ("通知先", "destinationsSummary"), ("Slack接続の確認", "slackDirectoryNotice")])
        .edit(Mutation::new("社内通知を保存", "PUT", "/v1/erp/reservation-types/{reservationTypeId}/staff-notification", NOTIFICATION_FIELDS)
            .prepare(prepare_notification).execute(save_notification)
            .explain("この種別で今後発生する予約・変更・キャンセルの社内通知を設定します。お客様へのメールとは別の設定です。")),
];

const TYPE_ACTIONS: &[Mutation] = &[Mutation::new(
    "予約種別を削除",
    "DELETE",
    "/v1/erp/reservation-types/{id}",
    &[],
)
.execute(delete_type)
.destructive()
.explain("使用中の予約種別は削除できません。その場合は編集で利用を停止してください。")];
const RESOURCE_ACTIONS: &[Mutation] = &[Mutation::new(
    "場所・設備を削除",
    "DELETE",
    "/v1/erp/reservation-resources/{id}",
    &[],
)
.execute(delete_resource)
.destructive()
.explain("予約などで使用中の場所・設備は削除できません。その場合は編集で利用を停止してください。")];

pub fn decorate(mut resource: Resource) -> Resource {
    match resource.key {
        "reservation-types" => {
            if let Some(edit) = resource.edit {
                resource.edit = Some(edit.execute(save_type));
            }
            resource.actions = TYPE_ACTIONS;
        }
        "reservation-resources" => {
            if let Some(edit) = resource.edit {
                resource.edit = Some(edit.execute(save_resource));
            }
            resource.actions = RESOURCE_ACTIONS;
        }
        _ => {}
    }
    resource
}

fn required<'a>(value: &'a Value, key: &str, label: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{label}を確認してください"))
}
fn rows<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    value[key]
        .as_array()
        .ok_or_else(|| "予約設定の応答形式を確認できません".into())
}
fn type_path(id: &str) -> String {
    format!("/v1/erp/reservation-types/{}", api::encode_query(id))
}
fn notification_path(id: &str) -> String {
    format!("{}/staff-notification", type_path(id))
}
fn resource_path(id: &str) -> String {
    format!("/v1/erp/reservation-resources/{}", api::encode_query(id))
}
fn get_type(token: &str, tenant: &Tenant, id: &str) -> Result<Value, String> {
    api::fetch_record(token, tenant, TYPES_PATH).and_then(|data| {
        rows(&data, "items")?
            .iter()
            .find(|row| row["id"].as_str() == Some(id))
            .cloned()
            .ok_or_else(|| "予約種別を確認できません。読み直してください".into())
    })
}
fn compare_master(original: &Value, current: &Value, kind: &str) -> Result<(), String> {
    let keys: &[&str] = if kind == "type" {
        &[
            "id",
            "code",
            "name",
            "description",
            "resourceModel",
            "active",
            "quantityPolicy",
            "quantityPresentation",
            "phoneBookingEnabled",
            "phoneBookingNumber",
            "paymentPolicyJson",
            "cancellationPolicyJson",
            "industryExtensionId",
            "updatedAt",
        ]
    } else {
        &[
            "id",
            "name",
            "resourceType",
            "resourceModel",
            "active",
            "updatedAt",
        ]
    };
    if keys.iter().any(|key| original[*key] != current[*key]) {
        return Err("予約設定が変更されています。読み直してから操作してください（409）".into());
    }
    Ok(())
}
fn master_command(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
    kind: &str,
    delete: bool,
) -> Result<Value, String> {
    let id = required(original, "id", "予約設定")?;
    let (path, current) = if kind == "type" {
        (type_path(id), get_type(token, tenant, id)?)
    } else {
        let path = resource_path(id);
        let current = api::fetch_record(token, tenant, &path)?;
        (path, current)
    };
    compare_master(original, &current, kind)?;
    let result = api::mutate_record(
        token,
        tenant,
        if delete { "DELETE" } else { "PATCH" },
        &path,
        body,
        None,
    )?;
    if !delete && result["id"].as_str() != Some(id) {
        return Err("保存した予約設定を確認できません。再読み込みしてください".into());
    }
    Ok(result)
}
fn save_type(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    master_command(token, tenant, body, original, "type", false)
}
fn delete_type(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    master_command(token, tenant, body, original, "type", true)
}
fn save_resource(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    master_command(token, tenant, body, original, "resource", false)
}
fn delete_resource(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    master_command(token, tenant, body, original, "resource", true)
}

fn normalized_setting(value: &Value) -> Result<Value, String> {
    if !value.is_object() || FLAGS.iter().any(|key| !value[*key].is_boolean()) {
        return Err("社内通知の設定を確認できません".into());
    }
    let mut value = value.clone();
    for key in ["recipients", "staffIds", "slackTargets"] {
        if value[key].is_null() {
            value[key] = json!([]);
        }
        rows(&value, key)?;
    }
    for key in ["recipients", "staffIds"] {
        if rows(&value, key)?.iter().any(|item| !item.is_string()) {
            return Err("社内通知の宛先を確認できません".into());
        }
    }
    for target in rows(&value, "slackTargets")? {
        required(target, "channel", "Slackのチャンネル")?;
        if !target["connectionId"].is_null() && !target["connectionId"].is_string() {
            return Err("Slack接続の設定を確認できません".into());
        }
    }
    Ok(value)
}
fn fetch_notification(token: &str, tenant: &Tenant, id: &str) -> Result<Value, String> {
    normalized_setting(&api::fetch_record(token, tenant, &notification_path(id))?)
}
fn fetch_connections(token: &str, tenant: &Tenant, id: &str) -> Result<Vec<Value>, String> {
    let value = api::fetch_record(
        token,
        tenant,
        &format!("{}/slack-connections", notification_path(id)),
    )?;
    let mut connections = rows(&value, "items")?.clone();
    for row in &mut connections {
        required(row, "id", "Slack接続")?;
        required(row, "status", "Slack接続の状態")?;
        let name = row["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("名前のないSlack接続");
        row["displayName"] = json!(if row["status"] == "active" {
            name.to_owned()
        } else {
            format!("{name}（再接続が必要）")
        });
    }
    Ok(connections)
}
fn notification_view(
    source: &Value,
    reservation_type: &Value,
    connections: Result<Vec<Value>, String>,
) -> Result<Value, String> {
    let mut view = normalized_setting(source)?;
    view["_setting"] = source.clone();
    view["reservationTypeId"] = reservation_type["id"].clone();
    view["reservationTypeName"] = reservation_type["name"].clone();
    view["staff"] = json!(
        rows(source, "staffIds")?
            .iter()
            .map(|id| json!({"staffId":id}))
            .collect::<Vec<_>>()
    );
    view["destinationsSummary"] = json!(format!(
        "メール{}件・スタッフ{}人・Slack{}件",
        rows(source, "recipients")?.len(),
        rows(source, "staffIds")?.len(),
        rows(source, "slackTargets")?.len()
    ));
    view["notificationState"] = json!(if source["enabled"] == true {
        "通知する"
    } else {
        "通知しない"
    });
    match connections {
        Ok(connections) => {
            view["_slackConnections"] = json!(connections);
            view["slackDirectoryNotice"] =
                json!("接続先は名称で選べます。未選択は会社の既定の接続です。");
        }
        Err(_) => {
            view["_slackConnections"] = json!([]);
            view["slackDirectoryNotice"] = json!(
                "接続候補を読み込めませんでした。既存の通知先は保持します。新しい接続を選ぶには再読み込みしてください。"
            );
        }
    }
    Ok(view)
}
fn load_notifications(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let id = required(filters, "reservationTypeId", "予約種別")?;
    let reservation_type = get_type(token, tenant, id)?;
    let source = fetch_notification(token, tenant, id)?;
    Ok(
        json!({"items":[notification_view(&source,&reservation_type,fetch_connections(token,tenant,id))?]}),
    )
}

fn email_valid(value: &str) -> bool {
    let Some((local, host)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && !host.contains('@')
        && !value.chars().any(char::is_whitespace)
}
fn slack_channel_valid(value: &str) -> bool {
    if let Some(name) = value.strip_prefix('#') {
        (1..=58).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b))
    } else {
        (9..=20).contains(&value.len())
            && value.starts_with(['C', 'G'])
            && value
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    }
}
fn prepare_notification(body: &mut Value, _: &Value) -> Result<(), String> {
    if FLAGS.iter().any(|key| !body[*key].is_boolean()) {
        return Err("通知するタイミングを確認してください".into());
    }
    let mut recipients = Vec::new();
    let mut seen = HashSet::new();
    for value in rows(body, "recipients")? {
        for raw in value
            .as_str()
            .ok_or("メールアドレスを確認してください")?
            .split([',', '、', '\n'])
        {
            let address = raw.trim();
            if address.is_empty() {
                continue;
            }
            if !email_valid(address) {
                return Err(format!("メールアドレスを確認してください: {address}"));
            }
            if seen.insert(address.to_lowercase()) {
                recipients.push(json!(address));
            }
        }
    }
    if recipients.len() > 10 {
        return Err("メールの送信先は10件までです".into());
    }
    let mut staff_ids = Vec::new();
    let mut seen = HashSet::new();
    for row in rows(body, "staff")? {
        let id = required(row, "staffId", "スタッフ")?.trim();
        if seen.insert(id.to_owned()) {
            staff_ids.push(json!(id));
        }
    }
    if staff_ids.len() > 20 {
        return Err("通知するスタッフは20人までです".into());
    }
    let mut slack_targets = Vec::new();
    for row in rows(body, "slackTargets")? {
        let channel = required(row, "channel", "Slackのチャンネル")?.trim();
        if !slack_channel_valid(channel) {
            return Err("Slackのチャンネルは #front-desk のように指定してください（英小文字・数字・- _ .、58文字まで）".into());
        }
        let connection = row["connectionId"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let target = json!({"connectionId":connection,"channel":channel});
        if !slack_targets.contains(&target) {
            slack_targets.push(target);
        }
    }
    if slack_targets.len() > 5 {
        return Err("Slackの通知先は5件までです".into());
    }
    if body["enabled"] == true
        && recipients.is_empty()
        && staff_ids.is_empty()
        && slack_targets.is_empty()
    {
        return Err("通知先を1件以上設定してください".into());
    }
    *body = json!({"enabled":body["enabled"],"notifyOnCreated":body["notifyOnCreated"],"notifyOnChanged":body["notifyOnChanged"],"notifyOnCancelled":body["notifyOnCancelled"],"recipients":recipients,"staffIds":staff_ids,"slackTargets":slack_targets,"staffAccountEmails":{}});
    Ok(())
}
fn compare_notification(original: &Value, current: &Value) -> Result<(), String> {
    for key in FLAGS
        .iter()
        .copied()
        .chain(["recipients", "staffIds", "slackTargets"])
    {
        if original[key] != current[key] {
            return Err(
                "社内通知の設定が変更されています。読み直してから保存してください（409）".into(),
            );
        }
    }
    Ok(())
}
fn validate_added_destinations(
    body: &Value,
    current: &Value,
    staff: Option<&Value>,
    connections: Option<&[Value]>,
) -> Result<(), String> {
    for id in rows(body, "staffIds")? {
        if !rows(current, "staffIds")?.contains(id)
            && !staff.is_some_and(|staff| {
                staff["items"]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|row| row["id"] == *id))
            })
        {
            return Err("追加したスタッフを確認できません。候補を読み直してください".into());
        }
    }
    for target in rows(body, "slackTargets")? {
        if rows(current, "slackTargets")?.contains(target) || target["connectionId"].is_null() {
            continue;
        }
        if !connections.is_some_and(|rows| {
            rows.iter()
                .any(|row| row["id"] == target["connectionId"] && row["status"] == "active")
        }) {
            return Err(
                "新しい通知先のSlack接続が利用できません。接続候補を読み直してください".into(),
            );
        }
    }
    Ok(())
}
fn save_notification(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let id = required(original, "reservationTypeId", "予約種別")?;
    let current = fetch_notification(token, tenant, id)?;
    compare_notification(&original["_setting"], &current)?;
    let added_staff = rows(body, "staffIds")?
        .iter()
        .any(|id| !rows(&current, "staffIds").unwrap().contains(id));
    let added_connections = rows(body, "slackTargets")?.iter().any(|target| {
        !target["connectionId"].is_null()
            && !rows(&current, "slackTargets").unwrap().contains(target)
    });
    let staff = if added_staff {
        Some(api::fetch_record(token, tenant, STAFF.path)?)
    } else {
        None
    };
    let connections = if added_connections {
        Some(fetch_connections(token, tenant, id)?)
    } else {
        None
    };
    validate_added_destinations(body, &current, staff.as_ref(), connections.as_deref())?;
    let result = normalized_setting(&api::mutate_record(
        token,
        tenant,
        "PUT",
        &notification_path(id),
        body,
        None,
    )?)?;
    compare_notification(body, &result)
        .map_err(|_| "保存結果を確認できません。社内通知の設定を再読み込みしてください")?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setting() -> Value {
        json!({"enabled":true,"notifyOnCreated":true,"notifyOnChanged":false,"notifyOnCancelled":true,"recipients":["front@example.com"],"staffIds":["staff-one"],"slackTargets":[{"connectionId":"slack-one","channel":"#front-desk"}],"staffAccountEmails":{"staff-one":"account@example.com"}})
    }
    fn view() -> Value {
        notification_view(
            &setting(),
            &json!({"id":"type-one","name":"コース予約"}),
            Err("network".into()),
        )
        .unwrap()
    }
    #[test]
    fn unavailable_directories_never_drop_existing_destinations() {
        let original = view();
        let op = RESOURCES[0].edit.unwrap();
        let mut draft = model::draft(op.fields, &original);
        draft["enabled"] = json!(false);
        draft["recipients"] = json!(" changed@example.com,changed@EXAMPLE.com\nfront@example.com ");
        let body = model::request_body(op, &draft, &original, "").unwrap();
        assert_eq!(
            body["recipients"],
            json!(["changed@example.com", "front@example.com"])
        );
        assert_eq!(body["staffIds"], setting()["staffIds"]);
        assert_eq!(body["slackTargets"], setting()["slackTargets"]);
        assert_eq!(body["staffAccountEmails"], json!({}));
        assert!(body.get("_slackConnections").is_none());
        validate_added_destinations(&body, &setting(), None, None).unwrap();
    }
    #[test]
    fn new_staff_and_slack_targets_require_current_directory_members() {
        let mut body = setting();
        body["staffIds"] = json!(["staff-new"]);
        assert!(validate_added_destinations(&body, &setting(), None, None).is_err());
        let staff = json!({"items":[{"id":"staff-new","name":"社員"}]});
        validate_added_destinations(&body, &setting(), Some(&staff), None).unwrap();
        body["slackTargets"][0]["channel"] = json!("#another");
        assert!(validate_added_destinations(&body, &setting(), Some(&staff), None).is_err());
        let connections = vec![json!({"id":"slack-one","status":"active"})];
        validate_added_destinations(&body, &setting(), Some(&staff), Some(&connections)).unwrap();
        body["slackTargets"][0]["connectionId"] = Value::Null;
        validate_added_destinations(&body, &setting(), Some(&staff), None).unwrap();
    }
    #[test]
    fn invalid_destinations_and_empty_enabled_settings_are_rejected() {
        let original = view();
        let op = RESOURCES[0].edit.unwrap();
        let mut draft = model::draft(op.fields, &original);
        draft["recipients"] = json!("invalid");
        assert!(model::request_body(op, &draft, &original, "").is_err());
        draft["recipients"] = json!("");
        draft["staff"] = json!([]);
        draft["slackTargets"] = json!([]);
        assert!(model::request_body(op, &draft, &original, "").is_err());
        draft["enabled"] = json!(false);
        assert!(model::request_body(op, &draft, &original, "").is_ok());
        for channel in ["#Front", "plain-name", "#", "C123", "#space here"] {
            assert!(!slack_channel_valid(channel));
        }
        for channel in ["#front-desk", "#a.b_c", "C0123456789", "G01234567"] {
            assert!(slack_channel_valid(channel));
        }
    }
    #[test]
    fn draft_candidates_are_not_authority_and_stale_settings_are_blocked() {
        let original = view();
        let op = RESOURCES[0].edit.unwrap();
        let mut draft = model::draft(op.fields, &original);
        draft["_slackConnections"] = json!([{"id":"injected","displayName":"wrong"}]);
        draft["slackTargets"][0]["connectionId"] = json!("injected");
        let body = model::request_body(op, &draft, &original, "").unwrap();
        assert!(body.get("_slackConnections").is_none());
        assert!(validate_added_destinations(&body, &setting(), None, None).is_err());
        let mut changed = setting();
        changed["notifyOnChanged"] = json!(true);
        assert!(compare_notification(&setting(), &changed).is_err());
        let mut emails = setting();
        emails["staffAccountEmails"] = json!({});
        compare_notification(&setting(), &emails).unwrap();
        let before = json!({"id":"type-one","quantityPolicy":{"maxCount":4},"active":true});
        let mut after = before.clone();
        after["quantityPolicy"]["maxCount"] = json!(2);
        assert!(compare_master(&before, &after, "type").is_err());
    }
}
