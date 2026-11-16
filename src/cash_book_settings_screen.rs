//! 帳簿設定の画面。店舗・レジごとの帳簿を足し、項目・振り分けのルール・印刷形式を変える。
//! Web 版（cash-book-settings-page.tsx / cash-book-template-editor.tsx）と同じことができる。
//!
//! 帳簿は JSON のまま下書き（`draft`）に写して直し、「保存」で一覧ごとサーバーへ送る。
//! 帳簿は消せない（サーバーが断る）。店舗とレジは、作った後は変えられない。

use base64::Engine;
use serde_json::{Value, json};

use super::*;
use whisker::Owner;
use crate::cash_book_screen::*;
use crate::cash_book_settings as rules;

#[derive(Clone, Copy)]
pub(crate) struct Editor {
    pub(crate) token: RwSignal<Option<String>>,
    pub(crate) tenant: RwSignal<Option<Tenant>>,
    /// サーバーの設定（`{ data: { books }, version }`）。読むまでは `None`。
    settings: RwSignal<Option<Value>>,
    stores: RwSignal<Vec<Value>>,
    load_error: RwSignal<String>,
    /// 開いている帳簿の番号。一覧の数と同じなら新しい帳簿。
    index: RwSignal<Option<usize>>,
    pub(crate) draft: RwSignal<Value>,
    /// 入力欄を作り直すたびに増やす（行を足した・消したとき）。
    pub(crate) view: RwSignal<u64>,
    saving: RwSignal<bool>,
    message: RwSignal<String>,
    /// 保存していない変更を捨てる前に、一度だけ知らせる。
    warned: RwSignal<bool>,
    owner: Option<Owner>,
}

impl Editor {
    fn books(&self) -> Vec<Value> {
        self.settings.with(|s| {
            s.as_ref()
                .and_then(|s| s["data"]["books"].as_array().cloned())
                .unwrap_or_default()
        })
    }

    fn load(self) {
        self.load_error.set(String::new());
        let Some(tenant) = self.tenant.get_untracked() else {
            return;
        };
        spawn_in(self.owner, async move {
            let result = authed(self.token, move |access| {
                let settings = api::cash_book_settings(access, &tenant)?
                    .unwrap_or_else(|| json!({ "data": { "books": [] }, "version": 0 }));
                // 店舗が読めなくても帳簿は直せるようにする（足すときだけ要る）。
                let stores = api::inventory_stores(access, &tenant);
                Ok((settings, stores))
            })
            .await;
            match result {
                Ok((settings, stores)) => {
                    self.settings.set(Some(settings));
                    match stores {
                        Ok(list) => self.stores.set(list),
                        Err(message) => self
                            .load_error
                            .set(format!("店舗マスタを読み込めませんでした。{message}")),
                    }
                }
                Err(message) => self.load_error.set(format!("設定を保存・取得できませんでした。{message}")),
            }
        });
    }

    fn open(self, index: usize, book: Value) {
        self.draft.set(book);
        self.index.set(Some(index));
        self.message.set(String::new());
        self.warned.set(false);
        self.view.update(|v| *v += 1);
    }

    fn is_new(&self) -> bool {
        self.index.get().is_some_and(|i| i >= self.books().len())
    }

    fn dirty(&self) -> bool {
        let Some(index) = self.index.get_untracked() else {
            return false;
        };
        let original = self.books().get(index).cloned();
        self.draft.with_untracked(|draft| original.as_ref() != Some(draft))
    }

    /// 下書きを直す。`rebuild` は行の数が変わったとき（入力欄を作り直す）。
    pub(crate) fn edit(self, rebuild: bool, change: impl FnOnce(&mut Value)) {
        // 保存の途中は変えさせない（送った後の変更が「保存しました」で消えないように）。
        if self.saving.get_untracked() {
            self.message.set("保存中です。終わってから変えてください。".into());
            self.view.update(|v| *v += 1);
            return;
        }
        self.draft.update(change);
        self.warned.set(false);
        if rebuild {
            self.view.update(|v| *v += 1);
        }
    }

    fn back(self) {
        if self.dirty() && !self.warned.get_untracked() {
            self.warned.set(true);
            self.message
                .set("保存していない帳簿設定があります。もう一度押すと破棄します。".into());
            return;
        }
        self.index.set(None);
        self.message.set(String::new());
    }

    fn save(self) {
        if self.saving.get_untracked() {
            return;
        }
        let (Some(index), Some(settings), Some(tenant)) = (
            self.index.get_untracked(),
            self.settings.get_untracked(),
            self.tenant.get_untracked(),
        ) else {
            return;
        };
        let mut books = self.books();
        let draft = self.draft.get_untracked();
        if index < books.len() {
            books[index] = draft;
        } else {
            books.push(draft);
        }
        if let Err(message) = rules::validate_books(&books, index) {
            self.message.set(message);
            return;
        }
        let submitted = self.draft.get_untracked();
        let mut data = settings["data"].clone();
        data["books"] = Value::Array(books);
        let version = settings["version"].as_i64().unwrap_or(0);
        self.saving.set(true);
        self.message.set("保存中…".into());
        spawn_in(self.owner, async move {
            let result = authed(self.token, move |access| {
                api::save_cash_book_settings(access, &tenant, &data, version)
            })
            .await;
            self.saving.set(false);
            match result {
                Ok(saved) => {
                    self.settings.set(Some(saved));
                    // 送った後に変わっていたら（入力欄の途中の文字など）、閉じずに残す。
                    if self.draft.with_untracked(|d| *d != submitted) {
                        self.message.set("保存しました。そのあとの変更はまだ保存していません。".into());
                    } else {
                        self.index.set(None);
                        self.message.set("保存しました。".into());
                    }
                }
                Err(message) => self.message.set(format!("設定を保存・取得できませんでした。{message}")),
            }
        });
    }
}

