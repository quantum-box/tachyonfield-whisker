//! 帳票の印刷用 HTML。Web 版の印刷（CashBookSheet・レジ管理表の印刷）と同じ並び・同じ文言で A4 に組む。
//! iOS の印刷画面に渡すと、プリンタへ送るか PDF として保存・共有できる。

use serde_json::Value;

use crate::cash_book::{self as cash, Day, Template};
use crate::register_count as reg;

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn yen(v: Option<i64>) -> String {
    cash::format_optional_yen(v)
}

/// 印刷形式（設定の `print`）。
struct Print {
    title: String,
    company: String,
    stacked: bool,
    reiwa: bool,
    font_size: f64,
    approval: String,
    footer: String,
}

impl Print {
    fn from(value: &Value, default_title: &str) -> Self {
        let s = |k: &str| value[k].as_str().unwrap_or("").to_owned();
        Self {
            title: Some(s("title")).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| default_title.to_owned()),
            company: s("companyName"),
            stacked: value["layout"] == "stacked",
            reiwa: value["dateStyle"] == "reiwa",
            font_size: value["fontSize"].as_f64().filter(|f| (8.0..=14.0).contains(f)).unwrap_or(10.0),
            approval: s("approvalLabel"),
            footer: s("footer"),
        }
    }
}

/// 令和の日付（「令和 8年　11月　15日」）。
fn reiwa(date: &str) -> String {
    let p: Vec<u32> = date.split('-').filter_map(|x| x.parse().ok()).collect();
    match p.as_slice() {
        [y, m, d] => format!("令和 {}年　{m}月　{d}日", y.saturating_sub(2018)),
        _ => date.to_owned(),
    }
}

