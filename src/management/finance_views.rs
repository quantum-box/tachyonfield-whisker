//! Read-only business aggregates and an explicitly confirmed billing reminder.

use super::{model, schema::*};
use crate::api::{self, Tenant};
use serde_json::{Value, json};
use std::{collections::HashSet, time::Duration};

const PROFIT_FILTERS: &[Field] = &[
    Field::new("date_from", "開始日", InputKind::Date)
        .required()
        .default("@monthStart"),
    Field::new("date_to", "終了日", InputKind::Date)
        .required()
        .default("@today"),
    Field::new(
        "group_by",
        "集計単位",
        InputKind::Select(&[
            Choice::new("day", "日別"),
            Choice::new("week", "週別"),
            Choice::new("month", "月別"),
        ]),
    )
    .required()
    .default("day"),
    Field::new(
        "customer_id",
        "顧客",
        InputKind::Reference(Lookup {
            path: "/v1/erp/customers",
            list_key: "data",
            value_key: "id",
            label_keys: &["displayName", "name"],
        }),
    ),
    Field::new(
        "product_id",
        "商品",
        InputKind::Reference(Lookup {
            path: "/v1/order/products?limit=100",
            list_key: "items",
            value_key: "id",
            label_keys: &["name", "skuCode"],
        }),
    ),
    Field::new(
        "staff_id",
        "担当者",
        InputKind::Reference(Lookup {
            path: "/v1/erp/hrm/staff",
            list_key: "items",
            value_key: "id",
            label_keys: &["name"],
        }),
    ),
];
const LOSS_FILTERS: &[Field] = &[
    Field::new("from", "開始日", InputKind::Date)
        .required()
        .default("@monthStart"),
    Field::new("to", "終了日", InputKind::Date)
        .required()
        .default("@today"),
];
const PROFIT_TREND: &[Field] = &[
    Field::text("period", "期間"),
    Field::new("sales", "売上（円）", InputKind::Decimal),
    Field::new("purchase", "仕入（円）", InputKind::Decimal),
    Field::new("grossProfit", "粗利益（円）", InputKind::Decimal),
    Field::new("grossMarginPct", "粗利率（%）", InputKind::Decimal),
];
const LOSS_BREAKDOWN: &[Field] = &[
    Field::text("label", "区分"),
    Field::new("confirmedCost", "確認済み原価（円）", InputKind::Decimal),
    Field::new("weightGrams", "重量（g）", InputKind::Decimal),
    Field::new("recordCount", "件数", InputKind::Integer),
    Field::new("costUnconfirmedCount", "原価未確認件数", InputKind::Integer),
    Field::new("weightMissingCount", "重量未確認件数", InputKind::Integer),
];

