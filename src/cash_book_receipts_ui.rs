//! 出納帳の画面で、レシートを入れる部分（Web 版の cash-book-page.tsx の収入・支出の欄と同じ）。
//!
//! - 支出：この日に現金で買った仕入のレシートを選んで行にする。ほかの日に買ったもの（立替の精算など）も選べる
//! - 収入：締めレシートを撮るか、読み取り済みのものから入れる。品目ごとに収入の欄へ振り分けて足す
//!
//! 開いた日を変えると、この部品ごと作り直される（前の日のレシートを別の日に入れないため）。

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use whisker::Owner;
use crate::cash_book::{self as model, Day};
use crate::cash_book_receipts::{self as rules, Receipt, SalesIncome};
use crate::cash_book_screen::*;
use crate::cash_book_settings_screen::Hint;

/// アップロードした日で見る、前後の幅（紙のレシートは何週間か後に上げることがある）。
const UPLOAD_LAG_DAYS: i64 = 60;
/// ほかの日のレシートを選べる幅（前後）。
const OTHER_DAYS_WINDOW: i64 = 31;
/// ほかの日に入れてあるか確かめる幅（前後）。
const USAGE_DAYS: i64 = 92;

/// この日のレシートの状態。
#[derive(Clone, Copy)]
pub(crate) struct Receipts {
    ledger: Ledger,
    list: RwSignal<Vec<Receipt>>,
    truncated: RwSignal<bool>,
    loading: RwSignal<bool>,
    error: RwSignal<String>,
    /// どこかの帳簿にもう入っているもの（サーバーで確かめた）。
    claimed: RwSignal<HashSet<String>>,
    /// ほかの日の出納帳に入れてあるものと、その日。
    used_elsewhere: RwSignal<HashMap<String, String>>,
    /// 最初の選び方から切り替えたレシート。
    toggled: RwSignal<HashSet<String>>,
    chosen_sales: RwSignal<HashSet<String>>,
    other_open: RwSignal<bool>,
    other: RwSignal<Vec<Receipt>>,
    other_loading: RwSignal<bool>,
    other_error: RwSignal<String>,
    other_truncated: RwSignal<bool>,
    other_chosen: RwSignal<HashSet<String>>,
    last_income: RwSignal<Option<SalesIncome>>,
    busy: RwSignal<bool>,
    scanning: RwSignal<bool>,
    /// 一言のお知らせ（入れた枚数・断られた理由など）。
    message: RwSignal<String>,
    /// 開いたときの読み込みの番号（日を変えたら古い結果を捨てる）。
    generation: u64,
    /// その日の画面の持ち主（日を変えたら止まる。ボタンが消えても止まらない）。
    owner: Option<Owner>,
}

fn today_plus(days: i64) -> String {
    model::shift_date(&model::tokyo_today(), days)
}

fn min_date(a: String, b: String) -> String {
    if a < b { a } else { b }
}

/// 種類ごとにレシートを読む。
fn read_receipts(
    access: &str,
    tenant: &Tenant,
    types: &[&str],
    from: &str,
    to: &str,
) -> Result<(Vec<Receipt>, bool), String> {
    let mut all = Vec::new();
    let mut truncated = false;
    for kind in types {
        let (items, cut) = api::list_receipts(access, tenant, kind, from, to)?;
        truncated |= cut;
        all.extend(items.iter().filter_map(Receipt::from_value));
    }
    Ok((all, truncated))
}

impl Receipts {
    fn new(ledger: Ledger) -> Self {
        Self {
            ledger,
            list: signal(Vec::new()),
            truncated: signal(false),
            loading: signal(true),
            error: signal(String::new()),
            claimed: signal(HashSet::new()),
            used_elsewhere: signal(HashMap::new()),
            toggled: signal(HashSet::new()),
            chosen_sales: signal(HashSet::new()),
            other_open: signal(false),
            other: signal(Vec::new()),
            other_loading: signal(false),
            other_error: signal(String::new()),
            other_truncated: signal(false),
            other_chosen: signal(HashSet::new()),
            last_income: signal(None),
            busy: signal(false),
            scanning: signal(false),
            message: signal(String::new()),
            generation: ledger.generation.get_untracked(),
            owner: Owner::current(),
        }
    }

    fn current(&self) -> bool {
        self.ledger.generation.get_untracked() == self.generation
    }

    fn with_book(&self) -> bool {
        self.ledger.book.with_untracked(Option::is_some)
    }

