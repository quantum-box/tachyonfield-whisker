//! Native administration with explicit, reviewed request fields.
use super::schema::*;
use crate::api::{self, Tenant};
use serde_json::{Value, json};

const ROLES: &[Choice] = &[
    Choice::new("field:admin", "管理者"),
    Choice::new("field:staff", "スタッフ"),
    Choice::new("field:viewer", "閲覧のみ"),
];
fn not_owner(v: &Value) -> bool {
    v["isOwner"].as_bool() == Some(false)
}
fn member_edit(b: &mut Value, o: &Value) -> Result<(), String> {
    let role = b["role"]
        .as_str()
        .and_then(|r| r.strip_prefix("field:"))
        .ok_or("基本ロールを選んでください")?
        .to_owned();
    let policies = o["customPolicyIds"]
        .as_array()
        .ok_or("既存の権限を確認できません。読み直してください")?;
    if !policies.iter().all(Value::is_string) {
        return Err("既存の権限を確認できません".into());
    }
    b["role"] = json!(role);
    b["customPolicyIds"] = json!(policies);
    Ok(())
}
fn invite(b: &mut Value, _: &Value) -> Result<(), String> {
    let role = b["role"]
        .as_str()
        .and_then(|r| r.strip_prefix("field:"))
        .ok_or("基本ロールを選んでください")?
        .to_owned();
    b["role"] = json!(role);
    b["customPolicyIds"] = json!([]);
    b["notifyUser"] = json!(true);
    b["email"] = json!(b["email"].as_str().unwrap_or("").trim().to_lowercase());
    Ok(())
}
fn invitation_result(v: &Value) -> Result<(), String> {
    if v["invitationEmailSent"].as_bool() == Some(false) {
        return Err("招待は登録されましたが、招待メールを送信できませんでした。登録済みのため再招待する前にメンバー一覧を確認してください".into());
    }
    if v["user"].is_object() || v["invitationSent"].as_bool() == Some(true) {
        Ok(())
    } else {
        Err("招待結果を確認できませんでした。メンバー一覧を確認してください".into())
    }
}
fn renewal(b: &mut Value, _: &Value) -> Result<(), String> {
    if !b["withinDays"]
        .as_i64()
        .is_some_and(|d| (1..=366).contains(&d))
    {
        return Err("通知までの日数は1〜366で入力してください".into());
    }
    Ok(())
}
fn flow(b: &mut Value, o: &Value) -> Result<(), String> {
    for key in ["name", "steps", "minAmount", "siteId"] {
        let value = o.get(key).ok_or("承認ルールを読み直してください")?;
        b[key] = value.clone();
    }
    Ok(())
}
fn enable_flow(b: &mut Value, o: &Value) -> Result<(), String> {
    flow(b, o)?;
    b["active"] = json!(true);
    Ok(())
}
fn disable_flow(b: &mut Value, o: &Value) -> Result<(), String> {
    flow(b, o)?;
    b["active"] = json!(false);
    Ok(())
}
fn new_flow(b: &mut Value, _: &Value) -> Result<(), String> {
    *b = json!({"name":"経費の承認","resourceType":"expense","active":true,"steps":[{"approverRole":"approver","minApprovals":1,"name":"承認","stepOrder":1}]});
    Ok(())
}
fn webhook(b: &mut Value, o: &Value) -> Result<(), String> {
    let url = b["url"].as_str().ok_or("通知先URLを入力してください")?;
    if !url.starts_with("https://") || url.chars().any(char::is_whitespace) {
        return Err("通知先URLはhttps://で始まるURLを入力してください".into());
    }
    b["event_types"] = o["event_types"]
        .as_array()
        .filter(|v| !v.is_empty())
        .cloned()
        .map(Value::Array)
        .ok_or("通知対象のイベントを確認できません。読み直してください")?;
    Ok(())
}
fn normalize_keys(value: &Value) -> Result<Value, String> {
    let rows = value
        .as_array()
        .or_else(|| value["apiKeys"].as_array())
        .or_else(|| value["api_keys"].as_array())
        .ok_or("APIキー一覧の応答を読み取れませんでした")?;
    let mut items = Vec::new();
    for row in rows {
        let pick = |a: &str, b: &str| {
            row.get(a)
                .or_else(|| row.get(b))
                .cloned()
                .unwrap_or(Value::Null)
        };
        let revoked = pick("revokedAt", "revoked_at");
        items.push(json!({"id":row["id"],"name":row["name"],"keyPrefix":pick("keyPrefix","key_prefix"),"createdAt":pick("createdAt","created_at"),"lastUsedAt":pick("lastUsedAt","last_used_at"),"revokedAt":revoked,"status":if revoked.is_null(){"有効"}else{"無効"},"useCases":row.get("useCases").or_else(||row.get("use_cases")).or_else(||row.get("scopes"))}));
    }
    Ok(json!({"items":items}))
}
fn keys(token: &str, tenant: &Tenant, _: &Value) -> Result<Value, String> {
    normalize_keys(&api::fetch_record(token, tenant, "/v1/field/api-keys")?)
}
fn active_key(v: &Value) -> bool {
    v["id"].is_string() && v["revokedAt"].is_null()
}

