//! Template-driven daily reports. The server template defines the fixed inputs;
//! confirmation and sales adoption keep their own state and version checks.
use super::{model, schema::*};
use crate::api::{self, Tenant};
use serde_json::{Value, json};

const FILTERS: &[Field] = &[
    Field::new(
        "siteId",
        "拠点",
        InputKind::Reference(Lookup {
            path: "/v1/erp/work-reports/sites",
            list_key: "sites",
            value_key: "id",
            label_keys: &["name", "code"],
        }),
    )
    .required(),
    Field::new("businessDate", "営業日", InputKind::Date)
        .required()
        .default("@today"),
    Field::new(
        "templateKey",
        "日報の種類",
        InputKind::DependentReference {
            lookup: Lookup {
                path: "/v1/erp/work-reports/sites",
                list_key: "sites",
                value_key: "key|id",
                label_keys: &["name", "code"],
            },
            parent_key: "siteId",
            children_key: "templates",
        },
    )
    .help("未選択の場合は拠点の標準の日報を使います"),
];
const AMOUNTS: &[Field] = &[
    Field::new("key", "項目", InputKind::Hidden),
    Field::new("label", "項目名", InputKind::ReadOnly),
    Field::new("hint", "入力の目安", InputKind::ReadOnly),
    Field::new("amount", "金額", InputKind::Money).empty(Empty::Null),
];
const COUNTS: &[Field] = &[
    Field::new("key", "項目", InputKind::Hidden),
    Field::new("label", "項目名", InputKind::ReadOnly),
    Field::new("amount", "人数・件数", InputKind::Integer)
        .empty(Empty::Null)
        .help("数えていなければ空欄のままにします。0とは区別されます。"),
];
const FIELDS: &[Field] = &[
    Field::new("currency", "通貨", InputKind::ReadOnly),
    Field::new("amounts", "売上と内訳", InputKind::Lines(AMOUNTS)).fixed_rows(),
    Field::new("counts", "人数・件数", InputKind::Lines(COUNTS)).fixed_rows(),
    Field::new("note", "日報のメモ", InputKind::Multiline).empty(Empty::Null),
    Field::new("proxySourceNote", "代理入力の元資料", InputKind::Multiline).empty(Empty::Null),
    Field::new(
        "attachments",
        "締めレポート・資料",
        InputKind::Attachments("sales"),
    ),
];
const REASON: &[Field] = &[Field::new("reason", "理由", InputKind::Multiline).empty(Empty::Null)];
const REQUIRED_REASON: &[Field] = &[Field::new("reason", "理由", InputKind::Multiline).required()];

fn text(value: &Value, key: &str) -> String {
    model::input_text(model::value_at(value, key))
}
fn report(row: &Value) -> &Value {
    &row["_report"]
}
fn editable(row: &Value) -> bool {
    report(row).is_null() || matches!(report(row)["status"].as_str(), Some("draft" | "returned"))
}
fn submitted(row: &Value) -> bool {
    report(row)["status"].as_str() == Some("submitted")
}
fn confirmed(row: &Value) -> bool {
    report(row)["status"].as_str() == Some("confirmed")
}
fn failed_adoption(row: &Value) -> bool {
    confirmed(row)
        && report(row)["revisions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|r| {
                r["revisionNo"] == report(row)["confirmedRevisionNo"]
                    && r["adoption"]["status"].as_str() == Some("failed")
            })
}

fn item_key(item: &Value) -> String {
    format!(
        "{}:{}:{}",
        text(item, "measureKey"),
        text(item, "category"),
        text(item, "taxRateBp")
    )
}
fn is_count(key: &str) -> bool {
    matches!(key, "transaction_count" | "customer_count" | "group_count")
}
fn direct_measure(key: &str) -> Option<&'static str> {
    match key {
        "gross_sales" => Some("grossSalesMinor"),
        "discount" => Some("discountMinor"),
        "refund" => Some("refundMinor"),
        "transaction_count" => Some("transactionCount"),
        "customer_count" => Some("customerCount"),
        "group_count" => Some("groupCount"),
        _ => None,
    }
}

