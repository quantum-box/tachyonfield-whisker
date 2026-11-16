//! 証憑OCR の画面（Web 版の receipts-page.tsx と同じことをする）。
//!
//! - 一覧：状態・種別・未処理のみで絞る。読み取り中のものがあるあいだは 5 秒ごとに読み直す
//! - アップロード：種別（仕入・売上・あとで仕分け）と支払った人を選び、写真・PDF を上げる
//! - 詳細：原本、読み取り結果、種別の変更、重複候補の確認、再解析、読み取りを直す、台帳へ反映、経費にする
//!
//! Google Drive からの取り込みと、レジの読み方の設定は Web で行う。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::cash_book_receipts as rules;
use crate::cash_book_screen::{Button, FieldRow, TextRow, sleep, spawn_in};
use crate::cash_book_settings_screen::{Choice, Hint};
use crate::receipt_ocr::{self as model, OcrReceipt};
use whisker::Owner;

/// 読み取り中のものを見に行く間隔（Web と同じ）。
const POLL_INTERVAL: Duration = Duration::from_secs(5);

fn options(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// 画面の状態。
#[derive(Clone, Copy)]
struct Ocr {
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    owner: Option<Owner>,
    list: RwSignal<Vec<OcrReceipt>>,
    loaded: RwSignal<bool>,
    loading: RwSignal<bool>,
    error: RwSignal<String>,
    truncated: RwSignal<bool>,
    status: RwSignal<String>,
    kind: RwSignal<String>,
    unprocessed: RwSignal<bool>,
    upload_kind: RwSignal<String>,
    payer: RwSignal<String>,
    uploading: RwSignal<bool>,
    message: RwSignal<String>,
    /// 開いている証憑（一覧に無いときは 1 件だけ読んだもの）。
    selected: RwSignal<Option<OcrReceipt>>,
    /// 読み直すたびに増やす（古い応答で上書きしないため）。
    generation: RwSignal<u64>,
}

impl Ocr {
    fn load(self) {
        let Some(tenant) = self.tenant.get_untracked() else {
            return;
        };
        let (status, kind, unprocessed) = (
            self.status.get_untracked(),
            self.kind.get_untracked(),
            self.unprocessed.get_untracked(),
        );
        self.generation.update(|g| *g += 1);
        let generation = self.generation.get_untracked();
        self.loading.set(true);
        spawn_in(self.owner, async move {
            let result = authed(self.token, move |access| {
                api::search_receipts(access, &tenant, &kind, &status, unprocessed)
            })
            .await;
            if self.generation.get_untracked() != generation {
                return;
            }
            self.loading.set(false);
            self.loaded.set(true);
            match result {
                Ok((items, truncated)) => {
                    let list: Vec<OcrReceipt> =
                        items.iter().filter_map(OcrReceipt::from_value).collect();
                    // 開いている証憑も新しくする。
                    if let Some(open) = self.selected.get_untracked()
                        && let Some(fresh) = list.iter().find(|r| r.id() == open.id())
                    {
                        self.selected.set(Some(fresh.clone()));
                    }
                    self.list.set(list);
                    self.truncated.set(truncated);
                    self.error.set(String::new());
                }
                Err(message) => self
                    .error
                    .set(format!("証憑OCR一覧を取得できませんでした。{message}")),
            }
        });
    }

    /// 開いている証憑が一覧に無いとき（絞り込みの外）に、それだけ読み直す。
    fn refresh_selected(self) {
        let (Some(tenant), Some(open)) =
            (self.tenant.get_untracked(), self.selected.get_untracked())
        else {
            return;
        };
        if self
            .list
            .with_untracked(|l| l.iter().any(|r| r.id() == open.id()))
        {
            return;
        }
        let id = open.id().to_owned();
        spawn_in(self.owner, async move {
            if let Ok(value) = authed(self.token, move |access| {
                api::get_receipt(access, &tenant, &id)
            })
            .await
                && let Some(fresh) = OcrReceipt::from_value(&value)
                && self
                    .selected
                    .with_untracked(|s| s.as_ref().is_some_and(|s| s.id() == fresh.id()))
            {
                self.selected.set(Some(fresh));
            }
        });
    }

    /// サーバーが返した 1 件で、一覧と詳細を置き換える。
    fn replace(self, value: &Value) -> Option<OcrReceipt> {
        let fresh = OcrReceipt::from_value(value)?;
        self.list.update(|l| {
            if let Some(slot) = l.iter_mut().find(|r| r.id() == fresh.id()) {
                *slot = fresh.clone();
            }
        });
        if self
            .selected
            .with_untracked(|s| s.as_ref().is_some_and(|s| s.id() == fresh.id()))
        {
            self.selected.set(Some(fresh.clone()));
        }
        Some(fresh)
    }

    fn any_reading(self) -> bool {
        self.list
            .with_untracked(|l| l.iter().any(OcrReceipt::is_reading))
            || self
                .selected
                .with_untracked(|s| s.as_ref().is_some_and(OcrReceipt::is_reading))
    }

    fn upload(self, file: field_glass::PickedFile) {
        if self.uploading.get_untracked() {
            return;
        }
        let Some(tenant) = self.tenant.get_untracked() else {
            return;
        };
        let ok_type = matches!(
            file.content_type.as_str(),
            "" | "image/jpeg" | "image/png" | "image/heic" | "image/heif" | "application/pdf"
        );
        if !ok_type {
            self.message
                .set("JPEG、PNG、PDF、HEIC のいずれかを選択してください。".into());
            return;
        }
        let bytes = match std::fs::read(&file.path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.message
                    .set(format!("ファイルを読み込めませんでした。{e}"));
                return;
            }
        };
        let content_type = if file.content_type.is_empty() {
            "image/jpeg".to_owned()
        } else {
            file.content_type.clone()
        };
        let name = if file.name.is_empty() {
            "receipt.jpg".to_owned()
        } else {
            file.name.clone()
        };
        let kind = self.upload_kind.get_untracked();
        let payer = self.payer.get_untracked();
        self.uploading.set(true);
        self.message.set(String::new());
        spawn_in(self.owner, async move {
            let result = authed(self.token, move |access| {
                api::upload_receipt(
                    access,
                    &tenant,
                    &name,
                    &content_type,
                    &bytes,
                    &kind,
                    (!payer.is_empty()).then_some(payer.as_str()),
                )
            })
            .await;
            self.uploading.set(false);
            match result {
                Ok(value) => {
                    let duplicate = value["alreadyUploaded"].as_bool().unwrap_or(false);
                    self.message.set(if duplicate {
                        "すでに取り込み済みです。重複登録はしていません。".into()
                    } else {
                        "1 件の証憑を受け付けました。OCR解析を開始しました。".into()
                    });
                    if let Some(fresh) = OcrReceipt::from_value(&value) {
                        // 上げたものの種別で見えるようにして、開く。
                        if !self.kind.get_untracked().is_empty() {
                            self.kind.set(fresh.receipt.receipt_type.clone());
                        }
                        self.selected.set(Some(fresh));
                    }
                    self.load();
                }
                Err(message) => self
                    .message
                    .set(format!("証憑をアップロードできませんでした。{message}")),
            }
        });
    }
}

