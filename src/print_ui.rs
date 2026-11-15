//! 印刷の部分（1 日分と、期間印刷）。期間印刷は保存済みの日だけを日付順に 1 日 1 ページで組む。

use super::*;
use crate::cash_book::{self as cash, Day, SavedDay, Template};
use crate::cash_book_screen::*;
use crate::cash_book_settings_screen::Hint;
use crate::print;
use crate::register_count as reg;

/// どの帳票か。
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Cash,
    Register,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Cash => "出納帳",
            Kind::Register => "レジ管理表",
        }
    }
}

fn button_style() -> Css {
    css!(height: px(48), margin: (px(0), px(16)))
}

/// 1 日分の印刷ボタン。
#[component]
pub(crate) fn day_print(html: Signal<String>, name: Signal<String>, landscape: Signal<bool>) -> Element {
    render! {
        View(style: group_style().margin_top(px(12))) {
            field_glass::PrintButton(label: "この日を印刷・PDF".to_owned(), html: html, name: name, landscape: landscape, style: button_style())
        }
    }
}

/// 期間印刷。`book_id` が無ければ従来の帳簿（出納帳のみ）。
#[component]
pub(crate) fn period_print(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    kind: Kind,
    book_id: Signal<Option<String>>,
    fallback: Signal<Option<Template>>,
    date: Signal<String>,
) -> Element {
    let open = signal(false);
    let from = signal(String::new());
    let to = signal(String::new());
    let loading = signal(false);
    let message = signal(String::new());
    let html = signal(String::new());
    let name = signal(String::new());
    let landscape = signal(false);
    let reset = move || {
        let d = date.get();
        from.set(format!("{}-01", &d[..7]));
        to.set(month_end(&d));
        html.set(String::new());
        message.set(String::new());
    };
    let prepare = move || {
        let (a, b) = (from.get_untracked().trim().to_owned(), to.get_untracked().trim().to_owned());
        let (Some(a), Some(b)) = (crate::cash_book_receipts::normalize_date(&a), crate::cash_book_receipts::normalize_date(&b)) else {
            message.set("開始日と終了日を入力してください。".into());
            return;
        };
        if b < a {
            message.set("終了日は開始日以降にしてください。".into());
            return;
        }
        if cash::shift_date(&a, 91) < b {
            message.set("一度に指定できる期間は92日間です。".into());
            return;
        }
        let Some(current) = tenant.get_untracked() else { return };
        let id = book_id.get();
        let fb = fallback.get();
        loading.set(true);
        message.set(String::new());
        html.set(String::new());
        spawn_local(async move {
            let (fa, fb_to) = (a.clone(), b.clone());
            let result = authed(token, move |access| -> Result<(Vec<String>, bool), String> {
                match kind {
                    Kind::Cash => {
                        let history = api::cash_book_days(access, &current, &cash::shift_date(&fa, -62), &cash::shift_date(&fa, -1), id.as_deref())?;
                        let range = api::cash_book_days(access, &current, &fa, &fb_to, id.as_deref())?;
                        let mut all: Vec<SavedDay> = cash::saved_days(&history, id.is_none());
                        all.extend(cash::saved_days(&range, id.is_none()));
                        let days = cash::with_carried_balances(&all.into_iter().map(|s| s.day).collect::<Vec<_>>());
                        let mut landscape = false;
                        let pages = days
                            .iter()
                            .filter(|d| d.date >= fa && d.date <= fb_to)
                            .map(|d: &Day| {
                                landscape |= print::cash_landscape(d, fb.as_ref());
                                print::cash_book_page(d, fb.as_ref(), cash::first_voucher_number(&d.date, &days))
                            })
                            .collect();
                        Ok((pages, landscape))
                    }
                    Kind::Register => {
                        let Some(id) = id.clone() else { return Ok((Vec::new(), true)) };
                        let value = api::register_count_days(access, &current, &fa, &fb_to, &id)?;
                        let days: Vec<reg::Day> = value["items"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|i| Some(reg::Day::from_value(i["date"].as_str()?, &i["data"])))
                            .collect();
                        let landscape = days.first().is_none_or(print::register_landscape);
                        Ok((days.iter().map(print::register_page).collect(), landscape))
                    }
                }
            })
            .await;
            loading.set(false);
            match result {
                Ok((pages, _)) if pages.is_empty() => {
                    message.set(format!("この期間に保存された{}はありません。", if kind == Kind::Cash { "現金出納帳" } else { "レジ管理表" }));
                }
                Ok((pages, l)) => {
                    let title = print::file_name(kind.name(), &a, &b);
                    html.set(print::document(&title, &pages, l));
                    name.set(title);
                    landscape.set(l);
                    message.set(format!("{} 日分を用意しました。下のボタンで印刷・PDF保存できます。", pages.len()));
                }
                Err(_) => message.set("期間の印刷を準備できませんでした。もう一度お試しください。".into()),
            }
        });
    };
    render! {
        View(style: group_style().margin_top(px(12))) {
            FieldRow(
                label: computed(move || format!("{}の期間印刷", if kind == Kind::Cash { "現金出納帳" } else { "レジ管理表" })),
                value: computed(move || if open.get() { "閉じる".to_owned() } else { "開く".to_owned() }),
                on_tap: move |_| {
                    let next = !open.get_untracked();
                    open.set(next);
                    if next { reset(); }
                },
            )
            Show(when: move || open.get()) {
                Hint(text: "期間内に保存済みの帳票を日付順に印刷します。印刷画面で「PDFに保存」も選べます。")
                FieldRow(label: computed(|| "今月（月初から月末まで）".to_owned()), value: computed(String::new), on_tap: move |_| reset())
                TextRow(label: "開始日", text: from, editable: true, on_text: move |_| html.set(String::new()))
                TextRow(label: "終了日", text: to, editable: true, on_text: move |_| html.set(String::new()))
                FieldRow(
                    label: computed(move || if loading.get() { "読み込み中…".to_owned() } else { "期間の帳票を用意する".to_owned() }),
                    value: computed(String::new),
                    on_tap: move |_| if !loading.get_untracked() { prepare() },
                )
                Show(when: move || !message.with(String::is_empty)) {
                    Text(value: computed(move || message.get()), style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(6), px(16))))
                }
                Show(when: move || !html.with(String::is_empty)) {
                    field_glass::PrintButton(label: "期間を印刷・PDF".to_owned(), html: computed(move || html.get()), name: computed(move || name.get()), landscape: computed(move || landscape.get()), style: button_style())
                }
            }
        }
    }
}

/// その月の末日。
fn month_end(date: &str) -> String {
    let first = format!("{}-01", &date[..7]);
    let next = cash::shift_date(&first, 32);
    cash::shift_date(&format!("{}-01", &next[..7]), -1)
}