pub const RESOURCES:&[Resource]=&[
    Resource::new("settings-members","管理","メンバー管理","/v1/field/iam/users","users").title(&["name","email"]).subtitle(&["email"]).status("role")
        .fields(&[("名前","name"),("メール","email"),("基本ロール","role"),("オーナー","isOwner")])
        .create(Mutation::new("招待メールを送信","POST","/v1/field/iam/users/invite",&[Field::new("email","メールアドレス",InputKind::Email).required(),Field::new("role","基本ロール",InputKind::Select(ROLES)).required().default("field:staff")]).prepare(invite).validate_response(invitation_result).explain("入力したアドレスへ、この会社への招待メールを送信します。付与するロールを確認してください。"))
        .edit(Mutation::new("基本ロールを変更","PUT","/v1/field/iam/users/{id}/role",&[Field::new("role","基本ロール",InputKind::Select(ROLES)).required()]).prepare(member_edit).when(not_owner))
        .actions(&[Mutation::new("メンバーを削除","DELETE","/v1/field/iam/users/{id}",&[]).when(not_owner).actor_exclude("id").destructive().explain("この会社へのアクセスを削除します。対象のメンバーを確認してください。")]).admin_only().route("settings/users"),
    Resource::new("settings-expense-approval","管理","経費の承認設定","/v1/erp/approval-flows?resourceType=expense","items").pagination(Pagination::Offset{limit:100}).title(&["name"]).status("active")
        .fields(&[("ルール","name"),("有効","active"),("対象となる最低金額","minAmount")])
        .read_lines(&[Field::new("steps","承認の順序",InputKind::Lines(&[Field::text("name","段階"),Field::text("approverRole","承認ロール"),Field::new("minApprovals","必要な人数",InputKind::Integer)]))])
        .create(Mutation::new("経費の承認ルールを追加","POST","/v1/erp/approval-flows",&[]).prepare(new_flow).explain("今後提出される経費に、承認者1名の承認を必要とする標準ルールを追加します。"))
        .actions(&[Mutation::new("有効にする","PUT","/v1/erp/approval-flows/{id}",&[]).prepare(enable_flow).when(|v|v["active"].as_bool()==Some(false)),Mutation::new("このルールを停止","PUT","/v1/erp/approval-flows/{id}",&[]).prepare(disable_flow).when(|v|v["active"].as_bool()==Some(true)).destructive().explain("このルールを停止します。他に有効なルールがあれば承認は引き続き必要です。承認待ちの経費の状態は変わりません。")]).admin_only().route("settings/expense-approval"),
    Resource::new("settings-renewal-alerts","管理","販売契約の更新通知","/v1/erp/sales-contracts/renewal-alert-settings","").title(&[])
        .fields(&[("Slack通知","slackEnabled"),("更新までの日数","withinDays"),("メンション","slackMention")])
        .edit(Mutation::new("更新通知を保存","PUT","/v1/erp/sales-contracts/renewal-alert-settings",&[Field::new("slackEnabled","Slackへ通知",InputKind::Bool),Field::new("withinDays","何日前から通知するか",InputKind::Integer).required(),Field::text("slackMention","メンション").empty(Empty::Null)]).prepare(renewal)).admin_only().route("settings/sales-contract-renewal-alerts"),
    Resource::new("settings-webhooks","管理","Webhook","/v1/storekit/webhook-endpoints","").title(&["description","url"]).status("status")
        .fields(&[("説明","description"),("通知先URL","url"),("状態","status"),("通知対象","event_types"),("署名","signing_enabled")])
        .edit(Mutation::new("通知先を更新","PATCH","/v1/storekit/webhook-endpoints/{id}",&[Field::text("url","通知先URL").required(),Field::new("description","説明",InputKind::Multiline).empty(Empty::Null),Field::new("status","状態",InputKind::Select(&[Choice::new("active","有効"),Choice::new("paused","停止中")])).required()]).prepare(webhook))
        .actions(&[Mutation::new("Webhookを削除","DELETE","/v1/storekit/webhook-endpoints/{id}",&[]).destructive().explain("この通知先へのイベント送信を終了します。")]).admin_only().route("settings/webhooks"),
    Resource::new("settings-api-keys","管理","APIキー","/v1/field/api-keys","items").load(keys).title(&["name"]).status("status")
        .fields(&[("名前","name"),("キーの接頭辞","keyPrefix"),("作成日時","createdAt"),("最終利用日時","lastUsedAt"),("無効化日時","revokedAt"),("用途","useCases")])
        .actions(&[Mutation::new("APIキーを無効にする","POST","/v1/field/api-keys/{id}/revoke",&[]).when(active_key).destructive().explain("このキーを使う連携は利用できなくなります。対象を確認してください。")]).admin_only().route("settings/api-keys"),
    Resource::new("settings-payment-status","管理","決済の接続状況","/v1/erp/payments/stripe-connect/account","").title(&["accountMode"])
        .fields(&[("接続の種類","accountMode"),("決済が有効","chargesEnabled"),("必要情報の提出済み","detailsSubmitted")]).admin_only().route("settings/payments"),
    Resource::new("settings-rollout","管理","ERPの提供状況","/v1/field/rollout/status","").title(&["mode"])
        .fields(&[("提供モード","mode"),("利用できる","enabled"),("適用元","sources")]).admin_only().route("settings/erp-rollout"),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_member_role_keeps_custom_policies() {
        let old = json!({"id":"us_a","isOwner":false,"customPolicyIds":["p1","p2"]});
        let mut body = json!({"role":"field:viewer"});
        member_edit(&mut body, &old).unwrap();
        assert_eq!(body, json!({"role":"viewer","customPolicyIds":["p1","p2"]}));
        assert!(member_edit(&mut json!({"role":"field:staff"}), &json!({})).is_err());
    }
    #[test]
    fn api_key_adapter_drops_secret_and_preserves_revocation() {
        let v=normalize_keys(&json!({"api_keys":[{"id":"key","name":"連携","key":"SECRET","key_prefix":"f_","created_at":"2026-01-01","revoked_at":"2026-02-01","scopes":["field_customer_read"]}]})).unwrap();
        assert!(!v.to_string().contains("SECRET"));
        assert!(!active_key(&v["items"][0]));
        assert_eq!(v["items"][0]["useCases"][0], "field_customer_read");
    }
}
