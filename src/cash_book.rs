//! 現金出納帳（1 日 1 件）。Web 版（apps/client/src/features/cash-book）と同じ形で読み書きする。
//!
//! サーバーは中身を読まずに JSON のまま置くので、Web 版と同じ項目名・同じ意味で保存する。
//! 金額は円の整数。未入力は `None` にして、0 と「書いていない」を分ける。
//! この画面が知らない項目は `extra` に残し、保存のときにそのまま返す。

use std::collections::{BTreeMap, HashMap};

use chrono::{Duration, NaiveDate};
use serde_json::{Map, Value, json};

#[derive(Clone, Debug, PartialEq)]
pub struct IncomeRow {
    pub key: String,
    pub label: String,
    /// 現金が入ってこない売上（△）。合計から引く。
    pub deduct: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExpenseCategory {
    pub key: String,
    pub label: String,
    /// 紙に用意してある行数。
    pub slots: usize,
    /// 紙に最初から書いてある取引内容。
    pub fixed_description: Option<String>,
}

/// 帳簿の設定のうち、記入に使う部分。元の JSON は `raw` に持ち、その日に写して保存する。
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub name: String,
    pub incomes: Vec<IncomeRow>,
    pub expenses: Vec<ExpenseCategory>,
    pub default_expense: String,
    pub raw: Value,
}

/// 帳簿の設定が無い（従来の帳簿の）日に使う項目。Web 版の INCOME_ROWS / EXPENSE_CATEGORIES。
pub fn legacy_template() -> Template {
    let incomes = [
        (false, "dogRunSales", "ドッグラン売上"),
        (false, "cafeSales", "カフェ売上"),
        (false, "discountTickets", "割引チケット"),
        (true, "credit", "クレジット（△）"),
        (true, "codePayment", "コード決済（△）"),
        (false, "merchandise", "物販"),
        (false, "bankWithdrawal", "預金引出"),
    ];
    let expenses = [
        ("cafeSupplies", "カフェ材料仕入", 7, None),
        ("equipment", "備品消耗品費", 3, None),
        ("misc", "雑費", 4, None),
        ("books", "新聞図書費", 2, None),
        ("bankDeposit", "普通預金", 1, Some("現金預金")),
        ("pettyCash", "小口現金", 1, Some("コンテンツへの現金振替")),
    ];
    Template {
        name: "従来の現金出納帳".into(),
        incomes: incomes
            .iter()
            .map(|(deduct, key, label)| IncomeRow {
                key: (*key).into(),
                label: (*label).into(),
                deduct: *deduct,
            })
            .collect(),
        expenses: expenses
            .iter()
            .map(|(key, label, slots, fixed)| ExpenseCategory {
                key: (*key).into(),
                label: (*label).into(),
                slots: *slots,
                fixed_description: fixed.map(str::to_owned),
            })
            .collect(),
        default_expense: "cafeSupplies".into(),
        raw: Value::Null,
    }
}

impl Template {
    /// 設定の JSON を読む。記入に要る形が欠けていれば `None`（その帳簿は使わない）。
    pub fn from_value(value: &Value) -> Option<Self> {
        let incomes: Vec<IncomeRow> = value["incomes"]
            .as_array()?
            .iter()
            .map(|row| {
                Some(IncomeRow {
                    key: row["key"].as_str()?.to_owned(),
                    label: row["label"].as_str()?.to_owned(),
                    deduct: row["deduct"].as_bool().unwrap_or(false),
                })
            })
            .collect::<Option<_>>()?;
        let expenses: Vec<ExpenseCategory> = value["expenses"]
            .as_array()?
            .iter()
            .map(|row| {
                Some(ExpenseCategory {
                    key: row["key"].as_str()?.to_owned(),
                    label: row["label"].as_str()?.to_owned(),
                    slots: row["slots"].as_u64().unwrap_or(1) as usize,
                    fixed_description: row["fixedDescription"].as_str().map(str::to_owned),
                })
            })
            .collect::<Option<_>>()?;
        if incomes.is_empty() || expenses.is_empty() {
            return None;
        }
        let default_expense = value["defaultExpense"]
            .as_str()
            .filter(|key| expenses.iter().any(|row| row.key == *key))
            .unwrap_or(&expenses[0].key)
            .to_owned();
        Some(Self {
            name: value["name"].as_str().unwrap_or("").to_owned(),
            incomes,
            expenses,
            default_expense,
            raw: value.clone(),
        })
    }

