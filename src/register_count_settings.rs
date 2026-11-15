//! 帳簿設定のうち、レジ管理表（registerCount）の部分。Web 版の register-count-settings-editor.tsx と同じ項目。

use serde_json::{Value, json};

use super::*;
use crate::cash_book_receipts::normalize_text;
use crate::cash_book_screen::*;
use crate::cash_book_settings as rules;
use crate::cash_book_settings_screen::{
    Choice, Editor, Hint, PrintEditor, RemoveRow, push, remove_at, rows_of,
};
use crate::register_count as model;

const SOURCES: [(&str, &str); 4] = [
    ("manual", "手入力"),
    ("items", "売上明細・商品"),
    ("credit", "OCR クレジット"),
    ("codePayment", "OCR コード・電子マネー"),
];
const EFFECTS: [(&str, &str); 3] = [("add", "加算"), ("subtract", "控除"), ("memo", "集計外")];

fn pairs(list: &'static [(&'static str, &'static str)]) -> Vec<(String, String)> {
    list.iter().map(|(k, l)| ((*k).to_owned(), (*l).to_owned())).collect()
}

#[component]
pub(crate) fn register_editor(editor: Editor) -> Element {
    let configured = computed(move || editor.draft.with(|d| !d["registerCount"].is_null()));
    let item_rows = computed(move || {
        editor.draft.with(|d| {
            rows_of(&d["registerCount"]["rows"])
                .iter()
                .filter(|r| r["source"] == "items")
                .map(|r| (r["key"].as_str().unwrap_or("").to_owned(), r["label"].as_str().filter(|l| !l.is_empty()).unwrap_or("名称未設定").to_owned()))
                .collect::<Vec<_>>()
        })
    });
    let remainder_rows = computed(move || {
        editor.draft.with(|d| {
            rows_of(&d["registerCount"]["rows"])
                .iter()
                .filter(|r| r["source"] == "items" && r["effect"] == "add")
                .map(|r| (r["key"].as_str().unwrap_or("").to_owned(), r["label"].as_str().filter(|l| !l.is_empty()).unwrap_or("名称未設定").to_owned()))
                .collect::<Vec<_>>()
        })
    });
    let area_filter = signal(String::new());
    let areas = computed(move || {
        let q = normalize_text(&area_filter.get());
        let mut list = vec![(String::new(), "自動取得しない".to_owned())];
        list.extend(
            model::areas()
                .into_iter()
                .filter(|a| q.is_empty() || normalize_text(&format!("{}{}", a.prefecture, a.name)).contains(&q))
                .map(|a| (a.code, format!("{}・{}", a.prefecture, a.name))),
        );
        list
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Show(when: move || !configured.get()) {
                Hint(text: "この店舗・レジの日次記録に使う実績項目と印刷形式を設定します。")
                Button(label: "レジ管理表を設定する", on_tap: move |_| editor.edit(true, |d| d["registerCount"] = model::default_template()))
            }
            Show(when: move || configured.get()) {
                SectionHeader(title: computed(|| "天気の自動取得地域".to_owned()))
                View(style: group_style()) {
                    TextRow(label: "地域を絞り込む", text: area_filter, editable: true, on_text: |_| {})
                    Choice(
                        label: computed(|| "取得地域".to_owned()),
                        options: areas,
                        selected: computed(move || editor.draft.with(|d| d["registerCount"]["weatherAreaCode"].as_str().unwrap_or("").to_owned())),
                        on_pick: move |code: String| editor.edit(false, move |d| {
                            if code.is_empty() {
                                if let Some(o) = d["registerCount"].as_object_mut() { o.remove("weatherAreaCode"); }
                            } else {
                                d["registerCount"]["weatherAreaCode"] = json!(code);
                            }
                        }),
                    )
                }

                SectionHeader(title: computed(|| "実績項目と集計".to_owned()))
                Hint(text: "加算・控除の項目から入出金合計を求め、終了時と開始時の現金差額と照合します。集計外の項目は控えとして記録します。")
                ForEach(
                    each: move || {
                        let view = editor.view.get();
                        let n = editor.draft.with(|d| rows_of(&d["registerCount"]["rows"]).len());
                        (0..n).map(|i| (view, i)).collect::<Vec<_>>()
                    },
                    key: |k: &(u64, usize)| *k,
                    children: move |k: ReadSignal<(u64, usize)>| {
                        let (_, i) = k.get_untracked();
                        render! { RegisterRow(editor: editor, index: i) }
                    },
                )
                Button(label: "実績項目を追加", on_tap: move |_| editor.edit(true, |d| {
                    if rows_of(&d["registerCount"]["rows"]).len() >= 30 { return; }
                    push(&mut d["registerCount"]["rows"], json!({ "key": rules::new_key(), "label": "", "effect": "add", "source": "items", "quantity": false }));
                }))
                View(style: group_style().margin_top(px(12))) {
                    Choice(
                        label: computed(|| "売上合計と内訳の差額を入れる項目".to_owned()),
                        options: remainder_rows,
                        selected: computed(move || editor.draft.with(|d| d["registerCount"]["remainder"].as_str().unwrap_or("").to_owned())),
                        on_pick: move |k: String| editor.edit(false, move |d| d["registerCount"]["remainder"] = json!(k)),
                    )
                }

                SectionHeader(title: computed(|| "OCRの振り分け".to_owned()))
                Hint(text: "伝票の表記、商品マスタの商品名・SKUに一致した明細を、指定した実績項目へ取り込みます。")
                ForEach(
                    each: move || {
                        let view = editor.view.get();
                        let n = editor.draft.with(|d| rows_of(&d["registerCount"]["rules"]).len());
                        (0..n).map(|i| (view, i)).collect::<Vec<_>>()
                    },
                    key: |k: &(u64, usize)| *k,
                    children: move |k: ReadSignal<(u64, usize)>| {
                        let (_, i) = k.get_untracked();
                        let rule = editor.draft.with_untracked(|d| d["registerCount"]["rules"][i].clone());
                        let linked = rule["productId"].is_string();
                        let text = signal(rule["text"].as_str().unwrap_or("").to_owned());
                        render! {
                            View(style: group_style().margin_bottom(px(8))) {
                                TextRow(label: if linked { "商品マスタ連携・伝票での別名" } else { "伝票での表記" }, text: text, editable: true, on_text: move |t: String| editor.edit(false, move |d| d["registerCount"]["rules"][i]["text"] = json!(t)))
                                Choice(
                                    label: computed(|| "振り分け先".to_owned()),
                                    options: item_rows,
                                    selected: computed(move || editor.draft.with(|d| d["registerCount"]["rules"][i]["target"].as_str().unwrap_or("").to_owned())),
                                    on_pick: move |k: String| editor.edit(false, move |d| d["registerCount"]["rules"][i]["target"] = json!(k)),
                                )
                                RemoveRow(on_tap: move |_| editor.edit(true, move |d| remove_at(&mut d["registerCount"]["rules"], i)))
                            }
                        }
                    },
                )
                Button(label: "伝票の表記を追加", on_tap: move |_| editor.edit(true, |d| {
                    if rows_of(&d["registerCount"]["rules"]).len() >= 150 { return; }
                    let target = rows_of(&d["registerCount"]["rows"]).iter().find(|r| r["source"] == "items").and_then(|r| r["key"].as_str().map(str::to_owned)).unwrap_or_default();
                    push(&mut d["registerCount"]["rules"], json!({ "text": "", "target": target }));
                }))
                RegisterProductLink(editor: editor, targets: item_rows)

                SectionHeader(title: computed(|| "レジ管理表の印刷形式".to_owned()))
                PrintEditor(editor: editor, base: "registerCount")
            }
        }
    }
}

