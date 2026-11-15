//! 帳簿設定（店舗・レジごとの帳簿）。Web 版（cash-book-template.ts / cash-book-settings-api.ts）と
//! 同じ JSON を読み書きし、同じ決まりで確かめる。
//!
//! 設定は JSON のまま持ち、画面で変えた所だけ書き換える。この画面が知らない項目
//! （レジ管理表の設定 `registerCount` など）は、そのまま残してサーバーへ返す。

use serde_json::{Value, json};

/// 印刷形式の初期値（Web 版の DEFAULT_PRINT）。
pub fn default_print() -> Value {
    json!({
        "title": "現金出納帳",
        "companyName": "",
        "layout": "sideBySide",
        "dateStyle": "iso",
        "fontSize": 10,
        "approvalLabel": "確認印",
        "footer": "",
    })
}

/// 従来の現金出納帳（バナスクの紙と同じ並び）。
#[cfg_attr(not(test), allow(dead_code))]
pub fn banasuku_template() -> Value {
    let mut print = default_print();
    print["dateStyle"] = json!("reiwa");
    print["fontSize"] = json!(10.5);
    json!({
        "name": "従来の現金出納帳",
        "preset": "banasuku",
        "incomes": [
            { "key": "dogRunSales", "label": "ドッグラン売上", "deduct": false },
            { "key": "cafeSales", "label": "カフェ売上", "deduct": false },
            { "key": "discountTickets", "label": "割引チケット", "deduct": false },
            { "key": "credit", "label": "クレジット（△）", "deduct": true },
            { "key": "codePayment", "label": "コード決済（△）", "deduct": true },
            { "key": "merchandise", "label": "物販", "deduct": false },
            { "key": "bankWithdrawal", "label": "預金引出", "deduct": false },
        ],
        "expenses": [
            { "key": "cafeSupplies", "label": "カフェ材料仕入", "slots": 7 },
            { "key": "equipment", "label": "備品消耗品費", "slots": 3 },
            { "key": "misc", "label": "雑費", "slots": 4 },
            { "key": "books", "label": "新聞図書費", "slots": 2 },
            { "key": "bankDeposit", "label": "普通預金", "slots": 1, "fixedDescription": "現金預金" },
            { "key": "pettyCash", "label": "小口現金", "slots": 1, "fixedDescription": "コンテンツへの現金振替" },
        ],
        "defaultIncome": "cafeSales",
        "defaultExpense": "cafeSupplies",
        "incomeRules": [],
        "expenseRules": [],
        "print": print,
    })
}

/// 標準現金出納帳（新しく帳簿を足したときの形）。
pub fn generic_template() -> Value {
    json!({
        "name": "標準現金出納帳",
        "preset": "generic",
        "incomes": [
            { "key": "sales", "label": "売上", "deduct": false },
            { "key": "credit", "label": "クレジット（△）", "deduct": true },
            { "key": "codePayment", "label": "コード決済（△）", "deduct": true },
            { "key": "bankWithdrawal", "label": "預金引出", "deduct": false },
        ],
        "expenses": [
            { "key": "purchases", "label": "仕入", "slots": 7 },
            { "key": "misc", "label": "雑費", "slots": 4 },
            { "key": "bankDeposit", "label": "普通預金", "slots": 1 },
        ],
        "defaultIncome": "sales",
        "defaultExpense": "purchases",
        "incomeRules": [],
        "expenseRules": [],
        "print": default_print(),
    })
}

fn has_key(rows: &Value, key: &str) -> bool {
    rows.as_array()
        .is_some_and(|rows| rows.iter().any(|row| row["key"] == key))
}

