//! アプリから開ける機能の一覧と、API の値を画面用の文字にする処理。
//!
//! どの機能も「一覧を取って、1 件を開くと詳細」という同じ形なので、
//! 機能ごとの違い（API の場所・見出しに使う項目など）だけをここに書く。

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feature {
    pub label: &'static str,
    /// API のパス。件数の上限などのクエリも含める。
    pub path: &'static str,
    /// 応答の中で一覧が入っている項目名（`items` / `data`）。
    pub list_key: &'static str,
    /// 1 行目に出す項目。先頭から順に、値のあるものを使う。
    pub title: &'static [&'static str],
    /// 2 行目に「 · 」でつないで出す項目。
    pub subtitle: &'static [&'static str],
    /// 右端に小さく出す項目（状態など）。
    pub trailing: Option<&'static str>,
    /// 人・会社のように名前の頭文字アイコンを出すか。
    pub avatar: bool,
    /// メニューの四角いアイコンの色。
    pub color: u32,
}

const BLUE: u32 = 0x007AFF;
const GREEN: u32 = 0x34C759;
const ORANGE: u32 = 0xFF9500;
const PURPLE: u32 = 0xAF52DE;
const RED: u32 = 0xFF3B30;
const TEAL: u32 = 0x30B0C7;
const INDIGO: u32 = 0x5856D6;

pub const DEALS: Feature = Feature {
    label: "案件",
    path: "/v1/erp/deals",
    list_key: "items",
    title: &["name"],
    subtitle: &["clientName", "amount"],
    trailing: Some("stage"),
    avatar: false,
    color: BLUE,
};
pub const QUOTATIONS: Feature = Feature {
    label: "見積",
    path: "/v1/erp/quotations?limit=100",
    list_key: "items",
    title: &["clientName", "quotationNumber"],
    subtitle: &["quotationNumber", "totalAmount"],
    trailing: Some("status"),
    avatar: false,
    color: BLUE,
};
pub const ORDERS: Feature = Feature {
    label: "受注",
    path: "/v1/erp/orders?limit=100",
    list_key: "items",
    title: &["clientName", "orderNumber"],
    subtitle: &["orderNumber", "totalAmount"],
    trailing: Some("status"),
    avatar: false,
    color: BLUE,
};
pub const SALES_CONTRACTS: Feature = Feature {
    label: "販売契約",
    path: "/v1/erp/sales-contracts?limit=100",
    list_key: "items",
    title: &["title", "contractNumber"],
    subtitle: &["clientName", "monthlyAmountYen"],
    trailing: Some("status"),
    avatar: false,
    color: BLUE,
};
pub const CUSTOMERS: Feature = Feature {
    label: "個人顧客",
    path: "/v1/erp/customers",
    list_key: "data",
    title: &["name"],
    subtitle: &["email", "phone"],
    trailing: None,
    avatar: true,
    color: GREEN,
};
pub const CLIENTS: Feature = Feature {
    label: "取引先",
    path: "/v1/erp/clients",
    list_key: "items",
    title: &["name"],
    subtitle: &["industry", "phoneNumber"],
    trailing: None,
    avatar: true,
    color: GREEN,
};
pub const INQUIRIES: Feature = Feature {
    label: "お問い合わせ",
    path: "/v1/erp/inquiries",
    list_key: "items",
    title: &["title", "subject", "publicQuestion", "kind"],
    subtitle: &["kind", "createdAt"],
    trailing: Some("status"),
    avatar: false,
    color: GREEN,
};
pub const RESERVATIONS: Feature = Feature {
    label: "予約",
    path: "/v1/erp/reservations",
    list_key: "items",
    title: &["customerName", "reservationNumber"],
    subtitle: &["startsAt", "priceAmount"],
    trailing: Some("status"),
    avatar: true,
    color: ORANGE,
};
pub const STAFF: Feature = Feature {
    label: "スタッフ",
    path: "/v1/erp/hrm/staff",
    list_key: "items",
    title: &["name"],
    subtitle: &["jobTitle", "employmentType"],
    trailing: None,
    avatar: true,
    color: PURPLE,
};
pub const PRODUCTS: Feature = Feature {
    label: "商品",
    path: "/v1/order/products?limit=100",
    list_key: "items",
    title: &["name"],
    subtitle: &["category", "listPrice"],
    trailing: Some("status"),
    avatar: false,
    color: TEAL,
};
pub const PURCHASE_ORDERS: Feature = Feature {
    label: "発注書",
    path: "/v1/erp/purchase-orders?limit=100",
    list_key: "items",
    title: &["vendorName", "purchaseOrderNumber"],
    subtitle: &["purchaseOrderNumber", "totalAmount"],
    trailing: Some("status"),
    avatar: false,
    color: TEAL,
};
pub const INVOICES: Feature = Feature {
    label: "請求書",
    path: "/v1/invoices?limit=100",
    list_key: "items",
    title: &["clientName", "invoiceNumber"],
    subtitle: &["invoiceNumber", "totalAmount", "dueDate"],
    trailing: Some("status"),
    avatar: false,
    color: INDIGO,
};
pub const EXPENSES: Feature = Feature {
    label: "経費",
    path: "/v1/erp/expenses?limit=100",
    list_key: "items",
    title: &["memo", "payee", "payerName"],
    subtitle: &["expenseDate", "amount"],
    trailing: Some("status"),
    avatar: false,
    color: RED,
};