/// 実績項目 1 行：名前・入力元・集計方法・販売数・上へ・削除。
#[component]
fn register_row(editor: Editor, index: usize) -> Element {
    let row = editor.draft.with_untracked(|d| d["registerCount"]["rows"][index].clone());
    let label = signal(row["label"].as_str().unwrap_or("").to_owned());
    let get = move |k: &'static str| computed(move || editor.draft.with(|d| d["registerCount"]["rows"][index][k].as_str().unwrap_or("").to_owned()));
    let source = get("source");
    let effect = get("effect");
    let quantity = computed(move || editor.draft.with(|d| d["registerCount"]["rows"][index]["quantity"] == Value::Bool(true)));
    let is_remainder = computed(move || editor.draft.with(|d| d["registerCount"]["rows"][index]["key"] == d["registerCount"]["remainder"]));
    let payment = computed(move || matches!(source.get().as_str(), "credit" | "codePayment"));
    render! {
        View(style: group_style().margin_bottom(px(8))) {
            TextRow(label: "項目名", text: label, editable: true, on_text: move |t: String| editor.edit(false, move |d| d["registerCount"]["rows"][index]["label"] = json!(t)))
            Choice(
                label: computed(|| "入力元".to_owned()),
                options: computed(|| pairs(&SOURCES)),
                selected: source,
                on_pick: move |s: String| editor.edit(true, move |d| {
                    // 決済の入力元は 1 つずつ。選んだら控除・販売数なしにそろえる。
                    let taken = rows_of(&d["registerCount"]["rows"]).iter().enumerate().any(|(i, r)| i != index && r["source"] == s.as_str());
                    if (s == "credit" || s == "codePayment") && taken { return; }
                    let row = &mut d["registerCount"]["rows"][index];
                    row["source"] = json!(s);
                    if s == "credit" || s == "codePayment" {
                        row["effect"] = json!("subtract");
                        row["quantity"] = json!(false);
                    } else if s == "items" && row["effect"] == "memo" {
                        row["effect"] = json!("add");
                    }
                }),
            )
            Show(when: move || !payment.get()) {
                Choice(
                    label: computed(|| "集計方法".to_owned()),
                    options: computed(move || pairs(&EFFECTS).into_iter().filter(|(k, _)| !(k == "memo" && source.get() == "items")).collect()),
                    selected: effect,
                    on_pick: move |e: String| editor.edit(false, move |d| d["registerCount"]["rows"][index]["effect"] = json!(e)),
                )
                FieldRow(
                    label: computed(|| "販売数を記録".to_owned()),
                    value: computed(move || if quantity.get() { "✓".to_owned() } else { "しない".to_owned() }),
                    on_tap: move |_| editor.edit(false, |d| {
                        let row = &mut d["registerCount"]["rows"][index];
                        row["quantity"] = json!(row["quantity"] != Value::Bool(true));
                    }),
                )
            }
            Show(when: move || index > 0) {
                FieldRow(label: computed(|| "上へ".to_owned()), value: computed(String::new), on_tap: move |_| editor.edit(true, move |d| {
                    if let Some(rows) = d["registerCount"]["rows"].as_array_mut() { rows.swap(index, index - 1); }
                }))
            }
            Show(when: move || !is_remainder.get()) {
                RemoveRow(on_tap: move |_| editor.edit(true, move |d| {
                    let key = d["registerCount"]["rows"][index]["key"].clone();
                    remove_at(&mut d["registerCount"]["rows"], index);
                    if let Some(list) = d["registerCount"]["rules"].as_array_mut() {
                        list.retain(|r| r["target"] != key);
                    }
                }))
            }
        }
    }
}