    /// この日のレシートと、ほかの日の使い済みを読む。
    fn load(self) {
        let Some(tenant) = self.ledger.tenant.get_untracked() else { return };
        let date = self.ledger.date.get_untracked();
        let book_id = self.ledger.book_id();
        self.loading.set(true);
        self.error.set(String::new());
        spawn_in(self.owner, async move {
            let from = model::shift_date(&date, -1);
            let to = min_date(model::shift_date(&date, UPLOAD_LAG_DAYS), today_plus(1));
            let d = date.clone();
            let result = authed(self.ledger.token, move |access| {
                let (list, truncated) =
                    read_receipts(access, &tenant, &["purchase", "unsorted", "sales"], &from, &to)?;
                // ほかの日の出納帳に入れてあるレシート（前後 92 日）。
                let mut days: Vec<Day> = Vec::new();
                for (a, b) in [
                    (model::shift_date(&d, -USAGE_DAYS), model::shift_date(&d, -1)),
                    (model::shift_date(&d, 1), model::shift_date(&d, USAGE_DAYS)),
                ] {
                    let value = api::cash_book_days(access, &tenant, &a, &b, book_id.as_deref())?;
                    days.extend(model::saved_days(&value, false).into_iter().map(|s| s.day));
                }
                let ids: Vec<String> = list.iter().map(|r| r.id.clone()).collect();
                let claimed = api::receipt_uses(access, &tenant, &ids, false).unwrap_or_default();
                Ok((list, truncated, days, claimed))
            })
            .await;
            if !self.current() {
                return;
            }
            self.loading.set(false);
            match result {
                Ok((list, truncated, days, claimed)) => {
                    // この日にもう入れてあるものは「使い済み」に数えない。
                    let own: HashSet<String> = self.own_ids();
                    self.claimed.set(claimed.into_iter().filter(|id| !own.contains(id)).collect());
                    self.used_elsewhere.set(rules::receipts_used_on_other_days(&days, &date));
                    self.list.set(list);
                    self.truncated.set(truncated);
                }
                Err(message) => self.error.set(message),
            }
        });
    }