    pub fn category_label(&self, key: &str) -> String {
        self.expenses
            .iter()
            .find(|row| row.key == key)
            .map_or_else(|| key.to_owned(), |row| row.label.clone())
    }
}

/// 店舗・レジごとの帳簿（帳簿設定の 1 件）。
#[derive(Clone, Debug, PartialEq)]
pub struct Book {
    pub id: String,
    pub name: String,
    pub store_name: String,
    pub register_name: String,
    pub template: Template,
}

impl Book {
    pub fn label(&self) -> String {
        format!("{} / {}", self.store_name, self.register_name)
    }
}

/// 帳簿設定の応答（`{ data: { books }, version }`）から、使える帳簿を並び順のまま取り出す。
/// 1 件目の ID も返す（従来の帳簿の日を引き継ぐのは、設定の先頭の帳簿）。
pub fn books_from_settings(response: &Value) -> (Vec<Book>, Option<String>) {
    let list = response["data"]["books"].as_array().cloned().unwrap_or_default();
    let first = list
        .first()
        .and_then(|book| book["id"].as_str())
        .map(str::to_owned);
    let books = list
        .iter()
        .filter_map(|book| {
            Some(Book {
                id: book["id"].as_str()?.to_owned(),
                name: book["name"].as_str().unwrap_or("").to_owned(),
                store_name: book["storeName"].as_str().unwrap_or("").to_owned(),
                register_name: book["registerName"].as_str().unwrap_or("").to_owned(),
                template: Template::from_value(&book["template"])?,
            })
        })
        .collect();
    (books, first)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expense {
    pub id: String,
    pub amount: Option<i64>,
    pub category: String,
    pub description: String,
    pub payee: String,
    /// 取り込んだレシートの日付（買った日）。手で足した行は `None`。
    pub purchased_on: Option<String>,
    /// 取り込んだレシート。同じレシートを二度取り込まないために持つ。
    pub receipt_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    pub date: String,
    /// その日に使った帳簿の設定。従来の日には無い。
    pub template: Option<Value>,
    pub expenses: Vec<Expense>,
    pub incomes: BTreeMap<String, Option<i64>>,
    /// ① 前日現金残高。
    pub previous_balance: Option<i64>,
    /// ① を手で直したか。直していない日は、前の日の本日現金残高に合わせ続ける。
    pub previous_balance_manual: bool,
    pub sales_receipt_ids: Vec<String>,
    /// この日を確定した日時。確定した日は、取り消すまで変えられない。
    pub confirmed_at: Option<String>,
    /// この画面が知らない項目。保存のときにそのまま返す。
    pub extra: Map<String, Value>,
}

const KNOWN: [&str; 7] = [
    "template",
    "expenses",
    "incomes",
    "previousBalance",
    "previousBalanceManual",
    "salesReceiptIds",
    "confirmedAt",
];

fn yen(value: &Value) -> Option<i64> {
    value.as_f64().filter(|v| v.is_finite()).map(|v| v.round() as i64)
}

impl Day {
    pub fn empty(date: &str, template: Option<&Template>) -> Self {
        let rows = template.map_or_else(|| legacy_template().incomes, |t| t.incomes.clone());
        Self {
            date: date.to_owned(),
            template: template.map(|t| t.raw.clone()).filter(|raw| !raw.is_null()),
            expenses: Vec::new(),
            incomes: rows.into_iter().map(|row| (row.key, None)).collect(),
            previous_balance: None,
            previous_balance_manual: false,
            sales_receipt_ids: Vec::new(),
            confirmed_at: None,
            extra: Map::new(),
        }
    }

    /// 保存された JSON を読む。足りない欄は空にする（Web 版の `cashBookDayFromData`）。
    pub fn from_value(date: &str, data: &Value) -> Self {
        let template = data
            .get("template")
            .filter(|value| !value.is_null())
            .cloned();
        let parsed = template.as_ref().and_then(Template::from_value);
        let mut day = Self::empty(date, None);
        let rows = parsed.as_ref().map_or_else(|| legacy_template().incomes, |t| t.incomes.clone());
        day.template = template;
        day.incomes = rows
            .into_iter()
            .map(|row| {
                let value = yen(&data["incomes"][&row.key]);
                (row.key, value)
            })
            .collect();
        day.expenses = data["expenses"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| {
                Some(Expense {
                    id: row["id"].as_str()?.to_owned(),
                    amount: yen(&row["amount"]),
                    category: row["category"].as_str().unwrap_or("cafeSupplies").to_owned(),
                    description: row["description"].as_str().unwrap_or("").to_owned(),
                    payee: row["payee"].as_str().unwrap_or("").to_owned(),
                    purchased_on: row["purchasedOn"].as_str().map(str::to_owned),
                    receipt_id: row["receiptId"].as_str().map(str::to_owned),
                })
            })
            .collect();
        day.previous_balance = yen(&data["previousBalance"]);
        day.previous_balance_manual = data["previousBalanceManual"] == Value::Bool(true);
        day.confirmed_at = data["confirmedAt"].as_str().map(str::to_owned);
        day.sales_receipt_ids = data["salesReceiptIds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| id.as_str().map(str::to_owned))
            .collect();
        if let Some(object) = data.as_object() {
            day.extra = object
                .iter()
                .filter(|(key, _)| !KNOWN.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
        }
        day
    }

    /// 保存する JSON（日付は URL に入るので含めない）。
    pub fn to_value(&self) -> Value {
        let mut object = self.extra.clone();
        if let Some(template) = &self.template {
            object.insert("template".into(), template.clone());
        }
        object.insert(
            "expenses".into(),
            Value::Array(
                self.expenses
                    .iter()
                    .map(|row| {
                        json!({
                            "amount": row.amount,
                            "category": row.category,
                            "description": row.description,
                            "id": row.id,
                            "payee": row.payee,
                            "purchasedOn": row.purchased_on,
                            "receiptId": row.receipt_id,
                        })
                    })
                    .collect(),
            ),
        );
        object.insert(
            "incomes".into(),
            Value::Object(
                self.incomes
                    .iter()
                    .map(|(key, value)| (key.clone(), json!(value)))
                    .collect(),
            ),
        );
        object.insert("previousBalance".into(), json!(self.previous_balance));
        object.insert("previousBalanceManual".into(), json!(self.previous_balance_manual));
        object.insert("salesReceiptIds".into(), json!(self.sales_receipt_ids));
        object.insert("confirmedAt".into(), json!(self.confirmed_at));
        Value::Object(object)
    }

    /// この日に使う項目。保存された設定が読めなければ、帳簿の設定、無ければ従来の項目。
    pub fn template_or(&self, fallback: Option<&Template>) -> Template {
        self.template
            .as_ref()
            .and_then(Template::from_value)
            .or_else(|| fallback.cloned())
            .unwrap_or_else(legacy_template)
    }

    /// ② 現金収入合計。△ の行は引く。
    pub fn cash_income_total(&self) -> i64 {
        let template = self.template_or(None);
        template.incomes.iter().fold(0, |sum, row| {
            let value = self.incomes.get(&row.key).copied().flatten().unwrap_or(0);
            if row.deduct { sum - value } else { sum + value }
        })
    }

    /// ③ 支出合計。
    pub fn expense_total(&self) -> i64 {
        self.expenses.iter().filter_map(|row| row.amount).sum()
    }

    /// 本日現金残高（① + ② − ③）。
    pub fn closing_balance(&self) -> i64 {
        self.previous_balance.unwrap_or(0) + self.cash_income_total() - self.expense_total()
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmed_at.is_some()
    }
}

/// ① を手で直していない、確定していない日なら、前の日の本日現金残高に合わせる。
pub fn follow_carried_balance(day: &Day, carried: Option<i64>) -> Day {
    let mut next = day.clone();
    if let Some(carried) = carried {
        if !day.previous_balance_manual && day.confirmed_at.is_none() {
            next.previous_balance = Some(carried);
        }
    }
    next
}

/// 保存してある日を古い順に並べ、① を手で直していない日は前の日の本日現金残高に合わせ直す。
/// 確定した日と、手で直した日と、一番古い日はそのまま。
pub fn with_carried_balances(days: &[Day]) -> Vec<Day> {
    let mut sorted = days.to_vec();
    sorted.sort_by(|a, b| a.date.cmp(&b.date));
    let mut result: Vec<Day> = Vec::with_capacity(sorted.len());
    for day in sorted {
        let next = match result.last() {
            Some(previous) if !day.previous_balance_manual && day.confirmed_at.is_none() => {
                let mut day = day;
                day.previous_balance = Some(previous.closing_balance());
                day
            }
            _ => day,
        };
        result.push(next);
    }
    result
}

/// ① に入れる金額。その日より前で一番近い保存済みの日の本日現金残高。
pub fn carried_over_balance(date: &str, days: &[Day]) -> Option<i64> {
    days.iter()
        .filter(|day| day.date.as_str() < date)
        .max_by(|a, b| a.date.cmp(&b.date))
        .map(Day::closing_balance)
}

/// まとめる行の印。レシートから取り込んだ行と、支払先が空の行はまとめない。
fn group_key(row: &Expense, date: &str) -> Option<String> {
    if row.receipt_id.is_some() {
        return None;
    }
    let payee = row.payee.trim();
    if payee.is_empty() {
        return None;
    }
    let other_day = row
        .purchased_on
        .as_deref()
        .filter(|day| *day != date)
        .unwrap_or("");
    Some(format!("{}\u{0}{payee}\u{0}{other_day}", row.category))
}

/// 紙に書く行の数（手で足した同じ科目・同じ支払先の行は 1 行）。金額のある行だけ数える。
fn grouped(expenses: &[Expense], date: &str) -> Vec<(String, String, bool)> {
    // (最初の行の id, 科目, 金額があるか)
    let mut groups: Vec<(String, String, bool)> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for row in expenses {
        match group_key(row, date).and_then(|key| by_key.get(&key).copied().map(|i| (key, i))) {
            Some((_, index)) => groups[index].2 |= row.amount.is_some(),
            None => {
                if let Some(key) = group_key(row, date) {
                    by_key.insert(key, groups.len());
                }
                groups.push((row.id.clone(), row.category.clone(), row.amount.is_some()));
            }
        }
    }
    groups
}

/// その日に使う証番の数。
pub fn voucher_count(day: &Day) -> usize {
    grouped(&day.expenses, &day.date)
        .iter()
        .filter(|(_, _, has_amount)| *has_amount)
        .count()
}

/// その日の最初の証番。同じ月の前の日までに使った数の次。
pub fn first_voucher_number(date: &str, days: &[Day]) -> usize {
    let month = &date[..date.len().min(7)];
    1 + days
        .iter()
        .filter(|day| day.date.as_str() < date && day.date.starts_with(month))
        .map(voucher_count)
        .sum::<usize>()
}

/// 行ごとの証番（紙の科目の順、その中は上から）。まとめた行は同じ番号。金額の無い行には振らない。
pub fn voucher_numbers(day: &Day, start: usize, template: &Template) -> HashMap<String, usize> {
    let mut first_of: HashMap<String, String> = HashMap::new();
    let mut first_by_key: HashMap<String, String> = HashMap::new();
    for row in &day.expenses {
        let key = group_key(row, &day.date).unwrap_or_else(|| row.id.clone());
        let first = first_by_key.entry(key).or_insert_with(|| row.id.clone()).clone();
        first_of.insert(row.id.clone(), first);
    }
    let groups = grouped(&day.expenses, &day.date);
    let mut numbers: HashMap<String, usize> = HashMap::new();
    let mut next = start;
    for category in &template.expenses {
        for (id, row_category, has_amount) in &groups {
            if *row_category != category.key || !has_amount {
                continue;
            }
            numbers.insert(id.clone(), next);
            next += 1;
        }
    }
    // 設定に無い科目の行も、最後に番号を振る（紙からこぼれないように）。
    for (id, row_category, has_amount) in &groups {
        if *has_amount && !template.expenses.iter().any(|c| c.key == *row_category) {
            numbers.insert(id.clone(), next);
            next += 1;
        }
    }
    first_of
        .iter()
        .filter_map(|(id, first)| numbers.get(first).map(|n| (id.clone(), *n)))
        .collect()
}

/// 入力欄の文字を円の整数にする。「1,200」「１２００」「¥1200」も読む。空なら `Ok(None)`。
pub fn parse_yen(text: &str) -> Result<Option<i64>, ()> {
    let normalized: String = text
        .chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '−' | 'ー' | '－' => '-',
            _ => c,
        })
        .filter(|c| !matches!(c, ',' | '，' | '円' | '¥' | '￥') && !c.is_whitespace())
        .collect();
    if normalized.is_empty() {
        return Ok(None);
    }
    let digits = normalized.strip_prefix('-').unwrap_or(&normalized);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(());
    }
    normalized.parse::<i64>().map(Some).map_err(|_| ())
}