/// メニュー画面のグループ。Web 版のメニューと同じ並び。
pub const MENU: &[(&str, &[Feature])] = &[
    ("販売管理", &[DEALS, QUOTATIONS, ORDERS, SALES_CONTRACTS]),
    ("顧客管理", &[CLIENTS, CUSTOMERS, INQUIRIES]),
    ("予約・人事", &[RESERVATIONS, STAFF]),
    ("商品・仕入", &[PRODUCTS, PURCHASE_ORDERS]),
    ("請求・経費", &[INVOICES, EXPENSES]),
];

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Money,
    Date,
    DateTime,
    Status,
    Address,
    Bool,
    List,
    Count,
}

/// 詳細画面に出す項目と順番。ここに無い項目（ID など）は出さない。
const FIELDS: &[(&str, &str, Kind)] = &[
    ("clientName", "取引先", Kind::Text),
    ("customerName", "お客様", Kind::Text),
    ("vendorName", "仕入先", Kind::Text),
    ("payee", "支払先", Kind::Text),
    ("quotationNumber", "見積番号", Kind::Text),
    ("orderNumber", "受注番号", Kind::Text),
    ("invoiceNumber", "請求番号", Kind::Text),
    ("purchaseOrderNumber", "発注番号", Kind::Text),
    ("contractNumber", "契約番号", Kind::Text),
    ("reservationNumber", "予約番号", Kind::Text),
    ("status", "状態", Kind::Status),
    ("paymentStatus", "支払い", Kind::Status),
    ("employmentStatus", "在籍", Kind::Status),
    ("publicationStatus", "公開", Kind::Status),
    ("priceAmount", "料金", Kind::Money),
    ("paidAmount", "支払済み額", Kind::Money),
    ("quantity", "数量", Kind::Count),
    ("cancelReason", "キャンセル理由", Kind::Text),
    ("subject", "件名", Kind::Text),
    ("payerName", "立替者", Kind::Text),
    ("stage", "段階", Kind::Status),
    ("pipeline", "パイプライン", Kind::Text),
    ("amount", "金額", Kind::Money),
    ("totalAmount", "合計", Kind::Money),
    ("subtotalAmount", "小計", Kind::Money),
    ("taxAmount", "税額", Kind::Money),
    ("listPrice", "定価", Kind::Money),
    ("monthlyAmountYen", "月額", Kind::Money),
    ("depositAmount", "前受金", Kind::Money),
    ("startsAt", "開始", Kind::DateTime),
    ("endsAt", "終了", Kind::DateTime),
    ("expenseDate", "日付", Kind::Date),
    ("expectedCloseOn", "受注予定日", Kind::Date),
    ("validUntil", "有効期限", Kind::Date),
    ("dueDate", "支払期日", Kind::Date),
    ("expectedDeliveryDate", "納品予定日", Kind::Date),
    ("startDate", "開始日", Kind::Date),
    ("endDate", "終了日", Kind::Date),
    ("hiredAt", "入社日", Kind::Date),
    ("ownerName", "担当", Kind::Text),
    ("jobTitle", "役職", Kind::Text),
    ("employmentType", "雇用形態", Kind::Status),
    ("category", "カテゴリ", Kind::Text),
    ("kind", "種類", Kind::Status),
    ("skuCode", "SKU", Kind::Text),
    ("janCode", "JAN", Kind::Text),
    ("industry", "業種", Kind::Text),
    ("billingCycle", "請求周期", Kind::Status),
    ("phone", "電話", Kind::Text),
    ("phoneNumber", "電話", Kind::Text),
    ("customerPhone", "電話", Kind::Text),
    ("clientPhone", "電話", Kind::Text),
    ("faxNumber", "FAX", Kind::Text),
    ("email", "メール", Kind::Text),
    ("clientEmail", "メール", Kind::Text),
    ("customerEmail", "メール", Kind::Text),
    ("website", "Web サイト", Kind::Text),
    ("representative", "代表者", Kind::Text),
    ("corporationNumber", "法人番号", Kind::Text),
    ("headOfficeAddress", "本社", Kind::Address),
    ("address", "住所", Kind::Address),
    ("nameKana", "フリガナ", Kind::Text),
    ("birthDate", "生年月日", Kind::Date),
    ("member", "会員", Kind::Bool),
    ("orderCount", "注文回数", Kind::Count),
    ("lastOrderedAt", "最終注文", Kind::Date),
    ("publicQuestion", "内容", Kind::Text),
    ("tags", "タグ", Kind::List),
    ("memo", "メモ", Kind::Text),
    ("notes", "メモ", Kind::Text),
    ("description", "説明", Kind::Text),
    ("sentAt", "送付", Kind::DateTime),
    ("paidAt", "入金日", Kind::Date),
    ("confirmedAt", "確定", Kind::DateTime),
    ("createdAt", "作成", Kind::DateTime),
    ("updatedAt", "更新", Kind::DateTime),
];