#[component]
pub fn cash_book_settings_screen(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    on_close: Callback<()>,
) -> Element {
    let editor = Editor {
        token,
        tenant,
        settings: signal(None),
        stores: signal(Vec::new()),
        load_error: signal(String::new()),
        index: signal(None),
        draft: signal(Value::Null),
        view: signal(0),
        saving: signal(false),
        message: signal(String::new()),
        warned: signal(false),
        owner: Owner::current(),
    };
    editor.load();
    let editing = computed(move || editor.index.get().is_some());

    render! {
        View(
            style: computed(move || screen_style().background_color(hex(SYSTEM_BACKGROUND))),
        ) {
            NavBar(
                title: "帳簿設定",
                back_label: "戻る",
                on_back: move |_| {
                    if editor.index.get_untracked().is_some() {
                        editor.back();
                    } else {
                        on_close.call();
                    }
                },
                action_label: "",
                on_action: |_| {},
            )
            ScrollView(style: scroll_style()) {
                View(style: scroll_content_style().padding_bottom(px(160))) {
                    Show(when: move || !editor.load_error.with(String::is_empty)) {
                        ErrorNotice(message: computed(move || editor.load_error.get()))
                        Button(label: "読み直す", on_tap: move |_| editor.load())
                    }
                    Show(when: move || editor.settings.with(Option::is_none) && editor.load_error.with(String::is_empty)) {
                        Notice(message: "帳簿設定を読み込んでいます…")
                    }
                    Show(when: move || !editor.message.with(String::is_empty)) {
                        Text(
                            value: computed(move || editor.message.get()),
                            style: computed(move || css!(
                                color: hex(SECONDARY_LABEL),
                                font_size: px(14),
                                padding: (px(12), px(32), px(0), px(32)),
                            )),
                        )
                    }
                    Show(when: move || editor.settings.with(Option::is_some) && !editing.get()) {
                        BookList(editor: editor)
                    }
                    Show(when: move || editing.get()) {
                        BookEditor(editor: editor)
                    }
                }
            }
        }
    }
}

/// 帳簿の一覧と「帳簿を追加」。
#[component]
fn book_list(editor: Editor) -> Element {
    let has_stores = computed(move || editor.stores.with(|s| !s.is_empty()));
    let store_name = signal(String::new());
    let creating = signal(false);
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Hint(text: "店舗マスタから店舗を選び、店舗・レジごとの帳簿を設定できます。")
            Show(when: move || !has_stores.get() && editor.load_error.with(String::is_empty) && editor.settings.with(Option::is_some)) {
                Hint(text: "帳簿を追加するには、先に店舗マスタで店舗を登録してください。登録後、この画面で店舗・レジの帳簿を追加できます。")
                View(style: computed(move || group_style())) {
                    TextRow(label: "店舗名", text: store_name, editable: true, on_text: |_| {})
                    FieldRow(
                        label: computed(move || if creating.get() { "登録しています…".to_owned() } else { "店舗を登録する".to_owned() }),
                        value: computed(String::new),
                        on_tap: move |_| {
                            let name = store_name.get_untracked().trim().to_owned();
                            if name.is_empty() || creating.get_untracked() {
                                editor.message.set("店舗名を入れてください。".into());
                                return;
                            }
                            let Some(tenant) = editor.tenant.get_untracked() else { return };
                            creating.set(true);
                            spawn_in(editor.owner, async move {
                                let result = authed(editor.token, move |access| api::create_store(access, &tenant, &name)).await;
                                creating.set(false);
                                match result {
                                    Ok(store) => {
                                        editor.stores.update(|s| s.push(store));
                                        editor.message.set("店舗を登録しました。".into());
                                    }
                                    Err(message) => editor.message.set(format!("店舗を登録できませんでした。{message}")),
                                }
                            });
                        },
                    )
                }
            }
            SectionHeader(title: computed(move || format!("帳簿（{}件）", editor.books().len())))
            View(style: computed(move || group_style())) {
                Show(when: move || editor.books().is_empty()) {
                    FieldRow(
                        label: computed(|| "店舗・レジ別の帳簿はまだありません。".to_owned()),
                        value: computed(String::new),
                        on_tap: |_| {},
                    )
                }
                ForEach(
                    each: move || editor.books().into_iter().enumerate().collect::<Vec<_>>(),
                    key: |item: &(usize, Value)| (item.0, item.1["id"].as_str().unwrap_or("").to_owned()),
                    children: move |item: ReadSignal<(usize, Value)>| {
                        let (index, book) = item.get_untracked();
                        let title = format!(
                            "{}\n{} / {}",
                            book["name"].as_str().filter(|n| !n.is_empty()).unwrap_or("名前なし"),
                            book["storeName"].as_str().unwrap_or(""),
                            book["registerName"].as_str().unwrap_or(""),
                        );
                        render! {
                            FieldRow(
                                label: computed(move || title.clone()),
                                value: computed(|| "設定を開く".to_owned()),
                                on_tap: move |_| editor.open(index, book.clone()),
                            )
                        }
                    },
                )
            }
            Show(when: move || has_stores.get()) {
                Button(label: "帳簿を追加", on_tap: move |_| {
                    let Some(store) = editor.stores.with_untracked(|s| s.first().cloned()) else {
                        return;
                    };
                    let book = rules::new_book(
                        store["id"].as_str().unwrap_or(""),
                        store["name"].as_str().unwrap_or(""),
                    );
                    editor.open(editor.books().len(), book);
                })
            }
        }
    }
}

