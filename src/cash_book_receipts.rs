//! 現金出納帳にレシートを入れる決まり（Web 版の cash-book-model.ts / cash-book-category.ts /
//! cash-book-income.ts / receipt-schema.ts と同じ）。
//!
//! - 仕入のレシート → 支出の行（現金で払ったものだけ。科目は支払先の履歴とルールで決める）
//! - 締めレシート（売上の集計）→ 収入の各欄に足す（クレジット・コード決済は支払方法別の合計から）

use std::collections::HashMap;

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde_json::Value;

use crate::cash_book::{Day, Expense, Template};

/// 読み取った品目 1 つ。
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub name: String,
    pub amount: Option<f64>,
    pub product_id: Option<String>,
    pub cost_category: Option<String>,
}

/// レシート 1 枚（必要な所だけ）。
#[derive(Clone, Debug, PartialEq)]
pub struct Receipt {
    pub id: String,
    pub created_at: String,
    /// pending / processing / done / error / duplicate
    pub status: String,
    /// sales / purchase / unsorted
    pub receipt_type: String,
    pub payer_kind: Option<String>,
    pub error_message: Option<String>,
    pub file_name: Option<String>,
    pub counterparty: Option<String>,
    pub date: Option<String>,
    pub total: Option<f64>,
    pub payment_method: Option<String>,
    pub items: Vec<Item>,
    pub suggested_type: Option<String>,
    pub register_closed_at: Option<String>,
    pub receipt_number: Option<String>,
    pub payment_totals: Value,
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// 数（数字か、カンマ入りの文字）。読めなければ `None`。
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64().filter(|v| v.is_finite()),
        Value::String(s) => {
            let t: String = s.trim().chars().filter(|c| *c != ',').collect();
            let digits = t.strip_prefix('-').unwrap_or(&t);
            let mut parts = digits.split('.');
            let whole = parts.next().unwrap_or("");
            let frac = parts.next();
            let ok = !whole.is_empty()
                && whole.chars().all(|c| c.is_ascii_digit())
                && frac.is_none_or(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()))
                && parts.next().is_none();
            if ok { t.parse().ok() } else { None }
        }
        _ => None,
    }
}

fn first<'a>(data: &'a Value, keys: &[&str]) -> &'a Value {
    keys.iter()
        .map(|k| &data[*k])
        .find(|v| !v.is_null() && v.as_str().is_none_or(|s| !s.trim().is_empty()))
        .unwrap_or(&Value::Null)
}

