//! レジ管理表（レジ締め）。Web 版（apps/client/src/features/register-count）と同じ JSON・同じ計算。
//!
//! 店舗・レジの帳簿（帳簿設定の `registerCount`）ごとに 1 日 1 件。開始時と終了時に金種ごとの枚数を数え、
//! 増えた現金と入出金合計（売上 − 控除）を照らして過不足を出す。

use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value, json};

use crate::cash_book_receipts::{Receipt, normalize_text};

/// 金種（大きい順）。
pub const DENOMS: [i64; 9] = [10000, 5000, 1000, 500, 100, 50, 10, 5, 1];

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: String,
    pub label: String,
    /// add / subtract / memo
    pub effect: String,
    /// items / manual / credit / codePayment
    pub source: String,
    pub quantity: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub rows: Vec<Row>,
    pub remainder: String,
    pub weather_area_code: Option<String>,
    pub raw: Value,
}

impl Template {
    pub fn from_value(value: &Value) -> Option<Self> {
        let rows: Vec<Row> = value["rows"]
            .as_array()?
            .iter()
            .map(|row| {
                Some(Row {
                    key: row["key"].as_str()?.to_owned(),
                    label: row["label"].as_str().unwrap_or("").to_owned(),
                    effect: row["effect"].as_str().unwrap_or("add").to_owned(),
                    source: row["source"].as_str().unwrap_or("manual").to_owned(),
                    quantity: row["quantity"] == Value::Bool(true),
                })
            })
            .collect::<Option<_>>()?;
        if rows.is_empty() {
            return None;
        }
        Some(Self {
            remainder: value["remainder"].as_str().unwrap_or(&rows[0].key).to_owned(),
            rows,
            weather_area_code: value["weatherAreaCode"].as_str().map(str::to_owned),
            raw: value.clone(),
        })
    }

    pub fn row(&self, key: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.key == key)
    }
}

/// 新しく設定するときの形（Web 版の defaultRegisterCountTemplate）。
pub fn default_template() -> Value {
    json!({
        "rows": [
            { "key": "sales", "label": "売上", "effect": "add", "source": "items", "quantity": false },
            { "key": "credit", "label": "クレジット決済", "effect": "subtract", "source": "credit", "quantity": false },
            { "key": "codePayment", "label": "コード・電子マネー決済", "effect": "subtract", "source": "codePayment", "quantity": false },
            { "key": "payout", "label": "開始後出金", "effect": "subtract", "source": "manual", "quantity": false },
        ],
        "rules": [],
        "remainder": "sales",
        "print": {
            "title": "レジ管理表", "companyName": "", "layout": "sideBySide", "orientation": "landscape",
            "dateStyle": "iso", "fontSize": 9, "approvalLabel": "", "footer": "",
        },
    })
}

const CHECK: &str = "実績項目、決済の控除、振り分け先、未分類の項目を確認してください。";