/// 1 冊の帳簿の設定。
#[component]
fn book_editor(editor: Editor) -> Element {
    let form = signal("cash");
    let is_new = computed(move || editor.is_new());
    let banasuku = computed(move || editor.draft.with(|d| d["template"]["preset"] == "banasuku"));
    let store_options = computed(move || {
        editor.stores.with(|stores| {
            stores
                .iter()
                .map(|s| {
                    (
                        s["id"].as_str().unwrap_or("").to_owned(),
                        s["name"].as_str().unwrap_or("").to_owned(),
                    )
                })
                .collect::<Vec<_>>()
        })
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            SectionHeader(title: computed(move || if is_new.get() { "帳簿を追加".to_owned() } else { "帳簿の設定".to_owned() }))
            ForEach(
                each: move || vec![(editor.view.get(), is_new.get())],
                key: |k: &(u64, bool)| *k,
                children: move |k: ReadSignal<(u64, bool)>| {
                    let (_, new) = k.get_untracked();
                    let text = |path: &'static str| editor.draft.with_untracked(|d| d[path].as_str().unwrap_or("").to_owned());
                    let name = signal(text("name"));
                    let register = signal(text("registerName"));
                    render! {
                        View(style: computed(move || group_style())) {
                            TextRow(label: "帳簿名", text: name, editable: true, on_text: move |t: String| editor.edit(false, |d| d["name"] = json!(t)))
                            Show(when: move || new) {
                                Choice(
                                    label: computed(|| "店舗".to_owned()),
                                    options: store_options,
                                    selected: computed(move || editor.draft.with(|d| d["storeId"].as_str().unwrap_or("").to_owned())),
                                    on_pick: move |id: String| {
                                        let name = editor.stores.with_untracked(|s| {
                                            s.iter().find(|s| s["id"] == id.as_str()).and_then(|s| s["name"].as_str()).unwrap_or("").to_owned()
                                        });
                                        editor.edit(false, |d| {
                                            d["storeId"] = json!(id);
                                            d["storeName"] = json!(name);
                                        });
                                    },
                                )
                            }
                            Show(when: move || !new) {
                                FieldRow(
                                    label: computed(move || format!("店舗　{}", editor.draft.with(|d| d["storeName"].as_str().unwrap_or("").to_owned()))),
                                    value: computed(String::new),
                                    on_tap: |_| {},
                                )
                            }
                            TextRow(label: "レジ名", text: register, editable: new, on_text: move |t: String| editor.edit(false, |d| d["registerName"] = json!(t)))
                        }
                    }
                },
            )
            Hint(text: "店舗・レジごとに残高を繰り越して確定します。設定変更は新しく記入する日から適用され、記入済みの日は当時の定義を保ちます。")
            Show(when: move || banasuku.get()) {
                Button(label: "従来のOCRルールを編集できる設定に変換", on_tap: move |_| {
                    editor.edit(true, |d| d["template"] = rules::configure_legacy(&d["template"]));
                })
            }
            View(style: computed(move || group_style().margin_top(px(12)))) {
                Choice(
                    label: computed(|| "帳票".to_owned()),
                    options: computed(|| vec![("cash".to_owned(), "現金出納帳".to_owned()), ("register".to_owned(), "レジ管理表".to_owned())]),
                    selected: computed(move || form.get().to_owned()),
                    on_pick: move |f: String| form.set(if f == "register" { "register" } else { "cash" }),
                )
            }
            Show(when: move || form.get() == "cash") {
                TemplateEditor(editor: editor)
            }
            Show(when: move || form.get() == "register") {
                crate::register_count_settings::RegisterEditor(editor: editor)
            }
            Show(when: move || !editor.message.with(String::is_empty)) {
                Text(
                    value: computed(move || editor.message.get()),
                    style: computed(move || css!(color: hex(RED), font_size: px(14), padding: (px(16), px(32), px(0), px(32)))),
                )
            }
            Button(label: "保存", on_tap: move |_| editor.save())
            Button(label: "帳簿一覧へ戻る", on_tap: move |_| editor.back())
        }
    }
}

