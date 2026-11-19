//! Store opening checks and the operational signals used by Field's home.
use super::{model, schema::*};
use crate::api::{self, Tenant};
use serde_json::{Value, json};

const DATE: &[Field] = &[Field::new("date", "営業日", InputKind::Date)
    .required()
    .default("@today")];
const USERS: Lookup = Lookup {
    path: "/v1/field/iam/users?reference_only=true",
    list_key: "users",
    value_key: "id",
    label_keys: &["name", "email"],
};
const NOTE: &[Field] = &[Field::new("note", "理由・引継ぎ", InputKind::Multiline).required()];
const OUTCOMES: &[Choice] = &[
    Choice::new("ok", "問題なし"),
    Choice::new("notApplicable", "対象外"),
    Choice::new("problem", "問題あり"),
];
const ITEM_FIELDS: &[Field] = &[
    Field::new("outcome", "点検結果", InputKind::Select(OUTCOMES))
        .required()
        .default("ok"),
    Field::new("note", "問題の内容", InputKind::Multiline).empty(Empty::Null),
    Field::new("photos", "確認した写真の枚数", InputKind::Integer)
        .required()
        .default("0")
        .help("0〜10枚。画像自体を登録する項目ではありません。"),
];
const ITEM_DISPLAY: &[Field] = &[
    Field::new("label", "点検項目", InputKind::ReadOnly),
    Field::text("state", "結果"),
    Field::text("note", "問題の内容"),
    Field::text("lastProblem", "前回の問題"),
    Field::new("photos", "写真の枚数", InputKind::Integer),
    Field::text("exception.decision", "責任者の判断"),
    Field::text("exception.reason", "判断理由"),
    Field::text("followUp.title", "対処の仕事"),
];
fn ongoing(v: &Value) -> bool {
    v["reportedAt"].is_null() && v["id"].is_string()
}
fn not_held(v: &Value) -> bool {
    ongoing(v) && v["nextStep"]["kind"].as_str() != Some("onHold")
}
fn held(v: &Value) -> bool {
    ongoing(v) && v["nextStep"]["kind"].as_str() == Some("onHold")
}
fn can_record(v: &Value) -> bool {
    v["_reportedAt"].is_null() && v["_nextStep"].as_str() != Some("onHold")
}
fn needs_decision(v: &Value) -> bool {
    v["_isResponsible"].as_bool() == Some(true)
        && v["_needsDecision"].as_bool() == Some(true)
        && v["exception"].is_null()
}
fn can_follow_up(v: &Value) -> bool {
    !v["_reportedAt"].is_null()
        && v["followUp"].is_null()
        && matches!(v["state"].as_str(), Some("problem" | "notDone"))
}
fn reason(v: &Value, key: &str) -> Result<(), String> {
    let text = v[key].as_str().unwrap_or("").trim();
    if text.is_empty() || text.chars().count() > 200 {
        Err("理由・内容を1〜200文字で入力してください".into())
    } else {
        Ok(())
    }
}
fn work(body: &mut Value, _: &Value, operation: &str) -> Result<(), String> {
    if operation == "hold" {
        reason(body, "note")?;
    }
    body["operation"] = json!(operation);
    Ok(())
}
fn claim(b: &mut Value, o: &Value) -> Result<(), String> {
    work(b, o, "claim")
}
fn hold(b: &mut Value, o: &Value) -> Result<(), String> {
    work(b, o, "hold")
}
fn resume(b: &mut Value, o: &Value) -> Result<(), String> {
    work(b, o, "resume")
}
fn handover(b: &mut Value, o: &Value) -> Result<(), String> {
    work(b, o, "handOver")
}
fn item(b: &mut Value, _: &Value) -> Result<(), String> {
    if b["outcome"].as_str() == Some("problem") {
        reason(b, "note")?;
    } else {
        b.as_object_mut().unwrap().remove("note");
    }
    if !b["photos"].as_i64().is_some_and(|n| (0..=10).contains(&n)) {
        return Err("写真の枚数は0〜10で入力してください".into());
    }
    Ok(())
}
fn exception(b: &mut Value, _: &Value) -> Result<(), String> {
    reason(b, "reason")
}
fn follow_up(b: &mut Value, o: &Value) -> Result<(), String> {
    let request = json!({"itemKey":o["key"],"kind":b["kind"],"title":b["title"]});
    *b = json!({"items":[request]});
    Ok(())
}
fn create_check(b: &mut Value, _: &Value) -> Result<(), String> {
    let site = b
        .as_object_mut()
        .unwrap()
        .remove("siteId")
        .ok_or("点検する拠点を選んでください")?;
    b["siteIds"] = json!([site]);
    Ok(())
}
fn load_checks(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let path = model::query_path("/v1/erp/strategy/store-opening-checks", DATE, filters)?;
    let mut response = api::fetch_record(token, tenant, &path)?;
    let profile = api::fetch_record(token, tenant, "/v1/erp/hrm/me").unwrap_or(Value::Null);
    let staff = profile["staff"]["id"].as_str();
    let checks = response["checks"]
        .as_array_mut()
        .ok_or("開店前点検の応答を読み取れませんでした")?;
    for check in checks {
        check["_isResponsible"] =
            json!(staff.is_some_and(|id| check["responsible"].as_str() == Some(id)));
    }
    Ok(response)
}
fn flatten_checks(response: &Value) -> Result<Value, String> {
    let mut rows = Vec::new();
    for check in response["checks"]
        .as_array()
        .ok_or("開店前点検の応答を読み取れませんでした")?
    {
        for item in check["items"]
            .as_array()
            .ok_or("点検項目を読み取れませんでした")?
        {
            let mut row = item.clone();
            row["checkId"] = check["id"].clone();
            row["storeName"] = check["storeName"].clone();
            row["businessDate"] = check["businessDate"].clone();
            row["_reportedAt"] = check["reportedAt"].clone();
            row["_holder"] = check["work"]["holder"].clone();
            row["_nextStep"] = check["nextStep"]["kind"].clone();
            row["_isResponsible"] = check["_isResponsible"].clone();
            row["_needsDecision"] = json!(
                check["nextStep"]["kind"].as_str() == Some("decide")
                    && check["nextStep"]["items"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|key| *key == item["key"])
            );
            row["outcome"] = if matches!(
                item["state"].as_str(),
                Some("ok" | "problem" | "notApplicable")
            ) {
                item["state"].clone()
            } else {
                Value::Null
            };
            rows.push(row);
        }
    }
    Ok(json!({"items":rows}))
}
fn load_items(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    flatten_checks(&load_checks(token, tenant, filters)?)
}

