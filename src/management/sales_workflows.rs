//! Native sales documents and their explicit lifecycle commands.
//!
//! The server owns status transitions and authorization. Where its REST contract
//! has no expected-version argument, a fresh comparison catches changes made
//! while the form was open; it is not an atomic compare-and-swap.

use super::super::schema::*;
use super::{CLIENTS, status_is};
use crate::api::{self, Tenant};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const UNASSIGNED_CLIENT: &str = "fieldv2-unassigned-client";
const TEMPLATE_MARKER: &str = "__field_quotation_template:";
const DOCUMENT_MARKER: &str = "__field_quotation_document:";
const PRODUCTS: Lookup = Lookup {
    path: "/v1/order/products?limit=100",
    list_key: "items",
    value_key: "id",
    label_keys: &["name", "skuCode"],
};
const PRODUCT_SKUS: Lookup = Lookup {
    path: "/v1/order/products?limit=100",
    list_key: "items",
    value_key: "skuCode",
    label_keys: &["name", "skuCode"],
};

pub(super) const QUOTE_LINES: &[Field] = &[
    Field::new("productId", "商品（任意）", InputKind::Reference(PRODUCTS)).empty(Empty::Null),
    Field::text("description", "品名・内容").required(),
    Field::new("quantity", "数量", InputKind::Integer)
        .required()
        .default("1"),
    Field::text("unit", "単位")
        .default("式")
        .empty(Empty::String),
    Field::new("unitPrice", "単価（税抜）", InputKind::Money)
        .required()
        .default("0"),
    Field::new("discountAmount", "明細の値引額", InputKind::Money)
        .required()
        .default("0"),
    Field::new("taxRate", "税率（%）", InputKind::Integer)
        .required()
        .default("10"),
];

pub(super) const QUOTE_FIELDS: &[Field] = &[
    Field::text("quotationNumber", "見積番号・件名").required(),
    Field::new("clientId", "取引先", InputKind::Reference(CLIENTS))
        .empty(Empty::String)
        .help("下書きは取引先未設定でも保存できます。発行前に選択してください。"),
    Field::text("clientName", "帳票の宛名")
        .required()
        .help("取引先を変更した場合は、帳票の宛名・住所も確認してください。"),
    Field::new("clientEmail", "宛先メールアドレス", InputKind::Email).empty(Empty::String),
    Field::text("recipient.department", "宛先部署").empty(Empty::String),
    Field::text("recipient.contactName", "宛先担当者").empty(Empty::String),
    Field::text("recipient.postalCode", "宛先郵便番号").empty(Empty::String),
    Field::new("recipient.address", "宛先住所", InputKind::Multiline).empty(Empty::String),
    Field::text("recipient.phone", "宛先電話番号").empty(Empty::String),
    Field::new("currency", "通貨", InputKind::ReadOnly).default("JPY"),
    Field::new("items", "見積明細", InputKind::Lines(QUOTE_LINES))
        .required()
        .help("同じ税率の値引後金額を合算し、税率ごとに端数を切り捨てて税額を計算します。"),
    Field::new("validUntil", "有効期限", InputKind::Date)
        .help("新規作成時に空欄なら、サーバーの日付から30日後を使います。"),
    Field::text("issuer.name", "発行元名").empty(Empty::String),
    Field::text("issuer.representative", "発行元担当者").empty(Empty::String),
    Field::new("issuer.address", "発行元住所", InputKind::Multiline).empty(Empty::String),
    Field::text("issuer.phone", "発行元電話番号").empty(Empty::String),
    Field::new("issuer.email", "発行元メールアドレス", InputKind::Email).empty(Empty::String),
    Field::text("issuer.registrationNumber", "登録番号").empty(Empty::String),
    Field::new("notes", "備考", InputKind::Multiline).empty(Empty::String),
];

pub(super) const ORDER_LINES: &[Field] = &[
    Field::new(
        "sku",
        "在庫商品（任意）",
        InputKind::Reference(PRODUCT_SKUS),
    )
    .empty(Empty::Null)
    .help("出荷時の在庫引当対象です。在庫管理しない役務などは未設定にします。"),
    Field::text("description", "品名・内容").required(),
    Field::new("quantity", "数量", InputKind::Integer)
        .required()
        .default("1"),
    Field::new("unitPrice", "単価（税抜）", InputKind::Money)
        .required()
        .default("0"),
];

pub(super) const ORDER_CREATE_FIELDS: &[Field] = &[
    Field::text("orderNumber", "受注番号").help("空欄ならサーバーで自動採番します。"),
    Field::new("clientId", "取引先", InputKind::Reference(CLIENTS)).required(),
    Field::text("clientName", "取引先の表示名").required(),
    Field::new("clientEmail", "メールアドレス", InputKind::Email).empty(Empty::Null),
    Field::new("currency", "通貨", InputKind::ReadOnly).default("JPY"),
    Field::new("items", "受注明細", InputKind::Lines(ORDER_LINES)).required(),
    Field::new("taxAmount", "税額合計", InputKind::Money)
        .required()
        .default("0")
        .help("明細の税抜合計に加算する税額を入力してください。"),
    Field::new("notes", "備考", InputKind::Multiline).empty(Empty::Null),
];