pub const RESOURCES: &[Resource] = &[
    Resource::new(
        "profit-dashboard",
        "会計・監査",
        "売上・利益ダッシュボード",
        "/v1/erp/profit-summary",
        "",
    )
    .route("erp/dashboard")
    .id("period")
    .title(&["period"])
    .filters(PROFIT_FILTERS)
    .load(load_profit)
    .fields(&[
        ("売上（円）", "salesTotal"),
        ("仕入（円）", "purchaseTotal"),
        ("粗利益（円）", "grossProfit"),
        ("粗利率（%）", "grossMarginPct"),
        ("前期間からの売上増減（円）", "comparison.salesTotalDelta"),
        (
            "前期間からの仕入増減（円）",
            "comparison.purchaseTotalDelta",
        ),
        (
            "前期間からの粗利益増減（円）",
            "comparison.grossProfitDelta",
        ),
        ("粗利率の変化（ポイント）", "comparison.grossMarginPctDelta"),
        ("飲食店の原価確認", "restaurant.costStatusText"),
        ("飲食店の売上（円）", "restaurant.salesTotalStr"),
        ("理論食材原価（円）", "restaurant.theoreticalFoodCostStr"),
        (
            "実食材原価（円）",
            "restaurant.actualFoodCost.actualFoodCostStr",
        ),
        ("棚卸差異（円）", "restaurant.inventoryVarianceStr"),
        ("食品ロス（円）", "restaurant.lossTotalStr"),
        ("人件費（円）", "restaurant.laborCostStr"),
        ("共通費配賦（円）", "restaurant.sharedCostAllocationStr"),
        (
            "営業利益の目安（円）",
            "restaurant.operatingProfitIndicatorStr",
        ),
        ("不足している情報", "restaurant.missingSourceText"),
    ])
    .read_lines(&[
        Field::new("byPeriod", "期間別の推移", InputKind::Lines(PROFIT_TREND)),
        Field::new(
            "byCustomer",
            "顧客別",
            InputKind::Lines(&[
                Field::text("customerName", "顧客"),
                Field::new("salesTotal", "売上（円）", InputKind::Decimal),
                Field::new("transactionCount", "取引件数", InputKind::Integer),
            ]),
        ),
        Field::new(
            "byProduct",
            "商品別",
            InputKind::Lines(&[
                Field::text("productName", "商品"),
                Field::new("salesTotal", "売上（円）", InputKind::Decimal),
                Field::new("purchaseTotal", "仕入（円）", InputKind::Decimal),
                Field::new("grossProfit", "粗利益（円）", InputKind::Decimal),
                Field::new("grossMarginPct", "粗利率（%）", InputKind::Decimal),
            ]),
        ),
        Field::new(
            "byStaff",
            "担当者別",
            InputKind::Lines(&[
                Field::text("staffName", "担当者"),
                Field::new("salesTotal", "売上（円）", InputKind::Decimal),
                Field::new("orderCount", "受注件数", InputKind::Integer),
            ]),
        ),
        Field::new(
            "supplierCosts",
            "仕入先別",
            InputKind::Lines(&[
                Field::text("supplierName", "仕入先"),
                Field::new("totalCost", "仕入（円）", InputKind::Decimal),
                Field::new("entryCount", "件数", InputKind::Integer),
            ]),
        ),
        Field::new(
            "restaurant.byMenu",
            "メニュー別の原価",
            InputKind::Lines(&[
                Field::text("menuName", "メニュー"),
                Field::new("salesStr", "売上（円）", InputKind::Decimal),
                Field::new(
                    "theoreticalFoodCostStr",
                    "理論食材原価（円）",
                    InputKind::Decimal,
                ),
                Field::new(
                    "theoreticalGrossProfitStr",
                    "理論粗利益（円）",
                    InputKind::Decimal,
                ),
            ]),
        ),
        Field::new(
            "restaurant.losses",
            "食品ロスの内訳",
            InputKind::Lines(&[
                Field::text("reasonLabel", "理由"),
                Field::new("amountStr", "原価（円）", InputKind::Decimal),
            ]),
        ),
    ]),
    Resource::new(
        "food-loss-dashboard",
        "商品・在庫管理",
        "食品ロスダッシュボード",
        "/v1/field/inventory/food-loss/dashboard",
        "",
    )
    .route("erp/food-loss/dashboard")
    .id("period.from")
    .title(&["periodLabel"])
    .filters(LOSS_FILTERS)
    .load(load_loss)
    .fields(&[
        ("開始日", "period.from"),
        ("終了日", "period.to"),
        ("前期間との比較", "comparison.note"),
        ("利益への影響", "comparison.profitNote"),
        ("比較可能", "comparison.comparable"),
        ("前期間の原価（円）", "comparison.previousConfirmedCost"),
        ("原価の削減（円）", "comparison.costReduction"),
        ("重量の削減（g）", "comparison.weightReductionGrams"),
    ])
    .read_lines(&[
        Field::new(
            "metrics",
            "主要指標",
            InputKind::Lines(&[
                Field::text("label", "指標"),
                Field::text("displayValue", "値"),
                Field::text("statusText", "集計状況"),
                Field::text("note", "補足"),
                Field::new("includedCount", "含む件数", InputKind::Integer),
                Field::new("excludedCount", "除外件数", InputKind::Integer),
            ]),
        ),
        Field::new(
            "breakdowns.byReason",
            "理由別",
            InputKind::Lines(LOSS_BREAKDOWN),
        ),
        Field::new(
            "breakdowns.byIngredient",
            "材料別",
            InputKind::Lines(LOSS_BREAKDOWN),
        ),
        Field::new(
            "breakdowns.byMenu",
            "メニュー別",
            InputKind::Lines(LOSS_BREAKDOWN),
        ),
        Field::new(
            "breakdowns.byCategory",
            "カテゴリ別",
            InputKind::Lines(LOSS_BREAKDOWN),
        ),
        Field::new(
            "breakdowns.byWeekday",
            "曜日別",
            InputKind::Lines(LOSS_BREAKDOWN),
        ),
        Field::new(
            "breakdowns.byHour",
            "時間帯別",
            InputKind::Lines(LOSS_BREAKDOWN),
        ),
        Field::new(
            "trend",
            "日別推移",
            InputKind::Lines(&[
                Field::text("date", "日付"),
                Field::new("confirmedCost", "確認済み原価（円）", InputKind::Decimal),
                Field::new("weightGrams", "重量（g）", InputKind::Decimal),
                Field::new("recordCount", "件数", InputKind::Integer),
                Field::new("costUnconfirmedCount", "原価未確認件数", InputKind::Integer),
            ]),
        ),
        Field::new(
            "consumptionVariances",
            "使用量の差異",
            InputKind::Lines(&[
                Field::text("ingredientName", "材料"),
                Field::new("theoreticalGrams", "理論使用量（g）", InputKind::Decimal),
                Field::new("actualGrams", "実使用量（g）", InputKind::Decimal),
                Field::new("varianceGrams", "差異（g）", InputKind::Decimal),
            ]),
        ),
        Field::new(
            "sources",
            "集計元の接続",
            InputKind::Lines(&[
                Field::text("label", "集計元"),
                Field::new("connected", "接続済み", InputKind::Bool),
            ]),
        ),
    ]),
    Resource::new(
        "billing-queue",
        "請求・債権管理",
        "請求・入金の対応一覧",
        "/v1/invoices",
        "items",
    )
    .route("billing")
    .title(&["title"])
    .subtitle(&["counterparty", "amountText"])
    .status("priorityText")
    .load(load_billing)
    .fields(&[
        ("取引先", "counterparty"),
        ("種類", "kindText"),
        ("優先度", "priorityText"),
        ("状態", "status"),
        ("金額", "amountText"),
        ("支払期限", "dueDate"),
        ("発生日", "eventDate"),
        ("詳細", "detail"),
        ("送信先メール", "clientEmail"),
    ])
    .actions(&[Mutation::new(
        "支払案内メールを再送",
        "POST",
        "/v1/invoices/{invoiceId}/payment-link/resend",
        &[
            Field::new("recipient", "送信先", InputKind::ReadOnly).read("clientEmail"),
            Field::new(
                "confirmed",
                "送信先と請求内容を確認しました",
                InputKind::Bool,
            )
            .required(),
        ],
    )
    .when(|record| record["canSend"] == true)
    .prepare(prepare_reminder)
    .execute(send_reminder)
    .validate_response(reminder_result)
    .explain("請求書に保存されている宛先へ、支払いリンクのメールを実際に送信します。")]),
];

