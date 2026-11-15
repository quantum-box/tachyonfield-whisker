//! レジ管理表に、売上伝票（証憑 OCR）と売上台帳から実績を入れる部分（Web 版と同じ決まり）。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use serde_json::Value;

use super::*;
use crate::cash_book as cash;
use crate::cash_book_receipts::Receipt;
use crate::cash_book_screen::*;
use crate::cash_book_settings_screen::Hint;
use crate::register_count::{self as model, SalesResult};
use crate::register_count_screen::Sheet;

/// 売上伝票レポート（OCR）から入力。
#[component]
pub(crate) fn sales_import(sheet: Sheet) -> Element {
    let open = signal(false);
    let loading = signal(false);
    let error = signal(String::new());
    let message = signal(String::new());
    let truncated = signal(false);
    let reports = signal(Vec::<(Receipt, Value)>::new());
    let chosen = signal(HashSet::<String>::new());
    let products = signal(HashMap::<String, Value>::new());
    let scanning = signal(false);
    let owner = sheet.owner;

    let load = move || {
        let (Some(tenant), Some(day)) = (sheet.tenant.get_untracked(), sheet.day.get_untracked()) else { return };
        let date = day.date.clone();
        let own: HashSet<String> = day.receipt_ids.iter().cloned().collect();
        let product_ids: Vec<String> = day
            .template()
            .map(|t| t.raw["rules"].as_array().cloned().unwrap_or_default())
            .unwrap_or_default()
            .iter()
            .filter_map(|r| r["productId"].as_str().map(str::to_owned))
            .collect();
        loading.set(true);
        error.set(String::new());
        spawn_in(owner, async move {
            let today = cash::tokyo_today();
            let result = authed(sheet.token, move |access| {
                if date > today {
                    return Ok((Vec::new(), false, HashSet::new(), HashMap::new()));
                }
                let from = cash::shift_date(&date, -1);
                let upper = cash::shift_date(&date, 60);
                let to = if upper < today { upper } else { today.clone() };
                let mut list = Vec::new();
                let mut cut = false;
                for kind in ["sales", "unsorted"] {
                    let (items, t) = api::list_receipts(access, &tenant, kind, &from, &to)?;
                    cut |= t;
                    list.extend(items.into_iter().filter_map(|v| Receipt::from_value(&v).map(|r| (r, v))));
                }
                list.retain(|(r, _)| model::is_sales_report_for(r, &date));
                let ids: Vec<String> = list.iter().map(|(r, _)| r.id.clone()).collect();
                let claimed: HashSet<String> = api::receipt_uses(access, &tenant, &ids, true)?.into_iter().collect();
                let mut products = HashMap::new();
                for id in &product_ids {
                    if let Ok(p) = api::get_product(access, &tenant, id) {
                        products.insert(id.clone(), p);
                    }
                }
                Ok((list, cut, claimed, products))
            })
            .await;
            loading.set(false);
            match result {
                Ok((list, cut, claimed, found)) => {
                    let visible: Vec<(Receipt, Value)> = list
                        .into_iter()
                        .filter(|(r, _)| !claimed.contains(&r.id) || own.contains(&r.id))
                        .collect();
                    chosen.set(visible.iter().map(|(r, _)| r.id.clone()).filter(|id| own.contains(id)).collect());
                    reports.set(visible);
                    truncated.set(cut);
                    products.set(found);
                }
                Err(e) => error.set(format!("伝票を取得できませんでした。{e}")),
            }
        });
    };

    let preview = computed(move || {
        let day = sheet.day.get()?;
        let template = day.template()?;
        let picked: Vec<(Receipt, Value)> =
            reports.get().into_iter().filter(|(r, _)| chosen.with(|c| c.contains(&r.id))).collect();
        if picked.is_empty() {
            return None;
        }
        Some(model::sales_from_reports(&day, &template, &picked, &products.get()))
    });

    let scan = move |file: field_glass::PickedFile| {
        if scanning.get_untracked() {
            return;
        }
        let Some(tenant) = sheet.tenant.get_untracked() else { return };
        let Ok(bytes) = std::fs::read(&file.path) else {
            message.set("ファイルを読み込めませんでした。".into());
            return;
        };
        let content_type = if file.content_type.is_empty() { "image/jpeg".to_owned() } else { file.content_type.clone() };
        let name = if file.name.is_empty() { "report.jpg".to_owned() } else { file.name.clone() };
        let date = sheet.date.get_untracked();
        scanning.set(true);
        message.set(String::new());
        spawn_in(owner, async move {
            let result = authed(sheet.token, move |access| {
                let uploaded = api::upload_receipt(access, &tenant, &name, &content_type, &bytes, "unsorted")?;
                let id = uploaded["id"].as_str().ok_or("アップロードできませんでした。")?.to_owned();
                let mut value = uploaded;
                let mut sort_wait = 0;
                for _ in 0..60 {
                    let status = value["status"].as_str().unwrap_or("");
                    let waiting = status == "done"
                        && value["receiptType"] == "unsorted"
                        && Receipt::from_value(&value).is_some_and(|r| r.suggested_type.is_some())
                        && sort_wait < 3;
                    if status != "pending" && status != "processing" && !waiting {
                        let used = api::receipt_uses(access, &tenant, &[id.clone()], true)?;
                        return Ok(Some((value, !used.is_empty())));
                    }
                    if waiting {
                        sort_wait += 1;
                    }
                    std::thread::sleep(Duration::from_secs(2));
                    value = api::get_receipt(access, &tenant, &id)?;
                }
                Ok(None)
            })
            .await;
            scanning.set(false);
            match result {
                Ok(None) => message.set("証憑OCRで読み取りを続けています。少し待って「伝票一覧を更新」を押してください。".into()),
                Err(e) => message.set(e),
                Ok(Some((value, used))) => {
                    let Some(r) = Receipt::from_value(&value) else { return };
                    if r.status != "done" {
                        message.set(r.error_message.clone().unwrap_or_else(|| "読み取れませんでした。写真を撮り直してください。".into()));
                    } else if r.is_purchase() {
                        message.set("仕入のレシートと読み取ったため、証憑OCRに仕入として保存しました。".into());
                    } else if used && !sheet.day.with_untracked(|d| d.as_ref().is_some_and(|d| d.receipt_ids.contains(&r.id))) {
                        message.set("この伝票は別の日付または別のレジ管理表で使用済みです。".into());
                    } else if r.sales_day() != date {
                        message.set(format!("証憑OCRに保存しました。伝票の営業日は {} です。その日付を開くと反映できます。", r.sales_day()));
                    } else if !model::is_sales_report_for(&r, &date) {
                        message.set("証憑OCRに保存しました。営業日、売上の区分、金額の読み取り結果を確認してください。".into());
                    } else {
                        let id = r.id.clone();
                        reports.update(|l| {
                            l.retain(|(x, _)| x.id != id);
                            l.push((r, value));
                        });
                        chosen.update(|c| { c.insert(id); });
                        open.set(true);
                        message.set("証憑OCRに保存しました。読み取った伝票の金額を確認して反映してください。".into());
                    }
                }
            }
        });
    };

    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            View(style: group_style().margin_top(px(16))) {
                FieldRow(
                    label: computed(|| "売上伝票レポート（OCR）から入力".to_owned()),
                    value: computed(move || if open.get() { "閉じる".to_owned() } else { "開く".to_owned() }),
                    on_tap: move |_| {
                        let next = !open.get_untracked();
                        open.set(next);
                        if next { load(); }
                    },
                )
                Show(when: move || open.get()) {
                    View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Row, align_items: AlignItems::Center, gap: px(12), padding: (px(10), px(16)))) {
                        AttachButton(on_pick: move |f: field_glass::PickedFile| scan(f), style: css!(width: px(36), height: px(36)))
                        Text(
                            value: computed(move || if scanning.get() { "証憑OCRで伝票を読み取り中…".to_owned() } else { "写真・PDFを読み取る".to_owned() }),
                            style: css!(color: hex(TINT), font_size: px(16)),
                        )
                    }
                    FieldRow(label: computed(|| "伝票一覧を更新".to_owned()), value: computed(String::new), on_tap: move |_| load())
                    Show(when: move || loading.get()) { Hint(text: "伝票を読み込み中…") }
                    Show(when: move || !error.with(String::is_empty)) {
                        Text(value: computed(move || error.get()), style: css!(color: hex(RED), font_size: px(13), padding: (px(6), px(16))))
                    }
                    Show(when: move || truncated.get()) { Hint(text: "取得上限に達したため、一部の伝票が表示されていない可能性があります。") }
                    Show(when: move || !loading.get() && reports.with(Vec::is_empty) && error.with(String::is_empty)) {
                        Hint(text: "この日付で取り込める売上伝票はありません。売上伝票の写真かPDFを選んでください。")
                    }
                    Show(when: move || !reports.with(Vec::is_empty)) {
                        Hint(text: "この店舗・レジの伝票を選んでください。売上の内訳を入れ替え、手入力専用の項目は残します。")
                        ForEach(
                            each: move || reports.get().into_iter().map(|(r, _)| r).collect::<Vec<_>>(),
                            key: |r: &Receipt| r.id.clone(),
                            children: move |r: ReadSignal<Receipt>| {
                                let r = r.get_untracked();
                                let label = format!("{}　{}", r.title(), r.total.map(|t| format!("{}円", cash::format_yen(t as i64))).unwrap_or_default());
                                let id = r.id.clone();
                                let checked = { let id = id.clone(); computed(move || chosen.with(|c| c.contains(&id))) };
                                render! {
                                    FieldRow(
                                        label: computed(move || label.clone()),
                                        value: computed(move || if checked.get() { "✓".to_owned() } else { "選ぶ".to_owned() }),
                                        on_tap: move |_| { let id = id.clone(); chosen.update(move |c| { if !c.remove(&id) { c.insert(id); } }); },
                                    )
                                }
                            },
                        )
                    }
                    Show(when: move || preview.with(Option::is_some)) {
                        Text(
                            value: computed(move || preview_text(&sheet, preview.get())),
                            style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(16))),
                        )
                        FieldRow(
                            label: computed(|| "選んだ伝票を反映".to_owned()),
                            value: computed(String::new),
                            on_tap: move |_| {
                                let Some(Some(result)) = preview.get_untracked() else { return };
                                let ids: Vec<String> = chosen.get_untracked().into_iter().collect();
                                apply_sales(sheet, result, ids);
                                message.set("入力欄に反映しました。販売数と金額を確認して保存してください。".into());
                            },
                        )
                    }
                }
            }
            Show(when: move || !message.with(String::is_empty)) {
                Text(value: computed(move || message.get()), style: css!(color: hex(TINT), font_size: px(13), padding: (px(6), px(32))))
            }
        }
    }
}