/// 証憑OCR の画面。
#[component]
pub fn receipt_ocr_screen(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
) -> Element {
    let ocr = Ocr {
        token,
        tenant,
        owner: Owner::current(),
        list: signal(Vec::new()),
        loaded: signal(false),
        loading: signal(false),
        error: signal(String::new()),
        truncated: signal(false),
        status: signal(String::new()),
        kind: signal(String::new()),
        unprocessed: signal(false),
        upload_kind: signal("unsorted".to_owned()),
        payer: signal(String::new()),
        uploading: signal(false),
        message: signal(String::new()),
        selected: signal(None),
        generation: signal(0),
    };
    ocr.load();

    // 読み取り中のものがあるあいだ、5 秒ごとに読み直す。画面を閉じたら止める。
    let alive = Arc::new(AtomicBool::new(true));
    {
        let alive = alive.clone();
        spawn_local(async move {
            loop {
                sleep(POLL_INTERVAL).await;
                if !alive.load(Ordering::Relaxed) {
                    break;
                }
                if ocr.any_reading() && !ocr.loading.get_untracked() {
                    ocr.load();
                    ocr.refresh_selected();
                }
            }
        });
    }
    on_cleanup(move || alive.store(false, Ordering::Relaxed));

    // iPad で広いときは、左に一覧、右に選んだ証憑（メールアプリのような 2 列）。
    let pane = crate::pane_width();
    let split = computed(move || pane.get() >= crate::TWO_COLUMN_WIDTH);
    let opened = move || ocr.selected.with(|s| s.as_ref().map(|r| r.id().to_owned())).into_iter().collect::<Vec<_>>();
    render! {
        View(style: screen_style()) {
            NavBar(
                title: "証憑OCR",
                back_label: "",
                on_back: |_| {},
                action_label: "更新",
                on_action: move |_| ocr.load(),
            )
            // 一覧と詳細は 1 つずつだけ作り、並べ方だけを変える（回しても入力中のフォームが消えないように）。
            // 狭いとき：選んでいなければ一覧、選んでいれば詳細を出す。広いとき：左に一覧、右に詳細。
            View(
                style: computed(move || {
                    css!(display: Display::Flex, flex_direction: if split.get() { FlexDirection::Row } else { FlexDirection::Column }, flex_grow: 1.0)
                        .flex_basis(px(0))
                        .min_height(px(0))
                }),
            ) {
                ScrollView(
                    style: computed(move || {
                        let chosen = ocr.selected.with(Option::is_some);
                        if split.get() {
                            css!(width: px(380), flex_shrink: 0.0).border_right(hairline())
                        } else if chosen {
                            css!(display: Display::None)
                        } else {
                            scroll_style()
                        }
                    }),
                ) {
                    View(style: scroll_content_style().padding_bottom(px(120))) {
                        ListView(ocr: ocr)
                    }
                }
                ScrollView(
                    style: computed(move || {
                        if !split.get() && ocr.selected.with(Option::is_none) {
                            css!(display: Display::None)
                        } else {
                            css!(flex_grow: 1.0).flex_basis(px(0)).min_width(px(0)).min_height(px(0))
                        }
                    }),
                ) {
                    View(style: scroll_content_style().padding_bottom(px(120))) {
                        Show(when: move || split.get() && ocr.selected.with(Option::is_none)) {
                            Text(
                                value: "左の一覧から証憑を選ぶと、ここに原本と読み取り結果が出ます。",
                                style: css!(color: hex(SECONDARY_LABEL), font_size: px(15), padding: (px(80), px(40)), text_align: TextAlign::Center),
                            )
                        }
                        ForEach(
                            each: opened,
                            key: |id: &String| id.clone(),
                            children: move |_id: ReadSignal<String>| render! { Detail(ocr: ocr, split: split) },
                        )
                    }
                }
            }
        }
    }
}