fn signals(token: &str, tenant: &Tenant, _: &Value) -> Result<Value, String> {
    let date = model::expanded_default("@today");
    let now = model::expanded_default("@now");
    let paths = [
        (
            "daily",
            "日報・営業実績",
            format!("/v1/erp/master-control/organization/reports/daily-flash?businessDate={date}"),
        ),
        (
            "stock",
            "進行中の棚卸",
            "/v1/field/inventory/stocktakes?page=1&pageSize=1&status=counting".into(),
        ),
        (
            "stock-approval",
            "承認待ちの棚卸",
            "/v1/field/inventory/stocktakes?page=1&pageSize=1&status=submitted".into(),
        ),
        (
            "low",
            "低在庫",
            "/v1/field/inventory/low-stock-alerts".into(),
        ),
        (
            "expiry",
            "期限の確認",
            format!(
                "/v1/field/inventory/expiring-lots?as_of={}&within_days=3",
                api::encode_query(&now)
            ),
        ),
        (
            "sales",
            "今日の売上",
            format!("/v1/erp/sales-dashboard?date_from={date}&date_to={date}"),
        ),
    ];
    let rows = std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .into_iter()
            .map(|(key, name, path)| {
                (
                    key,
                    name,
                    scope.spawn(move || api::fetch_record(token, tenant, &path)),
                )
            })
            .collect();
        handles
            .into_iter()
            .map(|(key, name, job)| {
                let result = job
                    .join()
                    .unwrap_or_else(|_| Err("読み込みを完了できませんでした".into()));
                let mut row = json!({"id":key,"name":name,"businessDate":date});
                match result.and_then(|value| signal_summary(key, &value)) {
                    Ok((status, summary)) => {
                        row["status"] = json!(status);
                        row["summary"] = json!(summary);
                    }
                    Err(message) => {
                        row["status"] = json!("確認できません");
                        row["summary"] = json!(message);
                    }
                }
                row
            })
            .collect::<Vec<_>>()
    });
    Ok(json!({"items":rows}))
}
fn signal_summary(key: &str, value: &Value) -> Result<(&'static str, String), String> {
    let invalid = || "集計の応答を読み取れませんでした".to_owned();
    match key {
        "daily" => {
            let total = &value["report"]["total"];
            let sites = total["sitesInScope"].as_u64().ok_or_else(invalid)?;
            let missing = total["unenteredSiteIds"]
                .as_array()
                .ok_or_else(invalid)?
                .len();
            Ok((
                if missing > 0 {
                    "要確認"
                } else {
                    "確認済み"
                },
                format!("{sites}拠点中、営業実績が未採用 {missing}拠点"),
            ))
        }
        "stock" | "stock-approval" => {
            let count = value["total"].as_u64().ok_or_else(invalid)?;
            Ok((
                if count > 0 {
                    "要確認"
                } else {
                    "対象なし"
                },
                format!("{count}件"),
            ))
        }
        "low" => {
            let count = value["items"].as_array().ok_or_else(invalid)?.len();
            Ok((
                if count > 0 {
                    "要確認"
                } else {
                    "対象なし"
                },
                format!("{count}件"),
            ))
        }
        "expiry" => {
            let items = value["items"].as_array().ok_or_else(invalid)?;
            let expired = items
                .iter()
                .filter(|r| r["status"].as_str() == Some("expired"))
                .count();
            let unknown = value["unconfirmedExpiryLotCount"]
                .as_u64()
                .ok_or_else(invalid)?;
            Ok((
                if !items.is_empty() || unknown > 0 {
                    "要確認"
                } else {
                    "対象なし"
                },
                format!(
                    "期限切れ {expired}・期限間近 {}・期限未入力 {unknown} ロット",
                    items.len() - expired
                ),
            ))
        }
        "sales" => {
            let amount = &value["summary"]["totalSales"];
            if !amount.is_number() {
                return Err(invalid());
            }
            Ok(("集計", format!("{} 円", model::display(amount))))
        }
        _ => Err(invalid()),
    }
}

