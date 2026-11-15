//! レジ管理表（レジ締め）の画面。Web 版のレジ管理表と同じことを、スマホで入れやすい並びで出す。
//!
//! - 開始時・終了時の金種ごとの枚数、実績（売上・控除）、客数、担当、天気、メモ
//! - 増えた現金と入出金合計を照らして過不足を出す
//! - 売上伝票（証憑 OCR）から実績を入れる。売上台帳から販売数を集計する
//! - 入力が止まって少しで自動保存。版付き。「この日を締める」で締め、取り消すまで変えない

use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use whisker::Owner;
use crate::api::SaveError;
use crate::cash_book as cash;
use crate::cash_book_screen::*;
use crate::cash_book_settings_screen::{Choice, Hint};
use crate::register_count::{self as model, DENOMS, Day, Template};

const SAVE_DELAY: Duration = Duration::from_millis(800);

/// 帳簿のうち、レジ管理表を設定してあるもの。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RegisterBook {
    pub(crate) id: String,
    label: String,
    template: Value,
}

fn register_books(settings: &Value) -> Vec<RegisterBook> {
    settings["data"]["books"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| model::validate_template(&b["registerCount"]).is_ok())
        .filter_map(|b| {
            Some(RegisterBook {
                id: b["id"].as_str()?.to_owned(),
                label: format!(
                    "{} / {}",
                    b["storeName"].as_str().unwrap_or(""),
                    b["registerName"].as_str().unwrap_or("")
                ),
                template: b["registerCount"].clone(),
            })
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Idle,
    Saving,
    Saved,
    Error,
    Conflict,
}

#[derive(Clone, Copy)]
pub(crate) struct Sheet {
    pub(crate) token: RwSignal<Option<String>>,
    pub(crate) tenant: RwSignal<Option<Tenant>>,
    pub(crate) book: RwSignal<Option<RegisterBook>>,
    pub(crate) date: RwSignal<String>,
    pub(crate) day: RwSignal<Option<Day>>,
    version: RwSignal<Option<i64>>,
    /// 前の日の終了時の枚数（引き継ぎに使う）。
    previous_closing: RwSignal<Option<std::collections::BTreeMap<i64, Option<i64>>>>,
    load_error: RwSignal<String>,
    state: RwSignal<State>,
    save_error: RwSignal<String>,
    generation: RwSignal<u64>,
    pub(crate) view: RwSignal<u64>,
    edits: RwSignal<u64>,
    dirty: RwSignal<bool>,
    saving: RwSignal<bool>,
    /// 天気を手で選んだ（自動では入れ直さない）。
    weather_manual: RwSignal<bool>,
    weather_note: RwSignal<String>,
    pub(crate) owner: Option<Owner>,
}

impl Sheet {
    pub(crate) fn template(&self) -> Option<Template> {
        self.day.with(|d| d.as_ref().and_then(Day::template))
    }

    fn confirmed(&self) -> bool {
        self.day.with(|d| d.as_ref().is_some_and(Day::is_confirmed))
    }

    fn load(self) {
        self.flush();
        self.generation.update(|g| *g += 1);
        let generation = self.generation.get_untracked();
        self.day.set(None);
        self.load_error.set(String::new());
        self.save_error.set(String::new());
        self.state.set(State::Idle);
        self.dirty.set(false);
        self.weather_manual.set(false);
        self.weather_note.set(String::new());
        let (Some(tenant), Some(book)) = (self.tenant.get_untracked(), self.book.get_untracked()) else {
            return;
        };
        let date = self.date.get_untracked();
        spawn_in(self.owner, async move {
            // 前の日の保存が終わってから読む（前日の終了時を古い枚数で引き継がないように）。
            for _ in 0..300 {
                if !self.saving.get_untracked() {
                    break;
                }
                run_blocking(|| std::thread::sleep(Duration::from_millis(100))).await;
            }
            if self.generation.get_untracked() != generation {
                return;
            }
            let from = cash::shift_date(&date, -1);
            let to = date.clone();
            let id = book.id.clone();
            let result = authed(self.token, move |access| api::register_count_days(access, &tenant, &from, &to, &id)).await;
            if self.generation.get_untracked() != generation {
                return;
            }
            match result {
                Ok(value) => {
                    let items = value["items"].as_array().cloned().unwrap_or_default();
                    let find = |d: &str| items.iter().find(|i| i["date"] == d).cloned();
                    let previous = find(&cash::shift_date(&date, -1))
                        .map(|i| Day::from_value(&cash::shift_date(&date, -1), &i["data"]))
                        .filter(Day::has_closing_counts)
                        .map(|d| d.closing);
                    self.previous_closing.set(previous);
                    let current = find(&date);
                    self.version.set(current.as_ref().and_then(|i| i["version"].as_i64()));
                    let scope = json!({ "label": book.label, "template": book.template });
                    let day = current
                        .map(|i| Day::from_value(&date, &i["data"]))
                        .unwrap_or_else(|| Day::empty(&date, scope));
                    self.day.set(Some(day));
                    self.view.update(|v| *v += 1);
                    self.fetch_weather(false);
                }
                Err(message) => self.load_error.set(message),
            }
        });
    }

    pub(crate) fn update(self, allow_confirmed: bool, change: impl FnOnce(&mut Day)) {
        let Some(mut day) = self.day.get_untracked() else { return };
        if day.is_confirmed() && !allow_confirmed {
            return;
        }
        change(&mut day);
        if day.opening_from_previous {
            if let Some(prev) = self.previous_closing.get_untracked() {
                day.opening = prev;
            }
        }
        self.day.set(Some(day));
        self.dirty.set(true);
        if self.state.get_untracked() != State::Conflict {
            self.state.set(State::Saving);
        }
        self.edits.update(|e| *e += 1);
        let edits = self.edits.get_untracked();
        spawn_in(self.owner, async move {
            run_blocking(|| std::thread::sleep(SAVE_DELAY)).await;
            if self.edits.get_untracked() == edits {
                self.flush();
            }
        });
    }

    fn flush(self) {
        if self.saving.get_untracked() || !self.dirty.get_untracked() || self.state.get_untracked() == State::Conflict {
            return;
        }
        let (Some(day), Some(tenant), Some(book)) =
            (self.day.get_untracked(), self.tenant.get_untracked(), self.book.get_untracked())
        else {
            return;
        };
        self.saving.set(true);
        self.dirty.set(false);
        self.state.set(State::Saving);
        let generation = self.generation.get_untracked();
        let expected = self.version.get_untracked();
        spawn_in(self.owner, async move {
            let data = day.to_value();
            let date = day.date.clone();
            let result = authed(self.token, move |access| {
                match api::save_register_count_day(access, &tenant, &date, &data, expected, &book.id) {
                    Err(SaveError::Other(m)) if m == api::EXPIRED => Err(m),
                    other => Ok(other),
                }
            })
            .await
            .unwrap_or_else(|m| Err(SaveError::Other(m)));
            self.saving.set(false);
            if self.generation.get_untracked() != generation {
                self.flush();
                return;
            }
            match result {
                Ok(version) => {
                    self.version.set(Some(version));
                    self.save_error.set(String::new());
                    if self.dirty.get_untracked() {
                        self.flush();
                    } else {
                        self.state.set(State::Saved);
                    }
                }
                Err(SaveError::Conflict) | Err(SaveError::ReceiptUsed { .. }) => {
                    self.state.set(State::Conflict);
                    self.save_error
                        .set("ほかの端末で先に保存されたか、締めた日です。読み直してから入れ直してください。".into());
                }
                Err(SaveError::Other(message)) => {
                    self.dirty.set(true);
                    self.state.set(State::Error);
                    self.save_error.set(format!("保存できませんでした。もう一度保存してください。{message}"));
                }
            }
        });
    }

    /// 天気がまだ空なら、気象庁の予報から入れる（`force` は「予報を再取得」）。
    fn fetch_weather(self, force: bool) {
        let Some(day) = self.day.get_untracked() else { return };
        let Some(code) = day.template().and_then(|t| t.weather_area_code) else {
            self.weather_note.set("自動取得は未設定".into());
            return;
        };
        if day.is_confirmed() || (!force && (!day.weather.is_empty() || self.weather_manual.get_untracked())) {
            return;
        }
        let Some(area) = model::areas().into_iter().find(|a| a.code == code) else { return };
        let date = day.date.clone();
        let generation = self.generation.get_untracked();
        self.weather_note.set("天気を取得中…".into());
        spawn_in(self.owner, async move {
            let office = area.office_code.clone();
            let result = run_blocking(move || api::jma_forecast(&office)).await;
            if self.generation.get_untracked() != generation {
                return;
            }
            match result.map(|f| model::weather_from_forecast(&f, &area.code, &date)) {
                Ok(Some((weather, forecast))) => {
                    self.weather_note.set(format!("自動取得 {}・{}", area.prefecture, area.name));
                    let still_empty = self.day.with_untracked(|d| d.as_ref().is_some_and(|d| d.weather.is_empty() || force));
                    if still_empty {
                        self.update(false, move |d| {
                            d.weather = weather;
                            d.weather_forecast = forecast;
                        });
                        self.view.update(|v| *v += 1);
                    }
                }
                Ok(None) => self.weather_note.set("この日付の予報はありません。手動で選択できます。".into()),
                Err(_) => self.weather_note.set("天気を取得できませんでした。手動で選択できます。".into()),
            }
        });
    }
}

#[component]
pub fn register_count_screen(token: RwSignal<Option<String>>, tenant: RwSignal<Option<Tenant>>) -> Element {
    let books = signal(None::<Result<Vec<RegisterBook>, String>>);
    let sheet = Sheet {
        token,
        tenant,
        book: signal(None),
        date: signal(cash::tokyo_today()),
        day: signal(None),
        version: signal(None),
        previous_closing: signal(None),
        load_error: signal(String::new()),
        state: signal(State::Idle),
        save_error: signal(String::new()),
        generation: signal(0),
        view: signal(0),
        edits: signal(0),
        dirty: signal(false),
        saving: signal(false),
        weather_manual: signal(false),
        weather_note: signal(String::new()),
        owner: Owner::current(),
    };
    let focused = signal(None::<InputRef>);
    provide_context(Focused(focused));
    let settings_open = signal(false);
    let load_books = move || {
        books.set(None);
        let Some(current) = tenant.get_untracked() else { return };
        spawn_in(sheet.owner, async move {
            match authed(token, move |access| api::cash_book_settings(access, &current)).await {
                Ok(settings) => {
                    let list = settings.as_ref().map(register_books).unwrap_or_default();
                    let keep = sheet.book.get_untracked().and_then(|b| list.iter().find(|x| x.id == b.id).cloned());
                    sheet.book.set(keep.or_else(|| list.first().cloned()));
                    books.set(Some(Ok(list)));
                    sheet.load();
                }
                Err(message) => books.set(Some(Err(message))),
            }
        });
    };
    load_books();
    on_cleanup(move || {
        if sheet.dirty.get_untracked() && !sheet.saving.get_untracked() {
            sheet.flush();
        }
    });
    let go = move |days: i64| {
        if sheet.saving.get_untracked() {
            return;
        }
        sheet.date.update(|d| *d = cash::shift_date(d, days));
        sheet.load();
    };
    let options = computed(move || {
        books.with(|b| match b {
            Some(Ok(list)) => list.iter().map(|x| (x.id.clone(), x.label.clone())).collect::<Vec<_>>(),
            _ => Vec::new(),
        })
    });
    render! {
        View(style: css!(position: PositionKind::Relative, flex_grow: 1.0, display: Display::Flex, flex_direction: FlexDirection::Column).flex_basis(px(0)).min_height(px(0))) {
            KeyboardDone(focused: focused)
            View(style: screen_style()) {
                NavBar(
                    title: "レジ締め",
                    back_label: "",
                    on_back: |_| {},
                    action_label: "帳簿設定",
                    on_action: move |_| { sheet.flush(); settings_open.set(true); },
                )
                ScrollView(style: scroll_style()) {
                    View(style: scroll_content_style().padding_bottom(px(140))) {
                        Show(when: move || books.with(Option::is_none)) {
                            Notice(message: "レジの設定を読み込んでいます…")
                        }
                        Show(when: move || books.with(|b| matches!(b, Some(Err(_))))) {
                            ErrorNotice(message: computed(move || books.with(|b| match b {
                                Some(Err(m)) => format!("レジの設定を読み込めませんでした。{m}"),
                                _ => String::new(),
                            })))
                            Button(label: "再読み込み", on_tap: move |_| load_books())
                        }
                        Show(when: move || options.with(Vec::is_empty) && books.with(|b| matches!(b, Some(Ok(_))))) {
                            Text(
                                value: "レジ管理表を設定した店舗・レジがありません。右上の「帳簿設定」で帳簿を開き、「レジ管理表を設定する」を押してください。",
                                style: css!(color: hex(SECONDARY_LABEL), font_size: px(15), padding: (px(32), px(24)), text_align: TextAlign::Center),
                            )
                        }
                        Show(when: move || !options.with(Vec::is_empty)) {
                            View(style: group_style().margin_top(px(16))) {
                                Choice(
                                    label: computed(|| "店舗・レジ".to_owned()),
                                    options: options,
                                    selected: computed(move || sheet.book.with(|b| b.as_ref().map(|b| b.id.clone()).unwrap_or_default())),
                                    on_pick: move |id: String| {
                                        if sheet.dirty.get_untracked() || sheet.saving.get_untracked() { return; }
                                        let picked = books.with_untracked(|b| match b {
                                            Some(Ok(list)) => list.iter().find(|x| x.id == id).cloned(),
                                            _ => None,
                                        });
                                        sheet.book.set(picked);
                                        sheet.load();
                                    },
                                )
                            }
                            DateBar(date: sheet.date, on_shift: move |days: i64| go(days))
                            Show(when: move || !sheet.load_error.with(String::is_empty)) {
                                ErrorNotice(message: computed(move || format!("この日のレジ管理表を読み込めませんでした。{}", sheet.load_error.get())))
                                Button(label: "再読み込み", on_tap: move |_| sheet.load())
                            }
                            Show(when: move || sheet.day.with(Option::is_none) && sheet.load_error.with(String::is_empty)) {
                                Notice(message: "レジ管理表を読み込んでいます…")
                            }
                            Show(when: move || sheet.day.with(Option::is_some)) {
                                RegisterSheet(sheet: sheet)
                            }
                        }
                    }
                }
            }
            Show(when: move || settings_open.get()) {
                View(
                    style: css!(position: PositionKind::Absolute, top: px(0), bottom: px(0), left: px(0), right: px(0), display: Display::Flex, flex_direction: FlexDirection::Column).z_index(20),
                ) {
                    crate::cash_book_settings_screen::CashBookSettingsScreen(
                        token: token,
                        tenant: tenant,
                        on_close: move |_| { settings_open.set(false); load_books(); },
                    )
                }
            }
        }
    }
}

/// 1 日分のレジ管理表。
#[component]
fn register_sheet(sheet: Sheet) -> Element {
    let confirmed = computed(move || sheet.confirmed());
    let check = computed(move || sheet.day.with(|d| d.as_ref().map(Day::amount_check).unwrap_or_default()));
    let status = computed(move || match sheet.state.get() {
        State::Idle => String::new(),
        State::Saving => "保存中…".into(),
        State::Saved => "保存しました".into(),
        State::Error => "保存できませんでした".into(),
        State::Conflict => "保存していません".into(),
    });
    let weather_options = computed(|| {
        ["", "晴れ", "曇り", "雨", "雪"]
            .iter()
            .map(|w| ((*w).to_owned(), if w.is_empty() { "未選択".to_owned() } else { (*w).to_owned() }))
            .collect::<Vec<_>>()
    });
    let has_previous = computed(move || sheet.previous_closing.with(Option::is_some));
    let from_previous = computed(move || sheet.day.with(|d| d.as_ref().is_some_and(|d| d.opening_from_previous)));
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Show(when: move || !status.get().is_empty()) {
                Text(
                    value: status,
                    style: computed(move || css!(
                        color: hex(if matches!(sheet.state.get(), State::Error | State::Conflict) { RED } else { SECONDARY_LABEL }),
                        font_size: px(13),
                        text_align: TextAlign::Center,
                    )),
                )
            }
            Show(when: move || !sheet.save_error.with(String::is_empty)) {
                ErrorNotice(message: computed(move || sheet.save_error.get()))
                Show(when: move || sheet.state.get() == State::Conflict) {
                    Button(label: "読み直す", on_tap: move |_| { sheet.state.set(State::Idle); sheet.dirty.set(false); sheet.load(); })
                }
                Show(when: move || sheet.state.get() == State::Error) {
                    Button(label: "もう一度保存", on_tap: move |_| sheet.flush())
                }
            }
            Show(when: move || confirmed.get()) {
                Hint(text: "締め済みです。直すときは下の「締めを取り消す」を押してください。")
            }

            ForEach(
                each: move || vec![(sheet.view.get(), confirmed.get())],
                key: |k: &(u64, bool)| *k,
                children: move |k: ReadSignal<(u64, bool)>| {
                    let (_, locked) = k.get_untracked();
                    let get = |f: fn(&Day) -> String| sheet.day.with_untracked(|d| d.as_ref().map(f).unwrap_or_default());
                    let opening_staff = signal(get(|d| d.opening_staff.clone()));
                    let closing_staff = signal(get(|d| d.closing_staff.clone()));
                    let note = signal(get(|d| d.note.clone()));
                    let customers = sheet.day.with_untracked(|d| d.as_ref().and_then(|d| d.customer_count).map(|c| c.to_string()).unwrap_or_default());
                    render! {
                        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
                            SectionHeader(title: computed(|| "担当・天気".to_owned()))
                            View(style: group_style()) {
                                TextRow(label: "担当（開始）", text: opening_staff, editable: !locked, on_text: move |t: String| sheet.update(false, move |d| d.opening_staff = t))
                                TextRow(label: "担当（終了）", text: closing_staff, editable: !locked, on_text: move |t: String| sheet.update(false, move |d| d.closing_staff = t))
                                Choice(
                                    label: computed(|| "天気".to_owned()),
                                    options: weather_options,
                                    selected: computed(move || sheet.day.with(|d| d.as_ref().map(|d| d.weather.clone()).unwrap_or_default())),
                                    on_pick: move |w: String| {
                                        sheet.weather_manual.set(true);
                                        sheet.update(false, move |d| { d.weather = w; d.weather_forecast = Value::Null; });
                                    },
                                )
                                Show(when: move || !locked) {
                                    FieldRow(
                                        label: computed(move || sheet.weather_note.get()),
                                        value: computed(|| "予報を再取得".to_owned()),
                                        on_tap: move |_| sheet.fetch_weather(true),
                                    )
                                }
                            }
                            SectionHeader(title: computed(|| "開始時".to_owned()))
                            View(style: group_style()) {
                                Show(when: move || !locked && (has_previous.get() || from_previous.get())) {
                                    FieldRow(
                                        label: computed(move || if has_previous.get() { "前日の終了時を引き継ぐ".to_owned() } else { "前日の終了時の記録なし".to_owned() }),
                                        value: computed(move || if from_previous.get() { "✓".to_owned() } else { "引き継がない".to_owned() }),
                                        on_tap: move |_| {
                                            let on = !from_previous.get_untracked();
                                            sheet.update(false, move |d| d.opening_from_previous = on);
                                            sheet.view.update(|v| *v += 1);
                                        },
                                    )
                                }
                                CountTable(sheet: sheet, closing: false, locked: locked || from_previous.get_untracked())
                            }
                            SectionHeader(title: computed(|| "終了時".to_owned()))
                            View(style: group_style()) {
                                CountTable(sheet: sheet, closing: true, locked: locked)
                            }
                            SectionHeader(title: computed(|| "実績".to_owned()))
                            View(style: group_style()) {
                                CountRow(label: computed(|| "客数".to_owned()), unit: "人", initial: customers, editable: !locked, on_value: move |v: Option<i64>| sheet.update(false, move |d| d.customer_count = v))
                                ActualRows(sheet: sheet, locked: locked)
                            }
                            SectionHeader(title: computed(|| "メモ".to_owned()))
                            View(style: group_style()) {
                                TextRow(label: "メモ", text: note, editable: !locked, on_text: move |t: String| sheet.update(false, move |d| d.note = t))
                            }
                        }
                    }
                },
            )

            SectionHeader(title: computed(|| "金額チェック".to_owned()))
            View(style: group_style()) {
                TotalRow(label: "終了時現金", amount: computed(move || check.get().0))
                TotalRow(label: "開始時入金", amount: computed(move || check.get().1))
                TotalRow(label: "差額（増えた現金）", amount: computed(move || check.get().2))
                TotalRow(label: "入出金合計", amount: computed(move || check.get().3))
                View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding: (px(14), px(16)))) {
                    Text(value: "過不足", style: css!(color: hex(LABEL), font_size: px(17)))
                    Text(
                        value: computed(move || {
                            let x = check.get().4;
                            format!("{}円（{}）", cash::format_yen(x), model::over_short_label(x))
                        }),
                        style: computed(move || css!(color: hex(if check.get().4 == 0 { GREEN } else { RED }), font_size: px(17), font_weight: FontWeight::Numeric(600))),
                    )
                }
            }

            Show(when: move || !confirmed.get()) {
                crate::register_count_import::SalesImport(sheet: sheet)
                crate::register_count_import::ProductSales(sheet: sheet)
            }

            View(style: css!(height: px(20)))
            Show(when: move || !confirmed.get()) {
                Button(label: "この日を締める", on_tap: move |_| {
                    sheet.update(false, |d| d.confirmed_at = Some(cash::now_iso()));
                    sheet.flush();
                })
            }
            Show(when: move || confirmed.get()) {
                Button(label: "締めを取り消す", on_tap: move |_| {
                    sheet.update(true, |d| d.confirmed_at = None);
                    sheet.flush();
                })
            }
            crate::print_ui::DayPrint(
                html: computed(move || sheet.day.with(|d| d.as_ref().map(|d| {
                    crate::print::document(&crate::print::file_name("レジ管理表", &d.date, &d.date), &[crate::print::register_page(d)], crate::print::register_landscape(d))
                }).unwrap_or_default())),
                name: computed(move || crate::print::file_name("レジ管理表", &sheet.date.get(), &sheet.date.get())),
                landscape: computed(move || sheet.day.with(|d| d.as_ref().is_none_or(crate::print::register_landscape))),
            )
            crate::print_ui::PeriodPrint(
                token: sheet.token,
                tenant: sheet.tenant,
                kind: crate::print_ui::Kind::Register,
                book_id: computed(move || sheet.book.with(|b| b.as_ref().map(|b| b.id.clone()))),
                fallback: computed(|| None),
                include_legacy: computed(|| false),
                date: computed(move || sheet.date.get()),
            )
        }
    }
}

