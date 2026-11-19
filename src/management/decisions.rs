//! Source-backed approval queues. Business records are loaded before approval;
//! AI rejection preserves the reviewed action and attempt versions.
use super::{model, schema::*};
use crate::api::{self, Tenant};
use serde_json::{Value, json};

fn approvals(token: &str, tenant: &Tenant, _: &Value) -> Result<Value, String> {
    let resource = Resource::new(
        "approval-source",
        "",
        "",
        "/api/approvals?resourceType=purchase_order&status=Requested",
        "items",
    )
    .pagination(Pagination::Offset { limit: 100 });
    let collection = model::fetch_collection(token, tenant, resource, &json!({}))?;
    let rows: Vec<_> = collection
        .rows
        .into_iter()
        .map(|mut row| {
            row["approvalId"] = row["id"].clone();
            row["approvalStatus"] = row["status"].clone();
            row["approvalUpdatedAt"] = row["updatedAt"].clone();
            row["name"] = json!("発注書の承認");
            row
        })
        .collect();
    Ok(json!({"items":rows,"truncated":collection.truncated}))
}
fn requested(v: &Value) -> bool {
    v["approvalStatus"].as_str() == Some("Requested") && v["purchaseOrderNumber"].is_string()
}
fn decision(b: &mut Value, o: &Value, action: &str) -> Result<(), String> {
    if o["approvalId"].as_str().is_none() {
        return Err("承認対象を確認できません".into());
    }
    *b = json!({"action":action,"id":o["approvalId"]});
    Ok(())
}
fn approve(b: &mut Value, o: &Value) -> Result<(), String> {
    decision(b, o, "approve")
}
fn reject(b: &mut Value, o: &Value) -> Result<(), String> {
    decision(b, o, "reject")
}
fn remand(b: &mut Value, o: &Value) -> Result<(), String> {
    decision(b, o, "remand")
}
fn perform_approval(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let id = original["resourceId"]
        .as_str()
        .ok_or("発注書を確認できません")?;
    let path = format!(
        "/api/approvals?limit=100&resourceType=purchase_order&status=Requested&resourceId={}",
        api::encode_query(id)
    );
    let response = api::fetch_record(token, tenant, &path)?;
    let current = response["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["id"] == original["approvalId"])
        .ok_or("承認待ちの状態が変わりました。読み直してください")?;
    if current["updatedAt"] != original["approvalUpdatedAt"]
        || current["currentStep"] != original["currentStep"]
    {
        return Err("承認の段階が変わりました。最新の内容を読み直してください".into());
    }
    let order = api::fetch_record(
        token,
        tenant,
        &format!("/v1/erp/purchase-orders/{}", api::encode_query(id)),
    )?;
    if order["updatedAt"] != original["updatedAt"]
        || order["items"] != original["items"]
        || order["totalAmount"] != original["totalAmount"]
    {
        return Err("発注書が変更されました。内容を読み直してから判断してください".into());
    }
    let saved = api::mutate_record(token, tenant, "POST", "/api/approvals", body, None)?;
    if saved["id"] != original["approvalId"]
        || saved["resourceId"] != original["resourceId"]
        || saved["resourceType"].as_str() != Some("purchase_order")
    {
        return Err("承認結果の対象を確認できませんでした。一覧を読み直してください".into());
    }
    Ok(saved)
}

fn attempts(token: &str, tenant: &Tenant, _: &Value) -> Result<Value, String> {
    let response = api::fetch_record(token, tenant, "/v1/erp/strategy/actions")?;
    let actions = response["actions"]
        .as_array()
        .ok_or("行動の一覧を読み取れませんでした")?;
    let eligible: Vec<_> = actions
        .iter()
        .filter(|a| {
            a["action"]["stage"].as_str() == Some("adopted")
                && a["action"]["execution"].as_str() == Some("ai")
        })
        .collect();
    let mut items = Vec::new();
    let mut failures = 0;
    for action in eligible.iter().take(100) {
        let Some(id) = action["id"].as_str() else {
            failures += 1;
            continue;
        };
        match api::fetch_record(
            token,
            tenant,
            &format!(
                "/v1/erp/strategy/actions/{}/attempts",
                api::encode_query(id)
            ),
        ) {
            Ok(value) => {
                let Some(rows) = value["attempts"].as_array() else {
                    failures += 1;
                    continue;
                };
                for row in rows
                    .iter()
                    .filter(|a| a["latest"].as_bool() == Some(true) && a["ended"].is_null())
                {
                    let mut row = row.clone();
                    row["title"] = action["title"].clone();
                    row["actionId"] = action["id"].clone();
                    row["actionVersion"] = action["version"].clone();
                    row["proposal"] = json!(
                        row["runs"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|r| r["observed"]["review"]["proposal"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    );
                    items.push(row);
                }
            }
            Err(_) => failures += 1,
        }
    }
    Ok(
        json!({"items":items,"truncated":eligible.len()>100,"notice":if failures>0 {format!("{failures}件の行動の実行状況を取得できませんでした。表示できた実行だけを示しています。")}else{String::new()}}),
    )
}
fn waiting(v: &Value) -> bool {
    v["latest"].as_bool() == Some(true)
        && v["ended"].is_null()
        && v["businessVersionCurrent"].as_bool() == Some(true)
        && v["judgment"]["state"].as_str() == Some("waiting")
}
fn reject_ai(b: &mut Value, o: &Value) -> Result<(), String> {
    b["decision"] = json!("reject");
    b["expectedVersion"] = json!(
        o["actionVersion"]
            .as_u64()
            .ok_or("行動の版を確認できません")?
    );
    Ok(())
}
fn perform_rejection(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let action = original["actionId"]
        .as_str()
        .ok_or("判断対象を確認できません")?;
    let attempt = original["id"].as_str().ok_or("判断対象を確認できません")?;
    let base = format!(
        "/v1/erp/strategy/actions/{}/attempts",
        api::encode_query(action)
    );
    let latest = api::fetch_record(token, tenant, &base)?;
    let current = latest["attempts"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["id"] == original["id"])
        .ok_or("判断対象が変わりました。読み直してください")?;
    if !waiting(current)
        || current["judgment"]["id"] != original["judgment"]["id"]
        || current["businessVersion"] != original["businessVersion"]
    {
        return Err("確認中に実行の状態が変わりました。最新の内容を読み直してください".into());
    }
    api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{base}/{}/judgment", api::encode_query(attempt)),
        body,
        None,
    )
}

pub const RESOURCES: &[Resource] = &[
    Resource::new(
        "purchase-order-decisions",
        "ホーム",
        "発注書の判断待ち",
        "/api/approvals",
        "items",
    )
    .load(approvals)
    .id("approvalId")
    .title(&["purchaseOrderNumber", "definitionSnapshot.name", "name"])
    .subtitle(&["requestedAt"])
    .status("approvalStatus")
    .detail("/v1/erp/purchase-orders/{resourceId}", "")
    .fields(&[
        ("発注番号", "purchaseOrderNumber"),
        ("仕入先", "vendorName"),
        ("通貨", "currency"),
        ("合計", "@money:totalAmount"),
        ("申請日時", "requestedAt"),
        ("承認の段階", "currentStep"),
        ("備考", "notes"),
    ])
    .read_lines(&[Field::new(
        "items",
        "発注明細",
        InputKind::Lines(&[
            Field::text("description", "内容"),
            Field::new("quantity", "数量", InputKind::Decimal),
            Field::new("unitCost", "単価", InputKind::Money),
            Field::new("amount", "金額", InputKind::Money),
            Field::new("taxAmount", "税額", InputKind::Money),
            Field::text("taxCategory", "税区分"),
        ]),
    )])
    .actions(&[
        Mutation::new("発注書を承認", "POST", "/api/approvals", &[])
            .when(requested)
            .actor_exclude("requestedBy")
            .prepare(approve)
            .execute(perform_approval)
            .explain("発注先・明細・合計を確認し、現在の承認段階を承認します。"),
        Mutation::new("発注書を差し戻す", "POST", "/api/approvals", &[])
            .when(requested)
            .actor_exclude("requestedBy")
            .prepare(remand)
            .execute(perform_approval)
            .destructive(),
        Mutation::new("発注書を却下", "POST", "/api/approvals", &[])
            .when(requested)
            .actor_exclude("requestedBy")
            .prepare(reject)
            .execute(perform_approval)
            .destructive(),
    ])
    .route("decisions"),
    Resource::new(
        "ai-decisions",
        "ホーム",
        "AI実行の判断待ち",
        "/v1/erp/strategy/actions",
        "items",
    )
    .load(attempts)
    .title(&["title"])
    .subtitle(&["reason"])
    .status("judgment.state")
    .fields(&[
        ("行動", "title"),
        ("実行理由", "reason"),
        ("AIからの提案", "proposal"),
        ("判断の状態", "judgment.state"),
        ("確認期限", "judgment.until"),
        ("業務の版が一致", "businessVersionCurrent"),
        ("照合結果", "judgment.review.state"),
        ("照合した日時", "judgment.review.at"),
    ])
    .actions(&[
        Mutation::new(
            "判断を拒否",
            "POST",
            "/v1/erp/strategy/actions/{actionId}/attempts/{id}/judgment",
            &[Field::new("reason", "拒否する理由", InputKind::Multiline).required()],
        )
        .when(waiting)
        .prepare(reject_ai)
        .execute(perform_rejection)
        .destructive()
        .explain("提案された実行を拒否します。理由は判断の記録に残ります。"),
        Mutation::new(
            "実行の状態を照合",
            "POST",
            "/v1/erp/strategy/actions/{actionId}/attempts/{id}/reconcile",
            &[],
        )
        .explain("実行先に現在の状態を問い合わせ、Fieldの判断待ちの記録を更新します。"),
    ])
    .route("decisions"),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_needs_loaded_business_record_and_ai_needs_current_waiting_version() {
        assert!(!requested(&json!({"approvalStatus":"Requested"})));
        assert!(requested(
            &json!({"approvalStatus":"Requested","purchaseOrderNumber":"PO1"})
        ));
        let mut v = json!({"latest":true,"ended":null,"businessVersionCurrent":true,"judgment":{"state":"waiting"}});
        assert!(waiting(&v));
        v["businessVersionCurrent"] = json!(false);
        assert!(!waiting(&v));
    }
}