    /// この日にもう入れてあるレシート（支出の行と締めレシート）。
    fn own_ids(&self) -> HashSet<String> {
        self.ledger.day.with_untracked(|d| {
            d.as_ref()
                .map(|d| {
                    d.expenses
                        .iter()
                        .filter_map(|e| e.receipt_id.clone())
                        .chain(d.sales_receipt_ids.iter().cloned())
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    fn is_new(&self, id: &str) -> bool {
        let imported = self.ledger.day.with(|d| {
            d.as_ref()
                .is_some_and(|d| d.expenses.iter().any(|e| e.receipt_id.as_deref() == Some(id)))
        });
        !imported
            && !self.claimed.with(|c| c.contains(id))
            && !self.used_elsewhere.with(|u| u.contains_key(id))
    }

    /// 支出に入れられる、この日のレシート。
    fn candidates(&self) -> Vec<Receipt> {
        let date = self.ledger.date.get();
        let list = self.list.get();
        rules::receipts_for_day(&list, &date)
            .0
            .into_iter()
            .filter(|r| self.is_new(&r.id))
            .collect()
    }

    fn reading_count(&self) -> usize {
        let date = self.ledger.date.get();
        self.list.with(|list| rules::receipts_for_day(list, &date).1)
    }

    /// 帳簿を選んでいれば全部外した状態から。従来の帳簿は現金のものに印を付けておく。
    fn is_chosen(&self, r: &Receipt) -> bool {
        (self.with_book() || r.is_cashless()) == self.toggled.with(|t| t.contains(&r.id))
    }

    fn chosen(&self) -> Vec<Receipt> {
        let mut list: Vec<Receipt> = self.candidates().into_iter().filter(|r| self.is_chosen(r)).collect();
        let others = self.other_chosen.get();
        list.extend(self.other.get().into_iter().filter(|r| others.contains(&r.id) && self.is_new(&r.id)));
        list
    }

    fn load_other(self) {
        let Some(tenant) = self.ledger.tenant.get_untracked() else { return };
        let date = self.ledger.date.get_untracked();
        self.other_loading.set(true);
        self.other_error.set(String::new());
        spawn_in(self.owner, async move {
            let from = model::shift_date(&date, -OTHER_DAYS_WINDOW);
            let to = min_date(model::shift_date(&date, OTHER_DAYS_WINDOW), today_plus(1));
            let result = authed(self.ledger.token, move |access| {
                let (list, truncated) = read_receipts(access, &tenant, &["purchase", "unsorted"], &from, &to)?;
                let ids: Vec<String> = list.iter().map(|r| r.id.clone()).collect();
                let used = api::receipt_uses(access, &tenant, &ids, false)?;
                Ok((list, truncated, used))
            })
            .await;
            if !self.current() {
                return;
            }
            self.other_loading.set(false);
            match result {
                Ok((list, truncated, used)) => {
                    let used: HashSet<String> = used.into_iter().collect();
                    let date = self.ledger.date.get_untracked();
                    self.other.set(
                        rules::receipts_for_other_days(&list, &date)
                            .into_iter()
                            .filter(|r| !used.contains(&r.id))
                            .collect(),
                    );
                    self.other_truncated.set(truncated);
                }
                Err(_) => self.other_error.set("ほかの日のレシートを読み込めませんでした。".into()),
            }
        });
    }

    /// 選んだレシートを支出の行にする。直前にもう一度、使い済みでないか確かめる。
    fn import_expenses(self) {
        let to_import = self.chosen();
        if to_import.is_empty() || self.busy.get_untracked() || self.ledger.confirmed() {
            return;
        }
        let Some(tenant) = self.ledger.tenant.get_untracked() else { return };
        self.busy.set(true);
        let ids: Vec<String> = to_import.iter().map(|r| r.id.clone()).collect();
        spawn_in(self.owner, async move {
            let result = authed(self.ledger.token, move |access| api::receipt_uses(access, &tenant, &ids, false)).await;
            self.busy.set(false);
            if !self.current() {
                return;
            }
            let used: HashSet<String> = match result {
                Ok(used) => used.into_iter().collect(),
                Err(e) => {
                    self.message.set(format!(
                        "レシートが使用済みか確認できませんでした。もう一度押してください。（{e}）"
                    ));
                    return;
                }
            };
            let skipped = to_import.iter().filter(|r| used.contains(&r.id)).count();
            self.claimed.update(|c| c.extend(used.iter().cloned()));
            let template = self.ledger.template();
            let history = rules::payee_history(&self.ledger.nearby.get_untracked());
            let rows: Vec<model::Expense> = to_import
                .iter()
                .filter(|r| !used.contains(&r.id))
                .map(|r| {
                    let payee = r.counterparty.clone().unwrap_or_default();
                    let category = rules::category_for_receipt(r, &payee, &history, &template);
                    rules::expense_from_receipt(r, category)
                })
                .collect();
            let added = rows.len();
            self.ledger.update(false, move |d| {
                for row in rows {
                    if !d.expenses.iter().any(|e| e.receipt_id == row.receipt_id) {
                        d.expenses.push(row);
                    }
                }
            });
            self.ledger.view.update(|v| *v += 1);
            self.other_chosen.set(HashSet::new());
            let mut text = format!("レシート {added} 枚を支出に入れました。");
            if skipped > 0 {
                text.push_str(&format!("{skipped} 枚は、すでに帳簿に入っていたため取り込みませんでした。"));
            }
            self.message.set(text);
        });
    }

    /// この日の締めレシート（売上）。
    fn sales_for_day(&self) -> Vec<Receipt> {
        let date = self.ledger.date.get();
        self.list.with(|list| {
            list.iter()
                .filter(|r| r.is_sales() && r.status == "done" && r.sales_day() == date)
                .cloned()
                .collect()
        })
    }

    fn imported_sales(&self) -> HashSet<String> {
        self.ledger.day.with(|d| {
            d.as_ref()
                .map(|d| d.sales_receipt_ids.iter().cloned().collect())
                .unwrap_or_default()
        })
    }

    fn new_sales(&self) -> Vec<Receipt> {
        let imported = self.imported_sales();
        self.sales_for_day()
            .into_iter()
            .filter(|r| r.is_closing() && !imported.contains(&r.id) && !self.claimed.with(|c| c.contains(&r.id)))
            .collect()
    }

    fn sales_to_import(&self) -> Vec<Receipt> {
        let list = self.new_sales();
        if self.with_book() {
            let chosen = self.chosen_sales.get();
            list.into_iter().filter(|r| chosen.contains(&r.id)).collect()
        } else {
            list
        }
    }

    /// 締めレシートを収入に足す。
    fn apply_sales(self, candidates: Vec<Receipt>) {
        if candidates.is_empty() || self.busy.get_untracked() || self.ledger.confirmed() {
            return;
        }
        let Some(tenant) = self.ledger.tenant.get_untracked() else { return };
        let template = self.ledger.template();
        let product_ids: Vec<String> = template.raw["incomeRules"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["productId"].as_str().map(str::to_owned))
            .collect();
        let ids: Vec<String> = candidates.iter().map(|r| r.id.clone()).collect();
        self.busy.set(true);
        spawn_in(self.owner, async move {
            let result = authed(self.ledger.token, move |access| {
                let used = api::receipt_uses(access, &tenant, &ids, false)?;
                let mut products: HashMap<String, Value> = HashMap::new();
                for id in &product_ids {
                    let product = api::get_product(access, &tenant, id)
                        .map_err(|_| "商品マスタを読み直してから取り込んでください。".to_owned())?;
                    products.insert(id.clone(), product);
                }
                Ok((used, products))
            })
            .await;
            self.busy.set(false);
            if !self.current() || self.ledger.confirmed() {
                return;
            }
            let (used, products) = match result {
                Ok(value) => value,
                Err(message) => {
                    self.message.set(message);
                    return;
                }
            };
            let used: HashSet<String> = used.into_iter().collect();
            self.claimed.update(|c| c.extend(used.iter().cloned()));
            let imported = self.imported_sales();
            let sales: Vec<Receipt> = candidates
                .into_iter()
                .filter(|r| !used.contains(&r.id) && !imported.contains(&r.id))
                .collect();
            let skipped = ids_len(&used);
            if sales.is_empty() {
                if skipped > 0 {
                    self.message.set(format!("{skipped} 枚は、すでに帳簿に入っていたため取り込みませんでした。"));
                }
                return;
            }
            let income = rules::income_from_sales(&sales, &template, &products);
            let added: Vec<(String, i64)> = income.incomes.clone();
            let new_ids: Vec<String> = sales.iter().map(|r| r.id.clone()).collect();
            let count = new_ids.len();
            self.ledger.update(false, move |d| {
                for (key, value) in added {
                    let entry = d.incomes.entry(key).or_insert(None);
                    *entry = Some(entry.unwrap_or(0) + value);
                }
                d.sales_receipt_ids.extend(new_ids);
            });
            self.ledger.view.update(|v| *v += 1);
            self.last_income.set(Some(income));
            self.chosen_sales.set(HashSet::new());
            self.message.set(format!("締めレシート {count} 枚を収入に入れました。"));
        });
    }

    /// 締めレシートを撮って（選んで）上げ、読み取りを待って収入に入れる。
    fn scan(self, file: field_glass::PickedFile) {
        if self.scanning.get_untracked() || self.ledger.confirmed() {
            return;
        }
        let Some(tenant) = self.ledger.tenant.get_untracked() else { return };
        let bytes = match std::fs::read(&file.path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.message.set(format!("ファイルを読み込めませんでした。{e}"));
                return;
            }
        };
        let content_type = if file.content_type.is_empty() { "image/jpeg".to_owned() } else { file.content_type.clone() };
        let name = if file.name.is_empty() { "receipt.jpg".to_owned() } else { file.name.clone() };
        let scanned_for = self.ledger.date.get_untracked();
        self.scanning.set(true);
        self.message.set(String::new());
        spawn_in(self.owner, async move {
            let result = authed(self.ledger.token, move |access| {
                let uploaded = api::upload_receipt(access, &tenant, &name, &content_type, &bytes, "unsorted", None)?;
                let id = uploaded["id"].as_str().ok_or("アップロードできませんでした。")?.to_owned();
                let mut receipt = uploaded;
                let mut sort_wait = 0;
                for _ in 0..60 {
                    let status = receipt["status"].as_str().unwrap_or("");
                    let waiting_sort = status == "done"
                        && receipt["receiptType"] == "unsorted"
                        && Receipt::from_value(&receipt).is_some_and(|r| r.suggested_type.is_some())
                        && sort_wait < 3;
                    if status != "pending" && status != "processing" && !waiting_sort {
                        return Ok(Some(receipt));
                    }
                    if waiting_sort {
                        sort_wait += 1;
                    }
                    std::thread::sleep(Duration::from_secs(2));
                    receipt = api::get_receipt(access, &tenant, &id)?;
                }
                Ok(None)
            })
            .await;
            self.scanning.set(false);
            let receipt = match result {
                Ok(Some(value)) => Receipt::from_value(&value),
                Ok(None) => {
                    self.message.set(
                        "読み取りに時間がかかっています。少ししてから「締めレシートから入れる」を押してください。".into(),
                    );
                    return;
                }
                Err(message) => {
                    self.message.set(message);
                    return;
                }
            };
            let Some(receipt) = receipt else { return };
            if !self.current() || self.ledger.date.get_untracked() != scanned_for {
                self.message.set(format!("{scanned_for} の締めレシートを読み取りました。その日を開いて入れてください。"));
                return;
            }
            self.list.update(|l| {
                l.retain(|r| r.id != receipt.id);
                l.push(receipt.clone());
            });
            if receipt.status != "done" {
                self.message.set(
                    receipt
                        .error_message
                        .clone()
                        .unwrap_or_else(|| "レシートを読み取れませんでした。写真を撮り直してください。".into()),
                );
                return;
            }
            if receipt.is_purchase() {
                self.message.set("締めレシートではなく、買い物のレシートと読み取りました。仕入として残したので、支出の「選んだレシートを入れる」から入れてください。".into());
                return;
            }
            if self.imported_sales().contains(&receipt.id) {
                self.message.set("この締めレシートは、もう収入に入れてあります。".into());
                return;
            }
            let read = receipt.sales_day();
            let warning = (read != scanned_for)
                .then(|| format!("レシートの日付は {read} と読み取りました。日付が合っているか確かめてください。"));
            self.apply_sales(vec![receipt]);
            if let Some(w) = warning {
                self.message.update(|m| *m = format!("{w}{m}"));
            }
        });
    }
}

fn ids_len(set: &HashSet<String>) -> usize {
    set.len()
}

/// 収入の欄の下：締めレシート。
#[component]
pub(crate) fn sales_panel(receipts: Receipts) -> Element {
    let r = receipts;
    let confirmed = computed(move || r.ledger.confirmed());
    let count = computed(move || r.sales_to_import().len());
    let new_sales = computed(move || r.new_sales());
    let not_closing = computed(move || {
        let imported = r.imported_sales();
        r.sales_for_day()
            .into_iter()
            .filter(|s| !s.is_closing() && !imported.contains(&s.id))
            .map(|s| s.counterparty.clone().or(s.file_name.clone()).unwrap_or_else(|| "証憑".into()))
            .collect::<Vec<_>>()
    });
    let imported_count = computed(move || {
        let imported = r.imported_sales();
        r.sales_for_day().iter().filter(|s| imported.contains(&s.id)).count()
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Show(when: move || !confirmed.get()) {
                View(style: group_style().margin_top(px(12))) {
                    View(
                        style: css!(display: Display::Flex, flex_direction: FlexDirection::Row, align_items: AlignItems::Center, gap: px(12), padding: (px(10), px(16))),
                    ) {
                        AttachButton(on_pick: move |file: field_glass::PickedFile| r.scan(file), style: css!(width: px(36), height: px(36)))
                        Text(
                            value: computed(move || if r.scanning.get() { "締めレシートを読み取り中…".to_owned() } else { "締めレシートを撮って入れる".to_owned() }),
                            style: css!(color: hex(TINT), font_size: px(17)),
                        )
                    }
                    Show(when: move || r.with_book() && !new_sales.with(Vec::is_empty)) {
                        Text(value: "この店舗・レジの締めレシートを選択", style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(16), px(0), px(16))))
                        ForEach(
                            each: move || new_sales.get(),
                            key: |s: &Receipt| s.id.clone(),
                            children: move |s: ReadSignal<Receipt>| {
                                let s = s.get_untracked();
                                let id = s.id.clone();
                                let label = format!("{}　{}", s.title(), s.total.map(|t| format!("￥{}", model::format_yen(t.round() as i64))).unwrap_or_default());
                                let checked = { let id = id.clone(); computed(move || r.chosen_sales.with(|c| c.contains(&id))) };
                                render! {
                                    FieldRow(
                                        label: computed(move || label.clone()),
                                        value: computed(move || if checked.get() { "✓".to_owned() } else { "選ぶ".to_owned() }),
                                        on_tap: move |_| { let id = id.clone(); r.chosen_sales.update(move |c| { if !c.remove(&id) { c.insert(id); } }); },
                                    )
                                }
                            },
                        )
                    }
                    FieldRow(
                        label: computed(move || format!("締めレシートから入れる（{}）", count.get())),
                        value: computed(move || if r.busy.get() || r.loading.get() { "確認中…".to_owned() } else { String::new() }),
                        on_tap: move |_| r.apply_sales(r.sales_to_import()),
                    )
                }
            }
            Show(when: move || imported_count.get() > 0 && new_sales.with(Vec::is_empty)) {
                Text(
                    value: computed(move || format!("締めレシート {} 枚を収入に入れてあります。", imported_count.get())),
                    style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(32), px(0), px(32))),
                )
            }
            Show(when: move || !not_closing.with(Vec::is_empty)) {
                Text(
                    value: computed(move || format!(
                        "売上の種類で読み込んだレシートのうち {} はレジの締めレシートに見えないので、収入に入れていません。仕入のレシートなら、証憑OCRで種類を仕入に変えてください。",
                        not_closing.get().join("、"),
                    )),
                    style: css!(color: hex(ORANGE), font_size: px(13), padding: (px(8), px(32), px(0), px(32))),
                )
            }
            Show(when: move || r.last_income.with(Option::is_some)) {
                Text(
                    value: computed(move || {
                        let template = r.ledger.template();
                        r.last_income.with(|i| {
                            let Some(i) = i else { return String::new() };
                            let mut lines = vec!["締めレシートから入れた内訳（違っていたら上の欄を直してください）".to_owned()];
                            for (name, key, amount) in &i.lines {
                                let label = template.incomes.iter().find(|row| &row.key == key).map_or(key.clone(), |row| row.label.clone());
                                lines.push(format!("・{name} — {label} {}円", model::format_yen(*amount)));
                            }
                            if i.missing_payment_totals {
                                lines.push("支払方法別の合計が読み取れなかったレシートがあります。クレジットとコード決済は手で入れてください。".into());
                            }
                            lines.join("\n")
                        })
                    }),
                    style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(32), px(0), px(32))),
                )
            }
        }
    }
}