const QUOTE_SNAPSHOT: &[&str] = &[
    "id",
    "updatedAt",
    "status",
    "quotationNumber",
    "clientId",
    "clientName",
    "clientEmail",
    "items",
    "validUntil",
    "currency",
    "notes",
    "subtotalAmount",
    "discountAmount",
    "taxAmount",
    "totalAmount",
    "paymentLinkUrl",
    "convertedInvoiceId",
];
const ORDER_SNAPSHOT: &[&str] = &[
    "id",
    "updatedAt",
    "status",
    "orderNumber",
    "clientId",
    "clientName",
    "clientEmail",
    "items",
    "currency",
    "notes",
    "source",
    "subtotalAmount",
    "taxAmount",
    "totalAmount",
    "inventoryDecrementedAt",
    "convertedInvoiceId",
    "squareOrderId",
    "squarePaymentId",
];
const CONTRACT_BODY: &[&str] = &[
    "contractNumber",
    "title",
    "clientId",
    "clientName",
    "monthlyAmountYen",
    "lineItems",
    "billingCycle",
    "startDate",
    "endDate",
    "status",
    "terms",
    "notes",
    "templateId",
    "sourceId",
    "sourceType",
];

fn nonblank<'a>(record: &'a Value, key: &str) -> Option<&'a str> {
    record[key]
        .as_str()
        .filter(|value| !value.trim().is_empty())
}

fn text_limit(
    record: &Value,
    key: &str,
    label: &str,
    max: usize,
    required: bool,
) -> Result<(), String> {
    let value = record[key].as_str().unwrap_or_default();
    if (required && value.trim().is_empty()) || value.chars().count() > max {
        return Err(format!(
            "{label}は{}文字以内で入力してください{}",
            max,
            if required { "（必須）" } else { "" }
        ));
    }
    Ok(())
}

fn amount(record: &Value, key: &str, label: &str) -> Result<i64, String> {
    record[key]
        .as_i64()
        .filter(|value| *value >= 0)
        .ok_or_else(|| format!("{label}は0以上の金額で入力してください。"))
}

fn checked_document_id<'a>(original: &'a Value, prefix: &str) -> Result<&'a str, String> {
    nonblank(original, "id")
        .filter(|id| id.starts_with(prefix) && id.len() > prefix.len())
        .ok_or_else(|| "対象の書類を確認できません。一覧を再読み込みしてください。".into())
}

fn same_snapshot(
    original: &Value,
    fresh: &Value,
    fields: &[&str],
    revision: &str,
) -> Result<(), String> {
    if nonblank(original, revision).is_none() {
        return Err("書類の版を確認できません。詳細を再読み込みしてください。".into());
    }
    if fields.iter().any(|key| original[*key] != fresh[*key]) {
        return Err(
            "編集中に書類が変更されています。詳細を再読み込みして確認してください。（409）".into(),
        );
    }
    Ok(())
}

fn fresh_document(
    token: &str,
    tenant: &Tenant,
    original: &Value,
    collection: &str,
    id_prefix: &str,
    fields: &[&str],
) -> Result<(String, Value), String> {
    let id = checked_document_id(original, id_prefix)?;
    let path = format!("{collection}/{}", api::encode_query(id));
    let fresh = api::fetch_record(token, tenant, &path)?;
    same_snapshot(original, &fresh, fields, "updatedAt")?;
    Ok((path, fresh))
}

/// Percent decoding is limited to the document metadata value; a literal '+'
/// remains '+', matching the document format rather than form URL encoding.
fn decode_metadata(encoded: &str) -> Result<String, String> {
    let mut bytes = Vec::with_capacity(encoded.len());
    let input = encoded.as_bytes();
    let mut cursor = 0;
    while cursor < input.len() {
        if input[cursor] == b'%' {
            if cursor + 2 >= input.len() {
                return Err("帳票情報のエンコードを読み取れません。".into());
            }
            let high = (input[cursor + 1] as char).to_digit(16);
            let low = (input[cursor + 2] as char).to_digit(16);
            bytes.push(match (high, low) {
                (Some(high), Some(low)) => (high * 16 + low) as u8,
                _ => return Err("帳票情報のエンコードを読み取れません。".into()),
            });
            cursor += 3;
        } else {
            bytes.push(input[cursor]);
            cursor += 1;
        }
    }
    String::from_utf8(bytes).map_err(|_| "帳票情報の文字を読み取れません。".into())
}

fn quotation_document(original: &Value) -> Result<(String, Value, String), String> {
    let mut template = "builtin-simple".to_owned();
    let mut metadata = json!({});
    let mut visible = Vec::new();
    let mut seen_template = false;
    let mut seen_document = false;
    for line in original["notes"].as_str().unwrap_or_default().lines() {
        if let Some(value) = line.strip_prefix(TEMPLATE_MARKER) {
            if seen_template {
                return Err(
                    "帳票ひな形の情報が重複しています。Webで内容を確認してください。".into(),
                );
            }
            seen_template = true;
            if !value.trim().is_empty() {
                template = value.trim().to_owned();
            }
        } else if let Some(value) = line.strip_prefix(DOCUMENT_MARKER) {
            if seen_document {
                return Err("帳票の情報が重複しています。Webで内容を確認してください。".into());
            }
            seen_document = true;
            metadata = serde_json::from_str(&decode_metadata(value.trim())?)
                .map_err(|_| "帳票情報を読み取れません。Webで内容を確認してください。")?;
            if !metadata.is_object() {
                return Err("帳票情報の形式を読み取れません。".into());
            }
        } else {
            visible.push(line);
        }
    }
    Ok((template, metadata, visible.join("\n").trim().to_owned()))
}