fn normalize_entry(value: &Value, filters: &Value) -> Result<Value, String> {
    let template = &value["template"];
    let items = template["items"]
        .as_array()
        .ok_or("日報の入力項目を読み取れませんでした")?;
    let currency = template["currency"]
        .as_str()
        .filter(|s| s.len() == 3)
        .ok_or("日報の通貨を確認できません")?;
    let detail = &value["report"];
    let report = &detail["report"];
    let latest = report["revisions"]
        .as_array()
        .and_then(|r| r.last())
        .unwrap_or(&Value::Null);
    let content = if report["draft"].is_object() {
        &report["draft"]
    } else {
        &latest["content"]
    };
    let measures = &content["measures"];
    let mut amounts = Vec::new();
    let mut counts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in items {
        let key = item_key(item);
        if !seen.insert(key.clone()) {
            return Err("日報の入力項目が重複しています".into());
        }
        let measure = item["measureKey"]
            .as_str()
            .ok_or("日報の入力項目を確認できません")?;
        let value = if let Some(key) = direct_measure(measure) {
            measures[key].clone()
        } else if measure == "payment" {
            measures["payments"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|p| p["paymentCategory"] == item["category"])
                .map(|p| p["amountMinor"].clone())
                .unwrap_or(Value::Null)
        } else if measure == "net_sales_by_tax_rate" {
            measures["taxLines"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|p| {
                    p["taxCategory"] == item["category"] && p["taxRateBp"] == item["taxRateBp"]
                })
                .map(|p| {
                    let amount = p["netAmountMinor"].as_i64().and_then(|n| {
                        p["taxAmountMinor"]
                            .as_i64()
                            .and_then(|tax| n.checked_add(tax))
                    });
                    amount.map_or(Value::Null, |v| json!(v))
                })
                .unwrap_or(Value::Null)
        } else {
            return Err(format!(
                "この日報の入力項目（{}）にはまだ対応していません",
                text(item, "label")
            ));
        };
        let label = format!(
            "{}{}",
            text(item, "label"),
            if item["required"].as_bool() == Some(true) {
                "（必須）"
            } else {
                ""
            }
        );
        let row = json!({"key":key,"label":label,"amount":value,"hint":match measure { "gross_sales"=>"値引・返品前の税込金額", "net_sales_by_tax_rate"=>"税率別の税込売上（値引・返品後）", "payment"=>"値引・返品後に受け取った金額", _=>"" }});
        if is_count(measure) {
            counts.push(row);
        } else {
            amounts.push(row);
        }
    }
    Ok(json!({
        "id": report["id"].as_str().unwrap_or("new"),
        "name":template["name"],"siteId":filters["siteId"],"businessDate":filters["businessDate"],
        "currency":currency,"templateKey":template["key"],"status":report["status"].as_str().unwrap_or("未入力"),
        "amounts":amounts,"counts":counts,"attachments":content["attachments"].as_array().cloned().unwrap_or_default(),
        "note":content["note"],"proxySourceNote":report["proxySourceNote"],
        "plannedStatus":value["plannedStatus"],"needsRecheck":detail["needsRecheck"],
        "salesActual":detail["salesActual"],"issues":detail["issues"],"adoption":latest["adoption"],
        "_template":template,"_report":report,"_submittedBy":latest["submittedBy"]
    }))
}

fn load(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let path = model::query_path("/v1/erp/work-reports/entry", FILTERS, filters)?;
    let response = api::fetch_record(token, tenant, &path)?;
    Ok(json!({"entries":[normalize_entry(&response, filters)?]}))
}

fn amount(row: &Value, key: &str) -> Result<Option<i64>, String> {
    let v = &row[key];
    if v.is_null() {
        return Ok(None);
    }
    let v = v
        .as_i64()
        .filter(|v| *v >= 0)
        .ok_or("金額と人数は0以上の整数で入力してください")?;
    Ok(Some(v))
}