/// レジ管理表の設定を確かめる（Web 版の registerCountTemplateSchema と同じ決まり）。
pub fn validate_template(value: &Value) -> Result<(), String> {
    let Some(t) = Template::from_value(value) else {
        return Err("実績項目を 1 つ以上入れてください。".into());
    };
    if t.rows.len() > 30 {
        return Err("実績項目は 30 件までです。".into());
    }
    for row in &t.rows {
        let label = row.label.trim();
        if label.is_empty() || row.label.encode_utf16().count() > 100 {
            return Err("項目名を入れてください（100文字まで）。".into());
        }
        if !matches!(row.effect.as_str(), "add" | "subtract" | "memo")
            || !matches!(row.source.as_str(), "items" | "manual" | "credit" | "codePayment")
        {
            return Err(CHECK.into());
        }
    }
    let rules = value["rules"].as_array().cloned().unwrap_or_default();
    if rules.len() > 150 {
        return Err("振り分けは 150 件までです。".into());
    }
    let keys: Vec<&str> = t.rows.iter().map(|r| r.key.as_str()).collect();
    let unique_keys = keys.iter().enumerate().all(|(i, k)| !keys[..i].contains(k));
    let remainder_ok = t.row(&t.remainder).is_some_and(|r| r.source == "items" && r.effect == "add");
    let targets_ok = rules.iter().all(|r| {
        r["target"].as_str().and_then(|k| t.row(k)).is_some_and(|row| row.source == "items")
            && r["text"].as_str().is_some_and(|s| !s.trim().is_empty() && s.encode_utf16().count() <= 100)
    });
    let product_ids: Vec<&str> = rules.iter().filter_map(|r| r["productId"].as_str()).collect();
    let products_unique = product_ids.iter().enumerate().all(|(i, p)| !product_ids[..i].contains(p));
    let texts: Vec<String> = rules.iter().map(|r| normalize_text(r["text"].as_str().unwrap_or(""))).collect();
    let texts_unique = texts.iter().enumerate().all(|(i, s)| !texts[..i].contains(s));
    let payments: Vec<&Row> = t.rows.iter().filter(|r| r.source == "credit" || r.source == "codePayment").collect();
    let payments_ok = payments.iter().filter(|r| r.source == "credit").count() <= 1
        && payments.iter().filter(|r| r.source == "codePayment").count() <= 1
        && payments.iter().all(|r| r.effect == "subtract" && !r.quantity);
    let no_memo_items = t.rows.iter().all(|r| !(r.source == "items" && r.effect == "memo"));
    let area_ok = t.weather_area_code.as_deref().is_none_or(|c| c.len() == 6 && c.chars().all(|ch| ch.is_ascii_digit()));
    if !(unique_keys && remainder_ok && targets_ok && products_unique && texts_unique && payments_ok && no_memo_items && area_ok) {
        return Err(CHECK.into());
    }
    Ok(())
}

/// 1 日分のレジ管理表。
#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    pub date: String,
    /// `{ label, template }`。最初に保存した時点の帳簿の定義を写す。
    pub scope: Value,
    pub receipt_ids: Vec<String>,
    pub opening_staff: String,
    pub closing_staff: String,
    pub weather: String,
    pub weather_forecast: Value,
    pub customer_count: Option<i64>,
    pub opening: BTreeMap<i64, Option<i64>>,
    pub opening_from_previous: bool,
    pub closing: BTreeMap<i64, Option<i64>>,
    pub sales: BTreeMap<String, Option<i64>>,
    pub sales_quantities: BTreeMap<String, Option<i64>>,
    pub note: String,
    pub confirmed_at: Option<String>,
    /// この画面が知らない項目（dogRunDetail など）。保存のときにそのまま返す。
    pub extra: Map<String, Value>,
}

fn count(value: &Value) -> Option<i64> {
    value.as_i64().filter(|v| *v >= 0 && *v <= 9_007_199_254_740_991)
}

fn counts(value: &Value) -> BTreeMap<i64, Option<i64>> {
    DENOMS.iter().map(|d| (*d, count(&value[d.to_string()]))).collect()
}

fn counts_value(c: &BTreeMap<i64, Option<i64>>) -> Value {
    Value::Object(c.iter().map(|(d, n)| (d.to_string(), json!(n))).collect())
}

const KNOWN: [&str; 15] = [
    "scope", "receiptIds", "openingStaff", "closingStaff", "weather", "weatherForecast", "customerCount", "opening",
    "openingFromPrevious", "closing", "sales", "salesQuantities", "note", "confirmedAt", "date",
];

impl Day {
    pub fn empty(date: &str, scope: Value) -> Self {
        let template = Template::from_value(&scope["template"]);
        let rows = template.map(|t| t.rows).unwrap_or_default();
        let mut extra = Map::new();
        extra.insert(
            "dogRunDetail".into(),
            json!({ "monthlyCredit": null, "subscriptionCredit": null, "photo": null, "goods": null }),
        );
        Self {
            date: date.to_owned(),
            scope,
            receipt_ids: Vec::new(),
            opening_staff: String::new(),
            closing_staff: String::new(),
            weather: String::new(),
            weather_forecast: Value::Null,
            customer_count: None,
            opening: DENOMS.iter().map(|d| (*d, None)).collect(),
            opening_from_previous: false,
            closing: DENOMS.iter().map(|d| (*d, None)).collect(),
            sales: rows.iter().map(|r| (r.key.clone(), None)).collect(),
            sales_quantities: rows.iter().filter(|r| r.quantity).map(|r| (r.key.clone(), None)).collect(),
            note: String::new(),
            confirmed_at: None,
            extra,
        }
    }