pub(super) fn quote_prefill(original: &Value) -> Result<Value, String> {
    let (_, metadata, notes) = quotation_document(original)?;
    let mut visible = if original.is_object() {
        original.clone()
    } else {
        json!({})
    };
    visible["notes"] = json!(notes);
    visible["issuer"] = metadata
        .get("issuer")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    visible["recipient"] = metadata
        .get("recipient")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    for key in ["name", "email"] {
        if let Some(value) = nonblank(&metadata["recipient"], key) {
            visible[if key == "name" {
                "clientName"
            } else {
                "clientEmail"
            }] = json!(value);
        }
    }
    if visible["clientId"] == UNASSIGNED_CLIENT {
        visible["clientId"] = json!("");
    }
    if let Some(items) = visible["items"].as_array_mut() {
        for (index, item) in items.iter_mut().enumerate() {
            if !item.is_object() {
                return Err("見積明細を読み取れません。".into());
            }
            for key in ["productId", "taxRate", "unit"] {
                if item[key].is_null() {
                    let fallback = &metadata["lineItems"][index][key];
                    item[key] = if !fallback.is_null() {
                        fallback.clone()
                    } else {
                        match key {
                            "taxRate" => json!(10),
                            "unit" => json!("式"),
                            _ => Value::Null,
                        }
                    };
                }
            }
        }
    }
    Ok(visible)
}

pub(super) fn quote_editable(original: &Value) -> bool {
    status_is(original, "draft")
        && original["paymentLinkUrl"].is_null()
        && original["convertedInvoiceId"].is_null()
}
pub(super) fn quote_issuable(original: &Value) -> bool {
    quote_editable(original)
        && nonblank(original, "clientId").is_some_and(|id| id != UNASSIGNED_CLIENT)
}
pub(super) fn quote_cancellable(original: &Value) -> bool {
    (status_is(original, "draft") || status_is(original, "sent"))
        && original["paymentLinkUrl"].is_null()
        && original["convertedInvoiceId"].is_null()
}

fn prepare_quote(body: &mut Value, original: &Value) -> Result<(), String> {
    text_limit(body, "quotationNumber", "見積番号・件名", 255, true)?;
    text_limit(body, "clientName", "帳票の宛名", 255, true)?;
    text_limit(body, "clientEmail", "宛先メール", 255, false)?;
    text_limit(body, "notes", "備考", 4000, false)?;
    if body["notes"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .any(|line| line.starts_with(TEMPLATE_MARKER) || line.starts_with(DOCUMENT_MARKER))
    {
        return Err("帳票の管理情報は備考欄に入力できません。".into());
    }
    if nonblank(body, "clientId").is_none() {
        body["clientId"] = json!(UNASSIGNED_CLIENT);
    }
    let items = body["items"]
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or("見積明細を1件以上追加してください。")?;
    let mut taxable = BTreeMap::<i64, i128>::new();
    let mut gross_total = 0_i128;
    let mut discount_total = 0_i128;
    for item in items {
        text_limit(item, "description", "品名・内容", 255, true)?;
        text_limit(item, "unit", "単位", 32, false)?;
        let quantity = item["quantity"]
            .as_i64()
            .filter(|n| (1..=i32::MAX as i64).contains(n))
            .ok_or("数量は1〜2147483647の整数で入力してください。")?;
        let unit_price = amount(item, "unitPrice", "単価")?;
        let discount = amount(item, "discountAmount", "値引額")?;
        let tax_rate = item["taxRate"]
            .as_i64()
            .filter(|n| (0..=100).contains(n))
            .ok_or("税率は0〜100の整数で入力してください。")?;
        let gross = i128::from(quantity) * i128::from(unit_price);
        if gross > i128::from(i64::MAX) {
            return Err("明細の金額が大きすぎます。".into());
        }
        if i128::from(discount) > gross {
            return Err("値引額は明細小計以下にしてください。".into());
        }
        gross_total += gross;
        discount_total += i128::from(discount);
        if gross_total > i128::from(i64::MAX) {
            return Err("見積の小計が大きすぎます。".into());
        }
        *taxable.entry(tax_rate).or_default() += gross - i128::from(discount);
    }
    let tax: i128 = taxable
        .iter()
        .map(|(rate, value)| value * i128::from(*rate) / 100)
        .sum();
    if gross_total - discount_total + tax > i128::from(i64::MAX) {
        return Err("見積の合計が大きすぎます。".into());
    }
    body["taxAmount"] = json!(i64::try_from(tax).map_err(|_| "税額が大きすぎます。")?);

    let (template, mut metadata, _) = quotation_document(original)?;
    for section in ["issuer", "recipient"] {
        if !metadata[section].is_object() {
            metadata[section] = json!({});
        }
        if let Some(values) = body[section].as_object() {
            for (key, value) in values {
                let limit = if key == "address" {
                    500
                } else if key == "phone" || key == "registrationNumber" {
                    64
                } else if key == "postalCode" {
                    32
                } else {
                    255
                };
                if value.as_str().unwrap_or_default().chars().count() > limit {
                    return Err(format!("発行元・宛先の{key}が長すぎます。"));
                }
                metadata[section][key] = value.clone();
            }
        }
    }
    metadata["recipient"]["name"] = body["clientName"].clone();
    metadata["recipient"]["email"] = body["clientEmail"].clone();
    metadata["lineItems"] = Value::Array(
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| {
                json!({
                    "productId":line["productId"], "taxRate":line["taxRate"], "unit":line["unit"],
                })
            })
            .collect(),
    );
    let visible_notes = body["notes"].as_str().unwrap_or_default().trim();
    let encoded = api::encode_query(
        &serde_json::to_string(&metadata).map_err(|_| "帳票情報を保存できません。")?,
    );
    body["notes"] = json!(format!(
        "{TEMPLATE_MARKER}{template}\n{DOCUMENT_MARKER}{encoded}{}",
        if visible_notes.is_empty() {
            String::new()
        } else {
            format!("\n{visible_notes}")
        }
    ));
    let object = body
        .as_object_mut()
        .ok_or("見積の入力を確認してください。")?;
    object.remove("issuer");
    object.remove("recipient");
    Ok(())
}

