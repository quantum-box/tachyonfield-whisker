//! 現金出納帳の画面。1 日分を開いて、収入・支出を入れ、少し待って自動で保存する。
//!
//! Web 版（apps/client/src/features/cash-book/use-cash-book-day.ts）と同じ決まりで動く。
//! - ① 前日現金残高は、手で直していない日なら前の日の本日現金残高に合わせ続ける。
//! - 保存は版（version）付き。ほかの端末が先に保存していたら上書きせず、読み直してもらう。
//! - 確定した日は、取り消すまで変えられない。
//! - 開いている間、ほかの端末の保存を 10 秒ごとに見に行く（入力中は取り込まない）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::*;
use crate::api::SaveError;
use crate::cash_book::{self as model, Book, Day, Expense, SavedDay, Template};

/// 前日残高の引き継ぎと証番に見る日数（Web 版の HISTORY_DAYS）。
const HISTORY_DAYS: i64 = 62;
/// 入力が止まってから保存するまで。
const SAVE_DELAY: Duration = Duration::from_millis(800);
/// ほかの端末の保存を見に行く間隔。
const PULL_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, PartialEq)]
enum SaveState {
    Idle,
    Saving,
    Saved,
    Error,
    Conflict,
}

/// 帳簿を選ぶ前の状態。
#[derive(Clone, PartialEq)]
enum Books {
    Loading,
    Failed(String),
    /// 帳簿設定の API が無い環境、または帳簿が 1 つも無い：従来の帳簿だけ。
    Ready {
        books: Vec<Book>,
        first: Option<String>,
        /// 帳簿設定の API が無い環境。従来の帳簿にそのまま記入できる。
        legacy_api: bool,
    },
}

/// 1 日分の帳簿の状態。どれも Copy なので、閉じた関数にそのまま渡せる。
#[derive(Clone, Copy)]
struct Ledger {
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    /// 選んでいる帳簿。`None` は従来の帳簿。
    book: RwSignal<Option<Book>>,
    /// 従来の帳簿の日も合わせて読むか（設定の先頭の帳簿のとき）。
    include_legacy: RwSignal<bool>,
    date: RwSignal<String>,
    day: RwSignal<Option<Day>>,
    version: RwSignal<Option<i64>>,
    /// 今の日が従来の帳簿に置いてあるか。保存も同じ所へする。
    legacy_origin: RwSignal<bool>,
    carried: RwSignal<Option<i64>>,
    voucher_start: RwSignal<usize>,
    load_error: RwSignal<String>,
    state: RwSignal<SaveState>,
    save_error: RwSignal<String>,
    /// 読み込みのたびに増やす。古い読み込み・保存の結果を捨てるのに使う。
    generation: RwSignal<u64>,
    /// 入力欄を作り直すたびに増やす（読み込み・ほかの端末の保存を取り込んだとき）。
    view: RwSignal<u64>,
    /// 入力のたびに増やす。止まってから保存するのに使う。
    edits: RwSignal<u64>,
    dirty: RwSignal<bool>,
    saving: RwSignal<bool>,
}

impl Ledger {
    fn book_id(&self) -> Option<String> {
        self.book.with_untracked(|b| b.as_ref().map(|b| b.id.clone()))
    }

    fn book_template(&self) -> Option<Template> {
        self.book.with_untracked(|b| b.as_ref().map(|b| b.template.clone()))
    }

    /// この日に使う項目。
    fn template(&self) -> Template {
        let fallback = self.book.with(|b| b.as_ref().map(|b| b.template.clone()));
        self.day.with(|d| match d {
            Some(day) => day.template_or(fallback.as_ref()),
            None => fallback.unwrap_or_else(model::legacy_template),
        })
    }

    fn confirmed(&self) -> bool {
        self.day.with(|d| d.as_ref().is_some_and(Day::is_confirmed))
    }

    /// 日を読む。入力の途中なら先に保存を送ってから読む。
    fn load(self) {
        self.flush();
        self.generation.update(|g| *g += 1);
        let generation = self.generation.get_untracked();
        self.day.set(None);
        self.load_error.set(String::new());
        self.save_error.set(String::new());
        self.state.set(SaveState::Idle);
        self.dirty.set(false);
        let Some(tenant) = self.tenant.get_untracked() else {
            return;
        };
        let date = self.date.get_untracked();
        let book_id = self.book_id();
        let legacy = self.include_legacy.get_untracked() || book_id.is_none();
        spawn_local(async move {
            // 送ったばかりの保存が終わるまで待つ（戻ってきた日を古い中身で開かない）。
            for _ in 0..50 {
                if !self.saving.get_untracked() {
                    break;
                }
                sleep(Duration::from_millis(100)).await;
            }
            let from = model::shift_date(&date, -HISTORY_DAYS);
            let to = date.clone();
            let result = authed(self.token, move |access| {
                read_days(access, &tenant, &from, &to, book_id.as_deref(), legacy)
            })
            .await;
            if self.generation.get_untracked() != generation {
                return;
            }
            match result {
                Ok(saved) => self.loaded(&date, saved),
                Err(message) => self.load_error.set(message),
            }
        });
    }