/// 一覧：アップロード、絞り込み、証憑の並び。
#[component]
fn list_view(ocr: Ocr) -> Element {
    let upload_hint = computed(move || {
        if ocr.upload_kind.get() == "unsorted" {
            "写真やPDFを1枚ずつ選べます。売上か仕入かは後で決められます（未処理のみで後から一覧できます）。".to_owned()
        } else {
            "写真やPDFを1枚ずつ選べます。1ファイルずつ別の証憑として読み取ります。".to_owned()
        }
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            SectionHeader(title: computed(|| "証憑を追加".to_owned()))
            View(style: group_style()) {
                Choice(
                    label: computed(|| "種別".to_owned()),
                    options: computed(|| options(model::UPLOAD_TYPES)),
                    selected: computed(move || ocr.upload_kind.get()),
                    on_pick: move |k: String| ocr.upload_kind.set(k),
                )
                Choice(
                    label: computed(|| "支払い".to_owned()),
                    options: computed(|| options(model::PAYERS)),
                    selected: computed(move || ocr.payer.get()),
                    on_pick: move |k: String| ocr.payer.set(k),
                )
                View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Row, align_items: AlignItems::Center, gap: px(12), padding: (px(10), px(16)))) {
                    AttachButton(on_pick: move |f: field_glass::PickedFile| ocr.upload(f), style: css!(width: px(36), height: px(36)))
                    Text(
                        value: computed(move || if ocr.uploading.get() { "送信中…".to_owned() } else { "証憑を追加・撮影".to_owned() }),
                        style: css!(color: hex(TINT), font_size: px(17)),
                    )
                }
            }
            Text(value: upload_hint, style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(32), px(0), px(32))))
            Show(when: move || !ocr.message.with(String::is_empty)) {
                Text(
                    value: computed(move || ocr.message.get()),
                    style: css!(color: hex(TINT), font_size: px(14), padding: (px(8), px(32), px(0), px(32))),
                    on_tap: move |_| ocr.message.set(String::new()),
                )
            }

            SectionHeader(title: computed(|| "絞り込み".to_owned()))
            View(style: group_style()) {
                Choice(
                    label: computed(|| "状態".to_owned()),
                    options: computed(|| options(model::STATUS_FILTERS)),
                    selected: computed(move || ocr.status.get()),
                    on_pick: move |k: String| { ocr.status.set(k); ocr.load(); },
                )
                Choice(
                    label: computed(|| "種別".to_owned()),
                    options: computed(|| options(model::TYPE_FILTERS)),
                    selected: computed(move || ocr.kind.get()),
                    on_pick: move |k: String| { ocr.kind.set(k); ocr.load(); },
                )
                FieldRow(
                    label: computed(|| "未処理のみ".to_owned()),
                    value: computed(move || if ocr.unprocessed.get() { "オン".to_owned() } else { "オフ".to_owned() }),
                    on_tap: move |_| { ocr.unprocessed.update(|u| *u = !*u); ocr.load(); },
                )
            }

            Show(when: move || ocr.loading.get() && !ocr.loaded.get()) {
                Hint(text: "証憑を読み込んでいます…")
            }
            Show(when: move || !ocr.error.with(String::is_empty)) {
                Text(value: computed(move || ocr.error.get()), style: css!(color: hex(RED), font_size: px(14), padding: (px(12), px(32))))
                Button(label: "再読み込み", on_tap: move |_| ocr.load())
            }
            Show(when: move || ocr.loaded.get() && ocr.error.with(String::is_empty) && ocr.list.with(Vec::is_empty)) {
                Hint(text: "条件に一致する証憑はありません。")
            }
            Show(when: move || !ocr.list.with(Vec::is_empty)) {
                SectionHeader(title: computed(move || format!("証憑（{} 件）", ocr.list.with(Vec::len))))
                View(style: group_style()) {
                    ForEach(
                        each: move || ocr.list.get(),
                        // 中身が変わったら行を作り直す（行は開いたときの値を持つため）。
                        key: |r: &OcrReceipt| format!("{}:{:?}", r.id(), r),
                        children: move |r: ReadSignal<OcrReceipt>| {
                            let r = r.get_untracked();
                            let label = format!(
                                "{}\n{}・{}",
                                r.receipt.title(),
                                model::type_label(&r.receipt.receipt_type),
                                model::status_label(&r.receipt.status),
                            );
                            let value = r.summary();
                            let id = r.id().to_owned();
                            let chosen = computed(move || ocr.selected.with(|s| s.as_ref().is_some_and(|s| s.id() == id)));
                            render! {
                                FieldRow(
                                    label: computed(move || label.clone()),
                                    value: computed(move || if chosen.get() { format!("{value} ●") } else { value.clone() }),
                                    on_tap: move |_| ocr.selected.set(Some(r.clone())),
                                )
                            }
                        },
                    )
                }
                Show(when: move || ocr.truncated.get()) {
                    Hint(text: "新しいものから 200 件を出しています。古いものは絞り込んで探してください。")
                }
            }
        }
    }
}