/// 1 件の ID。一覧の並びを保つためのキーに使う。
pub fn id(item: &Value) -> String {
    item["id"].as_str().unwrap_or_default().to_owned()
}

pub fn title(feature: &Feature, item: &Value) -> String {
    feature
        .title
        .iter()
        .find_map(|key| format_field(key, item))
        .unwrap_or_else(|| "（名前なし）".into())
}

pub fn subtitle(feature: &Feature, item: &Value) -> String {
    let title = title(feature, item);
    feature
        .subtitle
        .iter()
        .filter_map(|key| format_field(key, item))
        .filter(|text| *text != title)
        .collect::<Vec<_>>()
        .join(" · ")
}

pub fn trailing(feature: &Feature, item: &Value) -> String {
    feature
        .trailing
        .and_then(|key| format_field(key, item))
        .unwrap_or_default()
}

/// 詳細画面の行（ラベルと値）。同じラベルが続く場合は最初の 1 つだけ。
pub fn detail_rows(feature: &Feature, item: &Value) -> Vec<(String, String)> {
    let title = title(feature, item);
    let mut rows: Vec<(String, String)> = Vec::new();
    for (key, label, _) in FIELDS {
        let Some(text) = format_field(key, item) else {
            continue;
        };
        // 状態は詳細画面の上に大きく出すので、行には出さない。
        let shown_above = feature.trailing == Some(*key);
        if shown_above || text == title || rows.iter().any(|(l, _)| l == label) {
            continue;
        }
        rows.push(((*label).to_owned(), text));
    }
    rows
}

/// 明細（`items` / `lineItems` / `lines`）を「品名 | 金額」の行にする。
pub fn line_rows(item: &Value) -> Vec<(String, String)> {
    let lines = ["items", "lineItems", "lines"]
        .iter()
        .find_map(|key| item[*key].as_array())
        .cloned()
        .unwrap_or_default();
    let currency = item["currency"].as_str();
    lines
        .iter()
        .filter(|line| line.is_object())
        .map(|line| {
            let name = ["name", "description", "productName", "title", "memo"]
                .iter()
                .find_map(|k| text(&line[*k]))
                .unwrap_or_else(|| "明細".into());
            let quantity = line["quantity"].as_f64().filter(|q| *q != 1.0);
            let label = match quantity {
                Some(q) => format!("{name} × {}", trim_number(q)),
                None => name,
            };
            let amount = [
                "amount",
                "totalAmount",
                "subtotal",
                "lineTotal",
                "total",
                "unitPrice",
            ]
            .iter()
            .find_map(|k| money(&line[*k], currency))
            .unwrap_or_default();
            (label, amount)
        })
        .collect()
}

fn format_field(key: &str, item: &Value) -> Option<String> {
    let value = &item[key];
    let kind = FIELDS
        .iter()
        .find(|(k, _, _)| *k == key)
        .map(|(_, _, kind)| *kind)
        .unwrap_or(Kind::Text);
    let text = match kind {
        Kind::Text => text(value),
        Kind::Money => money(value, item["currency"].as_str()),
        Kind::Date => date(value, false),
        Kind::DateTime => date(value, true),
        Kind::Status => text(value).map(|s| status_label(&s)),
        Kind::Address => address(value),
        Kind::Bool => value
            .as_bool()
            .map(|b| if b { "はい" } else { "いいえ" }.into()),
        Kind::List => value
            .as_array()
            .map(|a| a.iter().filter_map(text).collect::<Vec<_>>().join("、")),
        Kind::Count if key == "orderCount" => value.as_i64().map(|n| format!("{n} 回")),
        Kind::Count => value.as_f64().map(trim_number),
    }?;
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// 文字・数値・`{ name }` 形のオブジェクトを文字にする。
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        Value::Object(map) => ["name", "label", "displayName", "title"]
            .iter()
            .find_map(|k| map.get(*k).and_then(text)),
        _ => None,
    }
}