    fn loaded(self, date: &str, saved: Vec<SavedDay>) {
        let current = saved.iter().find(|item| item.day.date == date).cloned();
        let stored: Vec<&SavedDay> = saved.iter().filter(|item| item.day.date.as_str() < date).collect();
        let earlier =
            model::with_carried_balances(&stored.iter().map(|item| item.day.clone()).collect::<Vec<_>>());
        // 前の日を直して合わせ直した日は、開いていなくても保存しておく（Web 版と同じ）。
        for resolved in &earlier {
            let Some(item) = stored.iter().find(|item| item.day.date == resolved.date) else {
                continue;
            };
            if item.day.previous_balance != resolved.previous_balance {
                self.send_quietly(resolved.clone(), item.version, item.legacy);
            }
        }
        let carried = model::carried_over_balance(date, &earlier);
        let blank = Day::empty(date, self.book_template().as_ref());
        let base = current.as_ref().map_or(blank, |item| item.day.clone());
        let next = model::follow_carried_balance(&base, carried);
        self.version.set(current.as_ref().map(|item| item.version));
        self.legacy_origin.set(current.as_ref().is_some_and(|item| item.legacy));
        self.carried.set(carried);
        self.voucher_start.set(model::first_voucher_number(date, &earlier));
        let changed = current.as_ref().is_some_and(|item| item.day != next);
        self.day.set(Some(next));
        self.view.update(|v| *v += 1);
        // 保存してある日の ① が前の日とずれていたら、合わせた金額で保存し直す。
        if changed {
            self.dirty.set(true);
            self.flush();
        }
    }

    /// 入力を反映し、止まってから保存する。確定した日は `allow_confirmed` のときだけ変える。
    fn update(self, allow_confirmed: bool, change: impl FnOnce(&mut Day)) {
        let Some(mut day) = self.day.get_untracked() else {
            return;
        };
        if day.is_confirmed() && !allow_confirmed {
            return;
        }
        change(&mut day);
        // 確定を取り消したときなど、手で直していない日はすぐ前の日に合わせ直す。
        let day = model::follow_carried_balance(&day, self.carried.get_untracked());
        self.day.set(Some(day));
        self.dirty.set(true);
        if self.state.get_untracked() != SaveState::Conflict {
            self.state.set(SaveState::Saving);
        }
        self.edits.update(|e| *e += 1);
        let edits = self.edits.get_untracked();
        spawn_local(async move {
            sleep(SAVE_DELAY).await;
            if self.edits.get_untracked() == edits {
                self.flush();
            }
        });
    }

    /// 入力をすぐ保存する（確定のときなど）。
    fn update_now(self, allow_confirmed: bool, change: impl FnOnce(&mut Day)) {
        self.update(allow_confirmed, change);
        self.flush();
    }

    /// まだ送っていない入力を保存する。
    fn flush(self) {
        if self.saving.get_untracked()
            || !self.dirty.get_untracked()
            || self.state.get_untracked() == SaveState::Conflict
        {
            return;
        }
        let (Some(day), Some(tenant)) = (self.day.get_untracked(), self.tenant.get_untracked()) else {
            return;
        };
        self.saving.set(true);
        self.dirty.set(false);
        self.state.set(SaveState::Saving);
        let generation = self.generation.get_untracked();
        let expected = self.version.get_untracked();
        let book_id = if self.legacy_origin.get_untracked() { None } else { self.book_id() };
        spawn_local(async move {
            let result = save(self.token, tenant, day.clone(), expected, book_id).await;
            self.saving.set(false);
            if self.generation.get_untracked() != generation {
                // 別の日を開いた後に届いた結果。まだ送っていない入力があれば送る。
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
                        self.state.set(SaveState::Saved);
                    }
                }
                Err(SaveError::Conflict) => {
                    self.state.set(SaveState::Conflict);
                    self.save_error.set(
                        "ほかの端末でこの日が保存されました。読み直してから入れ直してください。".into(),
                    );
                }
                Err(SaveError::ReceiptUsed { receipt_id, date }) => {
                    // 版のぶつかりではない。入力は残し、行を直したら次の保存で送り直す。
                    self.dirty.set(true);
                    self.state.set(SaveState::Error);
                    self.save_error.set(receipt_used_message(&day, &receipt_id, &date));
                }
                Err(SaveError::Other(message)) => {
                    self.dirty.set(true);
                    self.state.set(SaveState::Error);
                    self.save_error.set(message);
                }
            }
        });
    }

    /// 画面に関わらない保存（前の日の合わせ直し・画面を閉じたときの送り残し）。結果は見ない。
    fn send_quietly(&self, day: Day, version: i64, legacy: bool) {
        let Some(tenant) = self.tenant.get_untracked() else {
            return;
        };
        let book_id = if legacy { None } else { self.book_id() };
        let token = self.token;
        spawn_local(async move {
            let _ = save(token, tenant, day, Some(version), book_id).await;
        });
    }

    /// ほかの端末の保存を取り込む。入力中・保存中・ぶつかった後は取り込まない。
    fn pull(self) {
        let idle = move || {
            self.day.with_untracked(Option::is_some)
                && !self.dirty.get_untracked()
                && !self.saving.get_untracked()
                && self.state.get_untracked() != SaveState::Conflict
        };
        if !idle() {
            return;
        }
        let Some(tenant) = self.tenant.get_untracked() else {
            return;
        };
        let generation = self.generation.get_untracked();
        let date = self.date.get_untracked();
        let book_id = if self.legacy_origin.get_untracked() { None } else { self.book_id() };
        let legacy = self.legacy_origin.get_untracked();
        spawn_local(async move {
            let day_for = date.clone();
            let result = authed(self.token, move |access| {
                api::cash_book_days(access, &tenant, &day_for, &day_for, book_id.as_deref())
                    .map(|value| model::saved_days(&value, legacy))
            })
            .await;
            let Ok(saved) = result else {
                return;
            };
            if self.generation.get_untracked() != generation || !idle() {
                return;
            }
            let Some(remote) = saved.into_iter().find(|item| item.day.date == date) else {
                return;
            };
            if self.version.get_untracked().is_some_and(|local| remote.version <= local) {
                return;
            }
            // ほかの端末が保存した ① はそのまま使う。
            self.version.set(Some(remote.version));
            self.day.set(Some(remote.day));
            self.view.update(|v| *v += 1);
        });
    }
}