/// 1 件の詳細と、その証憑でできること。
#[component]
fn detail(ocr: Ocr, split: ReadSignal<bool>) -> Element {
    let r = computed(move || ocr.selected.get());
    let get = move |f: fn(&OcrReceipt) -> String| {
        computed(move || r.with(|r| r.as_ref().map(f).unwrap_or_default()))
    };
    let busy = signal(false);
    let note = signal(String::new());
    let note_error = signal(false);
    // 開いているフォーム：fix（読み取りを直す）/ ledger（台帳へ反映の確認）/ expense / bundle（仕入・経費まとめて）/ type / distinct。
    let form = signal(String::new());
    let is = move |name: &'static str| move || form.get() == name;
    let status = get(|r| r.receipt.status.clone());
    let kind = get(|r| r.receipt.receipt_type.clone());
    let booked = computed(move || r.with(|r| r.as_ref().is_some_and(|r| r.expense_id.is_some())));
    let say = move |text: String, error: bool| {
        note.set(text);
        note_error.set(error);
    };

    // 種別を変えて（または、そのまま）読み直す。
    let reprocess = move |to: Option<String>| {
        if busy.get_untracked() {
            return;
        }
        let (Some(tenant), Some(open)) = (ocr.tenant.get_untracked(), r.get_untracked()) else {
            return;
        };
        let id = open.id().to_owned();
        busy.set(true);
        say(String::new(), false);
        spawn_in(ocr.owner, async move {
            let change = to.clone();
            let result = authed(ocr.token, move |access| {
                api::reprocess_receipt(access, &tenant, &id, change.as_deref())
            })
            .await;
            busy.set(false);
            form.set(String::new());
            match result {
                Ok(value) => {
                    let fresh = ocr.replace(&value);
                    let text = match (&to, fresh.as_ref().map(|f| f.receipt.status.as_str())) {
                        (Some(t), _) => format!("{}に変更しました。", model::type_label(t)),
                        (None, Some("duplicate")) => {
                            "再解析の結果、重複候補が見つかりました。".to_owned()
                        }
                        (None, Some("error")) => model::failure_message(
                            fresh
                                .as_ref()
                                .and_then(|f| f.receipt.error_message.as_deref()),
                        ),
                        _ => "OCR再解析が完了しました。".to_owned(),
                    };
                    say(text, false);
                    ocr.load();
                }
                Err(e) => say(
                    if to.is_some() {
                        model::type_change_error(&e)
                    } else {
                        format!("再解析を開始できませんでした。{e}")
                    },
                    true,
                ),
            }
        });
    };

    let confirm_distinct = move || {
        if busy.get_untracked() {
            return;
        }
        let (Some(tenant), Some(open)) = (ocr.tenant.get_untracked(), r.get_untracked()) else {
            return;
        };
        let id = open.id().to_owned();
        busy.set(true);
        spawn_in(ocr.owner, async move {
            let result = authed(ocr.token, move |access| {
                api::confirm_distinct_receipt(access, &tenant, &id)
            })
            .await;
            busy.set(false);
            form.set(String::new());
            match result {
                Ok(value) => {
                    ocr.replace(&value);
                    say("別の取引として登録しました。".into(), false);
                    ocr.load();
                }
                Err(e) => say(e, true),
            }
        });
    };

    let to_ledger = move || {
        if busy.get_untracked() {
            return;
        }
        let (Some(tenant), Some(open)) = (ocr.tenant.get_untracked(), r.get_untracked()) else {
            return;
        };
        let id = open.id().to_owned();
        let sales = open.receipt.receipt_type == "sales";
        busy.set(true);
        spawn_in(ocr.owner, async move {
            let result = authed(ocr.token, move |access| {
                api::ledger_from_receipt(access, &tenant, &id, sales)
            })
            .await;
            busy.set(false);
            form.set(String::new());
            match result {
                Ok(count) => say(
                    format!(
                        "{}台帳へ {count} 件反映しました。",
                        if sales { "売上" } else { "仕入" }
                    ),
                    false,
                ),
                Err(e) => say(format!("台帳へ反映できませんでした。{e}"), true),
            }
        });
    };

    let ledger_label = computed(move || {
        if busy.get() && form.get() == "ledger" {
            "反映中…".to_owned()
        } else {
            format!(
                "{}台帳へ反映",
                if kind.get() == "sales" {
                    "売上"
                } else {
                    "仕入"
                }
            )
        }
    });
    let ledger_reason = computed(move || {
        r.with(|r| {
            r.as_ref()
                .and_then(OcrReceipt::ledger_rejection)
                .unwrap_or("")
                .to_owned()
        })
    });
    let suggested = get(|r| r.receipt.suggested_type.clone().unwrap_or_default());

    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Show(when: move || !split.get()) {
                View(style: group_style().margin_top(px(12))) {
                    FieldRow(label: computed(|| "‹ 証憑の一覧へ".to_owned()), value: computed(String::new), on_tap: move |_| ocr.selected.set(None))
                }
            }
            Show(when: move || r.with(|r| r.as_ref().is_some_and(|r| r.file_url.is_some()))) {
                View(style: css!(padding: (px(12), px(16), px(0), px(16)))) {
                    field_glass::DocumentPreview(
                        url: computed(move || r.with(|r| r.as_ref().and_then(|r| r.file_url.clone()).unwrap_or_default())),
                        style: computed(move || css!(width: percent(100), height: px(if split.get() { 520 } else { 360 }))),
                    )
                }
            }

            SectionHeader(title: computed(|| "OCR解析・台帳マッピング".to_owned()))
            View(style: group_style()) {
                Info(label: "証憑", value: get(|r| r.receipt.title()))
                Info(label: "状態", value: get(|r| model::status_label(&r.receipt.status).to_owned()))
                Info(label: "種別", value: get(|r| model::type_label(&r.receipt.receipt_type).to_owned()))
                Info(label: "取引日", value: get(|r| r.receipt.date.clone().unwrap_or_else(|| "—".into())))
                Info(label: "取引先", value: get(|r| r.receipt.counterparty.clone().unwrap_or_else(|| "—".into())))
                Info(label: "合計", value: get(|r| r.receipt.total.map(|t| format!("￥{}", crate::cash_book::format_yen(t.round() as i64))).unwrap_or_else(|| "—".into())))
                Info(label: "決済方法", value: get(|r| model::payment_label(r.receipt.payment_method.as_deref()).to_owned()))
                Show(when: move || booked.get()) {
                    Info(label: "経費", value: computed(|| "起票済み".to_owned()))
                }
            }
            Show(when: move || status.get() == "error") {
                Text(
                    value: get(|r| model::failure_message(r.receipt.error_message.as_deref())),
                    style: css!(color: hex(RED), font_size: px(14), padding: (px(8), px(32))),
                )
            }

            // 種別：未仕分けなら売上か仕入かを決めて読み直す。決まっていれば変えられる。
            Show(when: move || kind.get() == "unsorted" && status.get() != "pending" && status.get() != "processing") {
                Text(
                    value: computed(move || {
                        let mut text = "種別：未仕分け。売上か仕入かを決めると、その種別で読み直し、経費や台帳に反映できるようになります。".to_owned();
                        match suggested.get().as_str() {
                            "purchase" => text.push_str("読み取り結果からは仕入と推定しています。"),
                            "sales" => text.push_str("読み取り結果からは売上と推定しています。"),
                            _ => {}
                        }
                        text
                    }),
                    style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(12), px(32), px(0), px(32))),
                )
                View(style: group_style().margin_top(px(8))) {
                    FieldRow(label: computed(|| "仕入として読み直す".to_owned()), value: computed(String::new), on_tap: move |_| reprocess(Some("purchase".into())))
                    FieldRow(label: computed(|| "売上として読み直す".to_owned()), value: computed(String::new), on_tap: move |_| reprocess(Some("sales".into())))
                }
            }
            Show(when: move || kind.get() != "unsorted" && !booked.get() && status.get() != "pending" && status.get() != "processing") {
                View(style: group_style().margin_top(px(12))) {
                    FieldRow(
                        label: computed(move || if kind.get() == "sales" { "仕入に変更".to_owned() } else { "売上に変更".to_owned() }),
                        value: computed(move || if form.get() == "type" { "閉じる".to_owned() } else { String::new() }),
                        on_tap: move |_| form.update(|f| *f = if f == "type" { String::new() } else { "type".into() }),
                    )
                    Show(when: is("type")) {
                        Hint(text: "同じ画像を使って変更先の種別で再解析します。経費や台帳へ反映済みの場合は変更できません。")
                        FieldRow(
                            label: computed(move || if busy.get() { "再解析中…".to_owned() } else { "種別を変更して再解析".to_owned() }),
                            value: computed(String::new),
                            on_tap: move |_| reprocess(Some(if kind.get_untracked() == "sales" { "purchase".into() } else { "sales".into() })),
                        )
                    }
                }
            }

            // 重複候補。
            Show(when: move || status.get() == "duplicate") {
                Text(
                    value: "同じ内容のレシートがすでに取り込まれています。重複候補のため、経費・仕入台帳・材料単価への登録を止めています。",
                    style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(12), px(32), px(0), px(32))),
                )
                View(style: group_style().margin_top(px(8))) {
                    Show(when: move || r.with(|r| r.as_ref().is_some_and(|r| r.duplicate_of.is_some()))) {
                        FieldRow(
                            label: computed(|| "登録済みのレシートを確認".to_owned()),
                            value: computed(String::new),
                            on_tap: move |_| {
                                let Some(other) = r.get_untracked().and_then(|r| r.duplicate_of) else { return };
                                let Some(tenant) = ocr.tenant.get_untracked() else { return };
                                spawn_in(ocr.owner, async move {
                                    match authed(ocr.token, move |access| api::get_receipt(access, &tenant, &other)).await {
                                        Ok(value) => ocr.selected.set(OcrReceipt::from_value(&value)),
                                        Err(e) => say(format!("証憑を取得できませんでした。{e}"), true),
                                    }
                                });
                            },
                        )
                    }
                    FieldRow(
                        label: computed(|| "別の取引として登録する".to_owned()),
                        value: computed(move || if form.get() == "distinct" { "閉じる".to_owned() } else { String::new() }),
                        on_tap: move |_| form.update(|f| *f = if f == "distinct" { String::new() } else { "distinct".into() }),
                    )
                    Show(when: is("distinct")) {
                        Hint(text: "登録済みのレシートと照合し、別の購入であることを確認してください。同じ購入を登録すると二重計上につながります。")
                        FieldRow(label: computed(|| "別の取引であることを確認して登録".to_owned()), value: computed(String::new), on_tap: move |_| confirm_distinct())
                    }
                }
            }

            // 操作。
            Show(when: move || status.get() == "error") {
                View(style: group_style().margin_top(px(12))) {
                    FieldRow(
                        label: computed(move || if busy.get() { "再解析中…".to_owned() } else { "再解析".to_owned() }),
                        value: computed(String::new),
                        on_tap: move |_| reprocess(None),
                    )
                }
            }
            Show(when: move || status.get() == "done") {
                View(style: group_style().margin_top(px(12))) {
                    FieldRow(
                        label: computed(|| "読み取りを直す".to_owned()),
                        value: computed(move || if form.get() == "fix" { "閉じる".to_owned() } else { String::new() }),
                        on_tap: move |_| form.update(|f| *f = if f == "fix" { String::new() } else { "fix".into() }),
                    )
                    Show(when: is("fix")) {
                        FixForm(ocr: ocr, form: form, say: move |(t, e): (String, bool)| say(t, e))
                    }
                }
            }
            Show(when: move || status.get() == "done" && kind.get() != "unsorted") {
                View(style: group_style().margin_top(px(12))) {
                    Show(when: move || kind.get() == "purchase") {
                        FieldRow(
                            label: computed(|| "仕入・経費をまとめて登録".to_owned()),
                            value: computed(move || if form.get() == "bundle" { "閉じる".to_owned() } else { String::new() }),
                            on_tap: move |_| form.update(|f| *f = if f == "bundle" { String::new() } else { "bundle".into() }),
                        )
                        Show(when: is("bundle")) {
                            ExpenseForm(ocr: ocr, bundle: true, form: form, say: move |(t, e): (String, bool)| say(t, e))
                        }
                    }
                    FieldRow(
                        label: computed(move || if booked.get() { "経費に起票済み".to_owned() } else { "経費にする".to_owned() }),
                        value: computed(move || if form.get() == "expense" { "閉じる".to_owned() } else { String::new() }),
                        on_tap: move |_| if !booked.get_untracked() { form.update(|f| *f = if f == "expense" { String::new() } else { "expense".into() }) },
                    )
                    Show(when: is("expense")) {
                        ExpenseForm(ocr: ocr, bundle: false, form: form, say: move |(t, e): (String, bool)| say(t, e))
                    }
                    FieldRow(
                        label: ledger_label,
                        value: computed(move || if form.get() == "ledger" { "閉じる".to_owned() } else { String::new() }),
                        on_tap: move |_| if ledger_reason.with_untracked(String::is_empty) { form.update(|f| *f = if f == "ledger" { String::new() } else { "ledger".into() }) },
                    )
                    Show(when: move || !ledger_reason.with(String::is_empty)) {
                        Text(value: ledger_reason, style: css!(color: hex(RED), font_size: px(13), padding: (px(6), px(16))))
                    }
                    Show(when: is("ledger")) {
                        Text(
                            value: computed(move || r.with(|r| r.as_ref().map(|r| format!(
                                "OCR解析結果を台帳へ反映しますか？\n反映先：{}台帳\n取引先：{}\nOCR金額：{}",
                                if r.receipt.receipt_type == "sales" { "売上" } else { "仕入" },
                                r.receipt.counterparty.clone().unwrap_or_else(|| "—".into()),
                                r.summary(),
                            )).unwrap_or_default())),
                            style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(16))),
                        )
                        FieldRow(label: computed(|| "台帳へ反映する".to_owned()), value: computed(String::new), on_tap: move |_| to_ledger())
                    }
                }
            }
            Show(when: move || !note.with(String::is_empty) && !note_error.get()) {
                Text(value: computed(move || note.get()), style: css!(color: hex(TINT), font_size: px(14), padding: (px(12), px(32))))
            }
            Show(when: move || !note.with(String::is_empty) && note_error.get()) {
                Text(value: computed(move || note.get()), style: css!(color: hex(RED), font_size: px(14), padding: (px(12), px(32))))
            }
        }
    }
}