    pub fn from_value(date: &str, data: &Value) -> Self {
        let mut day = Self::empty(date, data["scope"].clone());
        let text = |k: &str| data[k].as_str().unwrap_or("").to_owned();
        day.receipt_ids = data["receiptIds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        day.opening_staff = text("openingStaff");
        day.closing_staff = text("closingStaff");
        day.weather = text("weather");
        day.weather_forecast = data["weatherForecast"].clone();
        day.customer_count = count(&data["customerCount"]);
        day.opening = counts(&data["opening"]);
        day.opening_from_previous = data["openingFromPrevious"] == Value::Bool(true);
        day.closing = counts(&data["closing"]);
        for (k, v) in data["sales"].as_object().into_iter().flatten() {
            day.sales.insert(k.clone(), count(v));
        }
        for (k, v) in data["salesQuantities"].as_object().into_iter().flatten() {
            day.sales_quantities.insert(k.clone(), count(v));
        }
        day.note = text("note");
        day.confirmed_at = data["confirmedAt"].as_str().map(str::to_owned);
        if let Some(object) = data.as_object() {
            for (k, v) in object {
                if !KNOWN.contains(&k.as_str()) {
                    day.extra.insert(k.clone(), v.clone());
                }
            }
        }
        day
    }

    pub fn to_value(&self) -> Value {
        let mut o = self.extra.clone();
        if !self.scope.is_null() {
            o.insert("scope".into(), self.scope.clone());
            o.insert("receiptIds".into(), json!(self.receipt_ids));
        }
        o.insert("openingStaff".into(), json!(self.opening_staff));
        o.insert("closingStaff".into(), json!(self.closing_staff));
        o.insert("weather".into(), json!(self.weather));
        o.insert("weatherForecast".into(), self.weather_forecast.clone());
        o.insert("customerCount".into(), json!(self.customer_count));
        o.insert("opening".into(), counts_value(&self.opening));
        o.insert("openingFromPrevious".into(), json!(self.opening_from_previous));
        o.insert("closing".into(), counts_value(&self.closing));
        o.insert("sales".into(), json!(self.sales));
        o.insert("salesQuantities".into(), json!(self.sales_quantities));
        o.insert("note".into(), json!(self.note));
        o.insert("confirmedAt".into(), json!(self.confirmed_at));
        Value::Object(o)
    }

    pub fn template(&self) -> Option<Template> {
        Template::from_value(&self.scope["template"])
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmed_at.is_some()
    }

    /// 入出金合計（加算 − 控除。集計外は数えない）。
    pub fn sales_total(&self) -> i64 {
        let Some(t) = self.template() else { return 0 };
        t.rows.iter().fold(0, |sum, row| {
            let v = self.sales.get(&row.key).copied().flatten().unwrap_or(0);
            match row.effect.as_str() {
                "add" => sum + v,
                "subtract" => sum - v,
                _ => sum,
            }
        })
    }

    /// (終了時現金, 開始時入金, 増えた現金, 入出金合計, 過不足)
    pub fn amount_check(&self) -> (i64, i64, i64, i64, i64) {
        let closing = count_total(&self.closing);
        let opening = count_total(&self.opening);
        let increase = closing - opening;
        let sales = self.sales_total();
        (closing, opening, increase, sales, increase - sales)
    }

    pub fn has_closing_counts(&self) -> bool {
        self.closing.values().any(Option::is_some)
    }
}

pub fn count_total(c: &BTreeMap<i64, Option<i64>>) -> i64 {
    c.iter().map(|(d, n)| d * n.unwrap_or(0)).sum()
}

/// 「ぴったり」「300円 多い」「120円 足りない」。
pub fn over_short_label(x: i64) -> String {
    if x == 0 {
        return "ぴったり".into();
    }
    format!(
        "{}円 {}",
        crate::cash_book::format_yen(x.abs()),
        if x > 0 { "多い" } else { "足りない" }
    )
}

/// 枚数・件数の入力（全角数字、カンマ、「枚」「件」も読む）。空なら Ok(None)。
pub fn parse_count(text: &str) -> Result<Option<i64>, ()> {
    let s: String = text
        .chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .filter(|c| !matches!(c, ',' | '，' | '枚' | '件' | '円' | '¥' | '￥') && !c.is_whitespace())
        .collect();
    if s.is_empty() {
        return Ok(None);
    }
    if !s.chars().all(|c| c.is_ascii_digit()) {
        return Err(());
    }
    s.parse::<i64>().map(Some).map_err(|_| ())
}

/// 売上伝票（締めのレシート）から実績を作った結果。
#[derive(Clone, Debug, PartialEq)]
pub struct SalesResult {
    pub customer_count: Option<i64>,
    pub net_sales: i64,
    pub payment_needs_review: bool,
    pub remainder: i64,
    pub sales: BTreeMap<String, Option<i64>>,
    pub quantities: BTreeMap<String, Option<i64>>,
}

fn int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)),
        Value::String(s) => s.trim().replace(',', "").parse::<i64>().ok(),
        _ => None,
    }
}