/// 期間の日を読む。先頭の帳簿なら、従来の帳簿の日も合わせる。
fn read_days(
    access: &str,
    tenant: &Tenant,
    from: &str,
    to: &str,
    book_id: Option<&str>,
    include_legacy: bool,
) -> Result<Vec<SavedDay>, String> {
    let scoped = api::cash_book_days(access, tenant, from, to, book_id)?;
    let scoped = model::saved_days(&scoped, book_id.is_none());
    if book_id.is_none() || !include_legacy {
        return Ok(scoped);
    }
    let legacy = api::cash_book_days(access, tenant, from, to, None)?;
    Ok(model::merge_days(scoped, model::saved_days(&legacy, true)))
}

async fn save(
    token: RwSignal<Option<String>>,
    tenant: Tenant,
    day: Day,
    expected: Option<i64>,
    book_id: Option<String>,
) -> Result<i64, SaveError> {
    let data = day.to_value();
    let date = day.date.clone();
    // 期限切れだけは文字で返して `authed` に取り直してもらう。ほかの断りはそのまま返す。
    let result = authed(token, move |access| {
        match api::save_cash_book_day(access, &tenant, &date, &data, expected, book_id.as_deref()) {
            Err(SaveError::Other(message)) if message == api::EXPIRED => Err(message),
            other => Ok(other),
        }
    })
    .await;
    result.unwrap_or_else(|message| Err(SaveError::Other(message)))
}

async fn sleep(duration: Duration) {
    run_blocking(move || std::thread::sleep(duration)).await;
}

/// 「11月6日」。
fn month_day(date: &str) -> String {
    let mut parts = date.split('-').skip(1).filter_map(|p| p.parse::<u32>().ok());
    match (parts.next(), parts.next()) {
        (Some(m), Some(d)) => format!("{m}月{d}日"),
        _ => date.to_owned(),
    }
}