fn save(body: &mut Value, original: &Value, submit: bool) -> Result<(), String> {
    if !editable(original) {
        return Err("提出済みの日報は、取下げまたは訂正開始の後に変更できます".into());
    }
    let template = &original["_template"];
    let items = template["items"]
        .as_array()
        .ok_or("日報の項目を読み直してください")?;
    let mut measures = json!({"currency":original["currency"],"grossSalesMinor":0,"discountMinor":0,"refundMinor":0,"transactionCount":null,"customerCount":null,"groupCount":null,"payments":[],"taxLines":[]});
    for item in items {
        let measure = text(item, "measureKey");
        let bucket = if is_count(&measure) {
            "counts"
        } else {
            "amounts"
        };
        let key = item_key(item);
        let row = body[bucket]
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| row["key"].as_str() == Some(key.as_str()))
            .ok_or("日報の項目が変わっています。読み直してください")?;
        let value = amount(row, "amount")?;
        if submit && value.is_none() && item["required"].as_bool() == Some(true) {
            return Err(format!("{}を入力してください", text(item, "label")));
        }
        if let Some(key) = direct_measure(&measure) {
            measures[key] = value.map_or_else(
                || {
                    if is_count(&measure) {
                        Value::Null
                    } else {
                        json!(0)
                    }
                },
                |v| json!(v),
            );
        } else if let Some(value) = value {
            let category = item["category"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("日報の内訳区分を確認できません")?;
            if measure == "payment" {
                measures["payments"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"paymentCategory":category,"amountMinor":value}));
            } else if measure == "net_sales_by_tax_rate" {
                let rate = item["taxRateBp"].as_i64().unwrap_or(0);
                if !(0..=100_000).contains(&rate) {
                    return Err("日報の税率を確認できません".into());
                }
                let denominator = 10_000_i128 + rate as i128;
                let net = ((value as i128 * 10_000 + denominator / 2) / denominator) as i64;
                measures["taxLines"].as_array_mut().unwrap().push(json!({"taxCategory":category,"taxRateBp":rate,"netAmountMinor":net,"taxAmountMinor":value-net}));
            }
        }
    }
    let net = measures["grossSalesMinor"].as_i64().unwrap() as i128
        - measures["discountMinor"].as_i64().unwrap() as i128
        - measures["refundMinor"].as_i64().unwrap() as i128;
    if net < 0 {
        return Err("値引と返品の合計が売上総額を超えています".into());
    }
    if submit {
        for (key, label) in [("payments", "決済内訳"), ("taxLines", "税率別の売上")] {
            let rows = measures[key].as_array().unwrap();
            let total: i128 = rows
                .iter()
                .map(|row| {
                    if key == "payments" {
                        row["amountMinor"].as_i64().unwrap_or(0) as i128
                    } else {
                        row["netAmountMinor"].as_i64().unwrap_or(0) as i128
                            + row["taxAmountMinor"].as_i64().unwrap_or(0) as i128
                    }
                })
                .sum();
            if !rows.is_empty() && total != net {
                return Err(format!("{label}の合計が純売上と一致しません"));
            }
        }
        if template["attachmentsRequired"].as_bool() == Some(true)
            && body["attachments"].as_array().is_none_or(Vec::is_empty)
        {
            return Err("締めレポートなどの資料を添付してください".into());
        }
    }
    let mut result = json!({"siteId":original["siteId"],"businessDate":original["businessDate"],"templateKey":original["templateKey"],"measures":measures,"note":body["note"],"proxySourceNote":body["proxySourceNote"],"attachments":body["attachments"],"submit":submit});
    if !report(original).is_null() {
        result["expectedVersion"] = json!(
            report(original)["version"]
                .as_u64()
                .ok_or("更新前の版を確認できません")?
        );
    }
    *body = result;
    Ok(())
}
fn save_draft(body: &mut Value, original: &Value) -> Result<(), String> {
    save(body, original, false)
}
fn submit_report(body: &mut Value, original: &Value) -> Result<(), String> {
    save(body, original, true)
}
fn version(body: &mut Value, original: &Value) -> Result<(), String> {
    body["expectedVersion"] = json!(
        report(original)["version"]
            .as_u64()
            .ok_or("日報の版を読み直してください")?
    );
    Ok(())
}
fn confirm(body: &mut Value, original: &Value) -> Result<(), String> {
    if original["salesActual"]["locked"].as_bool() == Some(true)
        && text(body, "reason").trim().is_empty()
    {
        return Err("締め済みの営業日のため、訂正の理由を入力してください".into());
    }
    version(body, original)
}