/// 見るだけの 1 行。
#[component]
fn info(label: &'static str, value: Signal<String>) -> Element {
    render! {
        View(
            style: css!(display: Display::Flex, flex_direction: FlexDirection::Row, align_items: AlignItems::Center, gap: px(12), padding: (px(10), px(16)), min_height: px(44))
                .border_bottom(hairline()),
        ) {
            Text(value: label, style: css!(color: hex(SECONDARY_LABEL), font_size: px(15), width: px(72), flex_shrink: 0.0))
            Text(value: value, max_lines: 3u32, style: css!(color: hex(LABEL), font_size: px(16), flex_shrink: 1.0))
        }
    }
}

/// 読み取りを直す（変えた所だけ送る）。経費に反映済みなら直せない。
#[component]
fn fix_form(ocr: Ocr, form: RwSignal<String>, say: Callback<(String, bool)>) -> Element {
    let Some(open) = ocr.selected.get_untracked() else {
        return render! { View() };
    };
    let booked = open.expense_id.is_some();
    let sales_side = open.receipt.receipt_type != "purchase";
    let yen = |v: Option<f64>| {
        v.map(|v| (v.round() as i64).to_string())
            .unwrap_or_default()
    };
    let totals = &open.receipt.payment_totals;
    let initial = [
        (
            "date",
            open.receipt
                .date
                .as_deref()
                .and_then(rules::normalize_date)
                .unwrap_or_default(),
        ),
        ("total", yen(open.receipt.total)),
        (
            "counterparty",
            open.receipt.counterparty.clone().unwrap_or_default(),
        ),
        (
            "customer",
            open.customer_count
                .map(|c| (c as i64).to_string())
                .unwrap_or_default(),
        ),
        ("cash", yen(totals["cash"].as_f64())),
        ("credit_card", yen(totals["credit_card"].as_f64())),
        ("qr_code", yen(totals["qr_code"].as_f64())),
        ("ic_card", yen(totals["ic_card"].as_f64())),
        ("other", yen(totals["other"].as_f64())),
    ];
    let fields: Vec<RwSignal<String>> = initial.iter().map(|(_, v)| signal(v.clone())).collect();
    let [date, total, payee, customer, cash, credit, qr, ic, other] =
        fields.clone().try_into().unwrap_or_else(|_| unreachable!());
    let start = signal(
        initial
            .iter()
            .map(|(_, v)| v.clone())
            .collect::<Vec<String>>(),
    );
    let fields = signal(fields);
    let busy = signal(false);
    let error = signal(String::new());
    let id = signal(open.id().to_owned());
    let save = move || {
        if busy.get_untracked() || booked {
            return;
        }
        let Some(tenant) = ocr.tenant.get_untracked() else {
            return;
        };
        let now: Vec<String> = fields.with_untracked(|f| {
            f.iter()
                .map(|f| f.get_untracked().trim().to_owned())
                .collect()
        });
        let start = start.get_untracked();
        let changed = |i: usize| now[i] != start[i].trim();
        let mut body = json!({});
        if changed(0) {
            match rules::normalize_date(&now[0]) {
                Some(d) => body["date"] = json!(d),
                None => return error.set("日付を入れてください。".into()),
            }
        }
        if changed(1) {
            match now[1].replace(',', "").parse::<f64>() {
                Ok(v) if v >= 0.0 => body["total"] = json!(now[1].replace(',', "")),
                _ => return error.set("合計は 0 以上の数で入れてください。".into()),
            }
        }
        if changed(2) {
            body["counterparty"] = json!(now[2]);
        }
        if sales_side {
            if changed(3) {
                match now[3].parse::<u64>() {
                    Ok(v) => body["customerCount"] = json!(v),
                    Err(_) if now[3].is_empty() => body["customerCount"] = Value::Null,
                    Err(_) => return error.set("客数は 0 以上の整数で入れてください。".into()),
                }
            }
            if (4..9).any(changed) {
                let mut map = serde_json::Map::new();
                for (i, key) in ["cash", "credit_card", "qr_code", "ic_card", "other"]
                    .iter()
                    .enumerate()
                {
                    let v = &now[4 + i];
                    if v.is_empty() {
                        continue;
                    }
                    match v.replace(',', "").parse::<i64>() {
                        Ok(n) if n >= 0 => {
                            map.insert((*key).to_owned(), json!(n));
                        }
                        _ => {
                            return error
                                .set("支払方法別の合計は 0 以上の整数で入れてください。".into());
                        }
                    }
                }
                body["paymentTotals"] = Value::Object(map);
            }
        }
        if body.as_object().is_some_and(serde_json::Map::is_empty) {
            form.set(String::new());
            return;
        }
        let id = id.get_untracked();
        busy.set(true);
        error.set(String::new());
        spawn_in(ocr.owner, async move {
            let result = authed(ocr.token, move |access| {
                api::correct_receipt(access, &tenant, &id, &body)
            })
            .await;
            busy.set(false);
            match result {
                Ok(value) => {
                    let fresh = ocr.replace(&value);
                    form.set(String::new());
                    say.run((
                        if fresh.is_some_and(|f| f.receipt.status == "duplicate") {
                            "読み取りを直しました。同じ内容のレシートがあるため、重複の確認に回しました。".into()
                        } else {
                            "読み取りを直しました。".into()
                        },
                        false,
                    ));
                }
                Err(e) => error.set(if e.contains("（409）") {
                    "このレシートは経費に反映済みです。先に経費のほうを直してください。".into()
                } else {
                    e
                }),
            }
        });
    };
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Text(
                value: if booked {
                    "このレシートは経費に反映済みです。先に経費のほうを直してください。"
                } else {
                    "元のレシートを直します。このレシートを使っているところには、直した値が入ります。"
                },
                style: css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(8), px(16))),
            )
            TextRow(label: "日付", text: date, editable: !booked, on_text: |_| {})
            TextRow(label: "合計", text: total, editable: !booked, on_text: |_| {})
            TextRow(label: "支払先", text: payee, editable: !booked, on_text: |_| {})
            Show(when: move || sales_side) {
                TextRow(label: "客数", text: customer, editable: !booked, on_text: |_| {})
                TextRow(label: "現金", text: cash, editable: !booked, on_text: |_| {})
                TextRow(label: "クレジット", text: credit, editable: !booked, on_text: |_| {})
                TextRow(label: "QR", text: qr, editable: !booked, on_text: |_| {})
                TextRow(label: "IC", text: ic, editable: !booked, on_text: |_| {})
                TextRow(label: "その他", text: other, editable: !booked, on_text: |_| {})
            }
            Show(when: move || !booked) {
                FieldRow(
                    label: computed(move || if busy.get() { "直しています…".to_owned() } else { "直す".to_owned() }),
                    value: computed(String::new),
                    on_tap: move |_| save(),
                )
            }
            Show(when: move || !error.with(String::is_empty)) {
                Text(value: computed(move || error.get()), style: css!(color: hex(RED), font_size: px(13), padding: (px(6), px(16))))
            }
        }
    }
}

