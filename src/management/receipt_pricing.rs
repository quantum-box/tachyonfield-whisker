//! Turn reviewed purchase receipt lines into exact ingredient price observations.

use super::schema::*;
use crate::api::{self, Tenant};
use serde_json::{Value, json};
use std::{collections::HashSet, time::Duration};

const INGREDIENTS: Lookup = Lookup {
    path: "/v1/field/inventory/recipe-cost/ingredients",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const PACKAGES: Lookup = Lookup {
    path: "/v1/field/inventory/recipe-cost/package-specs",
    list_key: "items",
    value_key: "id",
    label_keys: &["productName"],
};
const TAX: &[Choice] = &[Choice::new("800", "軽減8%"), Choice::new("1000", "10%")];
const PRICE_LINES: &[Field] = &[
    Field::new("index", "", InputKind::Hidden),
    Field::new("name", "伝票の品名", InputKind::ReadOnly),
    Field::new("amount", "読み取った金額（円）", InputKind::ReadOnly),
    Field::new("recorded", "登録済み単価（円）", InputKind::ReadOnly).read("price.unitCost"),
    Field::new("include", "この明細を単価に記録", InputKind::Bool),
    Field::new("ingredientId", "材料", InputKind::Reference(INGREDIENTS)).read("hint.ingredientId"),
    Field::new("packageSpecId", "包装規格", InputKind::Reference(PACKAGES))
        .read("hint.packageSpecId")
        .empty(Empty::Null)
        .help("材料に対応する包装規格を選びます。直接の量を入力する場合は空欄"),
    Field::new("packCount", "パック数", InputKind::Decimal)
        .read("quantity")
        .default("1")
        .empty(Empty::Null),
    Field::new(
        "quantity",
        "直接入力する量（材料の基準単位）",
        InputKind::Decimal,
    )
    .read("directQuantity")
    .empty(Empty::Null),
    Field::new("taxRateBp", "税率", InputKind::Select(TAX)).read("hint.taxRateBp"),
    Field::new(
        "amountOverride",
        "訂正する税込金額（円）",
        InputKind::Decimal,
    )
    .empty(Empty::Null)
    .help("読み取った金額が正しければ空欄のままにします"),
];
const PRICE_FIELDS: &[Field] = &[
    Field::new("purchasedOn", "確認した仕入日", InputKind::Date).required(),
    Field::new("lines", "伝票明細", InputKind::Lines(PRICE_LINES)).fixed_rows(),
];

pub const RESOURCES: &[Resource] = &[
    Resource::new("receipt-pricing", "商品・在庫管理", "仕入伝票から単価を記録", "/v1/field/receipts?receipt_type=purchase&status=done", "items")
        .route("erp/recipe-cost/receipts")
        .title(&["extractedData.counterparty", "fileName"])
        .subtitle(&["extractedData.date", "createdAt"])
        .detail("/v1/field/inventory/recipe-cost/receipts/{id}/lines", "")
        .fields(&[("仕入先", "supplierName"), ("仕入日", "purchasedOn")])
        .read_lines(&[Field::new("lines", "読み取った明細", InputKind::Lines(&[
            Field::text("name", "品名"),
            Field::new("amount", "税込金額（円）", InputKind::Decimal),
            Field::new("quantity", "数量", InputKind::Decimal),
            Field::new("hint.ingredientId", "前回の材料", InputKind::Reference(INGREDIENTS)),
            Field::new("price.unitCost", "登録済み単価（円）", InputKind::Decimal),
        ]))])
        .actions(&[Mutation::new("選択した明細の単価を記録", "POST", "/v1/field/inventory/recipe-cost/receipts/{id}/prices", PRICE_FIELDS)
            .capability("inventory.manage").prepare(prepare_prices).execute(record_prices)
            .when(|record| record["lines"].as_array().is_some_and(|lines| !lines.is_empty()))
            .explain("仕入日・材料・税率・量を確認し、記録する明細を選択してください。税込金額から税抜単価を記録します。登録済みの明細は再登録できません。")]),
];

fn positive(value: &Value, label: &str) -> Result<(), String> {
    let text = value
        .as_str()
        .ok_or_else(|| format!("{label}を入力してください"))?;
    if text.starts_with('-') || !text.bytes().any(|c| matches!(c, b'1'..=b'9')) {
        return Err(format!("{label}は0より大きい数を入力してください"));
    }
    Ok(())
}

fn prepare_prices(body: &mut Value, original: &Value) -> Result<(), String> {
    let rows = body["lines"]
        .as_array()
        .ok_or("伝票明細を読み直してください")?;
    let mut selected = Vec::new();
    for row in rows.iter().filter(|row| row["include"] == true) {
        let mut row = row.clone();
        row.as_object_mut()
            .ok_or("明細の形式が不正です")?
            .remove("include");
        row["taxRateBp"] = match row["taxRateBp"].as_str() {
            Some("800") => json!(800),
            Some("1000") => json!(1000),
            _ => return Err("記録する明細の税率を選んでください".into()),
        };
        row["amountIncludesTax"] = json!(true);
        if row["packageSpecId"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
        {
            positive(&row["packCount"], "パック数")?;
            row["quantity"] = Value::Null;
        } else {
            positive(&row["quantity"], "仕入れた量")?;
            row["packCount"] = Value::Null;
        }
        selected.push(row);
    }
    if selected.is_empty() {
        return Err("記録する明細を選んでください".into());
    }
    body["lines"] = json!(selected);
    validate_lines(body, original)
}

fn validate_lines(body: &Value, original: &Value) -> Result<(), String> {
    let prior = original["lines"]
        .as_array()
        .ok_or("伝票明細を確認できません。読み直してください")?;
    let rows = body["lines"].as_array().ok_or("記録する明細がありません")?;
    let mut seen = HashSet::new();
    for row in rows {
        let index = row["index"]
            .as_u64()
            .ok_or("伝票明細の位置を確認できません")?;
        if !seen.insert(index) {
            return Err("同じ伝票明細が重複しています".into());
        }
        let source = prior
            .iter()
            .find(|line| line["index"].as_u64() == Some(index))
            .ok_or("伝票の明細が変わっています。読み直してください")?;
        if !source["price"].is_null() {
            return Err("単価を記録済みの明細があります。読み直してください".into());
        }
        if !row["ingredientId"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
        {
            return Err("記録する明細の材料を選んでください".into());
        }
        if !row["amountOverride"].is_null() {
            positive(&row["amountOverride"], "訂正金額")?;
        } else if source["amount"].is_null() {
            return Err("金額が読めていない明細の税込金額を入力してください".into());
        }
    }
    Ok(())
}

fn record_prices(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let id = original["receiptId"]
        .as_str()
        .or_else(|| original["id"].as_str())
        .ok_or("仕入伝票を読み直してください")?;
    let path = format!(
        "/v1/field/inventory/recipe-cost/receipts/{}",
        api::encode_query(id)
    );
    let fresh = api::fetch_record(token, tenant, &format!("{path}/lines"))?;
    ensure_receipt_review_unchanged(body, original, &fresh)?;
    validate_lines(body, &fresh)?;
    let packages = if body["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|line| line["packageSpecId"].as_str().is_some())
    {
        Some(api::fetch_record(token, tenant, PACKAGES.path)?)
    } else {
        None
    };
    for row in body["lines"].as_array().into_iter().flatten() {
        if let Some(package_id) = row["packageSpecId"].as_str() {
            let package = packages
                .as_ref()
                .and_then(|data| data["items"].as_array())
                .and_then(|rows| rows.iter().find(|item| item["id"] == package_id))
                .ok_or("選んだ包装規格を確認できません")?;
            if package["ingredientId"] != row["ingredientId"] || !package["retiredAt"].is_null() {
                return Err("材料に対応する有効な包装規格を選び直してください".into());
            }
        }
    }
    api::send_json(
        token,
        tenant,
        "POST",
        &format!("{path}/prices"),
        body,
        Duration::from_secs(30),
    )
}

fn ensure_receipt_review_unchanged(
    body: &Value,
    original: &Value,
    fresh: &Value,
) -> Result<(), String> {
    let changed =
        "伝票の読み取り内容が変わっています。品名・金額・数量を読み直してから確認してください";
    for key in ["receiptId", "supplierName", "purchasedOn"] {
        if original[key] != fresh[key] {
            return Err(changed.into());
        }
    }
    let prior = original["lines"].as_array().ok_or(changed)?;
    let current = fresh["lines"].as_array().ok_or(changed)?;
    for selected in body["lines"].as_array().ok_or(changed)? {
        let before = prior
            .iter()
            .find(|line| line["index"] == selected["index"])
            .ok_or(changed)?;
        let after = current
            .iter()
            .find(|line| line["index"] == selected["index"])
            .ok_or(changed)?;
        for key in ["name", "amount", "quantity"] {
            if before[key] != after[key] {
                return Err(changed.into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::management::model;
    #[test]
    fn reprocessed_receipt_cannot_attach_the_old_material_to_a_changed_line() {
        let original = json!({"receiptId":"receipt_a","supplierName":"仕入先","purchasedOn":"2026-10-01","lines":[{"index":4,"name":"豆","amount":1200,"quantity":2,"price":null}]});
        let body =
            json!({"lines":[{"index":4,"ingredientId":"ingredient_beans","amountOverride":null}]});
        assert!(ensure_receipt_review_unchanged(&body, &original, &original).is_ok());
        for (key, replacement) in [
            ("name", json!("肉")),
            ("amount", json!(9000)),
            ("quantity", json!(6)),
        ] {
            let mut fresh = original.clone();
            fresh["lines"][0][key] = replacement;
            assert!(ensure_receipt_review_unchanged(&body, &original, &fresh).is_err());
        }
    }
    #[test]
    fn receipt_prices_keep_source_identity_and_select_only_reviewed_lines() {
        let original = json!({"purchasedOn":"2026-10-01", "lines":[
            {"index":4,"name":"豆","amount":1200,"quantity":2,"hint":{"ingredientId":"ing_a","taxRateBp":800},"price":null},
            {"index":9,"name":"未確認","amount":null,"price":null}
        ]});
        let operation = RESOURCES[0].actions[0];
        let mut draft = model::draft(operation.fields, &original);
        draft["lines"][0]["include"] = json!(true);
        draft["lines"][0]["quantity"] = json!("0.00000000000000001");
        let body = model::request_body(operation, &draft, &original, "").unwrap();
        assert_eq!(body["lines"].as_array().unwrap().len(), 1);
        assert_eq!(body["lines"][0]["index"], 4);
        assert_eq!(body["lines"][0]["quantity"], "0.00000000000000001");
        assert_eq!(body["lines"][0]["taxRateBp"], 800);
        assert_eq!(body["lines"][0]["amountIncludesTax"], true);
        assert!(body["lines"][0].get("name").is_none());
        assert!(body["lines"][0].get("include").is_none());
        draft["lines"][0]["index"] = json!(999);
        assert!(model::request_body(operation, &draft, &original, "").is_err());
    }
    #[test]
    fn receipt_prices_do_not_guess_tax_or_duplicate_already_recorded_prices() {
        let original = json!({"purchasedOn":"2026-10-01", "lines":[{"index":0,"name":"豆","amount":1200,"price":{}}]});
        let operation = RESOURCES[0].actions[0];
        let mut draft = model::draft(operation.fields, &original);
        draft["lines"][0]["include"] = json!(true);
        draft["lines"][0]["ingredientId"] = json!("ing_a");
        draft["lines"][0]["quantity"] = json!("100");
        assert!(
            model::request_body(operation, &draft, &original, "")
                .unwrap_err()
                .contains("税率")
        );
        draft["lines"][0]["taxRateBp"] = json!("800");
        assert!(
            model::request_body(operation, &draft, &original, "")
                .unwrap_err()
                .contains("記録済み")
        );
    }
}