/// ほかの日に入っているレシートを入れて断られたときの文言（Web 版と同じ）。
fn receipt_used_message(day: &Day, receipt_id: &str, used_on: &str) -> String {
    let row = day.expenses.iter().find(|row| row.receipt_id.as_deref() == Some(receipt_id));
    let label = [
        row.map(|r| r.payee.trim().to_owned()).filter(|p| !p.is_empty()),
        row.and_then(|r| r.amount).map(|a| format!("{}円", model::format_yen(a))),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("・");
    let which = if label.is_empty() { "レシート".to_owned() } else { format!("レシート（{label}）") };
    if used_on == day.date {
        return format!("{which}は同じ日の別の店舗・レジの帳簿に入っています。元の帳簿から外してから保存してください。");
    }
    let other = month_day(used_on);
    format!("{which}は、すでに {other}の出納帳に入っています。この日の行からレシートを外すか、{other}の行から外してから保存してください。")
}

#[component]
pub fn cash_book_screen(token: RwSignal<Option<String>>, tenant: RwSignal<Option<Tenant>>) -> Element {
    let books = signal(Books::Loading);
    let ledger = Ledger {
        token,
        tenant,
        book: signal(None),
        include_legacy: signal(false),
        date: signal(model::tokyo_today()),
        day: signal(None),
        version: signal(None),
        legacy_origin: signal(false),
        carried: signal(None),
        voucher_start: signal(1),
        load_error: signal(String::new()),
        state: signal(SaveState::Idle),
        save_error: signal(String::new()),
        generation: signal(0),
        view: signal(0),
        edits: signal(0),
        dirty: signal(false),
        saving: signal(false),
    };
    let choosing_book = signal(false);

    let load_books = move || {
        books.set(Books::Loading);
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        spawn_local(async move {
            match authed(token, move |access| api::cash_book_settings(access, &current)).await {
                Ok(settings) => {
                    let (list, first) = settings
                        .as_ref()
                        .map(model::books_from_settings)
                        .unwrap_or_default();
                    let picked = list.first().cloned();
                    ledger.include_legacy.set(picked.as_ref().is_some_and(|b| Some(&b.id) == first.as_ref()));
                    ledger.book.set(picked);
                    books.set(Books::Ready { books: list, first, legacy_api: settings.is_none() });
                    ledger.load();
                }
                Err(message) => books.set(Books::Failed(message)),
            }
        });
    };
    load_books();

    // 開いている間、ほかの端末の保存を見に行く。閉じたら止める。
    let alive = Arc::new(AtomicBool::new(true));
    {
        let alive = alive.clone();
        spawn_local(async move {
            loop {
                sleep(PULL_INTERVAL).await;
                if !alive.load(Ordering::Relaxed) {
                    break;
                }
                ledger.pull();
            }
        });
    }
    on_cleanup(move || {
        alive.store(false, Ordering::Relaxed);
        // 閉じる前に、まだ送っていない入力を送る。
        if ledger.dirty.get_untracked() && !ledger.saving.get_untracked() {
            if let (Some(day), Some(version)) = (ledger.day.get_untracked(), ledger.version.get_untracked()) {
                ledger.send_quietly(day, version, ledger.legacy_origin.get_untracked());
            } else if let (Some(day), Some(tenant)) = (ledger.day.get_untracked(), tenant.get_untracked()) {
                let book_id = ledger.book_id();
                spawn_local(async move {
                    let _ = save(token, tenant, day, None, book_id).await;
                });
            }
        }
    });

    let go = move |days: i64| {
        if ledger.day.with_untracked(Option::is_none) && ledger.load_error.with_untracked(String::is_empty) {
            return;
        }
        ledger.date.update(|d| *d = model::shift_date(d, days));
        ledger.load();
    };

    let status = computed(move || match ledger.state.get() {
        SaveState::Idle => String::new(),
        SaveState::Saving => "保存中…".into(),
        SaveState::Saved => "保存しました".into(),
        SaveState::Error | SaveState::Conflict => "保存できていません".into(),
    });

    // 帳簿設定がある会社では、従来の帳簿は記録のある日だけ開ける（新しく書くのは店舗・レジの帳簿）。
    let blocked = computed(move || {
        let (has_books, legacy_api) = books.with(|b| match b {
            Books::Ready { books, legacy_api, .. } => (!books.is_empty(), *legacy_api),
            _ => (false, true),
        });
        let legacy_book = ledger.book.with(Option::is_none);
        let has_record = ledger.version.get().is_some();
        if legacy_api || !legacy_book || has_record {
            return None;
        }
        Some(if has_books {
            "この日に従来の記録はありません。新しく記入するには上で店舗・レジの帳簿を選んでください。".to_owned()
        } else {
            "店舗・レジの帳簿がまだありません。管理者に Web の「帳簿設定」で作ってもらってください。".to_owned()
        })
    });

    render! {
        View(style: screen_style()) {
            NavBar(
                title: "現金出納帳",
                back_label: "",
                on_back: |_| {},
                action_label: "",
                on_action: |_| {},
            )
            ScrollView(style: scroll_style()) {
                View(style: scroll_content_style().padding_bottom(px(120))) {
                    Show(when: move || books.with(|b| *b == Books::Loading)) {
                        Notice(message: "帳簿設定を読み込んでいます…")
                    }
                    Show(when: move || books.with(|b| matches!(b, Books::Failed(_)))) {
                        ErrorNotice(message: computed(move || books.with(|b| match b {
                            Books::Failed(message) => format!("帳簿設定を読み込めませんでした。{message}"),
                            _ => String::new(),
                        })))
                        Button(label: "再読み込み", on_tap: move |_| load_books())
                    }
                    Show(when: move || books.with(|b| matches!(b, Books::Ready { .. }))) {
                        BookPicker(books: books, ledger: ledger, open: choosing_book)
                        DateBar(ledger: ledger, on_shift: move |days: i64| go(days))
                        Show(when: move || !ledger.load_error.with(String::is_empty)) {
                            ErrorNotice(message: computed(move || ledger.load_error.get()))
                            Button(label: "再読み込み", on_tap: move |_| ledger.load())
                        }
                        Show(when: move || ledger.day.with(Option::is_none) && ledger.load_error.with(String::is_empty)) {
                            Notice(message: "読み込み中…")
                        }
                        Show(when: move || ledger.day.with(Option::is_some) && blocked.get().is_some()) {
                            Text(
                                value: computed(move || blocked.get().unwrap_or_default()),
                                style: css!(
                                    color: hex(SECONDARY_LABEL),
                                    font_size: px(15),
                                    padding: (px(32), px(24)),
                                    text_align: TextAlign::Center,
                                ),
                            )
                        }
                        Show(when: move || ledger.day.with(Option::is_some) && blocked.get().is_none()) {
                            DaySheet(ledger: ledger, status: status)
                        }
                    }
                }
            }
        }
    }
}

/// 店舗・レジの帳簿を選ぶ。帳簿が無ければ出さない。
#[component]
fn book_picker(books: RwSignal<Books>, ledger: Ledger, open: RwSignal<bool>) -> Element {
    let options = computed(move || {
        books.with(|b| match b {
            Books::Ready { books, .. } => books.clone(),
            _ => Vec::new(),
        })
    });
    let first = computed(move || {
        books.with(|b| match b {
            Books::Ready { first, .. } => first.clone(),
            _ => None,
        })
    });
    let label = computed(move || {
        ledger
            .book
            .with(|b| b.as_ref().map_or_else(|| "従来の帳簿".to_owned(), Book::label))
    });
    let pick = move |book: Option<Book>| {
        open.set(false);
        if book.as_ref().map(|b| &b.id) == ledger.book.with_untracked(|b| b.as_ref().map(|b| b.id.clone())).as_ref() {
            return;
        }
        ledger.include_legacy.set(
            book.as_ref().is_some_and(|b| Some(&b.id) == first.get_untracked().as_ref()),
        );
        ledger.book.set(book);
        ledger.load();
    };
    render! {
        Show(when: move || options.with(|o| !o.is_empty())) {
            SectionHeader(title: computed(|| "店舗・レジ".to_owned()))
            View(style: group_style()) {
                FieldRow(
                    label: computed(move || label.get()),
                    value: computed(move || if open.get() { "閉じる".to_owned() } else { "変える".to_owned() }),
                    on_tap: move |_| {
                        if !ledger.dirty.get_untracked() && !ledger.saving.get_untracked() {
                            open.update(|o| *o = !*o);
                        }
                    },
                )
                Show(when: move || open.get()) {
                    ForEach(
                        each: move || {
                            let mut list: Vec<Option<Book>> = vec![None];
                            list.extend(options.get().into_iter().map(Some));
                            list
                        },
                        key: |b: &Option<Book>| b.as_ref().map(|b| b.id.clone()),
                        children: move |book: ReadSignal<Option<Book>>| {
                            let book = book.get_untracked();
                            let title = book.as_ref().map_or_else(
                                || "従来の帳簿".to_owned(),
                                |b| format!("{} — {}", b.label(), b.name),
                            );
                            let id = book.as_ref().map(|b| b.id.clone());
                            let checked = computed(move || {
                                ledger.book.with(|b| b.as_ref().map(|b| b.id.clone())) == id
                            });
                            render! {
                                FieldRow(
                                    label: computed(move || title.clone()),
                                    value: computed(move || if checked.get() { "✓".to_owned() } else { String::new() }),
                                    on_tap: move |_| pick(book.clone()),
                                )
                            }
                        },
                    )
                }
            }
        }
    }
}

/// 「‹ 2026年11月15日（日） ›」と「今日」。
#[component]
fn date_bar(ledger: Ledger, on_shift: Callback<i64>) -> Element {
    let today = model::tokyo_today();
    let is_today = computed(move || ledger.date.get() == today);
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: (px(16), px(16), px(4), px(16)),
            ),
        ) {
            Text(
                value: "‹",
                style: css!(color: hex(TINT), font_size: px(34), padding: (px(0), px(12))),
                on_tap: move |_| on_shift.run(-1),
            )
            View(
                style: css!(display: Display::Flex, flex_direction: FlexDirection::Column, align_items: AlignItems::Center),
            ) {
                Text(
                    value: computed(move || model::date_label(&ledger.date.get())),
                    style: css!(color: hex(LABEL), font_size: px(20), font_weight: FontWeight::Numeric(600)),
                )
                Show(when: move || !is_today.get()) {
                    Text(
                        value: "今日へ",
                        style: css!(color: hex(TINT), font_size: px(15), margin_top: px(4)),
                        on_tap: move |_| {
                            let today = model::tokyo_today();
                            let current = ledger.date.get_untracked();
                            let (Ok(a), Ok(b)) = (
                                chrono::NaiveDate::parse_from_str(&current, "%Y-%m-%d"),
                                chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d"),
                            ) else {
                                return;
                            };
                            on_shift.run((b - a).num_days());
                        },
                    )
                }
            }
            Text(
                value: "›",
                style: css!(color: hex(TINT), font_size: px(34), padding: (px(0), px(12))),
                on_tap: move |_| on_shift.run(1),
            )
        }
    }
}