fn validate_period(filters: &Value, from: &str, to: &str) -> Result<(), String> {
    if filters[from]
        .as_str()
        .zip(filters[to].as_str())
        .is_some_and(|(a, b)| a > b)
    {
        Err("終了日は開始日以降を選んでください".into())
    } else {
        Ok(())
    }
}

fn camel_keys(value: Value) -> Value {
    match value {
        Value::Array(rows) => Value::Array(rows.into_iter().map(camel_keys).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let mut upper = false;
                    let mut converted = String::new();
                    for c in key.chars() {
                        if c == '_' {
                            upper = true;
                        } else if upper {
                            converted.extend(c.to_uppercase());
                            upper = false;
                        } else {
                            converted.push(c);
                        }
                    }
                    (converted, camel_keys(value))
                })
                .collect(),
        ),
        value => value,
    }
}

fn load_profit(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    validate_period(filters, "date_from", "date_to")?;
    let path = model::query_path(RESOURCES[0].path, PROFIT_FILTERS, filters)?;
    let mut result = camel_keys(api::fetch_record(token, tenant, &path)?);
    for key in [
        "period",
        "salesTotal",
        "purchaseTotal",
        "grossProfit",
        "grossMarginPct",
    ] {
        if result[key].is_null() {
            return Err("利益集計の応答を確認できません。読み直してください".into());
        }
    }
    if let Some(restaurant) = result
        .get_mut("restaurant")
        .filter(|value| value.is_object())
    {
        restaurant["costStatusText"] = json!(if restaurant["actualCostStatus"] == "confirmed" {
            "確認済み"
        } else {
            "未確認の原価があります"
        });
        let missing = restaurant["missingSources"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|source| match source.as_str() {
                Some("pos_sales") => "POS売上",
                Some("theoretical_consumption") => "理論使用量",
                Some("inventory_count") => "棚卸",
                Some("food_loss") => "食品ロス",
                Some("labor_cost") => "人件費",
                Some("shared_cost_allocation") => "共通費配賦",
                _ => "未確認の集計元",
            })
            .collect::<Vec<_>>()
            .join("、");
        restaurant["missingSourceText"] = json!(if missing.is_empty() {
            "なし".into()
        } else {
            missing
        });
    }
    Ok(result)
}