pub const RESOURCES:&[Resource]=&[
    Resource::new("store-ops-today","ホーム","店舗の今日やること","/v1/field/inventory/low-stock-alerts","items").load(signals).title(&["name"]).subtitle(&["summary"]).status("status").fields(&[("業務","name"),("対象日","businessDate"),("状況","summary")]).route("store-ops"),
    Resource::new("home-operations","ホーム","業務の状況","/v1/field/inventory/low-stock-alerts","items").load(signals).title(&["name"]).subtitle(&["summary"]).status("status").fields(&[("業務","name"),("対象日","businessDate"),("状況","summary")]).route("/"),
    Resource::new("store-opening-checks","ホーム","開店前点検","/v1/erp/strategy/store-opening-checks","checks").load(load_checks).filters(DATE).title(&["storeName"]).subtitle(&["businessDate"]).status("nextStep.kind")
        .fields(&[("店舗","storeName"),("営業日","businessDate"),("次にすること","nextStep.kind"),("残りの項目数","nextStep.left"),("報告日時","reportedAt"),("保留の理由","work.hold.note")])
        .read_lines(&[Field::new("items","点検結果",InputKind::Lines(ITEM_DISPLAY))])
        .create(Mutation::new("点検を作成","POST","/v1/erp/strategy/macro-harness/businesses/storeOpeningCheck/actions",&[
            Field::new("periodId","計画期間",InputKind::Reference(Lookup{path:"/v1/erp/strategy/periods",list_key:"periods",value_key:"id",label_keys:&["name"]})).required(),
            Field::new("ownerId","責任者",InputKind::Reference(Lookup{path:"/v1/erp/hrm/staff",list_key:"items",value_key:"id",label_keys:&["name"]})).required(),
            Field::new("siteId","点検する拠点",InputKind::Reference(Lookup{path:"/v1/erp/master-control/organization",list_key:"sites",value_key:"id",label_keys:&["name","code"]})).required(),
            Field::new("businessDate","営業日",InputKind::Date).required().default("@today"),
        ]).prepare(create_check).explain("指定した拠点と営業日の点検の仕事を作成します。計画期間と責任者を確認してください。"))
        .actions(&[
            Mutation::new("点検を引き受ける","POST","/v1/erp/strategy/store-opening-checks/{id}/work",&[]).prepare(claim).when(not_held),
            Mutation::new("担当を引き継ぐ","POST","/v1/erp/strategy/store-opening-checks/{id}/work",&[Field::new("to","引継ぎ先",InputKind::Reference(USERS)).required(),Field::new("note","引継ぎのメモ",InputKind::Multiline)]).prepare(handover).when(not_held).actor_match("work.holder"),
            Mutation::new("点検を保留する","POST","/v1/erp/strategy/store-opening-checks/{id}/work",NOTE).prepare(hold).when(not_held).actor_match("work.holder"),
            Mutation::new("点検を再開する","POST","/v1/erp/strategy/store-opening-checks/{id}/work",&[]).prepare(resume).when(held).actor_match("work.holder"),
            Mutation::new("点検を報告する","POST","/v1/erp/strategy/store-opening-checks/{id}/report",&[]).when(not_held).actor_match("work.holder").explain("表示された各項目の結果を確認してください。記入していない項目は未実施のまま報告されます。"),
        ]).route("erp/store-opening-check"),
    Resource::new("store-opening-items","ホーム","開店前点検の記入","/v1/erp/strategy/store-opening-checks","items").load(load_items).filters(DATE).id("checkId|key").title(&["label"]).subtitle(&["storeName","businessDate"]).status("state")
        .fields(&[("店舗","storeName"),("営業日","businessDate"),("点検項目","label"),("結果","state"),("問題の内容","note"),("前回の問題","lastProblem"),("写真の枚数","photos"),("責任者の判断","exception.decision"),("判断理由","exception.reason"),("対処の仕事","followUp.title")])
        .edit(Mutation::new("点検結果を記録","PUT","/v1/erp/strategy/store-opening-checks/{checkId}/items/{key}",ITEM_FIELDS).prepare(item).when(can_record).actor_match("_holder"))
        .actions(&[
            Mutation::new("責任者の判断を記録","PUT","/v1/erp/strategy/store-opening-checks/{checkId}/items/{key}/exception",&[Field::new("decision","判断",InputKind::Select(&[Choice::new("openAnyway","このまま開ける"),Choice::new("defer","今日は見送る")])).required(),Field::new("reason","判断の理由",InputKind::Multiline).required()]).prepare(exception).when(needs_decision),
            Mutation::new("対処の仕事を作る","POST","/v1/erp/strategy/store-opening-checks/{checkId}/follow-ups",&[Field::new("kind","対処",InputKind::Select(&[Choice::new("remedy","対処する"),Choice::new("supportRequest","支援を頼む")])).required(),Field::text("title","仕事の名前").required()]).prepare(follow_up).when(can_follow_up),
        ]).route("erp/store-opening-check"),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn check_items_keep_parent_identity_and_existing_photo_count() {
        let flat=flatten_checks(&json!({"checks":[{"id":"check/a","storeName":"店","businessDate":"2026-11-19","reportedAt":null,"nextStep":{"kind":"record"},"work":{"holder":"us_a"},"items":[{"key":"safety/b","label":"安全","state":"problem","photos":4,"note":"要確認"}]}]})).unwrap();
        let row = &flat["items"][0];
        let op = RESOURCES[3].edit.unwrap();
        let draft = model::draft(op.fields, row);
        assert_eq!(
            model::request_body(op, &draft, row, "").unwrap()["photos"],
            4
        );
        assert_eq!(
            model::record_path(RESOURCES[3], op.path, row).unwrap(),
            "/v1/erp/strategy/store-opening-checks/check%2Fa/items/safety%2Fb"
        );
        assert_eq!(row["_holder"], "us_a");
    }
    #[test]
    fn malformed_home_sources_are_not_counted_as_zero() {
        assert!(signal_summary("stock", &json!({})).is_err());
        assert!(signal_summary("daily", &json!({"report":{"total":{"sitesInScope":3}}})).is_err());
        assert_eq!(
            signal_summary("stock", &json!({"total":0})).unwrap().0,
            "対象なし"
        );
    }
}