pub(super) fn prepare_quote_create(body: &mut Value, original: &Value) -> Result<(), String> {
    prepare_quote(body, original)?;
    body["currency"] = json!("JPY");
    body["status"] = json!("Draft");
    Ok(())
}
pub(super) fn prepare_quote_edit(body: &mut Value, original: &Value) -> Result<(), String> {
    if !quote_editable(original) {
        return Err("決済リンクのない見積下書きだけを編集できます。".into());
    }
    // Currency is inherited for all Money fields and is intentionally not part of PATCH.
    if nonblank(original, "currency").is_none() {
        return Err("見積の通貨を確認できません。".into());
    }
    prepare_quote(body, original)
}
pub(super) fn issue_quote(body: &mut Value, original: &Value) -> Result<(), String> {
    if !quote_issuable(original) {
        return Err("取引先を設定した見積下書きだけを発行できます。".into());
    }
    *body = json!({"status":"Sent"});
    Ok(())
}
pub(super) fn cancel_quote(body: &mut Value, original: &Value) -> Result<(), String> {
    if !quote_cancellable(original) {
        return Err("決済リンク・変換済み請求のある見積は取消できません。".into());
    }
    *body = json!({"status":"Cancelled"});
    Ok(())
}
pub(super) fn quote_patch(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let (path, fresh) = fresh_document(
        token,
        tenant,
        original,
        "/v1/erp/quotations",
        "quo_",
        QUOTE_SNAPSHOT,
    )?;
    if body.get("items").is_some() && !quote_editable(&fresh) {
        return Err("見積は編集可能な下書きではありません。".into());
    }
    api::mutate_record(token, tenant, "PATCH", &path, body, None)
}
pub(super) fn quote_accept(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let (path, fresh) = fresh_document(
        token,
        tenant,
        original,
        "/v1/erp/quotations",
        "quo_",
        QUOTE_SNAPSHOT,
    )?;
    if !status_is(&fresh, "sent") {
        return Err("発行済みの見積から受注を作成してください。".into());
    }
    api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{path}/accept-order"),
        body,
        None,
    )
}
pub(super) fn quote_response(value: &Value) -> Result<(), String> {
    checked_document_id(value, "quo_").map(|_| ())
}
pub(super) fn order_response(value: &Value) -> Result<(), String> {
    checked_document_id(value, "ord_").map(|_| ())
}
pub(super) fn converted_order_response(value: &Value) -> Result<(), String> {
    nonblank(value, "orderId")
        .map(|_| ())
        .ok_or_else(|| "作成した受注を確認できません。一覧で確認してください。".into())
}

pub(super) fn prepare_order_create(body: &mut Value, _: &Value) -> Result<(), String> {
    text_limit(body, "orderNumber", "受注番号", 255, false)?;
    text_limit(body, "clientId", "取引先", 255, true)?;
    text_limit(body, "clientName", "取引先の表示名", 255, true)?;
    text_limit(body, "notes", "備考", 4000, false)?;
    if body["clientId"] == UNASSIGNED_CLIENT {
        return Err("取引先を選択してください。".into());
    }
    let items = body["items"]
        .as_array()
        .filter(|rows| !rows.is_empty())
        .ok_or("受注明細を1件以上追加してください。")?;
    let mut total = i128::from(amount(body, "taxAmount", "税額合計")?);
    for item in items {
        text_limit(item, "description", "品名・内容", 255, true)?;
        let quantity = item["quantity"]
            .as_i64()
            .filter(|n| *n > 0)
            .ok_or("明細の数量は1以上で入力してください。")?;
        total += i128::from(quantity) * i128::from(amount(item, "unitPrice", "単価")?);
        if total > i128::from(i64::MAX) {
            return Err("受注の合計が大きすぎます。".into());
        }
    }
    body["currency"] = json!("JPY");
    body["status"] = json!("pending");
    body["source"] = json!("manual");
    Ok(())
}