/// 経費にする（`bundle` なら仕入台帳にも反映する）。
#[component]
fn expense_form(
    ocr: Ocr,
    bundle: bool,
    form: RwSignal<String>,
    say: Callback<(String, bool)>,
) -> Element {
    let Some(open) = ocr.selected.get_untracked() else {
        return render! { View() };
    };
    let already = open.expense_id.is_some();
    let categories = signal(Vec::<(String, String)>::new());
    let category = signal(String::new());
    let amount = signal(
        open.receipt
            .total
            .map(|t| (t.round() as i64).to_string())
            .unwrap_or_default(),
    );
    let date = signal(
        open.receipt
            .date
            .as_deref()
            .and_then(rules::normalize_date)
            .unwrap_or_default(),
    );
    let payee = signal(open.receipt.counterparty.clone().unwrap_or_default());
    let payer = signal(open.receipt.payer_kind.clone().unwrap_or_default());
    let memo = signal(String::new());
    let submit = signal(false);
    let busy = signal(false);
    let error = signal(String::new());
    if !already && let Some(tenant) = ocr.tenant.get_untracked() {
        spawn_in(ocr.owner, async move {
            match authed(ocr.token, move |access| {
                api::expense_categories(access, &tenant)
            })
            .await
            {
                Ok(list) => categories.set(list),
                Err(e) => error.set(format!("経費科目を読み込めませんでした。{e}")),
            }
        });
    }
    let id = signal(open.id().to_owned());
    let save = move || {
        if busy.get_untracked() {
            return;
        }
        let Some(tenant) = ocr.tenant.get_untracked() else {
            return;
        };
        let mut input = Value::Null;
        if !already {
            if category.get_untracked().is_empty() {
                return error.set("経費科目を選んでください。".into());
            }
            let Some(yen) = model::bookable_amount(&amount.get_untracked()) else {
                return error.set("金額は 1 円以上の整数で入れてください。".into());
            };
            if payer.get_untracked().is_empty() {
                return error.set("支払った人・手段を選んでください。".into());
            }
            let trimmed = |s: RwSignal<String>| {
                let t = s.get_untracked().trim().to_owned();
                (!t.is_empty()).then_some(t)
            };
            let mut body = json!({
                "amount": yen,
                "categoryId": category.get_untracked(),
                "payerKind": payer.get_untracked(),
                "submit": submit.get_untracked(),
            });
            if let Some(d) = trimmed(date) {
                match rules::normalize_date(&d) {
                    Some(d) => body["expenseDate"] = json!(d),
                    None => return error.set("取引日は 2026-11-06 の形で入れてください。".into()),
                }
            }
            if let Some(p) = trimmed(payee) {
                body["payee"] = json!(p);
            }
            if let Some(m) = trimmed(memo) {
                body["memo"] = json!(m);
            }
            input = body;
        }
        let id = id.get_untracked();
        let submitted = submit.get_untracked();
        busy.set(true);
        error.set(String::new());
        spawn_in(ocr.owner, async move {
            let result = authed(ocr.token, move |access| {
                // 起票済みなら経費は送らず、仕入台帳だけ確かめる。
                let expense = if input.is_null() {
                    None
                } else {
                    Some(api::book_receipt_expense(access, &tenant, &id, &input)?)
                };
                let ledger = if bundle {
                    Some(
                        api::ledger_from_receipt(access, &tenant, &id, false)
                            .map_err(|e| (expense.is_some(), e)),
                    )
                } else {
                    None
                };
                Ok((expense, ledger))
            })
            .await;
            busy.set(false);
            match result {
                Ok((expense, ledger)) => {
                    if let Some(value) = &expense {
                        ocr.replace(&value["receipt"]);
                    }
                    let text = match (&expense, ledger) {
                        (_, Some(Err((_, e)))) => {
                            error.set(format!("経費は登録済みですが、仕入台帳への反映を確認できませんでした。{e}"));
                            return;
                        }
                        (_, Some(Ok(_))) => "仕入台帳と経費への登録を確認しました。登録済みの記録は再利用しています。".to_owned(),
                        (Some(value), None) => {
                            let amount = value["expense"]["amount"].as_f64().map(|a| crate::cash_book::format_yen(a.round() as i64)).unwrap_or_default();
                            if value["created"].as_bool().unwrap_or(true) {
                                if submitted { format!("経費 ￥{amount} を申請しました。") } else { format!("経費 ￥{amount} を下書きしました。") }
                            } else {
                                "このレシートはすでに経費に起票済みです。".to_owned()
                            }
                        }
                        (None, None) => return,
                    };
                    form.set(String::new());
                    say.run((text, false));
                    ocr.load();
                }
                Err(e) => error.set(model::expense_error_message(&e)),
            }
        });
    };
    let button_label = computed(move || {
        if busy.get() {
            return "登録中…".to_owned();
        }
        match (bundle, already, submit.get()) {
            (true, true, _) => "仕入台帳への反映を確認・再実行".to_owned(),
            (true, false, false) => "仕入に反映して経費を下書き".to_owned(),
            (true, false, true) => "仕入に反映して経費を申請".to_owned(),
            (false, _, false) => "経費を下書き".to_owned(),
            (false, _, true) => "経費を申請".to_owned(),
        }
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Show(when: move || !already) {
                Choice(
                    label: computed(|| "経費科目".to_owned()),
                    options: computed(move || categories.get()),
                    selected: computed(move || category.get()),
                    on_pick: move |k: String| category.set(k),
                )
                TextRow(label: "金額", text: amount, editable: true, on_text: |_| {})
                TextRow(label: "取引日", text: date, editable: true, on_text: |_| {})
                TextRow(label: "支払先", text: payee, editable: true, on_text: |_| {})
                Choice(
                    label: computed(|| "支払った人・手段".to_owned()),
                    options: computed(|| options(&model::PAYERS[1..])),
                    selected: computed(move || payer.get()),
                    on_pick: move |k: String| payer.set(k),
                )
                Show(when: move || payer.get() == "employee_advance") {
                    Hint(text: "承認後、あなたへの立替精算（未払金）として計上されます。")
                }
                TextRow(label: "メモ", text: memo, editable: true, on_text: |_| {})
                FieldRow(
                    label: computed(|| "内容を確認したので、このまま承認へ申請する".to_owned()),
                    value: computed(move || if submit.get() { "✓".to_owned() } else { "　".to_owned() }),
                    on_tap: move |_| submit.update(|s| *s = !*s),
                )
            }
            FieldRow(label: button_label, value: computed(String::new), on_tap: move |_| save())
            Show(when: move || !error.with(String::is_empty)) {
                Text(value: computed(move || error.get()), style: css!(color: hex(RED), font_size: px(13), padding: (px(6), px(16))))
            }
        }
    }
}