/// 売上伝票（レジの締めレポート）の内訳を実績項目に振り分ける。合わなければ `None`
/// （Web 版の salesFromReports）。手入力の項目は今の値を残す。
pub fn sales_from_reports(
    day: &Day,
    template: &Template,
    reports: &[(Receipt, Value)],
    products: &HashMap<String, Value>,
) -> Option<SalesResult> {
    let rules = template.raw["rules"].as_array().cloned().unwrap_or_default();
    let mut sales: BTreeMap<String, Option<i64>> = template.rows.iter().map(|r| (r.key.clone(), None)).collect();
    let mut quantities: BTreeMap<String, Option<i64>> =
        template.rows.iter().filter(|r| r.quantity).map(|r| (r.key.clone(), None)).collect();
    for row in template.rows.iter().filter(|r| r.source == "manual") {
        sales.insert(row.key.clone(), day.sales.get(&row.key).copied().flatten());
        if row.quantity {
            quantities.insert(row.key.clone(), day.sales_quantities.get(&row.key).copied().flatten());
        }
    }
    let credit_key = template.rows.iter().find(|r| r.source == "credit").map(|r| r.key.clone());
    let code_key = template.rows.iter().find(|r| r.source == "codePayment").map(|r| r.key.clone());
    let add = |map: &mut BTreeMap<String, Option<i64>>, key: &str, v: i64| {
        let e = map.entry(key.to_owned()).or_insert(None);
        *e = Some(e.unwrap_or(0) + v);
    };
    let mut net_sales = 0i64;
    let mut customer_count = Some(0i64);
    let mut needs_review = false;
    let mut missing_quantity: Vec<String> = Vec::new();
    for (report, raw) in reports {
        let total = report.total.filter(|t| t.fract() == 0.0)? as i64;
        net_sales += total;
        let customers = int(&raw["extractedData"]["customer_count"]);
        customer_count = match (customer_count, customers) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
        let raw_items = raw["extractedData"]["items"].as_array().cloned().unwrap_or_default();
        for (i, item) in report.items.iter().enumerate() {
            let label = normalize_text(&item.name);
            let target = rules
                .iter()
                .find(|r| normalize_text(r["text"].as_str().unwrap_or("")) == label)
                .and_then(|r| r["target"].as_str().map(str::to_owned))
                .or_else(|| {
                    let mut found: Vec<String> = rules
                        .iter()
                        .filter(|r| {
                            r["productId"].as_str().and_then(|id| products.get(id)).is_some_and(|p| {
                                ["name", "skuCode"].iter().filter_map(|k| p[*k].as_str()).any(|v| normalize_text(v) == label)
                            })
                        })
                        .filter_map(|r| r["target"].as_str().map(str::to_owned))
                        .collect();
                    found.dedup();
                    (found.len() == 1).then(|| found.remove(0))
                });
            let Some(key) = target else { continue };
            let Some(row) = template.row(&key) else { continue };
            let sign = if row.effect == "subtract" { -1 } else { 1 };
            let Some(amount) = item.amount.filter(|a| a.fract() == 0.0).map(|a| a as i64) else { continue };
            if sign * amount < 0 {
                continue;
            }
            add(&mut sales, &key, amount.abs());
            if row.quantity {
                match raw_items.get(i).and_then(|it| int(&it["quantity"])).filter(|q| *q >= 0) {
                    Some(q) => add(&mut quantities, &key, q),
                    None => missing_quantity.push(key.clone()),
                }
            }
        }
        let pay = &report.payment_totals;
        let (cash, credit, qr, ic, other) =
            (int(&pay["cash"]), int(&pay["credit_card"]), int(&pay["qr_code"]), int(&pay["ic_card"]), int(&pay["other"]));
        if let (Some(k), Some(c)) = (&credit_key, credit) {
            add(&mut sales, k, c);
        }
        if let Some(k) = &code_key {
            if qr.is_some() || ic.is_some() {
                add(&mut sales, k, qr.unwrap_or(0) + ic.unwrap_or(0));
            }
        }
        if cash == Some(total - credit.unwrap_or(0) - qr.unwrap_or(0) - ic.unwrap_or(0)) {
            for k in [&credit_key, &code_key].into_iter().flatten() {
                sales.entry(k.clone()).or_insert(None).get_or_insert(0);
            }
        } else {
            needs_review = true;
        }
        if other.unwrap_or(0) != 0
            || (credit_key.is_none() && credit.unwrap_or(0) != 0)
            || (code_key.is_none() && qr.unwrap_or(0) + ic.unwrap_or(0) != 0)
        {
            needs_review = true;
        }
    }
    for key in missing_quantity {
        quantities.insert(key, None);
    }
    let categorized: i64 = template
        .rows
        .iter()
        .filter(|r| r.source == "items")
        .map(|r| {
            let v = sales.get(&r.key).copied().flatten().unwrap_or(0);
            if r.effect == "subtract" { -v } else { v }
        })
        .sum();
    let remainder = net_sales - categorized;
    let remainder_amount = sales.get(&template.remainder).copied().flatten().unwrap_or(0) + remainder;
    if remainder_amount < 0 {
        return None;
    }
    if remainder != 0 {
        sales.insert(template.remainder.clone(), Some(remainder_amount));
        if quantities.contains_key(&template.remainder) {
            quantities.insert(template.remainder.clone(), None);
        }
    }
    for k in [&credit_key, &code_key].into_iter().flatten() {
        if sales.get(k).copied().flatten().is_none() {
            sales.insert(k.clone(), day.sales.get(k).copied().flatten());
        }
    }
    Some(SalesResult {
        customer_count,
        net_sales,
        payment_needs_review: needs_review,
        remainder,
        sales,
        quantities,
    })
}