fn erp_order(record: &Value) -> bool {
    checked_document_id(record, "ord_").is_ok()
}
pub(super) fn order_pending(record: &Value) -> bool {
    erp_order(record) && status_is(record, "pending")
}
pub(super) fn order_confirmed(record: &Value) -> bool {
    erp_order(record) && status_is(record, "confirmed")
}
pub(super) fn order_shipped(record: &Value) -> bool {
    erp_order(record) && status_is(record, "shipped")
}
pub(super) fn order_cancellable(record: &Value) -> bool {
    (order_pending(record) || order_confirmed(record))
        && record["inventoryDecrementedAt"].is_null()
        && record["convertedInvoiceId"].is_null()
}
pub(super) fn confirm_order(body: &mut Value, _: &Value) -> Result<(), String> {
    *body = json!({"status":"confirmed"});
    Ok(())
}
pub(super) fn ship_order(body: &mut Value, _: &Value) -> Result<(), String> {
    *body = json!({"status":"shipped"});
    Ok(())
}
pub(super) fn complete_order(body: &mut Value, _: &Value) -> Result<(), String> {
    *body = json!({"status":"completed"});
    Ok(())
}
pub(super) fn cancel_order(body: &mut Value, original: &Value) -> Result<(), String> {
    if !order_cancellable(original) {
        return Err("出庫・請求前の未確定または確定受注だけを取消できます。".into());
    }
    *body = json!({"status":"cancelled"});
    Ok(())
}
pub(super) fn order_patch(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let (path, fresh) = fresh_document(
        token,
        tenant,
        original,
        "/v1/erp/orders",
        "ord_",
        ORDER_SNAPSHOT,
    )?;
    if body["status"] == "cancelled" && !order_cancellable(&fresh) {
        return Err("この受注は取消できません。".into());
    }
    api::mutate_record(token, tenant, "PATCH", &path, body, None)
}
pub(super) fn order_invoice(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let (path, fresh) = fresh_document(
        token,
        tenant,
        original,
        "/v1/erp/orders",
        "ord_",
        ORDER_SNAPSHOT,
    )?;
    if !super::order_can_invoice(&fresh) {
        return Err("この受注からは請求書を作成できません。".into());
    }
    api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{path}/convert-to-invoice"),
        body,
        None,
    )
}

pub(super) fn order_recreatable(record: &Value) -> bool {
    erp_order(record)
        && status_is(record, "cancelled")
        && record["source"] == "manual"
        && record["currency"] == "JPY"
        && record["inventoryDecrementedAt"].is_null()
        && record["convertedInvoiceId"].is_null()
        && record["squareOrderId"].is_null()
        && record["squarePaymentId"].is_null()
        && record["items"].as_array().is_some_and(|items| {
            !items.is_empty()
                && items.iter().all(|line| {
                    match (
                        line["quantity"].as_i64(),
                        line["unitPrice"].as_i64(),
                        line["amount"].as_i64(),
                    ) {
                        (Some(quantity), Some(price), Some(amount)) => {
                            quantity.checked_mul(price) == Some(amount)
                        }
                        _ => false,
                    }
                })
        })
}
pub(super) fn order_recreate_prefill(record: &Value) -> Result<Value, String> {
    if !order_recreatable(record) {
        return Err("取消済みの手動受注だけを訂正して新規作成できます。".into());
    }
    let mut draft = record.clone();
    draft["orderNumber"] = json!("");
    Ok(draft)
}
pub(super) fn order_recreate(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let (_, fresh) = fresh_document(
        token,
        tenant,
        original,
        "/v1/erp/orders",
        "ord_",
        ORDER_SNAPSHOT,
    )?;
    if !order_recreatable(&fresh) {
        return Err("この受注は訂正して新規作成できません。".into());
    }
    api::mutate_record(token, tenant, "POST", "/v1/erp/orders", body, None)
}

pub(super) fn standard_contract(record: &Value) -> bool {
    matches!(
        record["sourceType"].as_str(),
        Some("manual" | "quote" | "order" | "deal")
    )
}
pub(super) fn contract_draft(record: &Value) -> bool {
    standard_contract(record) && status_is(record, "draft")
}
pub(super) fn contract_active(record: &Value) -> bool {
    standard_contract(record) && status_is(record, "active")
}

fn contract_transition(body: &mut Value, original: &Value, target: &str) -> Result<(), String> {
    let allowed = match target {
        "active" => contract_draft(original),
        "terminated" => contract_active(original),
        _ => false,
    };
    if !allowed {
        return Err("契約の状態または作成元により、この操作はできません。".into());
    }
    let mut request = json!({});
    for key in CONTRACT_BODY {
        request[*key] = original.get(*key).cloned().unwrap_or(Value::Null);
    }
    request["status"] = json!(target);
    *body = request;
    Ok(())
}
pub(super) fn activate_contract(body: &mut Value, original: &Value) -> Result<(), String> {
    contract_transition(body, original, "active")
}
pub(super) fn terminate_contract(body: &mut Value, original: &Value) -> Result<(), String> {
    contract_transition(body, original, "terminated")
}