/// 金種ごとの枚数（開始時か終了時）。
#[component]
fn count_table(sheet: Sheet, closing: bool, locked: bool) -> Element {
    let total = computed(move || {
        sheet.day.with(|d| d.as_ref().map(|d| model::count_total(if closing { &d.closing } else { &d.opening })).unwrap_or(0))
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            ForEach(
                each: move || DENOMS.to_vec(),
                key: |d: &i64| *d,
                children: move |d: ReadSignal<i64>| {
                    let denom = d.get_untracked();
                    let initial = sheet.day.with_untracked(|day| {
                        day.as_ref()
                            .and_then(|day| if closing { day.closing.get(&denom) } else { day.opening.get(&denom) }.copied().flatten())
                            .map(|n| n.to_string())
                            .unwrap_or_default()
                    });
                    let amount = computed(move || {
                        sheet.day.with(|day| {
                            let n = day.as_ref().and_then(|day| if closing { day.closing.get(&denom) } else { day.opening.get(&denom) }.copied().flatten());
                            n.map(|n| format!("{}円", cash::format_yen(n * denom))).unwrap_or_default()
                        })
                    });
                    render! {
                        CountRow(
                            label: computed(move || format!("{}円　{}", cash::format_yen(denom), amount.get())),
                            unit: "枚",
                            initial: initial,
                            editable: !locked,
                            on_value: move |v: Option<i64>| sheet.update(false, move |day| {
                                if closing { day.closing.insert(denom, v); } else { day.opening.insert(denom, v); }
                            }),
                        )
                    }
                },
            )
            TotalRow(label: "合計", amount: total)
        }
    }
}