/// 1 日分の紙：残高のまとめ、収入、支出、確定。
#[component]
fn day_sheet(ledger: Ledger, status: Signal<String>) -> Element {
    let sum = move |f: fn(&Day) -> i64| {
        computed(move || ledger.day.with(|d| d.as_ref().map_or(0, f)))
    };
    let income = sum(Day::cash_income_total);
    let expense = sum(Day::expense_total);
    let closing = sum(Day::closing_balance);
    let confirmed = computed(move || ledger.confirmed());
    let previous_manual =
        computed(move || ledger.day.with(|d| d.as_ref().is_some_and(|d| d.previous_balance_manual)));

    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Show(when: move || !status.get().is_empty()) {
                Text(
                    value: status,
                    style: computed(move || css!(
                        color: hex(if matches!(ledger.state.get(), SaveState::Error | SaveState::Conflict) { RED } else { SECONDARY_LABEL }),
                        font_size: px(13),
                        text_align: TextAlign::Center,
                    )),
                )
            }
            Show(when: move || !ledger.save_error.with(String::is_empty)) {
                ErrorNotice(message: computed(move || ledger.save_error.get()))
                Show(when: move || ledger.state.get() == SaveState::Conflict) {
                    Button(label: "読み直す", on_tap: move |_| {
                        ledger.state.set(SaveState::Idle);
                        ledger.dirty.set(false);
                        ledger.load();
                    })
                }
                Show(when: move || ledger.state.get() == SaveState::Error) {
                    Button(label: "もう一度保存", on_tap: move |_| ledger.flush())
                }
            }
            Show(when: move || confirmed.get()) {
                Text(
                    value: "この日は確定しています。直すときは下の「確定を取り消す」を押してください。",
                    style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(32), px(0), px(32))),
                )
            }

            SectionHeader(title: computed(|| "残高".to_owned()))
            View(style: group_style()) {
                // ① は打ち直せる。打ち直した日は、前の日に合わせない。
                ForEach(
                    each: move || vec![(ledger.view.get(), confirmed.get())],
                    key: |k: &(u64, bool)| *k,
                    children: move |_k: ReadSignal<(u64, bool)>| {
                        let initial = ledger.day.with_untracked(|d| {
                            model::format_optional_yen(d.as_ref().and_then(|d| d.previous_balance))
                        });
                        render! {
                            AmountRow(
                                label: computed(|| "① 前日現金残高".to_owned()),
                                initial: initial,
                                editable: !ledger.confirmed(),
                                on_amount: move |amount: Option<i64>| ledger.update(false, |d| {
                                    d.previous_balance = amount;
                                    d.previous_balance_manual = true;
                                }),
                            )
                        }
                    },
                )
                Show(when: move || previous_manual.get() && !confirmed.get() && ledger.carried.get().is_some()) {
                    FieldRow(
                        label: computed(move || format!(
                            "前の日の残高（{}円）に合わせる",
                            model::format_optional_yen(ledger.carried.get()),
                        )),
                        value: computed(String::new),
                        on_tap: move |_| {
                            ledger.update(false, |d| d.previous_balance_manual = false);
                            ledger.view.update(|v| *v += 1);
                        },
                    )
                }
                TotalRow(label: "② 現金収入合計", amount: income)
                TotalRow(label: "③ 支出合計", amount: expense)
                TotalRow(label: "本日現金残高（①+②−③）", amount: closing)
            }

            SectionHeader(title: computed(|| "収入".to_owned()))
            View(style: group_style()) {
                ForEach(
                    each: move || {
                        let (view, locked) = (ledger.view.get(), confirmed.get());
                        ledger.template().incomes.into_iter().map(|row| (view, locked, row.key, row.label, row.deduct)).collect::<Vec<_>>()
                    },
                    key: |k: &(u64, bool, String, String, bool)| (k.0, k.1, k.2.clone()),
                    children: move |row: ReadSignal<(u64, bool, String, String, bool)>| {
                        let (_, locked, key, label, _) = row.get_untracked();
                        let initial = ledger.day.with_untracked(|d| {
                            model::format_optional_yen(d.as_ref().and_then(|d| d.incomes.get(&key).copied().flatten()))
                        });
                        render! {
                            AmountRow(
                                label: computed(move || label.clone()),
                                initial: initial,
                                editable: !locked,
                                on_amount: move |amount: Option<i64>| {
                                    let key = key.clone();
                                    ledger.update(false, move |d| { d.incomes.insert(key, amount); });
                                },
                            )
                        }
                    },
                )
            }

            SectionHeader(title: computed(|| "支出".to_owned()))
            ForEach(
                each: move || {
                    let (view, locked) = (ledger.view.get(), confirmed.get());
                    let ids = ledger.day.with(|d| d.as_ref().map(|d| d.expenses.iter().map(|e| e.id.clone()).collect::<Vec<_>>()).unwrap_or_default());
                    ids.into_iter().map(|id| (view, locked, id)).collect::<Vec<_>>()
                },
                key: |k: &(u64, bool, String)| k.clone(),
                children: move |row: ReadSignal<(u64, bool, String)>| {
                    let (_, locked, id) = row.get_untracked();
                    render! { ExpenseCard(ledger: ledger, id: id, locked: locked) }
                },
            )
            Show(when: move || !confirmed.get()) {
                Button(label: "＋ 支出の行を足す", on_tap: move |_| {
                    let category = ledger.template().default_expense;
                    ledger.update(false, move |d| d.expenses.push(Expense {
                        id: model::new_expense_id(),
                        amount: None,
                        category,
                        description: String::new(),
                        payee: String::new(),
                        purchased_on: None,
                        receipt_id: None,
                    }));
                    ledger.view.update(|v| *v += 1);
                })
            }
            Show(when: move || ledger.day.with(|d| d.as_ref().is_some_and(|d| d.expenses.is_empty())) && confirmed.get()) {
                Notice(message: "支出はありません")
            }

            View(style: css!(height: px(24)))
            Show(when: move || !confirmed.get()) {
                Button(label: "この日を確定する", on_tap: move |_| {
                    ledger.update_now(false, |d| d.confirmed_at = Some(model::now_iso()));
                })
            }
            Show(when: move || confirmed.get()) {
                Button(label: "確定を取り消す", on_tap: move |_| {
                    ledger.update_now(true, |d| d.confirmed_at = None);
                })
            }
        }
    }
}