/// Shift a decimal point without losing the API's exact base-10 digits.
fn percent(text: &str) -> Result<String, String> {
    shift_decimal(text, 2)
}

fn shift_decimal(text: &str, places: i32) -> Result<String, String> {
    let (negative, raw) = text.strip_prefix('-').map_or((false, text), |s| (true, s));
    let (raw, exponent) = if let Some((base, power)) = raw.split_once(['e', 'E']) {
        (
            base,
            power
                .parse::<i32>()
                .map_err(|_| "数値の指数を確認できません")?,
        )
    } else {
        (raw, 0)
    };
    let mut parts = raw.split('.');
    let integer = parts.next().unwrap_or("");
    let fraction = parts.next().unwrap_or("");
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|c| c.is_ascii_digit())
        || !fraction.bytes().all(|c| c.is_ascii_digit())
    {
        return Err("集計値の形式を確認できません".into());
    }
    let mut digits = format!("{integer}{fraction}");
    let split = i32::try_from(integer.len())
        .ok()
        .and_then(|length| length.checked_add(exponent))
        .and_then(|length| length.checked_add(places))
        .filter(|position| (-10_000..=10_000).contains(position))
        .ok_or("数値の桁数を確認できません")?;
    let split = if split <= 0 {
        digits = format!("{}{}", "0".repeat((1 - split) as usize), digits);
        1
    } else {
        split as usize
    };
    while digits.len() < split {
        digits.push('0');
    }
    let (left, right) = digits.split_at(split);
    let left = left.trim_start_matches('0');
    let left = if left.is_empty() { "0" } else { left };
    let right = right.trim_end_matches('0');
    let sign = if negative && (left != "0" || !right.is_empty()) {
        "-"
    } else {
        ""
    };
    Ok(if right.is_empty() {
        format!("{sign}{left}")
    } else {
        format!("{sign}{left}.{right}")
    })
}

fn metric_text(metric: &Value) -> Result<String, String> {
    if !matches!(
        metric["status"].as_str(),
        Some("complete" | "partial" | "unavailable")
    ) {
        return Err("指標の集計状況を確認できません".into());
    }
    if metric["status"] == "unavailable" || metric["value"].is_null() {
        return Ok("算出できません".into());
    }
    let raw = metric["value"].as_str().ok_or("指標の値を確認できません")?;
    let value = match metric["unit"].as_str() {
        Some("ratio") => format!("{}%", percent(raw)?),
        Some("yen") => format!("{raw}円"),
        Some("g") => format!("{raw}g"),
        _ => return Err("指標の単位を確認できません".into()),
    };
    Ok(if metric["status"] == "partial" {
        format!("{value} 以上")
    } else {
        value
    })
}