/// 商品マスタを探して、伝票の明細の振り分けに足す。
#[component]
fn register_product_link(editor: Editor, targets: Signal<Vec<(String, String)>>) -> Element {
    let query = signal(String::new());
    let products = signal(Vec::<Value>::new());
    let chosen = signal(String::new());
    let error = signal(String::new());
    let search = move || {
        let Some(tenant) = editor.tenant.get_untracked() else { return };
        let q = query.get_untracked();
        spawn_local(async move {
            match authed(editor.token, move |access| api::search_products(access, &tenant, &q, 0)).await {
                Ok((items, _)) => { products.set(items); error.set(String::new()); }
                Err(e) => error.set(e),
            }
        });
    };
    let options = computed(move || {
        products.with(|list| {
            list.iter()
                .map(|p| (p["id"].as_str().unwrap_or("").to_owned(), format!("{}{}", p["name"].as_str().unwrap_or(""), p["skuCode"].as_str().map(|s| format!("（{s}）")).unwrap_or_default())))
                .collect::<Vec<_>>()
        })
    });
    render! {
        View(style: group_style().margin_top(px(12))) {
            TextRow(label: "商品名・SKU", text: query, editable: true, on_text: |_| {})
            FieldRow(label: computed(|| "商品マスタを検索".to_owned()), value: computed(move || format!("{}件", products.with(Vec::len))), on_tap: move |_| search())
            Show(when: move || !error.with(String::is_empty)) {
                Text(value: computed(move || error.get()), style: css!(color: hex(RED), font_size: px(13), padding: (px(6), px(16))))
            }
            Show(when: move || !products.with(Vec::is_empty)) {
                Choice(label: computed(|| "商品".to_owned()), options: options, selected: computed(move || chosen.get()), on_pick: move |id: String| chosen.set(id))
                FieldRow(
                    label: computed(|| "選択した商品を追加".to_owned()),
                    value: computed(String::new),
                    on_tap: move |_| {
                        let id = chosen.get_untracked();
                        let Some(p) = products.with_untracked(|l| l.iter().find(|p| p["id"] == id.as_str()).cloned()) else { return };
                        let Some((target, _)) = targets.get().into_iter().next() else { return };
                        let units: Vec<u16> = p["name"].as_str().unwrap_or("").encode_utf16().take(100).collect();
                        let text = String::from_utf16_lossy(&units);
                        editor.edit(true, move |d| {
                            if let Some(list) = d["registerCount"]["rules"].as_array_mut() { list.retain(|r| r["productId"] != id.as_str()); }
                            push(&mut d["registerCount"]["rules"], json!({ "text": text, "productId": id, "target": target }));
                        });
                    },
                )
            }
        }
    }
}