fn apply_sales(sheet: Sheet, result: SalesResult, ids: Vec<String>) {
    sheet.update(false, move |d| {
        if let Some(c) = result.customer_count {
            d.customer_count = Some(c);
        }
        d.sales = result.sales;
        d.sales_quantities = result.quantities;
        d.receipt_ids = ids;
    });
    sheet.view.update(|v| *v += 1);
}

fn preview_text(sheet: &Sheet, preview: Option<Option<SalesResult>>) -> String {
    let Some(preview) = preview else { return String::new() };
    let Some(result) = preview else {
        return "選択した伝票の売上合計と明細が合いません。証憑OCRで読み取り結果を確認してください。".into();
    };
    let Some(template) = sheet.template() else { return String::new() };
    let mut lines = Vec::new();
    for row in template.rows.iter().filter(|r| r.source != "manual") {
        let amount = result.sales.get(&row.key).copied().flatten();
        let qty = result.quantities.get(&row.key).copied().flatten();
        let text = match (qty, amount) {
            (_, None) => "—".to_owned(),
            (Some(q), Some(a)) => format!("{q}件 ／ {}円", cash::format_yen(a)),
            (None, Some(a)) => format!("{}円", cash::format_yen(a)),
        };
        lines.push(format!("{}　{text}", row.label));
    }
    let mut day_after = sheet.day.get_untracked();
    if let Some(d) = day_after.as_mut() {
        d.sales = result.sales.clone();
    }
    let total_after = day_after.map(|d| d.sales_total()).unwrap_or(0);
    lines.push(format!("売上合計 {}円 ／ 反映後の入出金合計 {}円", cash::format_yen(result.net_sales), cash::format_yen(total_after)));
    match result.customer_count {
        Some(c) => lines.push(format!("客数 {c}人")),
        None => lines.push("客数を読み取れない伝票があるため、入力済みの客数を残します。".into()),
    }
    if result.remainder != 0 {
        let label = template.row(&template.remainder).map(|r| r.label.clone()).unwrap_or_default();
        lines.push(format!("内訳に振り分けられなかった差額 {}円 を{label}に仮反映します。", cash::format_yen(result.remainder)));
    }
    if result.payment_needs_review {
        lines.push("決済内訳を照合できませんでした。クレジット決済とコード決済の金額を確認してください。".into());
    }
    lines.join("\n")
}