/// 現金出納帳の設定（項目・振り分け・商品連携・印刷形式）。
#[component]
fn template_editor(editor: Editor) -> Element {
    let template = move || editor.draft.with(|d| d["template"].clone());
    let income_options = computed(move || {
        rows_of(&template()["incomes"])
            .into_iter()
            .filter(|row| row["deduct"] != Value::Bool(true))
            .map(|row| key_label(&row))
            .collect::<Vec<_>>()
    });
    let expense_options = computed(move || {
        rows_of(&template()["expenses"]).iter().map(key_label).collect::<Vec<_>>()
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            ForEach(
                each: move || vec![editor.view.get()],
                key: |v: &u64| *v,
                children: move |_v: ReadSignal<u64>| {
                    let name = signal(editor.draft.with_untracked(|d| d["template"]["name"].as_str().unwrap_or("").to_owned()));
                    render! {
                        View(style: computed(move || group_style().margin_top(px(16)))) {
                            TextRow(label: "テンプレート名", text: name, editable: true, on_text: move |t: String| editor.edit(false, |d| d["template"]["name"] = json!(t)))
                        }
                    }
                },
            )

            SectionHeader(title: computed(|| "売上・入金項目".to_owned()))
            Hint(text: "控除を選んだ項目は現金収入合計から引きます。クレジット・コード決済の項目はOCRの決済別合計と連携します。")
            ForEach(
                each: move || {
                    let view = editor.view.get();
                    (0..rows_of(&template()["incomes"]).len()).map(|i| (view, i)).collect::<Vec<_>>()
                },
                key: |k: &(u64, usize)| *k,
                children: move |k: ReadSignal<(u64, usize)>| {
                    let (_, i) = k.get_untracked();
                    render! { IncomeRow(editor: editor, index: i) }
                },
            )
            Button(label: "売上項目を追加", on_tap: move |_| {
                if rows_of(&template()["incomes"]).len() >= 40 { return; }
                editor.edit(true, |d| push(&mut d["template"]["incomes"], json!({ "key": rules::new_key(), "label": "新しい売上項目", "deduct": false })));
            })
            View(style: computed(move || group_style().margin_top(px(12)))) {
                Choice(
                    label: computed(|| "未分類の売上".to_owned()),
                    options: income_options,
                    selected: computed(move || template()["defaultIncome"].as_str().unwrap_or("").to_owned()),
                    on_pick: move |key: String| editor.edit(false, |d| d["template"]["defaultIncome"] = json!(key)),
                )
            }

            SectionHeader(title: computed(|| "支出項目".to_owned()))
            ForEach(
                each: move || {
                    let view = editor.view.get();
                    (0..rows_of(&template()["expenses"]).len()).map(|i| (view, i)).collect::<Vec<_>>()
                },
                key: |k: &(u64, usize)| *k,
                children: move |k: ReadSignal<(u64, usize)>| {
                    let (_, i) = k.get_untracked();
                    render! { ExpenseRow(editor: editor, index: i) }
                },
            )
            Button(label: "支出項目を追加", on_tap: move |_| {
                if rows_of(&template()["expenses"]).len() >= 40 { return; }
                editor.edit(true, |d| push(&mut d["template"]["expenses"], json!({ "key": rules::new_key(), "label": "新しい支出項目", "slots": 3 })));
            })
            View(style: computed(move || group_style().margin_top(px(12)))) {
                Choice(
                    label: computed(|| "未分類の支出".to_owned()),
                    options: expense_options,
                    selected: computed(move || template()["defaultExpense"].as_str().unwrap_or("").to_owned()),
                    on_pick: move |key: String| editor.edit(false, |d| d["template"]["defaultExpense"] = json!(key)),
                )
            }

            SectionHeader(title: computed(|| "OCRの振り分け".to_owned()))
            Hint(text: "上から順に照合します。支出は過去に選んだ同じ支払先の科目を優先し、連続するOCR品目分類のルールは金額が最も大きい振り分け先を使います。")
            RuleList(editor: editor, expense: false, options: income_options)
            RuleList(editor: editor, expense: true, options: expense_options)

            SectionHeader(title: computed(|| "商品マスタと売上項目の連携".to_owned()))
            Hint(text: "商品を選ぶと、最新の商品名・公開名・SKUとの完全一致で売上項目に振り分けます。")
            ProductLink(editor: editor, options: income_options)

            SectionHeader(title: computed(|| "会社別の印刷形式".to_owned()))
            PrintEditor(editor: editor, base: "template")
        }
    }
}