/// 支出の 1 行：科目（押すと選べる）、支払先、取引内容、金額、証番、削除。
#[component]
fn expense_card(ledger: Ledger, id: String, locked: bool) -> Element {
    let row = ledger
        .day
        .with_untracked(|d| d.as_ref().and_then(|d| d.expenses.iter().find(|e| e.id == id).cloned()));
    let Some(row) = row else {
        return render! { View() };
    };
    let choosing = signal(false);
    let payee = signal(row.payee.clone());
    let description = signal(row.description.clone());
    // 閉じた関数にそのまま渡せるよう、行の ID は signal に入れておく（Copy になる）。
    let id = signal(id.clone());
    let category = computed(move || {
        let key = ledger.day.with(|d| {
            d.as_ref()
                .and_then(|d| id.with(|id| d.expenses.iter().find(|e| &e.id == id).map(|e| e.category.clone())))
                .unwrap_or_default()
        });
        ledger.template().category_label(&key)
    });
    let voucher = computed(move || {
        let template = ledger.template();
        let start = ledger.voucher_start.get();
        ledger.day.with(|d| {
            d.as_ref()
                .and_then(|d| id.with(|id| model::voucher_numbers(d, start, &template).get(id).copied()))
                .map_or_else(String::new, |n| format!("証番 {n}"))
        })
    });
    let set = move |change: Box<dyn FnOnce(&mut Expense)>| {
        let id = id.get_untracked();
        ledger.update(false, move |d| {
            if let Some(row) = d.expenses.iter_mut().find(|e| e.id == id) {
                change(row);
            }
        });
    };
    let remove = move || {
        let id = id.get_untracked();
        ledger.update(false, move |d| d.expenses.retain(|e| e.id != id));
    };
    let from_receipt = row.receipt_id.is_some();
    let purchased_on = row.purchased_on.clone();
    let note = computed(move || {
        purchased_on
            .as_deref()
            .filter(|day| *day != ledger.date.get())
            .map(|day| format!("{} 購入", month_day(day)))
            .unwrap_or_default()
    });

    render! {
        View(style: group_style().margin_bottom(px(12))) {
            FieldRow(
                label: category,
                value: computed(move || if locked { voucher.get() } else if choosing.get() { "閉じる".to_owned() } else { "科目を変える".to_owned() }),
                on_tap: move |_| if !locked { choosing.update(|c| *c = !*c) },
            )
            Show(when: move || choosing.get()) {
                ForEach(
                    each: move || ledger.template().expenses.into_iter().map(|c| (c.key, c.label)).collect::<Vec<_>>(),
                    key: |c: &(String, String)| c.0.clone(),
                    children: move |c: ReadSignal<(String, String)>| {
                        let (key, label) = c.get_untracked();
                        render! {
                            FieldRow(
                                label: computed(move || format!("　{label}")),
                                value: computed(String::new),
                                on_tap: move |_| {
                                    let key = key.clone();
                                    set(Box::new(move |row| row.category = key));
                                    choosing.set(false);
                                },
                            )
                        }
                    },
                )
            }
            TextRow(
                label: "支払先",
                text: payee,
                editable: !locked,
                on_text: move |text: String| set(Box::new(move |row| row.payee = text)),
            )
            TextRow(
                label: "内容",
                text: description,
                editable: !locked,
                on_text: move |text: String| set(Box::new(move |row| row.description = text)),
            )
            AmountRow(
                label: computed(|| "金額".to_owned()),
                initial: model::format_optional_yen(row.amount),
                editable: !locked,
                on_amount: move |amount: Option<i64>| set(Box::new(move |row| row.amount = amount)),
            )
            Show(when: move || !locked) {
                View(
                    style: css!(
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        padding: (px(10), px(16)),
                    ),
                ) {
                    Text(
                        value: computed(move || {
                            [voucher.get(), if from_receipt { "レシートから".to_owned() } else { String::new() }, note.get()]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .collect::<Vec<_>>()
                                .join(" · ")
                        }),
                        style: css!(color: hex(SECONDARY_LABEL), font_size: px(13)),
                    )
                    Text(
                        value: "この行を消す",
                        style: css!(color: hex(RED), font_size: px(15)),
                        on_tap: move |_| remove(),
                    )
                }
            }
        }
    }
}