/// 支出の欄の上：この日のレシートと、ほかの日のレシート。
#[component]
pub(crate) fn purchase_panel(receipts: Receipts) -> Element {
    let r = receipts;
    let confirmed = computed(move || r.ledger.confirmed());
    let candidates = computed(move || r.candidates());
    let chosen_count = computed(move || r.chosen().len());
    let used_notes = computed(move || {
        let date = r.ledger.date.get();
        let used = r.used_elsewhere.get();
        r.list.with(|list| {
            rules::receipts_for_day(list, &date)
                .0
                .into_iter()
                .filter_map(|x| {
                    used.get(&x.id).map(|d| {
                        format!("{}（{d} に入れてあります）", x.counterparty.clone().unwrap_or_else(|| "支払先未取得".into()))
                    })
                })
                .collect::<Vec<_>>()
        })
    });
    let date_label = computed(move || r.ledger.date.get());
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Show(when: move || r.loading.get()) {
                Hint(text: "レシートを確認中…")
            }
            Show(when: move || !r.error.with(String::is_empty)) {
                Hint(text: "レシートを読み込めませんでした。手で入力はできます。")
            }
            Show(when: move || !confirmed.get() && !candidates.with(Vec::is_empty)) {
                SectionHeader(title: computed(|| "この日の仕入のレシート（まだ入れていないもの）".to_owned()))
                View(style: group_style()) {
                    ForEach(
                        each: move || candidates.get(),
                        key: |x: &Receipt| x.id.clone(),
                        children: move |x: ReadSignal<Receipt>| {
                            let x = x.get_untracked();
                            let label = format!(
                                "{}{}",
                                x.counterparty.clone().or(x.file_name.clone()).unwrap_or_else(|| "支払先未取得".into()),
                                if x.is_cashless() { "（カード払い）" } else { "" },
                            );
                            let amount = x.total.map(|t| format!("{}円", model::format_yen(t.round() as i64))).unwrap_or_else(|| "—".into());
                            let checked = { let x = x.clone(); computed(move || r.is_chosen(&x)) };
                            let id = x.id.clone();
                            render! {
                                FieldRow(
                                    label: computed(move || format!("{label}　{amount}")),
                                    value: computed(move || if checked.get() { "✓".to_owned() } else { "選ぶ".to_owned() }),
                                    on_tap: move |_| { let id = id.clone(); r.toggled.update(move |t| { if !t.remove(&id) { t.insert(id); } }); },
                                )
                            }
                        },
                    )
                    View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding: (px(12), px(16)))) {
                        Text(value: "全部選ぶ", style: css!(color: hex(TINT), font_size: px(15)), on_tap: move |_| {
                            let flip: HashSet<String> = r.candidates().into_iter().filter(|x| r.with_book() || x.is_cashless()).map(|x| x.id).collect();
                            r.toggled.set(flip);
                        })
                        Text(value: "全部外す", style: css!(color: hex(TINT), font_size: px(15)), on_tap: move |_| {
                            let flip: HashSet<String> = r.candidates().into_iter().filter(|x| !(r.with_book() || x.is_cashless())).map(|x| x.id).collect();
                            r.toggled.set(flip);
                        })
                    }
                }
            }
            Show(when: move || !used_notes.with(Vec::is_empty)) {
                Text(
                    value: computed(move || format!("この日に買ったレシートのうち {} は、ほかの日の出納帳に入れてあるので出していません。", used_notes.get().join("、"))),
                    style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(32), px(0), px(32))),
                )
            }
            Show(when: move || r.reading_count() > 0) {
                Text(
                    value: computed(move || format!("読み取り中のレシートが {} 枚あります。読み取りが終わってから入れられます（日付を開き直すと確認し直します）。", r.reading_count())),
                    style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(32), px(0), px(32))),
                )
            }
            Show(when: move || r.truncated.get()) {
                Hint(text: "この前後にアップロードしたレシートが多く、一部が出ていないかもしれません。足りない分は行を追加して入れてください。")
            }
            Show(when: move || !confirmed.get()) {
                View(style: group_style().margin_top(px(8))) {
                    FieldRow(
                        label: computed(move || if r.other_open.get() { "ほかの日のレシートを閉じる".to_owned() } else { "ほかの日に買ったレシートを入れる（立て替えを精算した日など）".to_owned() }),
                        value: computed(String::new),
                        on_tap: move |_| {
                            let open = !r.other_open.get_untracked();
                            r.other_open.set(open);
                            if open { r.load_other(); }
                        },
                    )
                    Show(when: move || r.other_open.get()) {
                        Text(
                            value: computed(move || format!("買った日はレシートの日付のまま残し、この日（{}）の支出として入れます。前後 31 日にアップロードしたレシートから選べます。", date_label.get())),
                            style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(16))),
                        )
                        Show(when: move || r.other_loading.get()) { Hint(text: "レシートを確認中…") }
                        Show(when: move || !r.other_error.with(String::is_empty)) { Hint(text: "ほかの日のレシートを読み込めませんでした。") }
                        Show(when: move || !r.other_loading.get() && r.other_error.with(String::is_empty) && r.other.with(Vec::is_empty)) {
                            Hint(text: "入れられるレシートはありません。")
                        }
                        ForEach(
                            each: move || r.other.get().into_iter().filter(|x| r.is_new(&x.id)).collect::<Vec<_>>(),
                            key: |x: &Receipt| x.id.clone(),
                            children: move |x: ReadSignal<Receipt>| {
                                let x = x.get_untracked();
                                let tag = if x.is_advance() { "（立替）" } else if x.is_cashless() { "（カード払い）" } else { "" };
                                let label = format!(
                                    "{} 購入　{}{tag}　{}",
                                    x.day(),
                                    x.counterparty.clone().unwrap_or_else(|| "支払先未取得".into()),
                                    x.total.map(|t| format!("{}円", model::format_yen(t.round() as i64))).unwrap_or_else(|| "—".into()),
                                );
                                let id = x.id.clone();
                                let checked = { let id = id.clone(); computed(move || r.other_chosen.with(|c| c.contains(&id))) };
                                render! {
                                    FieldRow(
                                        label: computed(move || label.clone()),
                                        value: computed(move || if checked.get() { "✓".to_owned() } else { "選ぶ".to_owned() }),
                                        on_tap: move |_| { let id = id.clone(); r.other_chosen.update(move |c| { if !c.remove(&id) { c.insert(id); } }); },
                                    )
                                }
                            },
                        )
                        Show(when: move || r.other_truncated.get()) {
                            Hint(text: "この前後にアップロードしたレシートが多く、一部が出ていないかもしれません。")
                        }
                    }
                    FieldRow(
                        label: computed(move || if r.loading.get() { "レシートを確認中…".to_owned() } else { format!("選んだレシートを入れる（{}）", chosen_count.get()) }),
                        value: computed(String::new),
                        on_tap: move |_| r.import_expenses(),
                    )
                }
            }
            Hint(text: "法人カード・会社払いのレシートは、店の現金が減らないので入れません。立て替えたレシートは、レジの現金で精算した日に「ほかの日に買ったレシートを入れる」から入れてください。")
        }
    }
}