/// 実績の項目（設定の行ごと）。販売数を記録する行は件数も入れる。
#[component]
fn actual_rows(sheet: Sheet, locked: bool) -> Element {
    let rows = sheet.template().map(|t| t.rows).unwrap_or_default();
    render! {
        ForEach(
            each: move || rows.clone(),
            key: |r: &model::Row| r.key.clone(),
            children: move |r: ReadSignal<model::Row>| {
                let row = r.get_untracked();
                let tag = match row.effect.as_str() { "subtract" => "控除", "memo" => "集計外", _ => "加算" };
                let label = format!("{}（{tag}）", row.label);
                let key = row.key.clone();
                let amount = sheet.day.with_untracked(|d| d.as_ref().and_then(|d| d.sales.get(&key).copied().flatten()).map(|n| n.to_string()).unwrap_or_default());
                let qty = sheet.day.with_untracked(|d| d.as_ref().and_then(|d| d.sales_quantities.get(&key).copied().flatten()).map(|n| n.to_string()).unwrap_or_default());
                let key_s = signal(key.clone());
                let label_q = signal(format!("{} の販売数", row.label));
                let qty = signal(qty);
                let quantity = row.quantity;
                render! {
                    View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
                        Show(when: move || quantity) {
                            CountRow(label: computed(move || label_q.get()), unit: "件", initial: qty.get_untracked(), editable: !locked, on_value: move |v: Option<i64>| {
                                let k = key_s.get_untracked();
                                sheet.update(false, move |d| { d.sales_quantities.insert(k, v); });
                            })
                        }
                        CountRow(label: computed(move || label.clone()), unit: "円", initial: amount, editable: !locked, on_value: move |v: Option<i64>| {
                            let k = key_s.get_untracked();
                            sheet.update(false, move |d| { d.sales.insert(k, v); });
                        })
                    }
                }
            },
        )
    }
}