pub(crate) fn rows_of(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn key_label(row: &Value) -> (String, String) {
    (
        row["key"].as_str().unwrap_or("").to_owned(),
        row["label"].as_str().unwrap_or("").to_owned(),
    )
}

pub(crate) fn push(list: &mut Value, item: Value) {
    if !list.is_array() {
        *list = json!([]);
    }
    if let Some(items) = list.as_array_mut() {
        items.push(item);
    }
}

pub(crate) fn remove_at(list: &mut Value, index: usize) {
    if let Some(items) = list.as_array_mut() {
        if index < items.len() {
            items.remove(index);
        }
    }
}

/// 売上・入金項目の 1 行：名前、控除、削除。
#[component]
fn income_row(editor: Editor, index: usize) -> Element {
    let row = editor.draft.with_untracked(|d| d["template"]["incomes"][index].clone());
    let key = row["key"].as_str().unwrap_or("").to_owned();
    let fixed = key == "credit" || key == "codePayment";
    let label = signal(row["label"].as_str().unwrap_or("").to_owned());
    let deduct = computed(move || editor.draft.with(|d| d["template"]["incomes"][index]["deduct"] == Value::Bool(true)));
    let removable = {
        let key = key.clone();
        computed(move || editor.draft.with(|d| rules::can_remove_income(&d["template"], &key)))
    };
    render! {
        View(style: computed(move || group_style().margin_bottom(px(8)))) {
            TextRow(label: "売上項目", text: label, editable: true, on_text: move |t: String| editor.edit(false, |d| d["template"]["incomes"][index]["label"] = json!(t)))
            FieldRow(
                label: computed(move || if deduct.get() { "控除（合計から引く）".to_owned() } else { "控除しない".to_owned() }),
                value: computed(move || if fixed { String::new() } else { "切り替える".to_owned() }),
                on_tap: move |_| {
                    if fixed { return; }
                    editor.edit(false, |d| {
                        let row = &mut d["template"]["incomes"][index];
                        row["deduct"] = json!(row["deduct"] != Value::Bool(true));
                    });
                },
            )
            Show(when: move || removable.get()) {
                RemoveRow(on_tap: move |_| editor.edit(true, |d| remove_at(&mut d["template"]["incomes"], index)))
            }
        }
    }
}

/// 支出項目の 1 行：名前、印刷行数、削除。
#[component]
fn expense_row(editor: Editor, index: usize) -> Element {
    let row = editor.draft.with_untracked(|d| d["template"]["expenses"][index].clone());
    let key = row["key"].as_str().unwrap_or("").to_owned();
    let label = signal(row["label"].as_str().unwrap_or("").to_owned());
    let slots = row["slots"].as_f64().map(|s| format!("{s}")).unwrap_or_default();
    let removable = computed(move || editor.draft.with(|d| rules::can_remove_expense(&d["template"], &key)));
    render! {
        View(style: computed(move || group_style().margin_bottom(px(8)))) {
            TextRow(label: "支出項目", text: label, editable: true, on_text: move |t: String| editor.edit(false, |d| d["template"]["expenses"][index]["label"] = json!(t)))
            NumberRow(label: "印刷行数", initial: slots, decimal: false, on_value: move |v: Option<f64>| editor.edit(false, |d| {
                d["template"]["expenses"][index]["slots"] = v.map_or(Value::Null, |v| json!(v as i64));
            }))
            Show(when: move || removable.get()) {
                RemoveRow(on_tap: move |_| editor.edit(true, |d| remove_at(&mut d["template"]["expenses"], index)))
            }
        }
    }
}

/// 振り分けのルールの一覧（売上か支出）と「ルールを追加」。
#[component]
fn rule_list(editor: Editor, expense: bool, options: Signal<Vec<(String, String)>>) -> Element {
    let field = if expense { "expenseRules" } else { "incomeRules" };
    let fallback = if expense { "defaultExpense" } else { "defaultIncome" };
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Text(
                value: if expense { "支出" } else { "売上" },
                style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(32), px(6), px(32)))),
            )
            ForEach(
                each: move || {
                    let view = editor.view.get();
                    let count = editor.draft.with(|d| rows_of(&d["template"][field]).len());
                    (0..count).map(|i| (view, i)).collect::<Vec<_>>()
                },
                key: |k: &(u64, usize)| *k,
                children: move |k: ReadSignal<(u64, usize)>| {
                    let (_, i) = k.get_untracked();
                    render! { RuleRow(editor: editor, expense: expense, index: i, options: options) }
                },
            )
            Button(label: "ルールを追加", on_tap: move |_| {
                editor.edit(true, |d| {
                    if rows_of(&d["template"][field]).len() >= 300 { return; }
                    let target = d["template"][fallback].clone();
                    push(&mut d["template"][field], json!({ "text": "", "target": target }));
                });
            })
        }
    }
}

/// 振り分けのルール 1 件：（支出は照合する所）、文字、振り分け先、削除。
#[component]
fn rule_row(editor: Editor, expense: bool, index: usize, options: Signal<Vec<(String, String)>>) -> Element {
    let field = if expense { "expenseRules" } else { "incomeRules" };
    let rule = editor.draft.with_untracked(|d| d["template"][field][index].clone());
    let linked = rule["productId"].is_string();
    let text = signal(rule["text"].as_str().unwrap_or("").to_owned());
    let source = computed(move || {
        editor.draft.with(|d| d["template"][field][index]["source"].as_str().unwrap_or("text").to_owned())
    });
    let sources = computed(|| {
        vec![
            ("text".to_owned(), "支払先・品名".to_owned()),
            ("payee".to_owned(), "支払先".to_owned()),
            ("costCategory".to_owned(), "OCR品目分類".to_owned()),
        ]
    });
    let categories = computed(|| {
        rules::COST_CATEGORIES
            .iter()
            .map(|(k, l)| ((*k).to_owned(), format!("{l}（{k}）")))
            .collect::<Vec<_>>()
    });
    render! {
        View(style: computed(move || group_style().margin_bottom(px(8)))) {
            Show(when: move || expense) {
                Choice(
                    label: computed(|| "照合する所".to_owned()),
                    options: sources,
                    selected: source,
                    on_pick: move |s: String| editor.edit(true, |d| {
                        let rule = &mut d["template"][field][index];
                        if s == "text" {
                            if let Some(obj) = rule.as_object_mut() { obj.remove("source"); }
                        } else {
                            rule["source"] = json!(s);
                        }
                    }),
                )
            }
            Show(when: move || expense && source.get() == "costCategory") {
                Choice(
                    label: computed(|| "OCR品目分類".to_owned()),
                    options: categories,
                    selected: computed(move || editor.draft.with(|d| d["template"][field][index]["text"].as_str().unwrap_or("").to_owned())),
                    on_pick: move |c: String| editor.edit(true, |d| d["template"][field][index]["text"] = json!(c)),
                )
            }
            Show(when: move || !(expense && source.get() == "costCategory")) {
                TextRow(label: if linked { "商品マスタ連携" } else { "文字" }, text: text, editable: !linked, on_text: move |t: String| {
                    editor.edit(false, |d| d["template"][field][index]["text"] = json!(t));
                })
            }
            Choice(
                label: computed(|| "振り分け先".to_owned()),
                options: options,
                selected: computed(move || editor.draft.with(|d| d["template"][field][index]["target"].as_str().unwrap_or("").to_owned())),
                on_pick: move |key: String| editor.edit(false, |d| d["template"][field][index]["target"] = json!(key)),
            )
            RemoveRow(on_tap: move |_| editor.edit(true, |d| remove_at(&mut d["template"][field], index)))
        }
    }
}