/// 売上伝票として使えるか（その日のもの・読み取り済み・合計が整数）。
pub fn is_sales_report_for(report: &Receipt, date: &str) -> bool {
    report.is_sales() && report.status == "done" && report.sales_day() == date && report.total.is_some_and(|t| t.fract() == 0.0)
}

/// 売上台帳から、販売数を記録する項目ごとの販売数と金額。`(数, 額, 合わなかった商品名, 合った件数)`。
pub fn product_sales(
    day: &Day,
    template: &Template,
    entries: &[Value],
) -> Result<(BTreeMap<String, i64>, BTreeMap<String, i64>, Vec<String>, usize), String> {
    let rules = template.raw["rules"].as_array().cloned().unwrap_or_default();
    let rows: Vec<&Row> = template
        .rows
        .iter()
        .filter(|r| r.quantity && r.source == "items" && rules.iter().any(|x| x["target"] == r.key.as_str()))
        .collect();
    let mut quantities = BTreeMap::new();
    let mut amounts = BTreeMap::new();
    let mut unmatched: Vec<String> = Vec::new();
    let mut matched = 0;
    for entry in entries {
        if entry["date"].as_str() != Some(day.date.as_str()) || entry["status"] != "confirmed" {
            continue;
        }
        if !entry["receiptId"].as_str().is_some_and(|r| day.receipt_ids.iter().any(|x| x == r)) {
            continue;
        }
        let name = entry["productName"].as_str().unwrap_or("").to_owned();
        let rule = rules
            .iter()
            .find(|r| r["productId"].as_str().is_some() && r["productId"] == entry["productId"])
            .or_else(|| rules.iter().find(|r| normalize_text(r["text"].as_str().unwrap_or("")) == normalize_text(&name)));
        let Some(row) = rule.and_then(|r| rows.iter().find(|row| r["target"] == row.key.as_str())) else {
            if !unmatched.contains(&name) {
                unmatched.push(name);
            }
            continue;
        };
        let (Some(q), Some(a)) = (int(&entry["quantity"]), int(&entry["totalAmount"])) else {
            return Err("販売数または金額を集計できません。売上台帳の内容を確認してください。".into());
        };
        *quantities.entry(row.key.clone()).or_insert(0) += q;
        *amounts.entry(row.key.clone()).or_insert(0) += if row.effect == "subtract" { -a } else { a };
        matched += 1;
    }
    // 返品が先に並んでいても数えられるよう、負かどうかは全部足してから見る。
    if quantities.values().chain(amounts.values()).any(|v| *v < 0) {
        return Err("販売数または金額を集計できません。売上台帳の内容を確認してください。".into());
    }
    Ok((quantities, amounts, unmatched, matched))
}