/// 従来の決まり（コードに書いてあった振り分け）を、編集できるルールにして足す。
pub fn configure_legacy(template: &Value) -> Value {
    let mut next = template.clone();
    next["preset"] = json!("generic");
    let mut expense_rules: Vec<Value> = Vec::new();
    for text in ["新聞", "書店", "書籍", "雑誌", "ブックス", "本屋", "book"] {
        expense_rules.push(json!({ "text": text, "target": "books" }));
    }
    for (text, target) in [
        ("consumable", "equipment"),
        ("equipment", "equipment"),
        ("other", "misc"),
        ("packaging", "cafeSupplies"),
        ("raw_material", "cafeSupplies"),
        ("shipping", "misc"),
    ] {
        expense_rules.push(json!({ "text": text, "target": target, "source": "costCategory" }));
    }
    for text in [
        "ダイソー", "セリア", "キャンドゥ", "ホームセンター", "カインズ", "コーナン", "コメリ", "ニトリ",
        "daiso", "seria", "cainz",
    ] {
        expense_rules.push(json!({ "text": text, "target": "equipment", "source": "payee" }));
    }
    let mut income_rules: Vec<Value> = Vec::new();
    for (target, words) in [
        ("dogRunSales", &["ドッグラン", "ドックラン", "dog run", "入場", "入園"][..]),
        ("discountTickets", &["チケット", "回数券", "割引券", "ticket"][..]),
        ("merchandise", &["物販", "グッズ", "goods", "ペット用品", "首輪", "リード", "おもちゃ"][..]),
    ] {
        for text in words {
            income_rules.push(json!({ "text": text, "target": target }));
        }
    }
    let mut incomes = template["incomeRules"].as_array().cloned().unwrap_or_default();
    incomes.extend(
        income_rules
            .into_iter()
            .filter(|rule| has_key(&template["incomes"], rule["target"].as_str().unwrap_or(""))),
    );
    let mut expenses = template["expenseRules"].as_array().cloned().unwrap_or_default();
    expenses.extend(
        expense_rules
            .into_iter()
            .filter(|rule| has_key(&template["expenses"], rule["target"].as_str().unwrap_or(""))),
    );
    next["incomeRules"] = Value::Array(incomes);
    next["expenseRules"] = Value::Array(expenses);
    next
}

/// 新しい項目の鍵（`item_` + 32 桁の 16 進）。
pub fn new_key() -> String {
    format!("item_{}", random_hex())
}

/// 新しい帳簿の ID。
pub fn new_book_id() -> String {
    let h = random_hex();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn random_hex() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for i in 0..2u64 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(i);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        hasher.write_u64(nanos);
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out
}

/// UTF-16 での長さ（Web 版の文字数の数え方）。
fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && key.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn valid_label(text: &str) -> bool {
    let trimmed = text.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}');
    !trimmed.is_empty() && utf16_len(text) <= 100
}

const CHECK_ITEMS: &str = "項目の重複、振り分け先、未分類の項目を確認してください。";

/// 現金出納帳の設定を確かめる。合わなければ、画面に出す一文を返す。
pub fn validate_template(template: &Value) -> Result<(), String> {
    if !template["name"].as_str().is_some_and(valid_label) {
        return Err("テンプレート名を入れてください（100文字まで）。".into());
    }
    let incomes = template["incomes"].as_array().cloned().unwrap_or_default();
    let expenses = template["expenses"].as_array().cloned().unwrap_or_default();
    if incomes.is_empty() || incomes.len() > 40 || expenses.is_empty() || expenses.len() > 40 {
        return Err("売上項目と支出項目は、それぞれ 1〜40 件にしてください。".into());
    }
    for row in incomes.iter().chain(&expenses) {
        if !row["key"].as_str().is_some_and(valid_key) {
            return Err(CHECK_ITEMS.into());
        }
        if !row["label"].as_str().is_some_and(valid_label) {
            return Err("項目名を入れてください（100文字まで）。".into());
        }
    }
    for row in &expenses {
        let slots = row["slots"].as_f64().unwrap_or(0.0);
        if slots.fract() != 0.0 || !(1.0..=30.0).contains(&slots) {
            return Err("印刷行数は 1〜30 にしてください。".into());
        }
        if row["fixedDescription"].as_str().is_some_and(|t| utf16_len(t) > 200) {
            return Err(CHECK_ITEMS.into());
        }
    }
    for (rows, rules, fallback) in [
        (&incomes, &template["incomeRules"], &template["defaultIncome"]),
        (&expenses, &template["expenseRules"], &template["defaultExpense"]),
    ] {
        let keys: Vec<&str> = rows.iter().filter_map(|row| row["key"].as_str()).collect();
        let unique = keys.iter().enumerate().all(|(i, key)| !keys[..i].contains(key));
        let rules = rules.as_array().cloned().unwrap_or_default();
        if rules.len() > 300 {
            return Err("振り分けのルールは 300 件までです。".into());
        }
        for rule in &rules {
            if !rule["text"].as_str().is_some_and(valid_label) {
                return Err("振り分けのルールの文字を入れてください（100文字まで）。".into());
            }
        }
        let targets_ok = rules
            .iter()
            .all(|rule| rule["target"].as_str().is_some_and(|t| keys.contains(&t)));
        if !unique || !fallback.as_str().is_some_and(|f| keys.contains(&f)) || !targets_ok {
            return Err(CHECK_ITEMS.into());
        }
    }
    let deducted = |key: &str| {
        incomes
            .iter()
            .any(|row| row["key"] == key && row["deduct"] == Value::Bool(true))
    };
    if !deducted("credit") || !deducted("codePayment") {
        return Err("クレジット・コード決済は控除項目として必要です。".into());
    }
    let default_deducts = incomes
        .iter()
        .any(|row| row["key"] == template["defaultIncome"] && row["deduct"] == Value::Bool(true));
    if default_deducts {
        return Err("未分類売上には収入項目を選んでください。".into());
    }
    validate_print(&template["print"])
}