fn contract_unchanged(original: &Value, fresh: &Value) -> Result<(), String> {
    let revision = nonblank(original, "aggregateRevision")
        .ok_or("契約の版を確認できません。詳細を再読み込みしてください。")?;
    if revision
        .parse::<u64>()
        .ok()
        .filter(|value| value.to_string() == revision)
        .is_none()
    {
        return Err("契約の版を読み取れません。".into());
    }
    if original["id"] != fresh["id"]
        || original["aggregateRevision"] != fresh["aggregateRevision"]
        || original["templateBodySnapshot"] != fresh["templateBodySnapshot"]
        || CONTRACT_BODY
            .iter()
            .any(|key| original[*key] != fresh[*key])
    {
        return Err("編集中に契約が変更されています。詳細を再読み込みしてください。（409）".into());
    }
    Ok(())
}
fn fresh_contract(
    token: &str,
    tenant: &Tenant,
    original: &Value,
) -> Result<(String, Value), String> {
    let id = checked_document_id(original, "salesct_")?;
    let path = format!("/v1/erp/sales-contracts/{}", api::encode_query(id));
    let fresh = api::fetch_record(token, tenant, &path)?;
    contract_unchanged(original, &fresh)?;
    if !standard_contract(&fresh) {
        return Err("この契約は専用の有効化・変更手続きが必要です。".into());
    }
    Ok((path, fresh))
}
pub(super) fn contract_update(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let (path, _) = fresh_contract(token, tenant, original)?;
    api::mutate_record(token, tenant, "PUT", &path, body, None)
}
pub(super) fn contract_delete(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let (path, fresh) = fresh_contract(token, tenant, original)?;
    if !contract_draft(&fresh) {
        return Err("下書き契約だけを削除できます。".into());
    }
    api::mutate_record(token, tenant, "DELETE", &path, body, None)
}
pub(super) fn contract_reapply(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let (path, fresh) = fresh_contract(token, tenant, original)?;
    if !contract_draft(&fresh) {
        return Err("下書き契約だけにひな形を再適用できます。".into());
    }
    api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{path}/reapply-template"),
        body,
        None,
    )
}
pub(super) fn contract_response(value: &Value) -> Result<(), String> {
    checked_document_id(value, "salesct_").map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::super::super::model;
    use super::*;

    fn resource(key: &str) -> Resource {
        *super::super::RESOURCES
            .iter()
            .find(|resource| resource.key == key)
            .unwrap()
    }
    fn action(key: &str, label: &str) -> Mutation {
        *resource(key)
            .actions
            .iter()
            .find(|operation| operation.label == label)
            .unwrap()
    }
    fn quote() -> Value {
        json!({"id":"quo_one","quotationNumber":"Q-1","clientId":"client-one","clientName":"取引先",
            "clientEmail":"recipient@example.com","status":"Draft","currency":"JPY","validUntil":"2027-01-20",
            "items":[{"description":"作業","quantity":2,"unitPrice":1500,"discountAmount":0,"taxRate":10,"unit":"時間","productId":null,"amount":3000}],
            "subtotalAmount":3000,"taxAmount":300,"totalAmount":3300,"discountAmount":0,
            "notes":"備考","updatedAt":"2026-11-19T01:00:00Z","paymentLinkUrl":null,"convertedInvoiceId":null})
    }
    fn order() -> Value {
        json!({"id":"ord_one","orderNumber":"ORDER-1","clientId":"client-one","clientName":"取引先",
            "clientEmail":null,"status":"pending","currency":"JPY","source":"manual",
            "items":[{"sku":"SKU-1","description":"商品","quantity":2,"unitPrice":1500,"amount":3000}],
            "subtotalAmount":3000,"taxAmount":300,"totalAmount":3300,"notes":null,
            "updatedAt":"2026-11-19T01:00:00Z","inventoryDecrementedAt":null,"convertedInvoiceId":null})
    }
    fn contract() -> Value {
        json!({"id":"salesct_one","aggregateRevision":"18446744073709551614","contractNumber":"CON-1",
            "title":"保守契約","clientId":"client-one","clientName":"取引先","monthlyAmountYen":9007199254740993u64,
            "lineItems":[{"description":"保守","quantity":1,"unitPrice":9007199254740993u64}],
            "billingCycle":"monthly","startDate":"2026-11-19","endDate":null,"status":"draft",
            "terms":"条項","notes":"記録","templateId":"tpl_one","templateBodySnapshot":"保存時の本文",
            "sourceType":"quote","sourceId":"quo_one"})
    }

    #[test]
    fn quote_form_decodes_legacy_metadata_and_keeps_unknown_document_fields() {
        let operation = resource("quotations").edit.unwrap();
        let mut original = quote();
        let metadata = json!({"futureSetting":{"keep":true},"issuer":{"name":"発行元A","futureField":"keep"},
            "recipient":{"name":"帳票用宛名","email":"printed@example.com","department":"営業部"},
            "lineItems":[{"productId":"product-a","unit":"式","taxRate":8}]});
        original["notes"] = json!(format!(
            "{TEMPLATE_MARKER}custom-template\n{DOCUMENT_MARKER}{}\n元の備考",
            api::encode_query(&metadata.to_string())
        ));
        original["items"][0]["productId"] = Value::Null;
        original["items"][0]["taxRate"] = Value::Null;
        original["items"][0]["unit"] = Value::Null;
        let before = original.clone();
        let mut draft = model::mutation_draft(operation, &original).unwrap();
        assert_eq!(draft["clientName"], "帳票用宛名");
        assert_eq!(draft["clientEmail"], "printed@example.com");
        assert_eq!(draft["notes"], "元の備考");
        assert_eq!(draft["items"][0]["taxRate"], "8");
        assert_eq!(draft["items"][0]["productId"], "product-a");
        assert_eq!(draft["issuer"]["name"], "発行元A");
        draft["items"][0]["quantity"] = json!("3");
        draft["notes"] = json!("更新した備考");
        draft["clientName"] = json!("新しい帳票用宛名");
        let body = model::request_body(operation, &draft, &original, "unused").unwrap();
        let (template, saved, notes) = quotation_document(&body).unwrap();
        assert_eq!(template, "custom-template");
        assert_eq!(saved["futureSetting"], metadata["futureSetting"]);
        assert_eq!(saved["issuer"]["futureField"], "keep");
        assert_eq!(saved["recipient"]["name"], "新しい帳票用宛名");
        assert_eq!(saved["lineItems"][0]["taxRate"], 8);
        assert_eq!(body["taxAmount"], 360);
        assert_eq!(notes, "更新した備考");
        assert_eq!(
            original, before,
            "display normalization must never replace the concurrency snapshot"
        );
        assert!(body.get("issuer").is_none() && body.get("recipient").is_none());
        assert!(body.get("currency").is_none());
    }

    #[test]
    fn quote_groups_tax_before_rounding_and_uses_no_float() {
        let operation = resource("quotations").create.unwrap();
        let mut draft = model::mutation_draft(operation, &Value::Null).unwrap();
        draft["quotationNumber"] = json!("Q-2");
        draft["clientName"] = json!("取引先");
        draft["items"] = json!([
            {"description":"10% A","quantity":"1","unitPrice":"5","discountAmount":"0","taxRate":"10","unit":"個","productId":""},
            {"description":"10% B","quantity":"1","unitPrice":"5","discountAmount":"0","taxRate":"10","unit":"個","productId":""},
            {"description":"8%","quantity":"1","unitPrice":"100","discountAmount":"10","taxRate":"8","unit":"個","productId":""}
        ]);
        let body = model::request_body(operation, &draft, &Value::Null, "unused").unwrap();
        assert_eq!(body["taxAmount"], 8);
        assert_eq!(body["clientId"], UNASSIGNED_CLIENT);
        assert_eq!(body["status"], "Draft");
        assert_eq!(body["currency"], "JPY");
        assert!(
            body.get("validUntil").is_none(),
            "the server must choose its default date"
        );
        assert!(body.get("sendEmail").is_none() && body.get("createPaymentLink").is_none());
        draft["items"] = json!([{ "description":"大きな金額", "quantity":"3", "unitPrice":"9007199254740993", "discountAmount":"0", "taxRate":"10", "unit":"式", "productId":"" }]);
        let body = model::request_body(operation, &draft, &Value::Null, "unused").unwrap();
        assert_eq!(body["items"][0]["unitPrice"], 9007199254740993u64);
        assert_eq!(body["taxAmount"], 2702159776422297u64);
    }

    #[test]
    fn quote_edit_money_uses_existing_currency_and_rebuilds_metadata_after_row_removal() {
        let mut original = quote();
        original["currency"] = json!("USD");
        original["items"][0]["unitPrice"] = json!(12345);
        let operation = resource("quotations").edit.unwrap();
        let mut draft = model::mutation_draft(operation, &original).unwrap();
        assert_eq!(draft["items"][0]["unitPrice"], "123.45");
        draft["items"][0]["discountAmount"] = json!("0.45");
        let body = model::request_body(operation, &draft, &original, "unused").unwrap();
        assert_eq!(body["items"][0]["unitPrice"], 12345);
        assert_eq!(body["items"][0]["discountAmount"], 45);
        assert_eq!(body["taxAmount"], 2464);
        assert_eq!(
            quotation_document(&body).unwrap().1["lineItems"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(body.get("currency").is_none());
    }

    #[test]
    fn quote_rejects_discount_overflow_invalid_quantity_and_bad_metadata() {
        let operation = resource("quotations").edit.unwrap();
        let original = quote();
        for (key, value) in [
            ("quantity", "0"),
            ("quantity", "2147483648"),
            ("unitPrice", "9223372036854775807"),
            ("discountAmount", "3001"),
            ("taxRate", "101"),
        ] {
            let mut draft = model::mutation_draft(operation, &original).unwrap();
            draft["items"][0][key] = json!(value);
            assert!(
                model::request_body(operation, &draft, &original, "unused").is_err(),
                "{key}={value}"
            );
        }
        let mut broken = original.clone();
        broken["notes"] = json!(format!("{DOCUMENT_MARKER}%ZZ"));
        assert!(model::mutation_draft(operation, &broken).is_err());
        assert_eq!(decode_metadata("a+b%2Bc").unwrap(), "a+b+c");
    }

    #[test]
    fn quote_lifecycle_blocks_payment_links_and_requires_a_real_client_before_issue() {
        let mut original = quote();
        let issue = action("quotations", "見積を発行");
        assert!(issue.is_available(&original));
        original["clientId"] = json!(UNASSIGNED_CLIENT);
        assert!(!issue.is_available(&original));
        original = quote();
        original["paymentLinkUrl"] = json!("https://example.com/payment");
        assert!(!resource("quotations").edit.unwrap().is_available(&original));
        assert!(!action("quotations", "見積を取り消す").is_available(&original));
        original = quote();
        original["status"] = json!("Sent");
        assert!(!resource("quotations").edit.unwrap().is_available(&original));
        assert!(action("quotations", "受注を作成").is_available(&original));
    }

    #[test]
    fn quote_preflight_detects_both_timestamp_and_content_changes() {
        let original = quote();
        same_snapshot(&original, &original, QUOTE_SNAPSHOT, "updatedAt").unwrap();
        let mut fresh = original.clone();
        fresh["updatedAt"] = json!("2026-11-19T02:00:00Z");
        assert!(same_snapshot(&original, &fresh, QUOTE_SNAPSHOT, "updatedAt").is_err());
        fresh = original.clone();
        fresh["items"][0]["unitPrice"] = json!(1600);
        assert!(same_snapshot(&original, &fresh, QUOTE_SNAPSHOT, "updatedAt").is_err());
        fresh = original.clone();
        fresh.as_object_mut().unwrap().remove("updatedAt");
        assert!(same_snapshot(&fresh, &fresh, QUOTE_SNAPSHOT, "updatedAt").is_err());
    }

    #[test]
    fn manual_order_form_submits_exact_lines_and_forces_unconfirmed_manual_origin() {
        let operation = resource("orders").create.unwrap();
        let mut draft = model::mutation_draft(operation, &Value::Null).unwrap();
        draft["clientId"] = json!("client-one");
        draft["clientName"] = json!("取引先");
        draft["items"] = json!([{ "sku":"SKU-A", "description":"役務", "quantity":"3", "unitPrice":"9007199254740993" }]);
        draft["taxAmount"] = json!("1");
        draft["status"] = json!("completed");
        draft["squarePaymentId"] = json!("forged");
        let body = model::request_body(operation, &draft, &Value::Null, "unused").unwrap();
        assert_eq!(body["items"][0]["unitPrice"], 9007199254740993u64);
        assert_eq!(body["status"], "pending");
        assert_eq!(body["source"], "manual");
        assert_eq!(body["taxAmount"], 1);
        assert!(body.get("squarePaymentId").is_none() && body.get("orderNumber").is_none());
        assert_eq!(
            operation.idempotency,
            Idempotency::None,
            "no unimplemented server idempotency may be claimed"
        );
        draft["items"][0]["quantity"] = json!("1024");
        assert!(model::request_body(operation, &draft, &Value::Null, "unused").is_err());
    }

    #[test]
    fn order_status_commands_follow_forward_transitions_and_protect_outbound_and_invoiced_orders() {
        let mut original = order();
        assert!(action("orders", "受注を確定").is_available(&original));
        assert!(!action("orders", "出荷済みにする").is_available(&original));
        assert!(action("orders", "受注を取り消す").is_available(&original));
        original["status"] = json!("confirmed");
        assert!(action("orders", "出荷済みにする").is_available(&original));
        original["inventoryDecrementedAt"] = json!("2026-11-19T02:00:00Z");
        assert!(!action("orders", "受注を取り消す").is_available(&original));
        original["inventoryDecrementedAt"] = Value::Null;
        original["convertedInvoiceId"] = json!("inv_a");
        assert!(!action("orders", "受注を取り消す").is_available(&original));
        original["status"] = json!("shipped");
        assert!(action("orders", "受注を完了").is_available(&original));
        original["status"] = json!("approval_needed");
        assert!(!action("orders", "受注を確定").is_available(&original));
        assert!(!action("orders", "受注を取り消す").is_available(&original));
    }

    #[test]
    fn order_correction_is_new_order_only_after_cancel_and_never_silently_drops_discounts() {
        let operation = action("orders", "明細を訂正して新規作成");
        let mut original = order();
        assert!(!operation.is_available(&original));
        original["status"] = json!("cancelled");
        assert!(operation.is_available(&original));
        let mut draft = model::mutation_draft(operation, &original).unwrap();
        assert_eq!(draft["orderNumber"], "");
        draft["items"][0]["quantity"] = json!("4");
        let body = model::request_body(operation, &draft, &original, "unused").unwrap();
        assert_eq!(body["items"][0]["quantity"], 4);
        assert_eq!(body["status"], "pending");
        assert!(body.get("id").is_none() && body.get("orderNumber").is_none());
        original["items"][0]["amount"] = json!(2900);
        assert!(!operation.is_available(&original));
        original["items"][0]["amount"] = json!(3000);
        original["source"] = json!("quo:one");
        assert!(!operation.is_available(&original));
    }

    #[test]
    fn order_preflight_detects_an_invoice_created_while_the_form_was_open() {
        let original = order();
        let mut fresh = original.clone();
        fresh["convertedInvoiceId"] = json!("inv_new");
        assert!(same_snapshot(&original, &fresh, ORDER_SNAPSHOT, "updatedAt").is_err());
    }

    #[test]
    fn contract_lifecycle_echoes_only_the_verified_commercial_fields_and_preserves_large_money() {
        let original = contract();
        let operation = action("sales-contracts", "契約を有効にする");
        let draft = model::mutation_draft(operation, &original).unwrap();
        let body = model::request_body(operation, &draft, &original, "unused").unwrap();
        for key in CONTRACT_BODY {
            if *key != "status" {
                assert_eq!(body[*key], original[*key], "{key}");
            }
        }
        assert_eq!(body["status"], "active");
        assert_eq!(body["monthlyAmountYen"], 9007199254740993u64);
        assert!(
            body.get("aggregateRevision").is_none() && body.get("templateBodySnapshot").is_none()
        );
        let mut active = original.clone();
        active["status"] = json!("active");
        assert!(!operation.is_available(&active));
        let terminate = action("sales-contracts", "契約を解約する");
        let body = model::request_body(terminate, &json!({}), &active, "unused").unwrap();
        assert_eq!(body["status"], "terminated");
        assert_eq!(body["sourceId"], "quo_one");
        assert!(!action("sales-contracts", "契約の下書きを削除").is_available(&active));
    }

    #[test]
    fn contract_does_not_bypass_provider_managed_activation_and_rejects_stale_revisions() {
        let original = contract();
        contract_unchanged(&original, &original).unwrap();
        let mut fresh = original.clone();
        fresh["aggregateRevision"] = json!("18446744073709551615");
        assert!(contract_unchanged(&original, &fresh).is_err());
        fresh = original.clone();
        fresh["lineItems"][0]["unitPrice"] = json!(100);
        assert!(contract_unchanged(&original, &fresh).is_err());
        let mut provider = original.clone();
        provider["sourceType"] = json!("provider_managed");
        for action in resource("sales-contracts").actions {
            assert!(!action.is_available(&provider));
        }
        assert!(activate_contract(&mut json!({}), &provider).is_err());
    }
}