/// 左に名前、右に金額の入力欄。打つたびに読み、読めない文字のときは赤く知らせる。
#[component]
fn amount_row(label: Signal<String>, initial: String, editable: bool, on_amount: Callback<Option<i64>>) -> Element {
    let text = signal(initial.clone());
    let invalid = signal(false);
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding_left: px(16),
                min_height: px(48),
            )
            .border_bottom(hairline()),
        ) {
            Text(
                value: label,
                style: css!(color: hex(LABEL), font_size: px(17), flex_grow: 1.0, flex_shrink: 1.0),
            )
            Show(when: move || invalid.get()) {
                Text(value: "数字で", style: css!(color: hex(RED), font_size: px(13), margin_right: px(6)))
            }
            Input(
                text: text,
                placeholder: "0",
                keyboard_type: KeyboardType::Number,
                editable: editable,
                on_input: move |value: String| match model::parse_yen(&value) {
                    Ok(amount) => {
                        invalid.set(false);
                        on_amount.run(amount);
                    }
                    Err(()) => invalid.set(true),
                },
                on_blur: move |_| {
                    // 打ち終えたら 3 桁区切りにそろえる。
                    if let Ok(amount) = model::parse_yen(&text.get_untracked()) {
                        text.set(model::format_optional_yen(amount));
                    }
                },
                style: css!(
                    width: px(140),
                    height: px(48),
                    font_size: px(17),
                    padding_right: px(16),
                    text_align: TextAlign::Right,
                    color: hex(if editable { LABEL } else { SECONDARY_LABEL }),
                    background_color: Color::rgba(0, 0, 0, 0.0),
                ),
            )
            Text(value: "円", style: css!(color: hex(SECONDARY_LABEL), font_size: px(15), margin_right: px(16)))
        }
    }
}