/// 円を 3 桁区切りで。
pub fn format_yen(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if value < 0 { format!("-{out}") } else { out }
}

pub fn format_optional_yen(value: Option<i64>) -> String {
    value.map(format_yen).unwrap_or_default()
}

/// 何日かずらした YYYY-MM-DD。読めない日付はそのまま返す。
pub fn shift_date(date: &str, days: i64) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| (d + Duration::days(days)).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| date.to_owned())
}

/// 「2026年11月15日（日）」。
pub fn date_label(date: &str) -> String {
    use chrono::Datelike;
    let Ok(d) = NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
        return date.to_owned();
    };
    let weekday = ["月", "火", "水", "木", "金", "土", "日"][d.weekday().num_days_from_monday() as usize];
    format!("{}年{}月{}日（{weekday}）", d.year(), d.month(), d.day())
}

/// 日本時間の今日。
pub fn tokyo_today() -> String {
    let tokyo = chrono::FixedOffset::east_opt(9 * 3600).expect("UTC+9");
    utc_now().with_timezone(&tokyo).format("%Y-%m-%d").to_string()
}

/// 今の時刻（ISO 8601、UTC）。確定した日時に使う。
pub fn now_iso() -> String {
    utc_now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// chrono の clock 機能は入れていないので、端末の時計から作る。
fn utc_now() -> chrono::DateTime<chrono::Utc> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    chrono::DateTime::from_timestamp(elapsed.as_secs() as i64, elapsed.subsec_nanos())
        .unwrap_or_default()
}