/// 商品マスタを探して、売上項目に結びつける。
#[component]
fn product_link(editor: Editor, options: Signal<Vec<(String, String)>>) -> Element {
    let query = signal(String::new());
    let products = signal(Vec::<Value>::new());
    let page = signal(0usize);
    let more = signal(false);
    let searching = signal(false);
    let error = signal(String::new());
    let chosen = signal(String::new());
    let target = signal(String::new());
    let search = move |to: usize| {
        let Some(tenant) = editor.tenant.get_untracked() else { return };
        let q = query.get_untracked();
        searching.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = authed(editor.token, move |access| api::search_products(access, &tenant, &q, to * 20)).await;
            searching.set(false);
            match result {
                Ok((items, has_more)) => {
                    products.set(items);
                    more.set(has_more);
                    page.set(to);
                    chosen.set(String::new());
                }
                Err(message) => error.set(message),
            }
        });
    };
    let product_options = computed(move || {
        products.with(|list| {
            list.iter()
                .map(|p| {
                    let name = p["name"].as_str().unwrap_or("");
                    let label = match p["skuCode"].as_str().filter(|s| !s.is_empty()) {
                        Some(sku) => format!("{name}（{sku}）"),
                        None => name.to_owned(),
                    };
                    (p["id"].as_str().unwrap_or("").to_owned(), label)
                })
                .collect::<Vec<_>>()
        })
    });
    render! {
        View(style: computed(move || group_style())) {
            TextRow(label: "商品名", text: query, editable: true, on_text: |_| {})
            FieldRow(
                label: computed(move || if searching.get() { "探しています…".to_owned() } else { "商品を検索".to_owned() }),
                value: computed(move || format!("{}件", products.with(Vec::len))),
                on_tap: move |_| search(0),
            )
            Show(when: move || !error.with(String::is_empty)) {
                ErrorNotice(message: computed(move || error.get()))
            }
            Show(when: move || products.with(|p| !p.is_empty())) {
                Choice(
                    label: computed(|| "商品を選択".to_owned()),
                    options: product_options,
                    selected: computed(move || chosen.get()),
                    on_pick: move |id: String| chosen.set(id),
                )
                Show(when: move || page.get() > 0) {
                    FieldRow(label: computed(|| "前へ".to_owned()), value: computed(String::new), on_tap: move |_| search(page.get_untracked().saturating_sub(1)))
                }
                Show(when: move || more.get()) {
                    FieldRow(label: computed(|| "次へ".to_owned()), value: computed(String::new), on_tap: move |_| search(page.get_untracked() + 1))
                }
                Choice(
                    label: computed(|| "振り分け先".to_owned()),
                    options: options,
                    selected: computed(move || {
                        let t = target.get();
                        if t.is_empty() { editor.draft.with(|d| d["template"]["defaultIncome"].as_str().unwrap_or("").to_owned()) } else { t }
                    }),
                    on_pick: move |key: String| target.set(key),
                )
                FieldRow(
                    label: computed(|| "商品を連携".to_owned()),
                    value: computed(String::new),
                    on_tap: move |_| {
                        let id = chosen.get_untracked();
                        let Some(product) = products.with_untracked(|p| p.iter().find(|p| p["id"] == id.as_str()).cloned()) else { return };
                        let units: Vec<u16> = product["name"].as_str().unwrap_or("").encode_utf16().take(100).collect();
                        let name = String::from_utf16_lossy(&units);
                        let mut to = target.get_untracked();
                        if to.is_empty() {
                            to = editor.draft.with_untracked(|d| d["template"]["defaultIncome"].as_str().unwrap_or("").to_owned());
                        }
                        editor.edit(true, |d| {
                            let list = &mut d["template"]["incomeRules"];
                            if let Some(items) = list.as_array_mut() {
                                items.retain(|rule| rule["productId"] != id.as_str());
                            }
                            push(list, json!({ "text": name, "productId": id, "target": to }));
                        });
                        chosen.set(String::new());
                    },
                )
            }
        }
    }
}