const BASE_CSS: &str = "
@page { size: A4; margin: 10mm; }
* { box-sizing: border-box; }
body { margin: 0; color: #000; background: #fff; font-family: 'Hiragino Kaku Gothic ProN', 'Hiragino Sans', sans-serif; }
.page { page-break-after: always; break-after: page; }
.page:last-child { page-break-after: auto; break-after: auto; }
table { border-collapse: collapse; width: 100%; }
th, td { border: 1px solid #000; padding: 0 1.5mm; height: 6.8mm; vertical-align: middle; font-variant-numeric: tabular-nums; }
.num { text-align: right; white-space: nowrap; }
.c { text-align: center; }
.b { font-weight: bold; }
.ell { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
";

/// 現金出納帳 1 日分のページ。`start` はその日の最初の証番。
pub fn cash_book_page(day: &Day, _fallback: Option<&Template>, start: usize) -> String {
    // 計算と同じ項目で出す（設定を写していない従来の日は従来の項目）。
    let template = day.template_or(None);
    let print = Print::from(&template.raw["print"], "現金出納帳");
    let print = if template.raw.is_null() {
        Print { reiwa: true, font_size: 10.5, approval: "確認印".into(), ..Print::from(&Value::Null, "現金出納帳") }
    } else {
        print
    };
    let income = day.cash_income_total();
    let spent = day.expense_total();
    let closing = day.closing_balance();
    let numbers = cash::voucher_numbers(day, start, &template);
    let date = if print.reiwa { reiwa(&day.date) } else { day.date.clone() };
    let ledger = template.raw["ledgerLabel"].as_str().unwrap_or("");

    let mut incomes = String::new();
    for row in &template.incomes {
        let v = day.incomes.get(&row.key).copied().flatten();
        incomes.push_str(&format!("<tr><th class=\"c b\" style=\"width:50%\">{}</th><td class=\"num\">{}</td></tr>", esc(&row.label), yen(v)));
    }
    incomes.push_str(&format!("<tr><th class=\"c b\">②現金収入合計</th><td class=\"num b\">{}</td></tr>", cash::format_yen(income)));

    let balances = format!(
        "<table><tr><th class=\"b\" style=\"width:50%;text-align:left\">①前日現金残高</th><td class=\"num\">{}</td></tr></table>\
         <table style=\"margin-top:4mm\"><tr><th class=\"b\" style=\"width:50%;text-align:left\">本日現金残高(①+②−③)</th><td class=\"num b\">{}</td></tr></table>",
        yen(day.previous_balance),
        cash::format_yen(closing),
    );

    // 紙の行：同じ科目・同じ支払先の手入力の行はまとめ、科目ごとに用意した行数を確保する。
    let grouped = group_rows(day);
    let mut rows = String::new();
    for category in &template.expenses {
        let list: Vec<&cash::Expense> = grouped.iter().filter(|r| r.category == category.key).collect();
        let count = category.slots.max(list.len()).max(1);
        for i in 0..count {
            let row = list.get(i);
            let label = if i == 0 { esc(&category.label) } else { String::new() };
            let (amount, payee, desc, number) = match row {
                Some(r) => {
                    let note = r
                        .purchased_on
                        .as_deref()
                        .filter(|d| *d != day.date)
                        .map(|d| {
                            let p: Vec<u32> = d.split('-').filter_map(|x| x.parse().ok()).collect();
                            if p.len() == 3 { format!("（{}/{} 購入）", p[1], p[2]) } else { String::new() }
                        })
                        .unwrap_or_default();
                    (
                        yen(r.amount),
                        esc(&r.payee),
                        format!("{note}{}", esc(&r.description)),
                        numbers.get(&r.id).map(|n| n.to_string()).unwrap_or_default(),
                    )
                }
                None => (
                    String::new(),
                    String::new(),
                    if i == 0 { esc(category.fixed_description.as_deref().unwrap_or("")) } else { String::new() },
                    String::new(),
                ),
            };
            rows.push_str(&format!(
                "<tr><td class=\"c b\">{label}</td><td class=\"num\">{amount}</td><td class=\"ell\">{payee}</td><td class=\"ell\">{desc}</td><td class=\"c\">{number}</td></tr>"
            ));
        }
    }
    // 設定に無い科目の行も、最後に出す（証番と同じ扱い。見える行と合計がそろうように）。
    let mut extra: Vec<&str> = Vec::new();
    for r in &grouped {
        if !template.expenses.iter().any(|c| c.key == r.category) && !extra.contains(&r.category.as_str()) {
            extra.push(&r.category);
        }
    }
    for key in extra {
        for (i, r) in grouped.iter().filter(|r| r.category == key).enumerate() {
            rows.push_str(&format!(
                "<tr><td class=\"c b\">{}</td><td class=\"num\">{}</td><td class=\"ell\">{}</td><td class=\"ell\">{}</td><td class=\"c\">{}</td></tr>",
                if i == 0 { esc(key) } else { String::new() },
                yen(r.amount),
                esc(&r.payee),
                esc(&r.description),
                numbers.get(&r.id).map(|n| n.to_string()).unwrap_or_default(),
            ));
        }
    }
    rows.push_str(&format!(
        "<tr><td class=\"c b\">③支出合計</td><td class=\"num b\">{}</td><td></td><td></td><td></td></tr>",
        cash::format_yen(spent)
    ));

    let grid = if print.stacked {
        format!("<table>{incomes}</table><div style=\"margin-top:4mm\">{balances}</div>")
    } else {
        format!(
            "<div style=\"display:grid;grid-template-columns:1fr 1.6fr;gap:8mm\"><table>{incomes}</table>\
             <div style=\"display:flex;flex-direction:column;justify-content:space-between\">{balances}</div></div>"
        )
    };
    format!(
        "<section class=\"page\" style=\"font-size:{fs}pt\">\
         <h2 style=\"text-align:center;font-size:18pt;letter-spacing:0.3em;margin:0 0 2mm\">{title}</h2>\
         {ledger}{company}\
         <div style=\"display:flex;justify-content:space-between;margin:2mm 0\"><span class=\"b\">＜収入＞</span><span class=\"b\">{date}</span></div>\
         {grid}\
         <div class=\"b\" style=\"margin-top:4mm\">＜支出＞</div>\
         <table style=\"table-layout:fixed\"><colgroup><col style=\"width:18%\"><col style=\"width:17%\"><col style=\"width:20%\"><col><col style=\"width:7%\"></colgroup>\
         <tr><th>勘定科目</th><th>金額</th><th>支払先</th><th>取引内容</th><th>証番</th></tr>{rows}</table>\
         <div style=\"display:flex;justify-content:flex-end;margin-top:4mm\"><table style=\"width:28mm\"><tr><th class=\"c\">{approval}</th></tr><tr><td style=\"height:22mm\"></td></tr></table></div>\
         {footer}</section>",
        fs = print.font_size,
        title = esc(&print.title),
        ledger = if ledger.is_empty() { String::new() } else { format!("<div class=\"c\">{}</div>", esc(ledger)) },
        company = if print.company.is_empty() { String::new() } else { format!("<div class=\"c\">{}</div>", esc(&print.company)) },
        approval = esc(&print.approval),
        footer = if print.footer.is_empty() { String::new() } else { format!("<p style=\"white-space:pre-wrap\">{}</p>", esc(&print.footer)) },
    )
}

/// 紙に書く支出の行（Web 版の groupedExpenses）。
fn group_rows(day: &Day) -> Vec<cash::Expense> {
    let mut groups: Vec<cash::Expense> = Vec::new();
    let mut keys: Vec<(String, usize)> = Vec::new();
    for row in &day.expenses {
        let other = row.purchased_on.as_deref().filter(|d| *d != day.date).unwrap_or("");
        let key = (row.receipt_id.is_none() && !row.payee.trim().is_empty())
            .then(|| format!("{}\u{0}{}\u{0}{other}", row.category, row.payee.trim()));
        match key.as_ref().and_then(|k| keys.iter().find(|(x, _)| x == k)).map(|(_, i)| *i) {
            Some(i) => {
                let g = &mut groups[i];
                g.amount = match (g.amount, row.amount) {
                    (None, None) => None,
                    (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
                };
                let mut parts: Vec<String> = g.description.split('、').chain(row.description.split('、'))
                    .map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
                parts.dedup();
                let mut seen = Vec::new();
                parts.retain(|p| if seen.contains(p) { false } else { seen.push(p.clone()); true });
                g.description = parts.join("、");
            }
            None => {
                if let Some(k) = key {
                    keys.push((k, groups.len()));
                }
                groups.push(row.clone());
            }
        }
    }
    groups
}

/// レジ管理表 1 日分のページ（Web 版のレジ管理表の印刷と同じ 3 列）。
pub fn register_page(day: &reg::Day) -> String {
    let template = day.template();
    let print = Print::from(&day.scope["template"]["print"], "レジ管理表");
    let label = day.scope["label"].as_str().unwrap_or("");
    let date = if print.reiwa { reiwa(&day.date) } else { cash::date_label(&day.date) };
    let count_table = |title: &str, c: &std::collections::BTreeMap<i64, Option<i64>>| {
        let mut rows = String::new();
        for d in reg::DENOMS {
            let n = c.get(&d).copied().flatten();
            rows.push_str(&format!(
                "<tr><td>{}円</td><td class=\"num\">{}枚</td><td class=\"num\">{}円</td></tr>",
                cash::format_yen(d),
                n.map(|n| n.to_string()).unwrap_or_default(),
                n.map(|n| cash::format_yen(n * d)).unwrap_or_default(),
            ));
        }
        format!(
            "<table><tr><th colspan=\"3\" style=\"font-size:13pt\">{title}</th></tr><tr><th>金種</th><th>枚数</th><th>合計</th></tr>{rows}\
             <tr><th>合計</th><td></td><td class=\"num b\">{}円</td></tr></table>",
            cash::format_yen(reg::count_total(c))
        )
    };
    let mut actuals = String::new();
    if let Some(t) = &template {
        for row in &t.rows {
            let v = day.sales.get(&row.key).copied().flatten();
            let q = day.sales_quantities.get(&row.key).copied().flatten();
            let name = if row.effect == "memo" { format!("{}（集計外）", row.label) } else { row.label.clone() };
            let sign = if row.effect == "subtract" && v.is_some() { "−" } else { "" };
            actuals.push_str(&format!(
                "<tr><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{sign}{}円</td></tr>",
                esc(&name),
                if row.quantity { q.map(|q| format!("{q}件")).unwrap_or_else(|| "件".into()) } else { String::new() },
                v.map(cash::format_yen).unwrap_or_default(),
            ));
        }
    }
    let (closing, opening, increase, sales, over) = day.amount_check();
    actuals.push_str(&format!("<tr><th>合計</th><td></td><td class=\"num b\">{}円</td></tr>", cash::format_yen(sales)));
    let check = format!(
        "<table style=\"margin-top:4mm\"><tr><th colspan=\"2\">金額チェック</th></tr>\
         <tr><td>終了時現金</td><td class=\"num\">{}円</td></tr><tr><td>開始時入金</td><td class=\"num\">{}円</td></tr>\
         <tr><td>差額（増えた現金）</td><td class=\"num\">{}円</td></tr><tr><td>入出金合計</td><td class=\"num\">{}円</td></tr>\
         <tr><td>過不足</td><td class=\"num b\">{}円</td></tr><tr><td>（現金在高）</td><td class=\"num\">{}円</td></tr></table>",
        cash::format_yen(closing), cash::format_yen(opening), cash::format_yen(increase),
        cash::format_yen(sales), cash::format_yen(over), cash::format_yen(closing),
    );
    let columns = if print.stacked { "1fr" } else { "28fr 25fr 47fr" };
    let attach = format!(
        "<div style=\"position:relative;border:1px solid #000;min-height:120mm;background-image:linear-gradient(#ddd 1px,transparent 1px),linear-gradient(90deg,#ddd 1px,transparent 1px);background-size:60px 17px\">\
         <table style=\"position:absolute;top:0;right:0;width:16%;background:#fff\"><tr><th>客数</th><td class=\"num\">{}</td></tr></table>\
         <div style=\"color:#888;font-size:13pt;padding:2mm\">入金／清算／返品レシート添付</div>\
         {}</div>",
        day.customer_count.map(|c| c.to_string()).unwrap_or_default(),
        if day.note.is_empty() { String::new() } else { format!("<div style=\"position:absolute;bottom:2mm;left:2mm;background:#fff\">メモ　{}</div>", esc(&day.note)) },
    );
    let footer = if print.footer.is_empty() && print.approval.is_empty() {
        String::new()
    } else {
        format!(
            "<div style=\"display:flex;justify-content:space-between;margin-top:3mm\"><span>{}</span><span>{}　＿＿＿＿</span></div>",
            esc(&print.footer),
            esc(&print.approval)
        )
    };
    let source = if day.weather_forecast.is_null() {
        String::new()
    } else {
        format!(
            "<div style=\"font-size:6pt\">天気は気象庁の予報を4区分に整理。 出典 https://www.jma.go.jp/bosai/forecast/#area_type=class10s&amp;area_code={}</div>",
            esc(day.weather_forecast["areaCode"].as_str().unwrap_or(""))
        )
    };
    format!(
        "<section class=\"page\" style=\"font-size:{fs}pt;border:0.45mm solid #000;padding:2mm\">\
         <div style=\"display:grid;grid-template-columns:29fr 38fr 26fr 7fr;min-height:9mm;font-size:16pt;border-bottom:1px solid #000\">\
         <div>{title}<div style=\"font-size:0.6em\">{company}　{label}</div></div><div class=\"c\">{date}</div>\
         <div style=\"font-size:10pt\">担当<br>開始 {os}<br>終了 {cs}</div><div class=\"c\" style=\"font-size:10pt\">天気<br>{weather}</div></div>\
         <div style=\"display:grid;grid-template-columns:{columns};gap:4mm;margin-top:3mm\">\
         <div>{open}<table style=\"margin-top:4mm\"><tr><th colspan=\"3\">入出金合計</th></tr>{actuals}</table></div>\
         <div>{close}{check}</div>{attach}</div>{footer}{source}</section>",
        fs = print.font_size,
        title = esc(&print.title),
        company = esc(&print.company),
        label = esc(label),
        os = esc(&day.opening_staff),
        cs = esc(&day.closing_staff),
        weather = esc(&day.weather),
        open = count_table("開始時", &day.opening),
        close = count_table("終了時", &day.closing),
    )
}

/// ページを 1 つの HTML にまとめる。
pub fn document(title: &str, pages: &[String], landscape: bool) -> String {
    let orientation = if landscape { "@page { size: A4 landscape; margin: 8mm; }" } else { "" };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>{BASE_CSS}{orientation}</style></head><body>{}</body></html>",
        esc(title),
        pages.join("")
    )
}

/// 印刷・PDF の名前（出納帳20261115 / 出納帳20261101_1130 / 出納帳20261201_20270105）。
pub fn file_name(kind: &str, from: &str, to: &str) -> String {
    let compact = |d: &str| d.replace('-', "");
    if from == to {
        format!("{kind}{}", compact(from))
    } else if from[..4] == to[..4] {
        format!("{kind}{}_{}", compact(from), &compact(to)[4..])
    } else {
        format!("{kind}{}_{}", compact(from), compact(to))
    }
}

/// 出納帳のページが横向きか（その日の設定）。
pub fn cash_landscape(day: &Day, _fallback: Option<&Template>) -> bool {
    day.template_or(None).raw["print"]["orientation"] == "landscape"
}

/// レジ管理表のページが横向きか（既定は横）。
pub fn register_landscape(day: &reg::Day) -> bool {
    day.scope["template"]["print"]["orientation"] != "portrait"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cash_sheet_has_web_layout() {
        let mut day = Day::empty("2026-11-15", None);
        day.previous_balance = Some(10000);
        day.incomes.insert("cafeSales".into(), Some(5000));
        day.expenses.push(cash::Expense {
            id: "a".into(),
            amount: Some(1200),
            category: "misc".into(),
            description: "紙<コップ>".into(),
            payee: "店".into(),
            purchased_on: Some("2026-11-10".into()),
            receipt_id: Some("r".into()),
        });
        let page = cash_book_page(&day, None, 3);
        assert!(page.contains("令和 8年　11月　15日"));
        assert!(page.contains("（11/10 購入）紙&lt;コップ&gt;"));
        assert!(page.contains("<td class=\"c\">3</td>"));
        assert!(page.contains("③支出合計"));
        assert!(page.contains("13,800"));
        assert!(page.contains("現金預金"));
        let mut odd = day.clone();
        odd.expenses[0].category = "unknownKey".into();
        assert!(cash_book_page(&odd, None, 1).contains("unknownKey"));
    }

    #[test]
    fn register_sheet_and_names() {
        let mut day = reg::Day::empty("2026-11-15", json!({ "label": "本店 / レジ1", "template": reg::default_template() }));
        day.closing.insert(1000, Some(3));
        day.sales.insert("sales".into(), Some(3000));
        let page = register_page(&day);
        assert!(page.contains("本店 / レジ1"));
        assert!(page.contains("入金／清算／返品レシート添付"));
        assert!(page.contains("3,000円"));
        assert!(register_landscape(&day));
        assert_eq!(file_name("出納帳", "2026-11-15", "2026-11-15"), "出納帳20261115");
        assert_eq!(file_name("出納帳", "2026-11-01", "2026-11-30"), "出納帳20261101_1130");
        assert_eq!(file_name("レジ管理表", "2026-12-01", "2027-01-05"), "レジ管理表20261201_20270105");
    }
}