/// レシートから入れた支出の行の下：読み取りを直す小さなフォーム。
#[component]
pub(crate) fn receipt_fix(ledger: Ledger, row_id: String, receipt_id: String) -> Element {
    let open = signal(false);
    let busy = signal(false);
    let error = signal(String::new());
    let row = ledger
        .day
        .with_untracked(|d| d.as_ref().and_then(|d| d.expenses.iter().find(|e| e.id == row_id).cloned()));
    let payee = signal(row.as_ref().map(|r| r.payee.clone()).unwrap_or_default());
    let date = signal(row.as_ref().and_then(|r| r.purchased_on.clone()).unwrap_or_default());
    let total = signal(row.as_ref().and_then(|r| r.amount).map(|a| a.to_string()).unwrap_or_default());
    let row_id = signal(row_id.clone());
    let receipt_id = signal(receipt_id.clone());
    let save = move || {
        if busy.get_untracked() {
            return;
        }
        let Some(tenant) = ledger.tenant.get_untracked() else { return };
        let Ok(amount) = model::parse_yen(&total.get_untracked()) else {
            error.set("金額は数字で入れてください。".into());
            return;
        };
        let mut reading = json!({ "counterparty": payee.get_untracked().trim() });
        let day = date.get_untracked();
        if !day.trim().is_empty() {
            match rules::normalize_date(&day) {
                Some(d) => reading["date"] = json!(d),
                None => {
                    error.set("日付は 2026-11-06 の形で入れてください。".into());
                    return;
                }
            }
        }
        if let Some(a) = amount {
            reading["total"] = json!(a.to_string());
        }
        let id = receipt_id.get_untracked();
        busy.set(true);
        error.set(String::new());
        spawn_in(ledger.owner, async move {
            let result = authed(ledger.token, move |access| api::correct_receipt(access, &tenant, &id, &reading)).await;
            busy.set(false);
            match result {
                Ok(value) => {
                    let Some(fixed) = Receipt::from_value(&value) else { return };
                    let row = row_id.get_untracked();
                    if fixed.status == "duplicate" {
                        ledger.update(false, move |d| d.expenses.retain(|e| e.id != row));
                        error.set("直した結果、ほかのレシートと同じものになりました。二重に数えないよう、この行を外しました。証憑OCRで確かめてください。".into());
                    } else {
                        ledger.update(false, move |d| {
                            if let Some(e) = d.expenses.iter_mut().find(|e| e.id == row) {
                                let updated = rules::expense_from_receipt(&fixed, e.category.clone());
                                e.amount = updated.amount;
                                e.payee = updated.payee;
                                e.purchased_on = updated.purchased_on;
                            }
                        });
                        open.set(false);
                    }
                    ledger.view.update(|v| *v += 1);
                }
                Err(message) => error.set(message),
            }
        });
    };
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Text(
                value: "金額・支払先・買った日はレシートの読み取りを使っています。違うときは「レシートを直す」で元のレシートを直してください。",
                style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(4), px(16))),
            )
            FieldRow(
                label: computed(|| "レシートを直す".to_owned()),
                value: computed(move || if open.get() { "閉じる".to_owned() } else { String::new() }),
                on_tap: move |_| if !ledger.confirmed() { open.update(|o| *o = !*o) },
            )
            Show(when: move || open.get()) {
                TextRow(label: "支払先", text: payee, editable: true, on_text: |_| {})
                TextRow(label: "買った日", text: date, editable: true, on_text: |_| {})
                TextRow(label: "金額", text: total, editable: true, on_text: |_| {})
                FieldRow(
                    label: computed(move || if busy.get() { "直しています…".to_owned() } else { "レシートを直して保存".to_owned() }),
                    value: computed(String::new),
                    on_tap: move |_| save(),
                )
            }
            Show(when: move || !error.with(String::is_empty)) {
                Text(value: computed(move || error.get()), style: css!(color: hex(RED), font_size: px(13), padding: (px(4), px(16))))
            }
        }
    }
}

/// 日を開いたときに作り、レシートを読み始める。
pub(crate) fn start(ledger: Ledger) -> Receipts {
    let receipts = Receipts::new(ledger);
    receipts.load();
    receipts
}

/// お知らせの一言。
#[component]
pub(crate) fn receipt_message(receipts: Receipts) -> Element {
    render! {
        Show(when: move || !receipts.message.with(String::is_empty)) {
            Text(
                value: computed(move || receipts.message.get()),
                style: css!(color: hex(TINT), font_size: px(14), padding: (px(8), px(32), px(0), px(32))),
                on_tap: move |_| receipts.message.set(String::new()),
            )
        }
    }
}