/// 数を打つ行（枚数・件数・円）。
#[component]
pub(crate) fn count_row(label: Signal<String>, unit: &'static str, initial: String, editable: bool, on_value: Callback<Option<i64>>) -> Element {
    let text = signal(initial.clone());
    let invalid = signal(false);
    let input = InputRef::new();
    render! {
        View(
            style: css!(display: Display::Flex, flex_direction: FlexDirection::Row, align_items: AlignItems::Center, padding_left: px(16), min_height: px(46))
                .border_bottom(hairline()),
        ) {
            Text(value: label, style: css!(color: hex(LABEL), font_size: px(16), flex_grow: 1.0, flex_shrink: 1.0))
            Show(when: move || invalid.get()) {
                Text(value: "数字で", style: css!(color: hex(RED), font_size: px(13), margin_right: px(6)))
            }
            Input(
                text: text,
                placeholder: "0",
                keyboard_type: KeyboardType::Number,
                editable: editable,
                input_ref: input,
                on_focus: move |_| remember_focus(input),
                on_input: move |value: String| match model::parse_count(&value) {
                    Ok(v) => { invalid.set(false); on_value.run(v); }
                    Err(()) => invalid.set(true),
                },
                style: css!(
                    width: px(110), height: px(46), font_size: px(17), padding_right: px(8),
                    text_align: TextAlign::Right,
                    color: hex(if editable { LABEL } else { SECONDARY_LABEL }),
                    background_color: Color::rgba(0, 0, 0, 0.0),
                ),
            )
            Text(value: unit, style: css!(color: hex(SECONDARY_LABEL), font_size: px(15), margin_right: px(16)))
        }
    }
}