fn load_loss(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    validate_period(filters, "from", "to")?;
    let path = model::query_path(RESOURCES[1].path, LOSS_FILTERS, filters)?;
    let mut result = api::fetch_record(token, tenant, &path)?;
    if result["period"]["from"] != filters["from"] || result["period"]["to"] != filters["to"] {
        return Err("集計期間が一致しません。読み直してください".into());
    }
    let mut metrics = Vec::new();
    for (key, label) in [
        ("lossCost", "食品ロス原価"),
        ("lossWeight", "食品ロス重量"),
        ("lossRateToPurchases", "仕入れに対するロス率"),
        ("expiredLossCost", "期限切れによる原価"),
        ("overproductionLossCost", "作り過ぎによる原価"),
        ("consumptionVariance", "使用量の差異"),
    ] {
        let mut metric = result["kpis"][key].clone();
        if !metric.is_object() {
            return Err("食品ロス指標を確認できません".into());
        }
        metric["label"] = json!(label);
        metric["displayValue"] = json!(metric_text(&metric)?);
        metric["statusText"] = json!(match metric["status"].as_str() {
            Some("complete") => "すべて含む",
            Some("partial") => "一部除外",
            _ => "算出できません",
        });
        metrics.push(metric);
    }
    result["metrics"] = json!(metrics);
    result["periodLabel"] = json!(format!(
        "{} 〜 {}",
        filters["from"].as_str().unwrap_or(""),
        filters["to"].as_str().unwrap_or("")
    ));
    Ok(result)
}

fn billing_source(
    token: &str,
    tenant: &Tenant,
    path: &'static str,
    pagination: Pagination,
) -> Result<model::Collection, String> {
    model::fetch_collection(
        token,
        tenant,
        Resource::new("billing-source", "", "", path, "items").pagination(pagination),
        &json!({}),
    )
}

fn load_billing(token: &str, tenant: &Tenant, _: &Value) -> Result<Value, String> {
    const AR_FILTERS: &[Field] = &[Field::new("as_of", "基準日", InputKind::Date).required()];
    let today = model::expanded_default("@today");
    let (sent, overdue, ar, square) = std::thread::scope(|scope| {
        let sent = scope.spawn(|| {
            billing_source(
                token,
                tenant,
                "/v1/invoices?status=Sent",
                Pagination::Offset { limit: 500 },
            )
        });
        let overdue = scope.spawn(|| {
            billing_source(
                token,
                tenant,
                "/v1/invoices?status=Overdue",
                Pagination::Offset { limit: 500 },
            )
        });
        let ar = scope.spawn(|| {
            model::fetch_collection(
                token,
                tenant,
                Resource::new(
                    "billing-ar",
                    "",
                    "",
                    "/v1/erp/ar-ap/items?kind=receivable",
                    "items",
                )
                .filters(AR_FILTERS),
                &json!({"as_of":today}),
            )
        });
        let square = scope.spawn(|| {
            billing_source(
                token,
                tenant,
                "/v1/invoice-reconciliations/square-payments?status=Unmatched",
                Pagination::Offset { limit: 100 },
            )
        });
        let failed = || Err("請求対応一覧の取得を完了できませんでした".to_owned());
        (
            sent.join().unwrap_or_else(|_| failed()),
            overdue.join().unwrap_or_else(|_| failed()),
            ar.join().unwrap_or_else(|_| failed()),
            square.join().unwrap_or_else(|_| failed()),
        )
    });
    let (mut sources, notice) = billing_sources([sent, overdue, ar, square])?;
    let square = sources.pop().unwrap();
    let ar = sources.pop().unwrap();
    let overdue = sources.pop().unwrap();
    let sent = sources.pop().unwrap();
    let truncated = sent.truncated || overdue.truncated || ar.truncated || square.truncated;
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for invoice in sent.rows.iter().chain(&overdue.rows) {
        let id = invoice["id"]
            .as_str()
            .ok_or("請求書の応答を確認できません")?;
        if !seen.insert(id) {
            continue;
        }
        let urgent = invoice["status"] == "Overdue"
            || invoice["dueDate"]
                .as_str()
                .is_some_and(|due| due < today.as_str());
        items.push(json!({"id":format!("invoice-{id}"), "invoiceId":id, "title":invoice["invoiceNumber"], "counterparty":invoice["clientName"], "amount":invoice["totalAmount"], "currency":invoice["currency"], "status":invoice["status"], "kindText":if urgent {"期限超過の請求"} else {"未入金の請求"}, "priorityText":if urgent {"優先対応"} else {"要確認"}, "dueDate":invoice["dueDate"], "eventDate":invoice["updatedAt"], "clientEmail":invoice["clientEmail"], "canSend":invoice["status"] != "Paid" && invoice["paymentLinkUrl"].as_str().is_some_and(|s| !s.is_empty()) && invoice["clientEmail"].as_str().is_some_and(|s| !s.is_empty()), "detail":if invoice["paymentLinkUrl"].is_string() {"支払いリンク発行済み"} else {"支払いリンク未発行"}}));
    }
    for row in ar.rows {
        if !positive_financial_number(&row["outstandingAmount"])? {
            continue;
        }
        let id = row["id"].as_str().ok_or("売掛明細の応答を確認できません")?;
        let urgent = row["daysOverdue"].as_u64().is_some_and(|days| days > 0);
        let partial = positive_financial_number(&row["settledAmount"])?;
        items.push(json!({"id":format!("ar-{id}"), "title":row["sourceNumber"].as_str().unwrap_or("売掛明細"), "counterparty":row["counterpartyName"], "amount":row["outstandingAmount"], "currency":row["currency"], "status":row["effectiveStatus"], "kindText":if partial {"一部入金済み"} else {"売掛金"}, "priorityText":if urgent {"優先対応"} else {"要確認"}, "dueDate":row["dueDate"], "canSend":false, "detail":if urgent {format!("{}日超過", model::input_text(&row["daysOverdue"]))} else {"未回収の売掛金です".into()}}));
    }
    for row in square.rows {
        let id = row["id"].as_str().ok_or("入金の応答を確認できません")?;
        items.push(json!({"id":format!("square-{id}"), "title":"Square未照合入金", "counterparty":row["payerHint"], "amount":row["amount"], "currency":row["currency"], "status":row["status"], "kindText":"入金の照合が必要", "priorityText":"優先対応", "eventDate":row["receivedAt"], "canSend":false, "detail":row["matchReason"].as_str().unwrap_or("請求書に消し込まれていません")}));
    }
    for row in &mut items {
        let raw = model::input_text(&row["amount"]);
        let currency = row["currency"]
            .as_str()
            .filter(|code| code.len() == 3 && code.bytes().all(|c| c.is_ascii_uppercase()))
            .ok_or("請求・入金の通貨を確認できません")?;
        let amount = shift_decimal(&raw, -(crate::features::minor_unit_digits(currency) as i32))?;
        row["amountText"] = json!(format!("{amount} {currency}"));
    }
    items.sort_by_key(|row| {
        (
            row["priorityText"] != "優先対応",
            row["dueDate"]
                .as_str()
                .or_else(|| row["eventDate"].as_str())
                .unwrap_or("")
                .to_owned(),
        )
    });
    Ok(json!({"items":items,"truncated":truncated,"notice":notice}))
}