impl Receipt {
    pub fn from_value(value: &Value) -> Option<Self> {
        let data = &value["extractedData"];
        let items = data["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item.is_object())
            .map(|item| Item {
                name: text(first(item, &["name", "item_name"])).unwrap_or_else(|| "名称未取得".into()),
                amount: number(first(item, &["amount", "total"])),
                product_id: text(&item["product_id"]),
                cost_category: text(&item["cost_category"]),
            })
            .collect();
        let suggested = text(first(data, &["suggested_receipt_type", "suggestedReceiptType"]))
            .filter(|s| s == "sales" || s == "purchase");
        Some(Self {
            id: value["id"].as_str()?.to_owned(),
            created_at: value["createdAt"].as_str().unwrap_or("").to_owned(),
            status: value["status"].as_str().unwrap_or("").to_owned(),
            receipt_type: value["receiptType"].as_str().unwrap_or("").to_owned(),
            payer_kind: text(&value["payerKind"]),
            error_message: text(&value["errorMessage"]),
            file_name: text(&value["fileName"]),
            counterparty: text(first(
                data,
                &["supplier_name", "supplierName", "store_name", "storeName", "merchant_name", "merchantName"],
            )),
            date: text(first(data, &["date", "transaction_date"])),
            total: number(first(data, &["total", "total_amount", "totalAmount"])),
            payment_method: text(first(data, &["payment_method", "paymentMethod"])),
            items,
            suggested_type: suggested,
            register_closed_at: text(&data["register_closed_at"]),
            receipt_number: text(&data["receipt_number"]),
            payment_totals: data["payment_totals"].clone(),
        })
    }

    /// 一覧に出す名前（支払先・日付・伝票番号）。
    pub fn title(&self) -> String {
        let parts: Vec<String> = [
            self.counterparty.clone(),
            self.date.clone(),
            self.receipt_number.as_ref().map(|n| format!("伝票番号 {n}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        if self.status == "done" && !parts.is_empty() {
            parts.join(" · ")
        } else {
            self.file_name.clone().unwrap_or_else(|| "証憑".into())
        }
    }

    pub fn is_purchase(&self) -> bool {
        self.receipt_type != "sales"
            && !(self.receipt_type == "unsorted" && self.suggested_type.as_deref() != Some("purchase"))
    }

    /// 店の現金で払ったもの（法人カード・立替・会社払いは除く。書いていなければ現金）。
    pub fn is_cash_purchase(&self) -> bool {
        self.is_purchase()
            && !matches!(
                self.payer_kind.as_deref(),
                Some("corporate_card" | "employee_advance" | "company_paid")
            )
    }

    pub fn is_advance(&self) -> bool {
        self.payer_kind.as_deref() == Some("employee_advance")
    }

    /// カード・コード決済で払ったと読めた（店の現金は減らない）。
    pub fn is_cashless(&self) -> bool {
        matches!(self.payment_method.as_deref(), Some("credit_card" | "qr_code" | "ic_card"))
    }

    /// 買った日。日付が読めていなければアップロードした日。
    pub fn day(&self) -> String {
        self.date
            .as_deref()
            .and_then(normalize_date)
            .unwrap_or_else(|| tokyo_date(&self.created_at))
    }

    pub fn is_sales(&self) -> bool {
        self.receipt_type == "sales"
            || (self.receipt_type == "unsorted" && self.suggested_type.as_deref() == Some("sales"))
    }

    /// レジの締めレシート（締めた時刻が読めたもの）。
    pub fn is_closing(&self) -> bool {
        self.register_closed_at.is_some()
    }

    pub fn sales_day(&self) -> String {
        self.date
            .as_deref()
            .and_then(normalize_date)
            .or_else(|| self.register_closed_at.as_deref().and_then(normalize_date))
            .unwrap_or_else(|| tokyo_date(&self.created_at))
    }
}

/// 「2026/11/6」「2026年11月6日」「2026-11-06T…」を YYYY-MM-DD に。
pub fn normalize_date(value: &str) -> Option<String> {
    let mut nums: Vec<u32> = Vec::new();
    let mut current = String::new();
    let mut gap = 0usize;
    for c in value.chars() {
        if c.is_ascii_digit() {
            if !current.is_empty() || gap <= 2 || nums.is_empty() {
                current.push(c);
            }
            gap = 0;
        } else {
            if !current.is_empty() {
                nums.push(current.parse().ok()?);
                current.clear();
                if nums.len() == 3 {
                    break;
                }
            }
            gap += 1;
            if !nums.is_empty() && gap > 2 {
                nums.clear();
            }
        }
    }
    if !current.is_empty() && nums.len() < 3 {
        nums.push(current.parse().ok()?);
    }
    let (y, m, d) = (*nums.first()?, *nums.get(1)?, *nums.get(2)?);
    if !(1000..=9999).contains(&y) {
        return None;
    }
    NaiveDate::from_ymd_opt(y as i32, m, d).map(|_| format!("{y:04}-{m:02}-{d:02}"))
}

/// 日本時間の日付。読めなければ先頭 10 文字。
pub fn tokyo_date(iso: &str) -> String {
    let tokyo = FixedOffset::east_opt(9 * 3600).expect("UTC+9");
    DateTime::parse_from_rfc3339(iso)
        .map(|t| t.with_timezone(&tokyo).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| iso.chars().take(10).collect())
}

fn day_number(date: &str) -> i64 {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| d.signed_duration_since(NaiveDate::default()).num_days())
        .unwrap_or(0)
}

/// その日に現金で買ったレシート（読み取り済み）と、まだ読み取り中の枚数。
pub fn receipts_for_day(receipts: &[Receipt], date: &str) -> (Vec<Receipt>, usize) {
    let mut ready = Vec::new();
    let mut reading = 0;
    for r in receipts.iter().filter(|r| r.is_cash_purchase()) {
        if r.status == "pending" || r.status == "processing" {
            if tokyo_date(&r.created_at) == date {
                reading += 1;
            }
        } else if r.status == "done" && r.day() == date {
            ready.push(r.clone());
        }
    }
    (ready, reading)
}

/// 買った日がこの日ではないレシート（立て替えの精算など）。近い日から。
pub fn receipts_for_other_days(receipts: &[Receipt], date: &str) -> Vec<Receipt> {
    let mut list: Vec<Receipt> = receipts
        .iter()
        .filter(|r| {
            r.status == "done" && (r.is_cash_purchase() || (r.is_purchase() && r.is_advance())) && r.day() != date
        })
        .cloned()
        .collect();
    let target = day_number(date);
    list.sort_by(|a, b| {
        let (da, db) = (a.day(), b.day());
        (day_number(&da) - target)
            .abs()
            .cmp(&(day_number(&db) - target).abs())
            .then_with(|| db.cmp(&da))
    });
    list
}

/// ほかの日の出納帳にもう入れてあるレシートと、その日。
pub fn receipts_used_on_other_days(days: &[Day], date: &str) -> HashMap<String, String> {
    let mut used = HashMap::new();
    for day in days.iter().filter(|d| d.date != date) {
        for row in &day.expenses {
            if let Some(id) = &row.receipt_id {
                used.insert(id.clone(), day.date.clone());
            }
        }
    }
    used
}

/// 文字をそろえる（全角・半角、空白、大文字小文字）。Web 版の normalizeCashBookText。
pub fn normalize_text(value: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    value
        .nfkc()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// 支払先ごとに前に選んだ科目（新しい日が勝つ）。
pub fn payee_history(days: &[Day]) -> HashMap<String, String> {
    let mut sorted: Vec<&Day> = days.iter().collect();
    sorted.sort_by(|a, b| a.date.cmp(&b.date));
    let mut history = HashMap::new();
    for day in sorted {
        for row in &day.expenses {
            let key = normalize_text(&row.payee);
            if !key.is_empty() {
                history.insert(key, row.category.clone());
            }
        }
    }
    history
}

/// レシートを支出の行にする。
pub fn expense_from_receipt(receipt: &Receipt, category: String) -> Expense {
    let names: Vec<&str> = receipt
        .items
        .iter()
        .map(|i| i.name.as_str())
        .filter(|n| *n != "名称未取得")
        .take(3)
        .collect();
    let mut description = names.join("、");
    if receipt.items.len() > 3 {
        description.push_str(" ほか");
    }
    Expense {
        id: format!("receipt:{}", receipt.id),
        amount: receipt.total.map(|t| t.round() as i64),
        category,
        description,
        payee: receipt.counterparty.clone().unwrap_or_default(),
        purchased_on: Some(receipt.day()),
        receipt_id: Some(receipt.id.clone()),
    }
}

/// OCR 品目分類ごとの振り分けで、金額が最も大きい先（同じなら先のもの）。
fn weighted_cost_category(receipt: &Receipt, map: &[(String, String)]) -> Option<String> {
    let mut weights: Vec<(String, f64)> = Vec::new();
    for item in &receipt.items {
        let Some(category) = &item.cost_category else { continue };
        let Some((_, target)) = map.iter().find(|(text, _)| text == category) else { continue };
        let weight = item.amount.filter(|a| *a > 0.0).unwrap_or(1.0);
        match weights.iter_mut().find(|(t, _)| t == target) {
            Some(entry) => entry.1 += weight,
            None => weights.push((target.clone(), weight)),
        }
    }
    let mut best: Option<(String, f64)> = None;
    for (target, weight) in weights {
        if best.as_ref().is_none_or(|(_, w)| weight > *w) {
            best = Some((target, weight));
        }
    }
    best.map(|(t, _)| t)
}

fn contains_any(text: &str, words: &[&str]) -> bool {
    let lower = text.to_lowercase();
    words.iter().any(|w| lower.contains(&w.to_lowercase()))
}

/// レシートの科目。支払先の履歴 → 設定のルール → 従来の決まり（Web 版の categoryForReceipt）。
pub fn category_for_receipt(
    receipt: &Receipt,
    payee: &str,
    history: &HashMap<String, String>,
    template: &Template,
) -> String {
    let has = |key: &str| template.expenses.iter().any(|row| row.key == key);
    let raw = &template.raw;
    let has_template = !raw.is_null();
    if let Some(found) = history.get(&normalize_text(payee)) {
        if !has_template || has(found) {
            return found.clone();
        }
    }
    let preset = raw["preset"].as_str().unwrap_or("banasuku");
    if has_template {
        let names: Vec<&str> = receipt.items.iter().map(|i| i.name.as_str()).collect();
        let joined = normalize_text(&format!("{payee} {}", names.join(" ")));
        let payee_norm = normalize_text(payee);
        let rules = raw["expenseRules"].as_array().cloned().unwrap_or_default();
        let mut i = 0;
        while i < rules.len() {
            let rule = &rules[i];
            let target = rule["target"].as_str().unwrap_or("").to_owned();
            let rule_text = rule["text"].as_str().unwrap_or("");
            match rule["source"].as_str() {
                Some("costCategory") => {
                    let mut map: Vec<(String, String)> = Vec::new();
                    while i < rules.len() && rules[i]["source"] == "costCategory" {
                        let text = rules[i]["text"].as_str().unwrap_or("").to_owned();
                        if !map.iter().any(|(t, _)| *t == text) {
                            map.push((text, rules[i]["target"].as_str().unwrap_or("").to_owned()));
                        }
                        i += 1;
                    }
                    if let Some(found) = weighted_cost_category(receipt, &map) {
                        return found;
                    }
                    continue;
                }
                Some("payee") => {
                    if !rule_text.is_empty() && payee_norm.contains(&normalize_text(rule_text)) {
                        return target;
                    }
                }
                _ => {
                    if !rule_text.is_empty() && joined.contains(&normalize_text(rule_text)) {
                        return target;
                    }
                }
            }
            i += 1;
        }
        if preset != "banasuku" {
            return template.default_expense.clone();
        }
    }
    let available = |key: &str| {
        if has_template && !has(key) { template.default_expense.clone() } else { key.to_owned() }
    };
    let names: Vec<&str> = receipt.items.iter().map(|i| i.name.as_str()).collect();
    let books = ["新聞", "書店", "書籍", "雑誌", "ブックス", "本屋", "book"];
    if contains_any(payee, &books) || contains_any(&names.join(" "), &books) {
        return available("books");
    }
    let defaults: Vec<(String, String)> = [
        ("consumable", "equipment"),
        ("equipment", "equipment"),
        ("other", "misc"),
        ("packaging", "cafeSupplies"),
        ("raw_material", "cafeSupplies"),
        ("shipping", "misc"),
    ]
    .iter()
    .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
    .collect();
    if let Some(found) = weighted_cost_category(receipt, &defaults) {
        return available(&found);
    }
    let shops = [
        "ダイソー", "セリア", "キャンドゥ", "ホームセンター", "カインズ", "コーナン", "コメリ", "ニトリ", "daiso",
        "seria", "cainz",
    ];
    if contains_any(payee, &shops) {
        return available("equipment");
    }
    available("cafeSupplies")
}

/// 締めレシートの品目を入れる収入の欄。
pub fn income_key_for_item(name: &str, template: &Template, products: &HashMap<String, Value>) -> String {
    let raw = &template.raw;
    let has_template = !raw.is_null();
    let default_income = raw["defaultIncome"].as_str().unwrap_or("cafeSales").to_owned();
    if has_template {
        let n = normalize_text(name);
        for rule in raw["incomeRules"].as_array().into_iter().flatten() {
            let target = rule["target"].as_str().unwrap_or("");
            let hit = match rule["productId"].as_str() {
                Some(id) => products.get(id).is_some_and(|p| {
                    ["name", "skuCode", "publicationName"]
                        .iter()
                        .filter_map(|k| p[*k].as_str())
                        .any(|v| normalize_text(v) == n)
                }),
                None => {
                    let t = normalize_text(rule["text"].as_str().unwrap_or(""));
                    !t.is_empty() && n.contains(&t)
                }
            };
            if hit {
                return target.to_owned();
            }
        }
        if raw["preset"].as_str().unwrap_or("banasuku") != "banasuku" {
            return default_income;
        }
    }
    let legacy = if contains_any(name, &["ドッグラン", "ドックラン", "dog run", "dogrun", "入場", "入園"]) {
        "dogRunSales"
    } else if contains_any(name, &["チケット", "回数券", "割引券", "ticket"]) {
        "discountTickets"
    } else if contains_any(name, &["物販", "グッズ", "goods", "ペット用品", "首輪", "リード", "おもちゃ"]) {
        "merchandise"
    } else {
        "cafeSales"
    };
    if has_template && !template.incomes.iter().any(|row| row.key == legacy) {
        default_income
    } else {
        legacy.to_owned()
    }
}

/// 締めレシートから収入に足す額と、その内訳。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SalesIncome {
    pub incomes: Vec<(String, i64)>,
    /// (品名, 入れた欄, 額)
    pub lines: Vec<(String, String, i64)>,
    /// 支払方法別の合計が読めなかったレシートがある。
    pub missing_payment_totals: bool,
}

impl SalesIncome {
    fn add(&mut self, key: &str, amount: i64) {
        match self.incomes.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 += amount,
            None => self.incomes.push((key.to_owned(), amount)),
        }
    }
}

pub fn income_from_sales(
    receipts: &[Receipt],
    template: &Template,
    products: &HashMap<String, Value>,
) -> SalesIncome {
    let mut result = SalesIncome::default();
    let has_template = !template.raw.is_null();
    let has_income = |key: &str| !has_template || template.incomes.iter().any(|row| row.key == key);
    let default_income = template.raw["defaultIncome"].as_str().unwrap_or("cafeSales").to_owned();
    for receipt in receipts {
        let mut item_sum = 0i64;
        for item in &receipt.items {
            let Some(value) = item.amount.map(|a| a.round() as i64).filter(|v| *v != 0) else { continue };
            let key = income_key_for_item(&item.name, template, products);
            result.add(&key, value);
            result.lines.push((item.name.clone(), key, value));
            item_sum += value;
        }
        if let Some(total) = receipt.total.map(|t| t.round() as i64) {
            if total != item_sum {
                let rest = total - item_sum;
                result.add(&default_income, rest);
                let label = if item_sum == 0 { "売上合計" } else { "品目に出ていない分（合計との差）" };
                result.lines.push((label.into(), default_income.clone(), rest));
            }
        }
        let totals = &receipt.payment_totals;
        let credit = number(&totals["credit_card"]).map(|v| v.round() as i64);
        let code: Vec<i64> = [&totals["qr_code"], &totals["ic_card"]]
            .iter()
            .filter_map(|v| number(v))
            .map(|v| v.round() as i64)
            .collect();
        if credit.is_none() && code.is_empty() {
            result.missing_payment_totals = true;
            continue;
        }
        if has_income("credit") {
            result.add("credit", credit.unwrap_or(0));
        }
        if has_income("codePayment") {
            result.add("codePayment", code.iter().sum());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cash_book::{Template, legacy_template};
    use serde_json::json;

    fn receipt(value: Value) -> Receipt {
        Receipt::from_value(&value).unwrap()
    }

    #[test]
    fn reads_receipts_and_dates() {
        let r = receipt(json!({
            "id": "r1", "createdAt": "2026-11-14T16:00:00Z", "status": "done", "receiptType": "purchase",
            "extractedData": { "store_name": " ダイソー ", "total": "1,100", "date": "2026年11月6日",
                "items": [{ "name": "皿", "amount": 600, "cost_category": "consumable" }, { "amount": 500 }] }
        }));
        assert_eq!(r.counterparty.as_deref(), Some("ダイソー"));
        assert_eq!(r.total, Some(1100.0));
        assert_eq!(r.day(), "2026-11-06");
        assert_eq!(tokyo_date(&r.created_at), "2026-11-15");
        let row = expense_from_receipt(&r, "misc".into());
        assert_eq!(row.amount, Some(1100));
        assert_eq!(row.description, "皿");
        assert_eq!(row.id, "receipt:r1");
        assert_eq!(normalize_date("2026/1/5 10:00"), Some("2026-01-05".into()));
        assert_eq!(normalize_date("2026-11-06T09:00:00+09:00"), Some("2026-11-06".into()));
        assert_eq!(normalize_date("abc"), None);
    }

    #[test]
    fn picks_cash_purchases_for_the_day() {
        let base = |id: &str, payer: Value, kind: &str, status: &str| {
            receipt(json!({ "id": id, "createdAt": "2026-11-06T01:00:00Z", "status": status, "receiptType": kind,
                "payerKind": payer, "extractedData": { "date": "2026-11-06", "total": 100 } }))
        };
        let list = vec![
            base("cash", Value::Null, "purchase", "done"),
            base("card", json!("corporate_card"), "purchase", "done"),
            base("sales", Value::Null, "sales", "done"),
            base("reading", Value::Null, "purchase", "processing"),
            base("unsorted", Value::Null, "unsorted", "done"),
        ];
        let (ready, reading) = receipts_for_day(&list, "2026-11-06");
        assert_eq!(ready.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["cash"]);
        assert_eq!(reading, 1);
        let advance = base("adv", json!("employee_advance"), "purchase", "done");
        let other = receipts_for_other_days(&[advance], "2026-11-10");
        assert_eq!(other.len(), 1);
    }

    #[test]
    fn categories_follow_history_then_rules_then_legacy() {
        let r = receipt(json!({ "id": "r", "status": "done", "receiptType": "purchase",
            "extractedData": { "store_name": "セリア", "items": [{ "name": "紙コップ", "amount": 300, "cost_category": "packaging" }] } }));
        let legacy = legacy_template();
        let mut history = HashMap::new();
        assert_eq!(category_for_receipt(&r, "セリア", &history, &legacy), "cafeSupplies");
        history.insert(normalize_text("セリア"), "misc".into());
        assert_eq!(category_for_receipt(&r, "セリア", &history, &legacy), "misc");
        let generic = Template::from_value(&json!({
            "name": "x", "preset": "generic",
            "incomes": [{ "key": "sales", "label": "売上", "deduct": false }],
            "expenses": [{ "key": "purchases", "label": "仕入", "slots": 7 }, { "key": "misc", "label": "雑費", "slots": 4 }],
            "defaultExpense": "purchases",
            "expenseRules": [{ "text": "ｾﾘｱ", "target": "misc", "source": "payee" }],
        }))
        .unwrap();
        assert_eq!(category_for_receipt(&r, "セリア", &HashMap::new(), &generic), "misc");
        assert_eq!(category_for_receipt(&r, "ローソン", &HashMap::new(), &generic), "purchases");
        let rules = Template::from_value(&json!({
            "name": "x", "preset": "generic",
            "incomes": [{ "key": "sales", "label": "売上", "deduct": false }],
            "expenses": [{ "key": "purchases", "label": "仕入", "slots": 7 }, { "key": "misc", "label": "雑費", "slots": 4 }],
            "defaultExpense": "purchases",
            "expenseRules": [{ "text": "packaging", "target": "misc", "source": "costCategory" }],
        }))
        .unwrap();
        assert_eq!(category_for_receipt(&r, "セリア", &HashMap::new(), &rules), "misc");
    }

    #[test]
    fn sales_receipts_split_into_incomes() {
        let r = receipt(json!({ "id": "s", "status": "done", "receiptType": "sales",
            "extractedData": { "total": 5000, "register_closed_at": "2026-11-06T21:00",
                "items": [{ "name": "ドッグラン 1日", "amount": 2000 }, { "name": "ラテ", "amount": 1500 }],
                "payment_totals": { "credit_card": 1000, "qr_code": 300, "ic_card": "200" } } }));
        assert!(r.is_sales() && r.is_closing());
        assert_eq!(r.sales_day(), "2026-11-06");
        let result = income_from_sales(&[r], &legacy_template(), &HashMap::new());
        let get = |k: &str| result.incomes.iter().find(|(key, _)| key == k).map(|(_, v)| *v);
        assert_eq!(get("dogRunSales"), Some(2000));
        assert_eq!(get("cafeSales"), Some(3000));
        assert_eq!(get("credit"), Some(1000));
        assert_eq!(get("codePayment"), Some(500));
        assert!(!result.missing_payment_totals);
    }
}