/// 印刷形式：表題・会社名・確認欄・備考・配置・日付・向き・文字サイズと、AI の案。
#[component]
pub(crate) fn print_editor(editor: Editor, base: &'static str) -> Element {
    let print = move |key: &'static str| {
        computed(move || editor.draft.with(|d| d[base]["print"][key].as_str().unwrap_or("").to_owned()))
    };
    let options = |pairs: &'static [(&'static str, &'static str)]| {
        computed(move || pairs.iter().map(|(k, l)| ((*k).to_owned(), (*l).to_owned())).collect::<Vec<_>>())
    };
    let set = move |key: &'static str| {
        move |value: String| editor.edit(false, move |d| d[base]["print"][key] = json!(value))
    };
    let orientation = computed(move || {
        editor.draft.with(|d| d[base]["print"]["orientation"].as_str().unwrap_or("portrait").to_owned())
    });
    let instructions = signal(String::new());
    let reference = signal(None::<(String, String, bool)>); // (名前, base64, PDF か)
    let pdf_page = signal(String::new());
    let suggestion = signal(None::<Value>);
    let generating = signal(false);
    let ai_error = signal(String::new());
    let ask = move || {
        if generating.get_untracked() { return; }
        let text = instructions.get_untracked();
        let file = reference.get_untracked();
        if text.trim().is_empty() && file.is_none() { return; }
        let Some(tenant) = editor.tenant.get_untracked() else { return };
        let current = editor.draft.with_untracked(|d| d[base]["print"].clone());
        let document = file.map(|(_, data, pdf)| (data, if pdf { pdf_page.get_untracked().trim().parse::<u32>().ok() } else { None }));
        generating.set(true);
        ai_error.set(String::new());
        spawn_local(async move {
            let result = authed(editor.token, move |access| {
                api::suggest_cash_book_print(access, &tenant, &text, &current, document.clone())
            })
            .await;
            generating.set(false);
            match result {
                Ok(value) => suggestion.set(Some(value)),
                Err(message) => ai_error.set(message),
            }
        });
    };
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            ForEach(
                each: move || vec![editor.view.get()],
                key: |v: &u64| *v,
                children: move |_v: ReadSignal<u64>| {
                    let get = |key: &str| editor.draft.with_untracked(|d| d[base]["print"][key].as_str().unwrap_or("").to_owned());
                    let title = signal(get("title"));
                    let company = signal(get("companyName"));
                    let approval = signal(get("approvalLabel"));
                    let footer = signal(get("footer"));
                    let size = editor.draft.with_untracked(|d| d[base]["print"]["fontSize"].as_f64().map(|s| format!("{s}")).unwrap_or_default());
                    render! {
                        View(style: computed(move || group_style())) {
                            TextRow(label: "表題", text: title, editable: true, on_text: set("title"))
                            TextRow(label: "会社名", text: company, editable: true, on_text: set("companyName"))
                            TextRow(label: "確認欄", text: approval, editable: true, on_text: set("approvalLabel"))
                            TextRow(label: "備考", text: footer, editable: true, on_text: set("footer"))
                            NumberRow(label: "文字サイズ", initial: size, decimal: true, on_value: move |v: Option<f64>| editor.edit(false, |d| {
                                d[base]["print"]["fontSize"] = v.map_or(Value::Null, |v| json!(v));
                            }))
                        }
                    }
                },
            )
            View(style: computed(move || group_style().margin_top(px(12)))) {
                Choice(
                    label: computed(|| "配置".to_owned()),
                    options: options(&[("sideBySide", "収入と残高を左右に配置"), ("stacked", "収入と残高を上下に配置")]),
                    selected: print("layout"),
                    on_pick: set("layout"),
                )
                Choice(
                    label: computed(|| "日付".to_owned()),
                    options: options(&[("iso", "西暦"), ("reiwa", "令和")]),
                    selected: print("dateStyle"),
                    on_pick: set("dateStyle"),
                )
                Choice(
                    label: computed(|| "用紙の向き".to_owned()),
                    options: options(&[("portrait", "縦"), ("landscape", "横")]),
                    selected: orientation,
                    on_pick: set("orientation"),
                )
            }

            SectionHeader(title: computed(|| "AIで印刷形式の案を作る".to_owned()))
            Hint(text: "JPEG・PNG・PDF、3 MiBまで。金額や個人情報を含まない見本を使ってください。")
            View(style: computed(move || group_style())) {
                TextRow(label: "AIへの依頼", text: instructions, editable: true, on_text: |_| {})
                View(
                    style: css!(display: Display::Flex, flex_direction: FlexDirection::Row, align_items: AlignItems::Center, gap: px(12), padding: (px(8), px(16))),
                ) {
                    AttachButton(
                        on_pick: move |file: field_glass::PickedFile| {
                            let pdf = file.content_type == "application/pdf";
                            let allowed = pdf || matches!(file.content_type.as_str(), "image/jpeg" | "image/png");
                            if !allowed {
                                ai_error.set("JPEG・PNG・PDF のどれかを選んでください。".into());
                                return;
                            }
                            match std::fs::read(&file.path) {
                                Ok(bytes) if bytes.len() <= 3 * 1024 * 1024 => {
                                    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                                    reference.set(Some((file.name.clone(), data, pdf)));
                                    ai_error.set(String::new());
                                }
                                Ok(_) => ai_error.set("参考帳票は 3 MiB までにしてください。".into()),
                                Err(e) => ai_error.set(format!("ファイルを読み込めませんでした。{e}")),
                            }
                        },
                        style: css!(width: px(36), height: px(36)),
                    )
                    Text(
                        value: computed(move || reference.with(|r| r.as_ref().map_or_else(|| "参考帳票の写真・PDF（なくても可）".to_owned(), |(name, _, _)| name.clone()))),
                        style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(15), flex_shrink: 1.0)),
                    )
                }
                Show(when: move || reference.with(|r| r.as_ref().is_some_and(|(_, _, pdf)| *pdf))) {
                    TextRow(label: "PDFのページ", text: pdf_page, editable: true, on_text: |_| {})
                }
                FieldRow(
                    label: computed(move || if generating.get() {
                        "印刷形式を作成中…".to_owned()
                    } else if reference.with(Option::is_some) {
                        "参考帳票から読み取る".to_owned()
                    } else {
                        "AIで印刷形式の案を作る".to_owned()
                    }),
                    value: computed(String::new),
                    on_tap: move |_| ask(),
                )
            }
            Show(when: move || !ai_error.with(String::is_empty)) {
                ErrorNotice(message: computed(move || ai_error.get()))
            }
            Show(when: move || suggestion.with(Option::is_some)) {
                Hint(text: "AIの案です。採用後に「保存」で会社の設定に反映します。")
                Text(
                    value: computed(move || suggestion.with(|s| s.as_ref().map(describe_print).unwrap_or_default())),
                    style: computed(move || css!(color: hex(LABEL), font_size: px(15), padding: (px(4), px(32)))),
                )
                Button(label: "この案を採用", on_tap: move |_| {
                    if let Some(value) = suggestion.get_untracked() {
                        editor.edit(true, |d| d[base]["print"] = value);
                        suggestion.set(None);
                    }
                })
                Button(label: "案を破棄", on_tap: move |_| suggestion.set(None))
            }
        }
    }
}