/// 気象庁の予報地域。
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    pub code: String,
    pub office_code: String,
    pub name: String,
    pub prefecture: String,
}

pub fn areas() -> Vec<Area> {
    let list: Value = serde_json::from_str(include_str!("jma-weather-areas.json")).unwrap_or(Value::Null);
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            Some(Area {
                code: a["code"].as_str()?.to_owned(),
                office_code: a["officeCode"].as_str()?.to_owned(),
                name: a["name"].as_str().unwrap_or("").to_owned(),
                prefecture: a["prefecture"].as_str().unwrap_or("").to_owned(),
            })
        })
        .collect()
}

/// 気象庁の予報 JSON から、その日の天気（晴れ・曇り・雨・雪）と出どころ。
pub fn weather_from_forecast(forecast: &Value, area_code: &str, date: &str) -> Option<(String, Value)> {
    for report in forecast.as_array().into_iter().flatten() {
        let issued = report["reportDatetime"].as_str().unwrap_or("");
        for series in report["timeSeries"].as_array().into_iter().flatten() {
            let Some(i) = series["timeDefines"]
                .as_array()
                .and_then(|t| t.iter().position(|d| d.as_str().is_some_and(|s| s.starts_with(date))))
            else {
                continue;
            };
            let Some(area) = series["areas"].as_array().and_then(|a| a.iter().find(|x| x["area"]["code"] == area_code)) else {
                continue;
            };
            let (Some(code), Some(desc)) = (area["weatherCodes"][i].as_str(), area["weathers"][i].as_str()) else {
                continue;
            };
            let first = code.chars().next()?;
            if code.len() != 3 || !code.chars().all(|c| c.is_ascii_digit()) || !('1'..='4').contains(&first) {
                return None;
            }
            let weather = ["晴れ", "曇り", "雨", "雪"][(first as u8 - b'1') as usize];
            return Some((
                weather.to_owned(),
                json!({ "areaCode": area_code, "issuedAt": issued, "description": desc }),
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Value {
        json!({ "label": "本店 / レジ1", "template": default_template() })
    }

    #[test]
    fn default_template_is_valid() {
        assert_eq!(validate_template(&default_template()), Ok(()));
        let mut bad = default_template();
        bad["remainder"] = json!("payout");
        assert!(validate_template(&bad).is_err());
        let mut bad = default_template();
        bad["rows"][1]["effect"] = json!("add");
        assert!(validate_template(&bad).is_err());
    }

    #[test]
    fn amount_check_matches_web() {
        let mut day = Day::empty("2026-11-08", scope());
        day.opening.insert(1000, Some(10));
        day.closing.insert(1000, Some(15));
        day.closing.insert(100, Some(3));
        day.sales.insert("sales".into(), Some(8000));
        day.sales.insert("credit".into(), Some(2000));
        day.sales.insert("payout".into(), Some(500));
        let (closing, opening, increase, sales, over) = day.amount_check();
        assert_eq!((closing, opening, increase, sales, over), (15300, 10000, 5300, 5500, -200));
        assert_eq!(over_short_label(over), "200円 足りない");
        assert_eq!(over_short_label(0), "ぴったり");
        let back = Day::from_value("2026-11-08", &day.to_value());
        assert_eq!(back, day);
        assert_eq!(parse_count("１２枚"), Ok(Some(12)));
        assert_eq!(parse_count("-1"), Err(()));
    }

    #[test]
    fn sales_reports_fill_rows_and_remainder() {
        let mut t = default_template();
        t["rows"].as_array_mut().unwrap().insert(1, json!({ "key": "dog", "label": "ドッグラン", "effect": "add", "source": "items", "quantity": true }));
        t["rules"] = json!([{ "text": "ドッグラン 1日券", "target": "dog" }]);
        let template = Template::from_value(&t).unwrap();
        let mut day = Day::empty("2026-11-16", json!({ "label": "x", "template": t }));
        day.sales.insert("payout".into(), Some(300));
        let raw = json!({ "id": "r", "status": "done", "receiptType": "sales", "extractedData": {
            "date": "2026-11-16", "total": 5000, "customer_count": 7,
            "items": [{ "name": "ドッグラン 1日券", "amount": 2000, "quantity": 2 }, { "name": "ラテ", "amount": 1500 }],
            "payment_totals": { "cash": 3200, "credit_card": 1300, "qr_code": 500, "ic_card": 0, "other": 0 } } });
        let receipt = Receipt::from_value(&raw).unwrap();
        assert!(is_sales_report_for(&receipt, "2026-11-16"));
        let result = sales_from_reports(&day, &template, &[(receipt, raw)], &HashMap::new()).unwrap();
        assert_eq!(result.sales["dog"], Some(2000));
        assert_eq!(result.quantities["dog"], Some(2));
        assert_eq!(result.sales["sales"], Some(3000));
        assert_eq!(result.sales["credit"], Some(1300));
        assert_eq!(result.sales["codePayment"], Some(500));
        assert_eq!(result.sales["payout"], Some(300));
        assert_eq!(result.customer_count, Some(7));
        assert!(!result.payment_needs_review);
    }

    #[test]
    fn product_sales_accept_refunds_in_any_order() {
        let mut t = default_template();
        t["rows"][0]["quantity"] = json!(true);
        t["rules"] = json!([{ "text": "ラテ", "target": "sales" }]);
        let template = Template::from_value(&t).unwrap();
        let mut day = Day::empty("2026-11-16", json!({ "label": "x", "template": t }));
        day.receipt_ids = vec!["r".into()];
        let entry = |q: i64, a: i64| json!({ "date": "2026-11-16", "status": "confirmed", "receiptId": "r", "productName": "ラテ", "quantity": q, "totalAmount": a });
        let (q, a, _, matched) = product_sales(&day, &template, &[entry(-1, -500), entry(2, 1000)]).unwrap();
        assert_eq!((q["sales"], a["sales"], matched), (1, 500, 2));
        assert!(product_sales(&day, &template, &[entry(-1, -500)]).is_err());
    }

    #[test]
    fn weather_reads_the_first_digit() {
        let forecast = json!([{ "reportDatetime": "2026-11-16T05:00:00+09:00", "timeSeries": [{
            "timeDefines": ["2026-11-16T05:00:00+09:00", "2026-11-17T00:00:00+09:00"],
            "areas": [{ "area": { "code": "130010" }, "weatherCodes": ["101", "300"], "weathers": ["晴れ 時々 くもり", "雨"] }] }] }]);
        let (w, src) = weather_from_forecast(&forecast, "130010", "2026-11-17").unwrap();
        assert_eq!(w, "雨");
        assert_eq!(src["areaCode"], "130010");
        assert!(weather_from_forecast(&forecast, "130010", "2026-11-20").is_none());
        assert!(areas().iter().any(|a| a.code == "130010"));
    }
}