fn validate_print(print: &Value) -> Result<(), String> {
    let title = print["title"].as_str().unwrap_or("");
    if title.trim().is_empty() || utf16_len(title) > 100 {
        return Err("表題を入れてください（100文字まで）。".into());
    }
    let within = |key: &str, max: usize| print[key].as_str().is_some_and(|t| utf16_len(t) <= max);
    if !within("companyName", 100) || !within("approvalLabel", 40) || !within("footer", 500) {
        return Err("会社名は100文字、確認欄は40文字、備考は500文字までです。".into());
    }
    let size = print["fontSize"].as_f64().unwrap_or(0.0);
    if !(8.0..=14.0).contains(&size) {
        return Err("文字サイズは 8〜14 にしてください。".into());
    }
    if !matches!(print["layout"].as_str(), Some("sideBySide" | "stacked"))
        || !matches!(print["dateStyle"].as_str(), Some("reiwa" | "iso"))
        || !matches!(print["orientation"].as_str(), None | Some("portrait" | "landscape"))
    {
        return Err("印刷形式を確認してください。".into());
    }
    Ok(())
}

/// 帳簿 1 件（帳簿名・店舗・レジ・設定）を確かめる。
pub fn validate_book(book: &Value) -> Result<(), String> {
    if !book["name"].as_str().is_some_and(valid_label) {
        return Err("帳簿名を入れてください（100文字まで）。".into());
    }
    let store = book["storeId"].as_str().unwrap_or("");
    if store.is_empty() || store.len() > 128 {
        return Err("店舗を選んでください。".into());
    }
    if !book["registerName"].as_str().is_some_and(valid_label) {
        return Err("レジ名を入れてください（100文字まで）。".into());
    }
    validate_template(&book["template"])
}

/// 帳簿の一覧ごと確かめる。別の帳簿に問題があるときは、その帳簿を名指しする。
pub fn validate_books(books: &[Value], editing: usize) -> Result<(), String> {
    if books.len() > 100 {
        return Err("帳簿は 100 件までです。".into());
    }
    for (index, book) in books.iter().enumerate() {
        let id = book["id"].as_str().unwrap_or("");
        if books[..index].iter().any(|b| b["id"] == id) {
            return Err("帳簿が重複しています。".into());
        }
        let pair = (&book["storeId"], &book["registerName"]);
        if books[..index]
            .iter()
            .any(|b| (&b["storeId"], &b["registerName"]) == pair)
        {
            return Err("同じ店舗・レジの帳簿がすでにあります。レジ名を変えてください。".into());
        }
        if let Err(message) = validate_book(book) {
            if index == editing {
                return Err(message);
            }
            let name = book["name"].as_str().filter(|n| !n.is_empty()).unwrap_or("名前なし");
            return Err(format!(
                "帳簿「{name}」（{} / {}）の設定に問題があるため保存できません。先にその帳簿を開いて修正してください。{message}",
                book["storeName"].as_str().unwrap_or(""),
                book["registerName"].as_str().unwrap_or(""),
            ));
        }
    }
    Ok(())
}

/// 新しい帳簿（先頭の店舗、標準の設定）。
pub fn new_book(store_id: &str, store_name: &str) -> Value {
    json!({
        "id": new_book_id(),
        "name": "",
        "storeId": store_id,
        "storeName": store_name,
        "registerName": "",
        "template": generic_template(),
    })
}