/// 印刷形式を一文に（AI の案を見せるため）。
fn describe_print(print: &Value) -> String {
    let s = |k: &str| print[k].as_str().unwrap_or("").to_owned();
    format!(
        "表題「{}」 会社名「{}」 確認欄「{}」\n{} · {} · {} · 文字 {}\n備考「{}」",
        s("title"),
        s("companyName"),
        s("approvalLabel"),
        if s("layout") == "stacked" { "上下に配置" } else { "左右に配置" },
        if s("dateStyle") == "reiwa" { "令和" } else { "西暦" },
        if s("orientation") == "landscape" { "横" } else { "縦" },
        print["fontSize"].as_f64().map(|f| f.to_string()).unwrap_or_default(),
        s("footer"),
    )
}

/// 選ぶ欄：押すと候補が開き、選ぶと閉じる。
#[component]
pub(crate) fn choice(
    label: Signal<String>,
    options: Signal<Vec<(String, String)>>,
    selected: Signal<String>,
    on_pick: Callback<String>,
) -> Element {
    let open = signal(false);
    let current = computed(move || {
        let key = selected.get();
        options.get().iter().find(|(k, _)| *k == key).map_or_else(|| "未選択".to_owned(), |(_, l)| l.clone())
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            FieldRow(
                label: computed(move || format!("{}　{}", label.get(), current.get())),
                value: computed(move || if open.get() { "閉じる".to_owned() } else { "変える".to_owned() }),
                on_tap: move |_| open.update(|o| *o = !*o),
            )
            Show(when: move || open.get()) {
                ForEach(
                    each: move || options.get(),
                    key: |o: &(String, String)| o.0.clone(),
                    children: move |o: ReadSignal<(String, String)>| {
                        let (key, text) = o.get_untracked();
                        let checked = {
                            let key = key.clone();
                            computed(move || selected.get() == key)
                        };
                        render! {
                            FieldRow(
                                label: computed(move || format!("　{text}")),
                                value: computed(move || if checked.get() { "✓".to_owned() } else { String::new() }),
                                on_tap: move |_| {
                                    on_pick.run(key.clone());
                                    open.set(false);
                                },
                            )
                        }
                    },
                )
            }
        }
    }
}

/// 数を打つ欄（印刷行数・文字サイズ）。
#[component]
pub(crate) fn number_row(label: &'static str, initial: String, decimal: bool, on_value: Callback<Option<f64>>) -> Element {
    let text = signal(initial.clone());
    let input = InputRef::new();
    render! {
        View(
            style: computed(move || css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding_left: px(16),
                min_height: px(48),
            )
            .border_bottom(hairline())),
        ) {
            Text(value: label, style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(15), flex_grow: 1.0)))
            Input(
                text: text,
                placeholder: "0",
                keyboard_type: if decimal { KeyboardType::Decimal } else { KeyboardType::Number },
                input_ref: input,
                on_focus: move |_| remember_focus(input),
                on_input: move |value: String| on_value.run(value.trim().parse::<f64>().ok()),
                style: computed(move || css!(
                    width: px(100),
                    height: px(48),
                    font_size: px(17),
                    padding_right: px(16),
                    text_align: TextAlign::Right,
                    color: hex(LABEL),
                    background_color: Color::rgba(0, 0, 0, 0.0),
                )),
            )
        }
    }
}

/// 赤い「削除」の行。
#[component]
pub(crate) fn remove_row(on_tap: Callback<()>) -> Element {
    render! {
        Text(
            value: "削除",
            style: computed(move || css!(color: hex(RED), font_size: px(15), padding: (px(12), px(16)))),
            on_tap: move |_| on_tap.call(),
        )
    }
}

/// 灰色の説明文。
#[component]
pub(crate) fn hint(text: &'static str) -> Element {
    render! {
        Text(
            value: text,
            style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(32), px(4), px(32)))),
        )
    }
}