/// 左に名前、右に文字の入力欄。
#[component]
fn text_row(label: &'static str, text: RwSignal<String>, editable: bool, on_text: Callback<String>) -> Element {
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding_left: px(16),
                min_height: px(48),
            )
            .border_bottom(hairline()),
        ) {
            Text(value: label, style: css!(color: hex(SECONDARY_LABEL), font_size: px(15), width: px(64)))
            Input(
                text: text,
                placeholder: "未入力",
                editable: editable,
                on_input: move |value: String| on_text.run(value),
                style: css!(
                    flex_grow: 1.0,
                    height: px(48),
                    font_size: px(17),
                    padding_right: px(16),
                    color: hex(if editable { LABEL } else { SECONDARY_LABEL }),
                    background_color: Color::rgba(0, 0, 0, 0.0),
                ),
            )
        }
    }
}

/// 合計の行（打てない）。
#[component]
fn total_row(label: &'static str, amount: Signal<i64>) -> Element {
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: (px(0), px(16)),
                min_height: px(48),
            )
            .border_bottom(hairline()),
        ) {
            Text(value: label, style: css!(color: hex(LABEL), font_size: px(17)))
            Text(
                value: computed(move || format!("{}円", model::format_yen(amount.get()))),
                style: css!(color: hex(LABEL), font_size: px(17), font_weight: FontWeight::Numeric(600)),
            )
        }
    }
}

/// 押せる行：左に名前、右に青い文字。
#[component]
fn field_row(label: Signal<String>, value: Signal<String>, on_tap: Callback<()>) -> Element {
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                gap: px(8),
                padding: (px(0), px(16)),
                min_height: px(48),
            )
            .border_bottom(hairline()),
            on_tap: move |_| on_tap.call(),
        ) {
            Text(
                value: label,
                max_lines: 2u32,
                style: css!(color: hex(LABEL), font_size: px(17), flex_shrink: 1.0),
            )
            Text(value: value, style: css!(color: hex(TINT), font_size: px(15), flex_shrink: 0.0))
        }
    }
}

/// 角の丸い青い文字のボタン（白い板の上）。
#[component]
fn button(label: &'static str, on_tap: Callback<()>) -> Element {
    render! {
        View(
            style: group_style().margin_top(px(12)),
            on_tap: move |_| on_tap.call(),
        ) {
            Text(
                value: label,
                style: css!(
                    color: hex(TINT),
                    font_size: px(17),
                    text_align: TextAlign::Center,
                    padding: (px(14), px(16)),
                ),
            )
        }
    }
}