/// 登録済み売上（売上台帳）から販売数と金額を集計。
#[component]
pub(crate) fn product_sales(sheet: Sheet) -> Element {
    let open = signal(false);
    let loading = signal(false);
    let error = signal(String::new());
    let message = signal(String::new());
    let result = signal(None::<(BTreeMap<String, i64>, BTreeMap<String, i64>, Vec<String>, usize)>);
    // 集計に使った伝票。今の伝票と違えば、その集計は使わない（別の伝票の数で上書きしないように）。
    let counted_for = signal(Vec::<String>::new());
    let current_ids = computed(move || sheet.day.with(|d| d.as_ref().map(|d| d.receipt_ids.clone()).unwrap_or_default()));
    let stale = computed(move || result.with(Option::is_some) && counted_for.get() != current_ids.get());
    let has_receipts = computed(move || sheet.day.with(|d| d.as_ref().is_some_and(|d| !d.receipt_ids.is_empty())));
    let load = move || {
        let (Some(tenant), Some(day)) = (sheet.tenant.get_untracked(), sheet.day.get_untracked()) else { return };
        let Some(template) = day.template() else { return };
        loading.set(true);
        error.set(String::new());
        spawn_in(sheet.owner, async move {
            let d = day.clone();
            let fetched = authed(sheet.token, move |access| {
                let mut entries = Vec::new();
                for id in &d.receipt_ids {
                    let value = api::sales_ledger(access, &tenant, &d.date, id)?;
                    let items = value["items"].as_array().cloned().unwrap_or_default();
                    let reported: i64 = value["summaries"].as_array().into_iter().flatten().filter_map(|s| s["entryCount"].as_i64()).sum();
                    if items.len() >= 500 || reported > items.len() as i64 {
                        return Err("売上台帳の取得上限に達したため、正確に集計できません。".into());
                    }
                    entries.extend(items);
                }
                Ok(entries)
            })
            .await;
            loading.set(false);
            let ids = day.receipt_ids.clone();
            match fetched.and_then(|entries| model::product_sales(&day, &template, &entries)) {
                Ok(found) => {
                    counted_for.set(ids);
                    result.set(Some(found));
                }
                Err(e) => error.set(if e.is_empty() { "売上台帳を取得できませんでした。".into() } else { e }),
            }
        });
    };
    let summary = computed(move || {
        let Some((quantities, amounts, unmatched, _)) = result.get() else { return String::new() };
        let template = sheet.template();
        let mut lines: Vec<String> = quantities
            .iter()
            .map(|(k, q)| {
                let label = template.as_ref().and_then(|t| t.row(k)).map(|r| r.label.clone()).unwrap_or_else(|| k.clone());
                format!("{label} {q}件 ／ {}円", cash::format_yen(amounts.get(k).copied().unwrap_or(0)))
            })
            .collect();
        if lines.is_empty() {
            lines.push("この日の対象商品の確定済み売上はありません。".into());
        }
        if !unmatched.is_empty() {
            lines.push(format!("対象外の商品 {}", unmatched.join("、")));
        }
        lines.push("反映すると、上記の項目の販売数と売上金額を置き換えます。決済方法別の控除は確認してください。".into());
        lines.join("\n")
    });
    render! {
        Show(when: move || has_receipts.get()) {
            View(style: group_style().margin_top(px(12))) {
                FieldRow(
                    label: computed(|| "登録済み売上から集計".to_owned()),
                    value: computed(move || if open.get() { "閉じる".to_owned() } else { "開く".to_owned() }),
                    on_tap: move |_| { let next = !open.get_untracked(); open.set(next); if next { load(); } },
                )
                Show(when: move || open.get()) {
                    Hint(text: "このレジ管理表に取り込んだ伝票にひもづく確定済み売上を、商品設定に沿って集計します。")
                    Show(when: move || loading.get()) { Hint(text: "売上台帳を集計中…") }
                    Show(when: move || !error.with(String::is_empty)) {
                        Text(value: computed(move || error.get()), style: css!(color: hex(RED), font_size: px(13), padding: (px(6), px(16))))
                        FieldRow(label: computed(|| "再取得".to_owned()), value: computed(String::new), on_tap: move |_| load())
                    }
                    Show(when: move || stale.get()) {
                        Text(value: "伝票を入れ替えたので、集計し直してください。", style: css!(color: hex(RED), font_size: px(13), padding: (px(6), px(16))))
                        FieldRow(label: computed(|| "集計し直す".to_owned()), value: computed(String::new), on_tap: move |_| load())
                    }
                    Show(when: move || result.with(Option::is_some) && !stale.get()) {
                        Text(value: summary, style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(16))))
                        Show(when: move || result.with(|r| r.as_ref().is_some_and(|r| r.3 > 0))) {
                            FieldRow(
                                label: computed(|| "販売数・金額を反映".to_owned()),
                                value: computed(String::new),
                                on_tap: move |_| {
                                    if stale.get() { return; }
                                    let Some((quantities, amounts, _, _)) = result.get_untracked() else { return };
                                    sheet.update(false, move |d| {
                                        for (k, v) in amounts { d.sales.insert(k, Some(v)); }
                                        for (k, v) in quantities { d.sales_quantities.insert(k, Some(v)); }
                                    });
                                    sheet.view.update(|v| *v += 1);
                                    message.set("反映しました。必要に応じて修正し、保存してください。".into());
                                },
                            )
                        }
                    }
                }
            }
            Show(when: move || !message.with(String::is_empty)) {
                Text(value: computed(move || message.get()), style: css!(color: hex(TINT), font_size: px(13), padding: (px(6), px(32))))
            }
        }
    }
}