/// 新しい支出の行の ID。
pub fn new_expense_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    format!("m{:x}{:x}", nanos, NEXT.fetch_add(1, Ordering::Relaxed))
}

/// 保存してある 1 日（版つき）。
#[derive(Clone, Debug, PartialEq)]
pub struct SavedDay {
    pub day: Day,
    pub version: i64,
    /// 従来の帳簿（店舗・レジを分ける前）に置いてある日か。保存も同じ所へする。
    pub legacy: bool,
}

/// 日の一覧の応答（`{ items: [{ date, version, data }] }`）を読む。
pub fn saved_days(response: &Value, legacy: bool) -> Vec<SavedDay> {
    response["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let date = item["date"].as_str()?;
            Some(SavedDay {
                day: Day::from_value(date, &item["data"]),
                version: item["version"].as_i64()?,
                legacy,
            })
        })
        .collect()
}

/// 帳簿の日と従来の帳簿の日を合わせる。同じ日があれば帳簿の方を使う。
pub fn merge_days(scoped: Vec<SavedDay>, legacy: Vec<SavedDay>) -> Vec<SavedDay> {
    let mut by_date: BTreeMap<String, SavedDay> = BTreeMap::new();
    for item in legacy.into_iter().chain(scoped) {
        by_date.insert(item.day.date.clone(), item);
    }
    by_date.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: &str, previous: Option<i64>, sales: i64, spent: i64) -> Day {
        let mut d = Day::empty(date, None);
        d.previous_balance = previous;
        d.incomes.insert("cafeSales".into(), Some(sales));
        if spent > 0 {
            d.expenses.push(Expense {
                id: format!("e-{date}"),
                amount: Some(spent),
                category: "misc".into(),
                description: String::new(),
                payee: String::new(),
                purchased_on: None,
                receipt_id: None,
            });
        }
        d
    }

    #[test]
    fn totals_subtract_deducted_income_and_expenses() {
        let mut d = day("2026-11-01", Some(10_000), 5_000, 1_200);
        d.incomes.insert("credit".into(), Some(2_000));
        assert_eq!(d.cash_income_total(), 3_000);
        assert_eq!(d.expense_total(), 1_200);
        assert_eq!(d.closing_balance(), 11_800);
    }

    #[test]
    fn carried_balances_follow_unless_manual_or_confirmed() {
        let a = day("2026-11-01", Some(1_000), 500, 0);
        let b = day("2026-11-02", Some(0), 0, 100);
        let mut c = day("2026-11-03", Some(7), 0, 0);
        c.previous_balance_manual = true;
        let carried = with_carried_balances(&[c.clone(), b, a]);
        assert_eq!(carried[1].previous_balance, Some(1_500));
        assert_eq!(carried[2].previous_balance, Some(7));
        assert_eq!(carried_over_balance("2026-11-03", &carried), Some(1_400));
        assert_eq!(carried_over_balance("2026-11-01", &carried), None);
    }

    #[test]
    fn round_trip_keeps_unknown_fields_and_template() {
        let data = json!({
            "template": { "name": "x", "incomes": [{"key": "sales", "label": "売上", "deduct": false}],
                          "expenses": [{"key": "misc", "label": "雑費", "slots": 2}], "defaultExpense": "misc" },
            "incomes": { "sales": 300 },
            "expenses": [{ "id": "a", "amount": 100, "category": "misc", "description": "", "payee": "店",
                           "purchasedOn": null, "receiptId": null }],
            "previousBalance": 50,
            "previousBalanceManual": true,
            "salesReceiptIds": ["r1"],
            "confirmedAt": null,
            "futureField": { "keep": true },
        });
        let d = Day::from_value("2026-11-01", &data);
        assert_eq!(d.closing_balance(), 250);
        let back = d.to_value();
        assert_eq!(back["futureField"], json!({ "keep": true }));
        assert_eq!(back["template"]["name"], "x");
        assert_eq!(back["incomes"], json!({ "sales": 300 }));
        assert_eq!(Day::from_value("2026-11-01", &back), d);
    }

    #[test]
    fn parses_yen_like_the_web_form() {
        assert_eq!(parse_yen("1,200"), Ok(Some(1200)));
        assert_eq!(parse_yen("１２００円"), Ok(Some(1200)));
        assert_eq!(parse_yen("¥ 30"), Ok(Some(30)));
        assert_eq!(parse_yen("−5"), Ok(Some(-5)));
        assert_eq!(parse_yen(""), Ok(None));
        assert_eq!(parse_yen("12a"), Err(()));
        assert_eq!(parse_yen("-"), Err(()));
        assert_eq!(format_yen(-1234567), "-1,234,567");
        assert_eq!(format_yen(100), "100");
    }

    #[test]
    fn vouchers_group_same_payee_and_continue_in_month() {
        let template = legacy_template();
        let mut d = Day::empty("2026-11-02", None);
        let row = |id: &str, category: &str, payee: &str, amount: Option<i64>| Expense {
            id: id.into(),
            amount,
            category: category.into(),
            description: String::new(),
            payee: payee.into(),
            purchased_on: None,
            receipt_id: None,
        };
        d.expenses = vec![
            row("a", "misc", "店", Some(100)),
            row("b", "cafeSupplies", "", Some(50)),
            row("c", "misc", "店", Some(20)),
            row("d", "misc", "", None),
        ];
        let numbers = voucher_numbers(&d, 4, &template);
        assert_eq!(numbers.get("b"), Some(&4));
        assert_eq!(numbers.get("a"), Some(&5));
        assert_eq!(numbers.get("c"), Some(&5));
        assert_eq!(numbers.get("d"), None);
        assert_eq!(voucher_count(&d), 2);
        let earlier = day("2026-11-01", None, 0, 10);
        let other_month = day("2026-10-31", None, 0, 10);
        assert_eq!(first_voucher_number("2026-11-02", &[earlier, other_month]), 2);
    }

    #[test]
    fn merge_prefers_scoped_days() {
        let legacy = SavedDay { day: day("2026-11-01", None, 1, 0), version: 3, legacy: true };
        let other = SavedDay { day: day("2026-11-02", None, 1, 0), version: 1, legacy: true };
        let scoped = SavedDay { day: day("2026-11-01", None, 2, 0), version: 1, legacy: false };
        let merged = merge_days(vec![scoped.clone()], vec![legacy, other]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0], scoped);
        assert!(merged[1].legacy);
    }

    #[test]
    fn shifts_dates() {
        assert_eq!(shift_date("2026-03-01", -1), "2026-02-28");
        assert_eq!(date_label("2026-11-15"), "2026年11月15日（日）");
    }
}