pub const RESOURCES: &[Resource] = &[
    Resource::new("work-report-editor","ホーム","日報入力・提出","/v1/erp/work-reports/entry","entries")
        .load(load).filters(FILTERS).title(&["name"]).subtitle(&["businessDate"]).status("status")
        .fields(&[("営業日","businessDate"),("営業予定","plannedStatus"),("状態","status"),("版番号","_report.version"),("通貨","currency"),("日報のメモ","note"),("代理入力の元資料","proxySourceNote"),("再確認が必要","needsRecheck"),("確認事項","issues"),("売上への採用","adoption.status"),("採用の結果","adoption.message"),("営業実績の売上","salesActual.netSalesMinor")])
        .read_lines(&[Field::new("amounts","売上と内訳",InputKind::Lines(AMOUNTS)), Field::new("counts","人数・件数",InputKind::Lines(COUNTS)),Field::new("_report.revisions","提出・確認の履歴",InputKind::Lines(&[
            Field::new("revisionNo","提出した版",InputKind::Integer),Field::text("submittedAt","提出日時"),Field::text("decision","確認の結果"),Field::text("decisionReason","判断の理由"),Field::text("correctionReason","訂正の理由"),Field::new("content.measures.grossSalesMinor","売上総額",InputKind::Money),Field::new("content.measures.discountMinor","値引",InputKind::Money),Field::new("content.measures.refundMinor","返品",InputKind::Money),Field::new("content.measures.customerCount","客数",InputKind::Integer),Field::text("adoption.status","実績への採用"),Field::text("adoption.message","採用結果")
        ]))])
        .edit(Mutation::new("下書きを保存","POST","/v1/erp/work-reports",FIELDS).prepare(save_draft).when(editable))
        .actions(&[
            Mutation::new("日報を提出","POST","/v1/erp/work-reports",FIELDS).prepare(submit_report).when(editable).explain("金額と内訳を確認し、この版を提出します。確認後、拠点の規則に従って営業実績へ採用されます。"),
            Mutation::new("提出を取り下げて直す","POST","/v1/erp/work-reports/{id}/withdraw",REASON).prepare(version).when(submitted).actor_match("_submittedBy").destructive(),
            Mutation::new("確認する","POST","/v1/erp/work-reports/{id}/confirm",REASON).prepare(confirm).when(submitted).actor_exclude("_submittedBy").explain("提出された版を確認し、拠点の採用規則に従って営業実績へ反映します。"),
            Mutation::new("差し戻す","POST","/v1/erp/work-reports/{id}/return",REQUIRED_REASON).prepare(version).when(submitted).actor_exclude("_submittedBy").destructive(),
            Mutation::new("訂正を始める","POST","/v1/erp/work-reports/{id}/reopen",REQUIRED_REASON).prepare(version).when(confirmed),
            Mutation::new("営業実績への採用を再試行","POST","/v1/erp/work-reports/{id}/adopt",REASON).when(failed_adoption),
        ]).route("work-reports"),
];

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(currency: &str) -> Value {
        normalize_entry(&json!({"template":{"key":"daily","name":"日報","currency":currency,"attachmentsRequired":false,"items":[
            {"measureKey":"gross_sales","label":"総売上","required":true},
            {"measureKey":"payment","category":"cash","label":"現金","required":true},
            {"measureKey":"net_sales_by_tax_rate","category":"standard","taxRateBp":1000,"label":"10%売上","required":true},
            {"measureKey":"customer_count","label":"客数","required":false}
        ]},"report":null}),&json!({"siteId":"site","businessDate":"2026-11-19"})).unwrap()
    }
    fn filled(original: &Value) -> Value {
        let mut body = model::draft(FIELDS, original);
        for row in body["amounts"].as_array_mut().unwrap() {
            row["amount"] = json!("110.00");
        }
        body
    }
    #[test]
    fn daily_reports_follow_template_and_keep_exact_money_and_unknown_counts() {
        let original = entry("USD");
        let mut draft = filled(&original);
        draft["counts"][0]["amount"] = json!("");
        let operation = RESOURCES[0].actions[0];
        let body = model::request_body(operation, &draft, &original, "").unwrap();
        assert_eq!(body["measures"]["grossSalesMinor"], 11000);
        assert_eq!(body["measures"]["taxLines"][0]["netAmountMinor"], 10000);
        assert!(body["measures"]["customerCount"].is_null());
        assert!(body.get("expectedVersion").is_none());
        assert!(body["measures"].get("netSalesMinor").is_none());
        assert!(body.get("amounts").is_none());
    }
    #[test]
    fn submitting_requires_matching_totals_required_values_and_attachments() {
        let mut original = entry("USD");
        let mut draft = filled(&original);
        draft["amounts"][1]["amount"] = json!("109");
        assert!(
            model::request_body(RESOURCES[0].actions[0], &draft, &original, "")
                .unwrap_err()
                .contains("一致")
        );
        draft["amounts"][1]["amount"] = json!("");
        assert!(model::request_body(RESOURCES[0].actions[0], &draft, &original, "").is_err());
        original["_template"]["attachmentsRequired"] = json!(true);
        assert!(
            model::request_body(RESOURCES[0].actions[0], &filled(&original), &original, "")
                .unwrap_err()
                .contains("添付")
        );
    }
    #[test]
    fn existing_report_versions_and_fixed_template_identity_are_preserved() {
        let mut original = entry("USD");
        original["_report"] = json!({"id":"r","version":7,"status":"returned"});
        let mut draft = filled(&original);
        let body = model::request_body(RESOURCES[0].edit.unwrap(), &draft, &original, "").unwrap();
        assert_eq!(body["expectedVersion"], 7);
        draft["amounts"].as_array_mut().unwrap().remove(0);
        assert!(model::request_body(RESOURCES[0].edit.unwrap(), &draft, &original, "").is_err());
        original["_report"]["status"] = json!("submitted");
        assert!(!editable(&original));
    }
}