fn positive_financial_number(value: &Value) -> Result<bool, String> {
    if !value.is_number() {
        return Err("売掛明細の金額を確認できません".into());
    }
    let decimal = shift_decimal(&model::input_text(value), 0)?;
    Ok(!decimal.starts_with('-') && decimal.bytes().any(|c| matches!(c, b'1'..=b'9')))
}

fn billing_sources(
    results: [Result<model::Collection, String>; 4],
) -> Result<(Vec<model::Collection>, String), String> {
    let labels = [
        "送付済み請求",
        "期限超過請求",
        "売掛明細",
        "Square未照合入金",
    ];
    let mut sources = Vec::new();
    let mut failed = Vec::new();
    for (label, result) in labels.into_iter().zip(results) {
        match result {
            Ok(source) => sources.push(source),
            Err(_) => {
                failed.push(label);
                sources.push(model::Collection::default());
            }
        }
    }
    if failed.len() == 4 {
        return Err(
            "請求・入金の情報を取得できませんでした。時間をおいて読み直してください".into(),
        );
    }
    let notice = if failed.is_empty() {
        String::new()
    } else {
        format!(
            "{}を確認できませんでした。取得できた領域の項目だけを表示しています。",
            failed.join("・")
        )
    };
    Ok((sources, notice))
}

fn prepare_reminder(body: &mut Value, _: &Value) -> Result<(), String> {
    if body["confirmed"] != true {
        return Err("送信先と請求内容を確認してください".into());
    }
    *body = json!({"sendEmail":true});
    Ok(())
}