fn money(value: &Value, currency: Option<&str>) -> Option<String> {
    let (amount, currency) = match value {
        Value::Number(n) => (n.as_f64()?, currency),
        Value::String(s) => (s.trim().parse::<f64>().ok()?, currency),
        Value::Object(map) => (
            map.get("amount")
                .and_then(|a| a.as_f64().or_else(|| a.as_str()?.parse().ok()))?,
            map.get("currency").and_then(Value::as_str).or(currency),
        ),
        _ => return None,
    };
    let number = with_commas(amount);
    Some(match currency {
        None | Some("JPY") | Some("jpy") => format!("¥{number}"),
        Some(code) => format!("{number} {}", code.to_uppercase()),
    })
}

fn with_commas(amount: f64) -> String {
    let negative = amount < 0.0;
    let whole = amount.abs().round() as u64;
    let digits = whole.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if negative { format!("-{out}") } else { out }
}

fn trim_number(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

/// 日付は「2026/11/13」、日時は日本時間で「2026/11/13 10:30」。
fn date(value: &Value, with_time: bool) -> Option<String> {
    let s = value.as_str()?.trim();
    let jst = FixedOffset::east_opt(9 * 3600)?;
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        let local = dt.with_timezone(&jst);
        return Some(if with_time {
            local.format("%Y/%m/%d %H:%M").to_string()
        } else {
            local.format("%Y/%m/%d").to_string()
        });
    }
    let day = s.get(..10).unwrap_or(s);
    NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .ok()
        .map(|d| d.format("%Y/%m/%d").to_string())
        .or_else(|| Some(s.to_owned()))
}

fn address(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_owned());
    }
    let part = |k: &str| value[k].as_str().unwrap_or_default().trim().to_owned();
    let body = ["state", "city", "address1", "address2"]
        .iter()
        .map(|k| part(k))
        .collect::<String>();
    if body.is_empty() {
        return None;
    }
    let postal = part("postalCode");
    Some(if postal.is_empty() {
        body
    } else {
        format!("〒{postal} {body}")
    })
}

/// よく出る状態の英語を日本語にする。知らない値はそのまま出す。
pub fn status_label(raw: &str) -> String {
    let label = match raw.to_ascii_lowercase().as_str() {
        "draft" => "下書き",
        "pending" => "保留中",
        "open" => "対応中",
        "new" => "新規",
        "sent" => "送付済み",
        "issued" => "発行済み",
        "accepted" => "承諾",
        "rejected" | "declined" => "辞退",
        "expired" => "期限切れ",
        "confirmed" => "確定",
        "approved" => "承認済み",
        "submitted" => "申請中",
        "paid" => "入金済み",
        "unpaid" => "未入金",
        "overdue" => "期限超過",
        "partially_paid" => "一部入金",
        "shipped" => "出荷済み",
        "received" => "受領済み",
        "delivered" => "納品済み",
        "completed" | "done" => "完了",
        "cancelled" | "canceled" => "キャンセル",
        "closed" => "終了",
        "active" => "有効",
        "inactive" | "archived" => "無効",
        "published" => "公開中",
        "unpublished" => "非公開",
        "won" => "受注",
        "lost" => "失注",
        "full_time" => "正社員",
        "part_time" => "パート",
        "contract" => "契約社員",
        "monthly" => "毎月",
        "yearly" | "annual" => "毎年",
        "no_show" => "無断キャンセル",
        "requested" => "申込み",
        "payment_pending" => "支払い待ち",
        "change_requested" => "変更依頼",
        "cancel_requested" => "キャンセル依頼",
        "waiting" => "キャンセル待ち",
        "admin_review" => "確認中",
        "suspended" => "停止中",
        "terminated" => "解約済み",
        "not_required" => "不要",
        "partial" => "一部支払い",
        "refund_pending" => "返金待ち",
        "refunded" => "返金済み",
        "fee_due" => "料金未収",
        "fee_paid" => "料金受領",
        "on_leave" => "休職中",
        "retired" => "退職",
        "invoiced" => "請求済み",
        "void" | "voided" => "無効",
        "failed" => "失敗",
        "processing" => "処理中",
        "answered" => "回答済み",
        "in_progress" => "対応中",
        "public" => "公開",
        "private" => "非公開",
        "service" => "サービス",
        "physical" => "物販",
        "digital" => "デジタル",
        "subscription" => "定期",
        "one_time" => "単発",
        "checked_in" => "来店済み",
        _ => return raw.to_owned(),
    };
    label.to_owned()
}