/// 項目を消してよいか（Web 版で「削除」を押せない条件の逆）。
pub fn can_remove_income(template: &Value, key: &str) -> bool {
    key != "credit"
        && key != "codePayment"
        && template["incomes"].as_array().is_some_and(|rows| rows.len() > 1)
        && template["defaultIncome"] != key
        && !targeted(&template["incomeRules"], key)
}

pub fn can_remove_expense(template: &Value, key: &str) -> bool {
    template["expenses"].as_array().is_some_and(|rows| rows.len() > 1)
        && template["defaultExpense"] != key
        && !targeted(&template["expenseRules"], key)
}

fn targeted(rules: &Value, key: &str) -> bool {
    rules
        .as_array()
        .is_some_and(|rules| rules.iter().any(|rule| rule["target"] == key))
}

/// OCR 品目分類の候補（文字のかわりに選べるもの）。
pub const COST_CATEGORIES: [(&str, &str); 6] = [
    ("raw_material", "原材料"),
    ("packaging", "包装資材"),
    ("consumable", "消耗品"),
    ("equipment", "備品"),
    ("shipping", "送料"),
    ("other", "その他"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid() {
        assert_eq!(validate_template(&generic_template()), Ok(()));
        assert_eq!(validate_template(&banasuku_template()), Ok(()));
        assert_eq!(validate_template(&configure_legacy(&banasuku_template())), Ok(()));
    }

    #[test]
    fn legacy_rules_only_target_existing_rows() {
        let generic = configure_legacy(&generic_template());
        // 標準の設定には books / equipment などが無いので、misc だけ残る。
        let targets: Vec<&str> = generic["expenseRules"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["target"].as_str())
            .collect();
        assert!(targets.iter().all(|t| *t == "misc"));
        assert_eq!(generic["incomeRules"], json!([]));
        assert_eq!(generic["preset"], "generic");
        let legacy = configure_legacy(&banasuku_template());
        assert_eq!(legacy["expenseRules"].as_array().unwrap().len(), 24);
        assert_eq!(legacy["incomeRules"].as_array().unwrap().len(), 16);
    }

    #[test]
    fn rejects_the_web_rules() {
        let mut t = generic_template();
        t["incomes"][1]["deduct"] = json!(false);
        assert_eq!(validate_template(&t), Err("クレジット・コード決済は控除項目として必要です。".into()));
        let mut t = generic_template();
        t["defaultIncome"] = json!("credit");
        assert_eq!(validate_template(&t), Err("未分類売上には収入項目を選んでください。".into()));
        let mut t = generic_template();
        t["expenseRules"] = json!([{ "text": "x", "target": "nope" }]);
        assert_eq!(validate_template(&t), Err(CHECK_ITEMS.into()));
        let mut t = generic_template();
        t["expenses"][0]["slots"] = json!(31);
        assert!(validate_template(&t).is_err());
    }

    #[test]
    fn books_must_differ_and_name_the_broken_one() {
        let mut a = new_book("s1", "本店");
        a["name"] = json!("A");
        a["registerName"] = json!("レジ1");
        let mut b = a.clone();
        b["id"] = json!(new_book_id());
        assert!(validate_books(&[a.clone(), b.clone()], 1).unwrap_err().contains("同じ店舗・レジ"));
        b["registerName"] = json!("レジ2");
        assert_eq!(validate_books(&[a.clone(), b.clone()], 1), Ok(()));
        a["name"] = json!("");
        let message = validate_books(&[a, b], 1).unwrap_err();
        assert!(message.starts_with("帳簿「名前なし」（本店 / レジ1）"));
    }

    #[test]
    fn keys_and_ids_have_the_web_shape() {
        let key = new_key();
        assert!(valid_key(&key) && key.len() == 37);
        let id = new_book_id();
        assert_eq!(id.len(), 36);
        assert_ne!(new_key(), new_key());
    }

    #[test]
    fn removal_rules() {
        let t = generic_template();
        assert!(!can_remove_income(&t, "credit"));
        assert!(!can_remove_income(&t, "sales"));
        assert!(can_remove_income(&t, "bankWithdrawal"));
        assert!(!can_remove_expense(&t, "purchases"));
        assert!(can_remove_expense(&t, "misc"));
    }
}