fn send_reminder(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let id = original["invoiceId"]
        .as_str()
        .ok_or("送信する請求書を確認できません")?;
    let path = format!("/v1/invoices/{}", api::encode_query(id));
    let fresh = api::fetch_record(token, tenant, &path)?;
    if fresh["clientEmail"] != original["clientEmail"]
        || fresh["updatedAt"] != original["eventDate"]
        || fresh["totalAmount"] != original["amount"]
        || fresh["currency"] != original["currency"]
        || !matches!(fresh["status"].as_str(), Some("Sent" | "Overdue"))
        || !fresh["paymentLinkUrl"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    {
        return Err("請求内容または送信先が変わっています。読み直してから確認してください".into());
    }
    api::send_json(
        token,
        tenant,
        "POST",
        &format!("{path}/payment-link/resend"),
        body,
        Duration::from_secs(30),
    )
}

fn reminder_result(value: &Value) -> Result<(), String> {
    if value["emailDeliveryStatus"] == "Failed" {
        return Err("メール送信に失敗しました。請求書の送信先を確認してください".into());
    }
    if value["paymentLinkStatus"] == "Failed"
        && !value["paymentLinkUrl"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    {
        return Err("支払いリンクの生成に失敗しました".into());
    }
    if !matches!(
        value["emailDeliveryStatus"].as_str(),
        Some("Sent" | "Pending" | "Ready")
    ) {
        return Err(
            "送信結果を確認できませんでした。再送前に請求書の状態を確認してください".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_loss_metrics_never_look_like_zero_or_complete_values() {
        assert_eq!(
            metric_text(&json!({"status":"unavailable","value":"0","unit":"yen"})).unwrap(),
            "算出できません"
        );
        assert_eq!(
            metric_text(
                &json!({"status":"partial","value":"0.01234567890123456789","unit":"ratio"})
            )
            .unwrap(),
            "1.234567890123456789% 以上"
        );
        assert_eq!(percent("-0.00000000000001").unwrap(), "-0.000000000001");
        assert_eq!(
            percent("9007199254740993.123").unwrap(),
            "900719925474099312.3"
        );
    }
    #[test]
    fn profit_key_normalization_preserves_exact_cost_strings_and_nulls() {
        let result = camel_keys(
            json!({"restaurant":{"sales_total_str":"9007199254740993.00001","actual_food_cost":null},"by_day":[{"gross_profit":1200}]}),
        );
        assert_eq!(
            result["restaurant"]["salesTotalStr"],
            "9007199254740993.00001"
        );
        assert!(result["restaurant"]["actualFoodCost"].is_null());
        assert_eq!(result["byDay"][0]["grossProfit"], 1200);
    }
    #[test]
    fn reminder_requires_confirmation_and_checks_successful_http_failure() {
        assert!(prepare_reminder(&mut json!({"confirmed":false}), &Value::Null).is_err());
        let mut body = json!({"confirmed":true});
        prepare_reminder(&mut body, &Value::Null).unwrap();
        assert_eq!(body, json!({"sendEmail":true}));
        assert!(reminder_result(&json!({"emailDeliveryStatus":"Failed"})).is_err());
        assert!(reminder_result(&json!({"emailDeliveryStatus":"Sent"})).is_ok());
        assert!(reminder_result(&json!({})).is_err());
    }

    #[test]
    fn billing_partial_reads_are_explicit_and_total_failure_is_not_an_empty_queue() {
        let (_, notice) = billing_sources([
            Ok(model::Collection::default()),
            Err("forbidden".into()),
            Ok(model::Collection::default()),
            Err("unavailable".into()),
        ])
        .unwrap();
        assert!(notice.contains("期限超過請求"));
        assert!(notice.contains("Square未照合入金"));
        assert!(billing_sources(std::array::from_fn(|_| Err("unavailable".into()))).is_err());
    }

    #[test]
    fn ar_amounts_include_json_decimals_and_preserve_currency_scale() {
        assert!(positive_financial_number(&json!(200.0)).unwrap());
        assert!(positive_financial_number(&json!(0.25)).unwrap());
        assert!(!positive_financial_number(&json!(0.0)).unwrap());
        assert!(!positive_financial_number(&json!(-0.25)).unwrap());
        assert_eq!(shift_decimal("12345.5", -2).unwrap(), "123.455");
        assert_eq!(shift_decimal("2.5e-7", -2).unwrap(), "0.0000000025");
        assert_eq!(
            shift_decimal("9007199254740993", -2).unwrap(),
            "90071992547409.93"
        );
    }
}
