//! TACHYON Field — Whisker で作った試作クライアント。
//!
//! ログイン → テナント選択 → ホーム。
//! 左端から右へ引く（か「☰」）と、表の画面がずれて裏のメニューが出る。
//! 裏のメニューで会社と分野（販売管理・顧客管理…）を選ぶ。下のタブは、その分野の機能と、
//! 右端の Field Agent（AI に頼む画面）。
//! iOS ではタブバー・一覧・メニューに iOS 標準の部品（Liquid Glass）を使う（field-glass）。

mod agent;
mod api;
mod features;
mod storage;

use agent::AgentScreen;
use api::Tenant;
use features::{Feature, MENU};
use field_glass::{AppearanceObserver, Backdrop, Glass, KeyboardObserver};
#[cfg(target_os = "ios")]
use field_glass::{
    MenuList as SystemMenuList, RecordList as SystemRecordList, Sheet as SystemSheet,
    TabBar as SystemTabBar,
};
use serde_json::Value;
use whisker::css::{
    AlignItems, Border, BorderStyle, Color, Display, EasingFunction, FlexDirection, FontWeight,
    JustifyContent, Overflow, PositionKind, TextAlign, TextOverflow, TransformFn, Transition,
    TransitionPropertyKind,
};
use whisker::event::TouchEvent;
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_input::{AutoCapitalize, Input, InputRef, KeyboardType, ReturnKey};
use whisker_safe_area::safe_area_insets;

// iOS のシステムカラー。ここには明るい表示（ライト）の色を書き、
// 暗い表示（ダーク）の色は `dark_of` で対応させる。色は必ず `hex()` を通して使う。
const SYSTEM_BACKGROUND: u32 = 0xFFFFFF; // 画面の背景（白）
const LABEL: u32 = 0x000000;
const SECONDARY_LABEL: u32 = 0x8A8A8E;
const SEPARATOR: u32 = 0xC6C6C8;
const CHEVRON: u32 = 0xC4C4C7;
const TINT: u32 = 0x007AFF; // iOS の青
const FILL: u32 = 0xE3E3E8; // 検索欄
const AVATAR: u32 = 0xA2A6B0;
const RED: u32 = 0xFF3B30;
const GREEN: u32 = 0x248A3D;
const ORANGE: u32 = 0xC93400;
const LOGO: u32 = 0x0A2540; // サインイン画面の「TF」
const DISABLED: u32 = 0xE5E5EA; // 押せないボタンの丸

/// 今、暗い表示（ダークモード）か。iPhone の設定が変わると `app` が書き換える。
static DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn is_dark() -> bool {
    DARK.load(std::sync::atomic::Ordering::Relaxed)
}

/// 明るい表示の色を渡すと、今の表示（明るい・暗い）に合う色を返す。
/// 対応のない色（機能ごとのアイコンの色など）は、どちらでもそのまま使う。
fn hex(light: u32) -> Color {
    Color::hex(if is_dark() { dark_of(light) } else { light })
}

/// iOS のダークモードの色。
fn dark_of(light: u32) -> u32 {
    match light {
        SYSTEM_BACKGROUND => 0x000000,
        LABEL => 0xFFFFFF,
        SECONDARY_LABEL => 0x8D8D93,
        SEPARATOR => 0x38383A,
        CHEVRON => 0x5A5A5E,
        TINT => 0x0A84FF,
        FILL => 0x1C1C1E,
        AVATAR => 0x636366,
        RED => 0xFF453A,
        GREEN => 0x30D158,
        ORANGE => 0xFF9F0A,
        LOGO => 0xFFFFFF,
        DISABLED => 0x3A3A3C,
        other => other,
    }
}

/// 裏のメニューの地の色。明るい表示は表と同じ白（表の画面は影付きの角丸の板で見分ける）、
/// 暗い表示は表の黒と見分けられる濃い灰色。
fn drawer_color() -> Color {
    Color::hex(if is_dark() { 0x1C1C1E } else { 0xFFFFFF })
}

/// 白い板（設定画面のような、角の丸いまとまり）の色。暗い表示では黒地に浮くよう濃い灰色。
fn cell_color() -> Color {
    hex(if is_dark() { 0x1C1C1E } else { 0xFFFFFF })
}

/// 入力欄の「例」の文字や、補足の文字の色（少し透けた灰色）。
fn placeholder_color() -> Color {
    if is_dark() {
        Color::rgba(235, 235, 245, 0.6)
    } else {
        Color::rgba(60, 60, 67, 0.6)
    }
}

/// タブの下に隠れないよう、一覧の最後にあける高さ。
const TAB_BAR_SPACE: f32 = 110.0;

#[whisker::main]
fn app() -> Element {
    // 明るい・暗いが分かってから画面を作る（起動直後に白く光らないように）。
    // iPhone の設定が切り替わったら、新しい色で画面を作り直す。
    let dark = signal(None::<bool>);
    render! {
        View(
            style: css!(flex_grow: 1.0, display: Display::Flex, flex_direction: FlexDirection::Column)
                .flex_basis(px(0))
                .min_height(px(0)),
        ) {
            AppearanceObserver(on_change: move |value: bool| {
                if dark.get_untracked() != Some(value) {
                    DARK.store(value, std::sync::atomic::Ordering::Relaxed);
                    dark.set(Some(value));
                }
            })
            ForEach(
                each: move || dark.get().into_iter().collect::<Vec<_>>(),
                key: |d: &bool| *d,
                children: move |_: ReadSignal<bool>| render! { Root },
            )
        }
    }
}

/// ログイン状態と選んだテナントを持つ。画面はこの 2 つで決まる。
#[component]
fn root() -> Element {
    let token = signal(None::<String>);
    let tenant = signal(None::<Tenant>);
    let insets = safe_area_insets();

    // 前回のサインインが残っていれば、パスワードなしで入り直す。
    // 通信の失敗で入り直せなかったときは、保存したものを消さずに「もう一度」を出す。
    // 消すのは、Cognito にトークンを断られたとき（期限切れ・取り消し済み）だけ。
    let restoring = signal(storage::refresh_token().is_some());
    let restore_error = signal(String::new());
    let restore = move || {
        let Some(refresh_token) = storage::refresh_token() else {
            restoring.set(false);
            return;
        };
        restoring.set(true);
        restore_error.set(String::new());
        spawn_local(async move {
            let result = run_blocking(move || api::refresh(&refresh_token)).await;
            match result {
                Ok(session) => {
                    if let Some(rotated) = &session.refresh_token {
                        storage::save_refresh_token(rotated);
                    }
                    tenant.set(storage::tenant());
                    token.set(Some(session.access_token));
                }
                Err(error) if error == api::EXPIRED => storage::forget_all(),
                Err(error) => restore_error.set(error),
            }
            restoring.set(false);
        });
    };
    restore();

    render! {
        View(
            style: computed(move || {
                css!(
                    flex_grow: 1.0,
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    background_color: hex(SYSTEM_BACKGROUND),
                    padding_top: px(insets.get().top as f32),
                )
                .flex_basis(px(0))
                .min_height(px(0))
            }),
        ) {
            Show(when: move || restoring.get()) {
                Notice(message: "読み込み中…")
            }
            Show(when: move || !restoring.get() && !restore_error.with(String::is_empty)) {
                RestoreFailed(
                    message: computed(move || restore_error.get()),
                    on_retry: move |_| restore(),
                    on_sign_in: move |_| {
                        storage::forget_all();
                        restore_error.set(String::new());
                    },
                )
            }
            // 復元が終わるまでは、どの画面も作らない（会社が決まる前に一覧を読み始めないため）。
            Show(
                when: move || !restoring.get() && restore_error.with(String::is_empty) && token.with(Option::is_none),
            ) {
                Login(token: token)
            }
            Show(
                when: move || !restoring.get() && token.with(Option::is_some) && tenant.with(Option::is_none),
            ) {
                TenantPicker(token: token, tenant: tenant)
            }
            // 会社を切り替えたら、ホームごと作り直す（前の会社の一覧や会話を残さない）。
            ForEach(
                each: move || {
                    if restoring.get() || token.with(Option::is_none) {
                        Vec::new()
                    } else {
                        tenant.with(|t| t.iter().map(|t| t.id.clone()).collect::<Vec<_>>())
                    }
                },
                key: |id: &String| id.clone(),
                children: move |_: ReadSignal<String>| render! { Home(token: token, tenant: tenant) },
            )
        }
    }
}

/// 起動時に通信できず入り直せなかったとき：理由と「もう一度」「サインインし直す」。
#[component]
fn restore_failed(
    message: Signal<String>,
    on_retry: Callback<()>,
    on_sign_in: Callback<()>,
) -> Element {
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                gap: px(16),
                padding: (px(64), px(32)),
            ),
        ) {
            Text(
                value: "サインインを確かめられませんでした",
                style: css!(color: hex(LABEL), font_size: px(20), font_weight: FontWeight::Bold),
            )
            Text(
                value: message,
                style: css!(
                    color: hex(SECONDARY_LABEL),
                    font_size: px(15),
                    text_align: TextAlign::Center,
                ),
            )
            View(
                style: css!(
                    padding: (px(12), px(28)),
                    border_radius: px(22),
                    background_color: hex(TINT),
                ),
                on_tap: move |_| on_retry.run(()),
            ) {
                Text(
                    value: "もう一度",
                    style: css!(
                        color: Color::hex(0xFFFFFF),
                        font_size: px(17),
                        font_weight: FontWeight::Bold,
                    ),
                )
            }
            Text(
                value: "サインインし直す",
                style: css!(color: hex(TINT), font_size: px(15)),
                on_tap: move |_| on_sign_in.run(()),
            )
        }
    }
}

// ---------------------------------------------------------------- ログイン

#[component]
fn login(token: RwSignal<Option<String>>) -> Element {
    let email = signal(String::new());
    let password = signal(String::new());
    let busy = signal(false);
    let error = signal(String::new());

    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        let (e, p) = (email.get_untracked(), password.get_untracked());
        if e.trim().is_empty() || p.is_empty() {
            error.set("メールアドレスとパスワードを入れてください".into());
            return;
        }
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = run_blocking(move || api::sign_in(e.trim(), &p)).await;
            busy.set(false);
            match result {
                Ok(session) => {
                    password.set(String::new());
                    if let Some(refresh_token) = &session.refresh_token {
                        storage::save_refresh_token(refresh_token);
                    }
                    token.set(Some(session.access_token));
                }
                Err(message) => error.set(message),
            }
        });
    };

    let insets = safe_area_insets();
    let fill = |top: f32| {
        css!(
            position: PositionKind::Absolute,
            top: px(top),
            bottom: px(0),
            left: px(0),
            right: px(0),
        )
    };

    render! {
        // 背景の絵の上に、ガラスの板で題名・入力欄を重ねる（Apple のサインイン画面の作り）。
        View(
            style: css!(
                flex_grow: 1.0,
                position: PositionKind::Relative,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                gap: px(32),
                padding: (px(0), px(20)),
                padding_bottom: px(48),
            ),
        ) {
            // 画面の上端（時計の裏）まで絵を広げる。
            Backdrop(style: computed(move || fill(-(insets.get().top as f32))))
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    gap: px(6),
                ),
            ) {
                View(
                    style: css!(
                        width: px(88),
                        height: px(88),
                        position: PositionKind::Relative,
                        display: Display::Flex,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        margin_bottom: px(14),
                    ),
                ) {
                    Glass(corner_radius: 22.0_f32, style: fill(0.0))
                    Text(
                        value: "TF",
                        style: css!(
                            color: hex(LOGO),
                            font_size: px(34),
                            font_weight: FontWeight::Bold,
                        ),
                    )
                }
                Text(
                    value: "TACHYON Field",
                    style: css!(
                        color: hex(LABEL),
                        font_size: px(34),
                        font_weight: FontWeight::Bold,
                    ),
                )
                Text(
                    value: "アカウントでサインイン",
                    style: css!(color: placeholder_color(), font_size: px(17)),
                )
            }
            // ガラスの板の上に入力欄 2 行、間に区切り線。
            View(
                style: css!(
                    position: PositionKind::Relative,
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                ),
            ) {
                Glass(corner_radius: 20.0_f32, style: fill(0.0))
                View(style: form_row_style().border_bottom(glass_hairline())) {
                    Input(
                        text: email,
                        placeholder: "メールアドレス",
                        keyboard_type: KeyboardType::Email,
                        auto_capitalize: AutoCapitalize::None,
                        autocorrect: false,
                        spell_check: false,
                        return_key: ReturnKey::Next,
                        style: glass_input_style(),
                    )
                }
                View(style: form_row_style()) {
                    Input(
                        text: password,
                        placeholder: "パスワード",
                        secure: true,
                        return_key: ReturnKey::Go,
                        on_submit: move |_| submit(),
                        style: glass_input_style(),
                    )
                }
            }
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    gap: px(12),
                ),
            ) {
                Show(when: move || !error.with(String::is_empty)) {
                    Text(
                        value: computed(move || error.get()),
                        style: css!(
                            color: hex(RED),
                            font_size: px(13),
                            text_align: TextAlign::Center,
                        ),
                    )
                }
                // 角の丸い大きなボタン（iOS の「続ける」ボタンの形）。
                View(
                    style: computed(move || {
                        css!(
                            display: Display::Flex,
                            flex_direction: FlexDirection::Row,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            height: px(52),
                            border_radius: px(26),
                            background_color: hex(TINT),
                            opacity: if busy.get() { 0.5 } else { 1.0 },
                        )
                    }),
                    on_tap: move |_| submit(),
                ) {
                    Text(
                        value: computed(move || {
                            if busy.get() {
                                "サインイン中…"
                            } else {
                                "サインイン"
                            }
                            .to_owned()
                        }),
                        style: css!(
                            color: Color::hex(0xFFFFFF),
                            font_size: px(17),
                            font_weight: FontWeight::Numeric(600),
                        ),
                    )
                }
            }
        }
    }
}

// ---------------------------------------------------------------- テナント選択

#[component]
fn tenant_picker(token: RwSignal<Option<String>>, tenant: RwSignal<Option<Tenant>>) -> Element {
    let tenants = signal(Vec::<Tenant>::new());
    let loading = signal(true);
    let error = signal(String::new());
    let last_id = computed(move || tenants.with(|v| v.last().map(|t| t.id.clone())));

    spawn_local(async move {
        let result = authed(token, |access| api::list_tenants(access)).await;
        loading.set(false);
        match result {
            Ok(list) => tenants.set(list),
            Err(message) => error.set(message),
        }
    });

    render! {
        View(style: screen_style()) {
            NavBar(
                title: "",
                back_label: "",
                on_back: |_| {},
                action_label: "サインアウト",
                on_action: move |_| sign_out(token, tenant),
            )
            ScrollView(style: scroll_style()) {
                View(style: scroll_content_style()) {
                    LargeTitle(title: "テナント")
                    Show(when: move || loading.get()) {
                        Notice(message: "読み込み中…")
                    }
                    Show(when: move || !error.with(String::is_empty)) {
                        ErrorNotice(message: computed(move || error.get()))
                    }
                    Show(
                        when: move || !loading.get() && error.with(String::is_empty) && tenants.with(Vec::is_empty),
                    ) {
                        Notice(message: "使えるテナントがありません")
                    }
                    Show(when: move || !tenants.with(Vec::is_empty)) {
                        SectionHeader(
                            title: computed(move || format!("{} 件", tenants.with(Vec::len))),
                        )
                        View(style: group_style()) {
                            ForEach(
                                each: move || tenants.get(),
                                key: |t: &Tenant| t.id.clone(),
                                children: move |t: ReadSignal<Tenant>| {
                                    let t = t.get_untracked();
                                    let picked = t.clone();
                                    let id = t.id.clone();
                                    let production = t.environment == "production";
                                    render! {
                                        ListRow(
                                            title: t.name.clone(),
                                            subtitle: if production {
                                                "本番"
                                            } else {
                                                "サンドボックス"
                                            },
                                            subtitle_color: if production { GREEN } else { ORANGE },
                                            trailing: "",
                                            avatar: true,
                                            separator: computed(move || last_id.get().as_deref() != Some(id.as_str())),
                                            on_tap: move |_| {
                                                storage::save_tenant(&picked);
                                                tenant.set(Some(picked.clone()));
                                            },
                                        )
                                    }
                                },
                            )
                        }
                        View(style: css!(height: px(32)))
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- ホーム（下のタブ）

/// 裏のメニューが開いているか。どの画面の「☰」からでも開けるよう、ホームが配る。
#[derive(Clone, Copy)]
struct Drawer(RwSignal<bool>);

/// 詳細画面を開いている間、左から右へ引くと一覧へ戻る。
/// `target` は戻る先（詳細を閉じるために空にする選択）、`drag` は今引いている幅。
#[derive(Clone, Copy)]
struct BackSwipe {
    target: RwSignal<Option<RwSignal<Option<Value>>>>,
    drag: RwSignal<Option<f32>>,
}

/// これより右へ引いて離したら戻る。足りなければ元の位置に戻す。
const BACK_DISTANCE: f32 = 90.0;
/// 詳細を出している間、後ろの一覧を左へ寄せておく幅。
const BEHIND_SHIFT: f32 = 90.0;
/// 詳細をこれだけ右へ引くと、後ろの一覧が元の位置まで戻りきる。
const BEHIND_TRAVEL: f32 = 320.0;

/// 裏のメニューの幅。表の画面はこの分だけ右にずれる。
const DRAWER_WIDTH: f32 = 320.0;
/// 指がこれだけ動くまでは、横に引いたのか縦にスクロールしたのか決めない。
const SLOP: f32 = 10.0;

/// 下のタブで今開いているもの。
#[derive(Clone, Copy, PartialEq)]
enum Page {
    /// 今の分野の n 番目の機能。
    Feature(usize),
    /// Field Agent（どの分野でもタブの右端）。
    Agent,
}

/// 分野（裏のメニューで選ぶ）ごとの機能。下のタブはこれに Agent を足したもの。
fn area_features(area: usize) -> &'static [Feature] {
    MENU.get(area).map_or(&[], |(_, items)| items)
}

/// テナントの環境を日本語で。
fn environment_label(environment: &str) -> &'static str {
    if environment == "production" {
        "本番"
    } else {
        "サンドボックス"
    }
}

#[component]
fn home(token: RwSignal<Option<String>>, tenant: RwSignal<Option<Tenant>>) -> Element {
    // 裏のメニューで選んだ分野と、下のタブで開いているもの。
    let area = signal(0usize);
    let page = signal(Page::Feature(0));
    // Agent との会話。タブを切り替えても消えないよう、ホームが持つ。
    let chat = agent::AgentChat::new();
    let open = signal(false);
    // 指で動かしている最中の位置。離したら None に戻し、開く・閉じるに寄せる。
    let drag = signal(None::<f32>);
    // 指を置いた位置（x, y）と、そのときの表の画面の位置。
    let start = signal(None::<(f32, f32, f32)>);
    provide_context(Drawer(open));
    let back = BackSwipe {
        target: signal(None),
        drag: signal(None),
    };
    provide_context(back);
    let insets = safe_area_insets();

    let offset = computed(move || {
        drag.get()
            .unwrap_or(if open.get() { DRAWER_WIDTH } else { 0.0 })
    });
    let finish = move || {
        if let Some(x) = drag.get_untracked() {
            open.set(x > DRAWER_WIDTH / 2.0);
        }
        if let Some(dx) = back.drag.get_untracked() {
            if dx > BACK_DISTANCE {
                if let Some(selected) = back.target.get_untracked() {
                    selected.set(None);
                }
            }
        }
        drag.set(None);
        back.drag.set(None);
        start.set(None);
    };

    render! {
        View(
            style: css!(
                flex_grow: 1.0,
                position: PositionKind::Relative,
            )
            .flex_basis(px(0))
            .min_height(px(0)),
        ) {
            SideMenu(token: token, tenant: tenant, area: area, page: page, open: open)
            View(
                style: computed(move || {
                    let x = offset.get();
                    let progress = (x / DRAWER_WIDTH).clamp(0.0, 1.0);
                    let shadow = progress * 0.22;
                    // 親は上の余白（時計などの帯）の下から始まるので、その分だけ上へ伸ばして帯まで塗る。
                    let safe_top = insets.get().top as f32;
                    let s = css!(
                        position: PositionKind::Absolute,
                        top: px(-safe_top),
                        padding_top: px(safe_top),
                        bottom: px(0),
                        left: px(0),
                        right: px(0),
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        background_color: hex(SYSTEM_BACKGROUND),
                        // 開くにつれて角を丸め、表の画面を 1 枚の板に見せる（画面の角に近い丸み）。
                        border_radius: px(44.0 * progress),
                        overflow: Overflow::Hidden,
                    )
                    .transform(TransformFn::TranslateX(px(x).into()))
                    .box_shadow(px(0), px(0), px(30), px(0), Color::rgba(0, 0, 0, shadow))
                    .z_index(5);
                    // 暗い表示では影が見えないので、縁に細い線を引いて板の形を見せる。
                    let s = if is_dark() && progress > 0.0 {
                        s.border(
                            Border::new()
                                .width(px(0.5))
                                .color(Color::rgba(255, 255, 255, 0.18 * progress))
                                .style(BorderStyle::Solid),
                        )
                    } else {
                        s
                    };
                    if drag.get().is_some() {
                        s
                    } else {
                        // 指を離したあとは、開く・閉じる位置まで滑らかに動かす。
                        s.transition(
                            Transition::new(TransitionPropertyKind::name("transform"))
                                .duration(ms(260))
                                .timing(EasingFunction::EaseOut),
                        )
                    }
                }),
                // 画面のどこからでも、左から右へ引けばメニューが出る（開いているときは右から左で閉じる）。
                on_capture_touchstart: move |e: TouchEvent| {
                    let (x, y) = (e.detail.x as f32, e.detail.y as f32);
                    start.set(Some((x, y, offset.get_untracked())));
                },
                on_capture_touchmove: move |e: TouchEvent| {
                    let Some((x0, y0, base)) = start.get_untracked() else {
                        return;
                    };
                    let dx = e.detail.x as f32 - x0;
                    if back.drag.get_untracked().is_some() {
                        back.drag.set(Some(dx.max(0.0)));
                        return;
                    }
                    if drag.get_untracked().is_none() {
                        let dy = e.detail.y as f32 - y0;
                        // 少し動いただけ（タップ）は、まだ決めない。
                        if dx.abs() < SLOP && dy.abs() < SLOP {
                            return;
                        }
                        // 縦の動きが勝つならスクロールに任せる。閉じているときに右から左へ引いたのも無視する。
                        if dy.abs() > dx.abs() || (!open.get_untracked() && dx < 0.0) {
                            start.set(None);
                            return;
                        }
                        // 詳細画面では、メニューではなく一覧へ戻る。
                        if !open.get_untracked() && back.target.get_untracked().is_some() {
                            back.drag.set(Some(dx));
                            return;
                        }
                    }
                    drag.set(Some((base + dx).clamp(0.0, DRAWER_WIDTH)));
                },
                on_capture_touchend: move |_| finish(),
                on_capture_touchcancel: move |_| finish(),
            ) {
                // 分野か機能が変わるたびに作り直すよう、(分野, 機能) を鍵にする。
                ForEach(
                    each: move || match page.get() {
                        Page::Feature(i) => vec![(area.get(), i)],
                        Page::Agent => Vec::new(),
                    },
                    key: |k: &(usize, usize)| *k,
                    children: move |k: ReadSignal<(usize, usize)>| {
                        let (a, i) = k.get_untracked();
                        let feature = area_features(a)[i];
                        render! {
                            FeatureList(
                                token: token,
                                tenant: tenant,
                                feature: feature,
                                back_label: "",
                                on_back: |_| {},
                            )
                        }
                    },
                )
                Show(when: move || page.get() == Page::Agent) {
                    AgentScreen(token: token, tenant: tenant, chat: chat)
                }
                TabBar(area: area, page: page)
                // 開いている間は表の画面を触れないようにし、タップで閉じる。
                Show(when: move || open.get()) {
                    View(
                        style: css!(
                            position: PositionKind::Absolute,
                            top: px(0),
                            bottom: px(0),
                            left: px(0),
                            right: px(0),
                        )
                        .z_index(50),
                        on_tap_catch: move |_| open.set(false),
                    )
                }
            }
        }
    }
}

/// 表の画面の裏にあるメニュー。画面を左から右へ引くか、「☰」で見える。
/// 会社（テナント）の切り替えと、分野（下のタブの中身）の切り替えをする。
#[component]
fn side_menu(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    area: RwSignal<usize>,
    page: RwSignal<Page>,
    open: RwSignal<bool>,
) -> Element {
    let insets = safe_area_insets();
    render! {
        View(
            style: computed(move || {
                let safe_top = insets.get().top as f32;
                css!(
                    position: PositionKind::Absolute,
                    top: px(-safe_top),
                    padding_top: px(safe_top),
                    bottom: px(0),
                    left: px(0),
                    width: px(DRAWER_WIDTH),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    background_color: drawer_color(),
                )
                .z_index(1)
            }),
        ) {
            DrawerItems(token: token, tenant: tenant, area: area, page: page, open: open)
        }
    }
}

/// 分野を選んだとき：下のタブをその分野の機能に差し替えて、最初の機能を開く。
fn choose_area(index: usize, area: RwSignal<usize>, page: RwSignal<Page>, open: RwSignal<bool>) {
    area.set(index);
    page.set(Page::Feature(0));
    open.set(false);
}

/// 機能ごとの SF Symbols（iOS 標準の一覧・タブのアイコン）。
#[cfg(target_os = "ios")]
fn feature_symbol(label: &str) -> &'static str {
    match label {
        "案件" => "briefcase.fill",
        "見積" => "doc.plaintext.fill",
        "受注" => "cart.fill",
        "販売契約" => "signature",
        "取引先" => "building.2.fill",
        "個人顧客" => "person.fill",
        "お問い合わせ" => "envelope.fill",
        "予約" => "calendar",
        "スタッフ" => "person.2.fill",
        "商品" => "shippingbox.fill",
        "発注書" => "doc.text.fill",
        "請求書" => "yensign",
        "経費" => "creditcard.fill",
        _ => "square.fill",
    }
}

/// 分野ごとの SF Symbols（裏のメニューのアイコン）。
#[cfg(target_os = "ios")]
fn area_symbol(name: &str) -> &'static str {
    match name {
        "販売管理" => "chart.line.uptrend.xyaxis",
        "顧客管理" => "person.2",
        "予約・人事" => "calendar",
        "商品・仕入" => "shippingbox",
        "請求・経費" => "yensign.circle",
        _ => "square.grid.2x2",
    }
}


/// 設定のシートの行の鍵（機能名と重ならない文字）。
#[cfg(target_os = "ios")]
const SIGN_OUT_KEY: &str = "__sign_out";
#[cfg(target_os = "ios")]
const TENANT_KEY: &str = "__tenant";

/// 設定のシートで今見せている中身。
#[cfg(target_os = "ios")]
#[derive(Clone, Copy, PartialEq)]
enum SheetPage {
    /// 今の会社とサインアウト。
    Settings,
    /// 会社の一覧（選ぶと切り替わる）。
    Tenants,
}

/// 裏のメニューの中身。上に会社名、真ん中に分野の一覧（iOS 標準の UITableView）、
/// 下に設定（⚙）。設定を押すと下からシートが出て、会社の切り替えとサインアウトができる。
/// 会社の切り替えを選ぶと、同じシートの中身が会社の一覧に変わり、選ぶとその場で切り替わる。
#[cfg(target_os = "ios")]
#[component]
fn drawer_items(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    area: RwSignal<usize>,
    page: RwSignal<Page>,
    open: RwSignal<bool>,
) -> Element {
    use serde_json::json;

    let choosing = signal(false);
    let tenants = signal(Vec::<Tenant>::new());
    let tenants_error = signal(String::new());
    let insets = safe_area_insets();
    let tenant_name =
        computed(move || tenant.with(|t| t.as_ref().map(|t| t.name.clone()).unwrap_or_default()));
    let environment = computed(move || {
        tenant.with(|t| environment_label(t.as_ref().map_or("", |t| t.environment.as_str())).to_owned())
    });

    let sections = computed(move || {
        let selected_area = area.get();
        let areas: Vec<serde_json::Value> = MENU
            .iter()
            .enumerate()
            .map(|(i, (name, _))| {
                json!({
                    "key": format!("area:{i}"),
                    "title": name,
                    "symbol": area_symbol(name),
                    "accessory": if i == selected_area { "check" } else { "none" },
                })
            })
            .collect();
        json!([{ "title": "分野", "rows": areas }])
        .to_string()
    });

    // 設定のシートの中身：今の会社（押すと会社の一覧に変わる）と、サインアウト。
    let sheet_page = signal(SheetPage::Settings);
    let settings_sections = computed(move || {
        let current = tenant.get();
        json!([
            { "title": "会社", "rows": [{
                "key": TENANT_KEY,
                "title": current.as_ref().map(|t| t.name.clone()).unwrap_or_default(),
                "subtitle": current.as_ref().map_or("", |t| environment_label(&t.environment)),
                "symbol": "building.columns.fill",
                "color": "#8E8E93",
            }] },
            { "title": "", "rows": [{ "key": SIGN_OUT_KEY, "title": "サインアウト", "destructive": true }] },
        ])
        .to_string()
    });

    // 会社の一覧のシートの中身：使える会社。今の会社に ✓。
    let tenant_sections = computed(move || {
        let current_id = tenant.with(|t| t.as_ref().map(|t| t.id.clone()).unwrap_or_default());
        let list = tenants.get();
        let mut rows: Vec<serde_json::Value> = list
            .into_iter()
            .map(|t| {
                json!({
                    "key": format!("tenant:{}", t.id),
                    "title": t.name,
                    "subtitle": environment_label(&t.environment),
                    "symbol": "building.columns.fill",
                    "color": if t.id == current_id { "#007AFF" } else { "#8E8E93" },
                    "accessory": if t.id == current_id { "check" } else { "none" },
                })
            })
            .collect();
        if rows.is_empty() {
            let error = tenants_error.get();
            rows.push(json!({
                "key": "__wait",
                "title": if error.is_empty() { "読み込み中…".to_owned() } else { error },
                "accessory": "none",
            }));
        }
        json!([{ "title": "", "rows": rows }]).to_string()
    });

    let load_tenants = move || {
        tenants_error.set(String::new());
        spawn_local(async move {
            match authed(token, api::list_tenants).await {
                Ok(list) if list.is_empty() => tenants_error.set("使える会社がありません".into()),
                Ok(list) => tenants.set(list),
                Err(message) => tenants_error.set(message),
            }
        });
    };

    render! {
        View(
            style: css!(display: Display::Flex, flex_direction: FlexDirection::Column, flex_grow: 1.0)
                .flex_basis(px(0))
                .min_height(px(0)),
        ) {
            // 今の会社名（大きく）。切り替えは下の設定から。
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: (px(20), px(24), px(8), px(24)),
                ),
            ) {
                Text(
                    value: tenant_name,
                    max_lines: 1u32,
                    style: css!(
                        color: hex(LABEL),
                        font_size: px(28),
                        font_weight: FontWeight::Bold,
                        text_overflow: TextOverflow::Ellipsis,
                    ),
                )
                Text(
                    value: environment,
                    style: css!(color: hex(SECONDARY_LABEL), font_size: px(14), margin_top: px(4)),
                )
            }
            SystemMenuList(
                sections: sections,
                on_select: move |key: String| {
                    if let Some(index) = key.strip_prefix("area:").and_then(|i| i.parse().ok()) {
                        choose_area(index, area, page, open);
                    }
                },
                style: css!(flex_grow: 1.0).flex_basis(px(0)).min_height(px(0)),
            )
            // 下の端：設定（ガラスの丸いボタン）。
            View(
                style: computed(move || {
                    css!(
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        padding: (px(8), px(20), px((insets.get().bottom as f32).max(16.0)), px(20)),
                    )
                }),
            ) {
                View(
                    style: css!(
                        position: PositionKind::Relative,
                        width: px(48),
                        height: px(48),
                        display: Display::Flex,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                    ),
                    on_tap: move |_| {
                        sheet_page.set(SheetPage::Settings);
                        choosing.set(true);
                    },
                ) {
                    Glass(
                        corner_radius: 24.0_f32,
                        style: css!(position: PositionKind::Absolute, top: px(0), bottom: px(0), left: px(0), right: px(0)),
                    )
                    Text(value: "⚙\u{FE0E}", style: css!(color: hex(LABEL), font_size: px(28)))
                }
            }
            SystemSheet(
                open: choosing,
                title: computed(move || match sheet_page.get() {
                    SheetPage::Settings => "設定".to_owned(),
                    SheetPage::Tenants => "会社を切り替える".to_owned(),
                }),
                sections: computed(move || match sheet_page.get() {
                    SheetPage::Settings => settings_sections.get(),
                    SheetPage::Tenants => tenant_sections.get(),
                }),
                search: computed(move || match sheet_page.get() {
                    SheetPage::Settings => String::new(),
                    SheetPage::Tenants => "会社を検索".to_owned(),
                }),
                on_select: move |key: String| {
                    if key == TENANT_KEY {
                        sheet_page.set(SheetPage::Tenants);
                        // 開くたびに読み直す（前の一覧は読み終わるまで見せておく）。
                        load_tenants();
                        return;
                    }
                    if key == SIGN_OUT_KEY {
                        choosing.set(false);
                        sign_out(token, tenant);
                        return;
                    }
                    let Some(id) = key.strip_prefix("tenant:") else {
                        return;
                    };
                    let picked = tenants.with_untracked(|list| list.iter().find(|t| t.id == id).cloned());
                    let Some(picked) = picked else {
                        return;
                    };
                    choosing.set(false);
                    if tenant.with_untracked(|t| t.as_ref().map(|t| t.id.as_str()) != Some(id)) {
                        open.set(false);
                        storage::save_tenant(&picked);
                        // 会社が変わるとホームごと作り直される（ルートが会社 ID を鍵にしている）。
                        tenant.set(Some(picked));
                    }
                },
                on_dismiss: move |_| choosing.set(false),
            )
        }
    }
}

/// 裏のメニューの中身。iOS 以外は自作の行を並べる。
/// 会社名を押すと、最初の会社選びの画面に戻る。
#[cfg(not(target_os = "ios"))]
#[component]
fn drawer_items(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    area: RwSignal<usize>,
    page: RwSignal<Page>,
    open: RwSignal<bool>,
) -> Element {
    let tenant_name =
        computed(move || tenant.with(|t| t.as_ref().map(|t| t.name.clone()).unwrap_or_default()));
    let environment = computed(move || {
        tenant.with(|t| {
            environment_label(t.as_ref().map_or("", |t| t.environment.as_str())).to_owned()
        })
    });
    render! {
        ScrollView(style: scroll_style()) {
            View(style: scroll_content_style()) {
                View(
                    style: css!(
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        gap: px(12),
                        padding: (px(20), px(16), px(12), px(16)),
                    ),
                    on_tap: move |_| {
                        storage::forget_tenant();
                        tenant.set(None);
                    },
                ) {
                    Avatar(name: tenant_name, size: 44.0_f32)
                    View(
                        style: css!(
                            display: Display::Flex,
                            flex_direction: FlexDirection::Column,
                            flex_shrink: 1.0,
                        ),
                    ) {
                        Text(
                            value: computed(move || tenant_name.get()),
                            max_lines: 1u32,
                            style: css!(
                                color: hex(LABEL),
                                font_size: px(20),
                                font_weight: FontWeight::Bold,
                            )
                            .text_overflow(TextOverflow::Ellipsis),
                        )
                        Text(
                            value: computed(move || format!("{} · タップで会社を切り替え", environment.get())),
                            style: css!(color: hex(SECONDARY_LABEL), font_size: px(13)),
                        )
                    }
                }
                SectionHeader(title: "分野")
                View(style: group_style()) {
                    ForEach(
                        each: || (0..MENU.len()).collect::<Vec<usize>>(),
                        key: |i: &usize| *i,
                        children: move |i: ReadSignal<usize>| {
                            let i = i.get_untracked();
                            let (name, items) = MENU[i];
                            render! {
                                MenuRow(
                                    label: name,
                                    color: items.first().map_or(AVATAR, |f| f.color),
                                    separator: i + 1 != MENU.len(),
                                    on_tap: move |_| choose_area(i, area, page, open),
                                )
                            }
                        },
                    )
                }
                View(style: css!(height: px(32)))
                View(style: group_style()) {
                    View(
                        style: css!(
                            display: Display::Flex,
                            align_items: AlignItems::Center,
                            padding: px(13),
                        ),
                        on_tap: move |_| sign_out(token, tenant),
                    ) {
                        Text(
                            value: "サインアウト",
                            style: css!(color: hex(RED), font_size: px(17)),
                        )
                    }
                }
                View(style: css!(height: px(48)))
            }
        }
    }
}

/// 下のタブの名前：今の分野の機能と、右端の Agent。
fn tab_labels(area: usize) -> Vec<String> {
    let mut labels: Vec<String> = area_features(area)
        .iter()
        .map(|f| f.label.to_owned())
        .collect();
    labels.push(AGENT_TAB.to_owned());
    labels
}

const AGENT_TAB: &str = "Agent";

/// タブの番号 → 開くもの。最後の 1 つは Agent。
fn page_for_tab(area: usize, index: usize) -> Page {
    if index < area_features(area).len() {
        Page::Feature(index)
    } else {
        Page::Agent
    }
}

/// 開いているもの → タブの番号。
fn tab_for_page(area: usize, page: Page) -> usize {
    match page {
        Page::Feature(i) => i,
        Page::Agent => area_features(area).len(),
    }
}

/// iOS は標準のタブバーをそのまま使う（iOS 26 以降は本物の Liquid Glass。
/// 選んだ印がガラスの粒のように滑って動く）。中身は分野ごとに差し替わる。
#[cfg(target_os = "ios")]
#[component]
fn tab_bar(area: RwSignal<usize>, page: RwSignal<Page>) -> Element {
    let insets = safe_area_insets();
    let bottom = insets.get_untracked().bottom as f32;
    let items = computed(move || tab_labels(area.get()));
    let icons = computed(move || {
        let mut icons: Vec<String> = area_features(area.get())
            .iter()
            .map(|f| feature_symbol(f.label).to_owned())
            .collect();
        icons.push("sparkles".to_owned());
        icons
    });
    let selected = computed(move || Some(tab_for_page(area.get(), page.get())));
    render! {
        SystemTabBar(
            items: items,
            icons: icons,
            selected: selected,
            on_select: move |i: usize| page.set(page_for_tab(area.get_untracked(), i)),
            style: css!(
                position: PositionKind::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(0),
                height: px(49.0 + bottom),
            )
            .z_index(10),
        )
    }
}

/// iOS 以外は、自作のタブをガラスの板（iOS 以外はすりガラス風）の上に並べる。
#[cfg(not(target_os = "ios"))]
#[component]
fn tab_bar(area: RwSignal<usize>, page: RwSignal<Page>) -> Element {
    let insets = safe_area_insets();
    render! {
        View(
            style: computed(move || {
                css!(
                    position: PositionKind::Absolute,
                    left: px(16),
                    right: px(16),
                    bottom: px((insets.get().bottom as f32).max(12.0)),
                    height: px(60),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    padding: (px(0), px(6)),
                )
                .z_index(10)
            }),
        ) {
            Glass(
                corner_radius: 30.0_f32,
                style: css!(
                    position: PositionKind::Absolute,
                    top: px(0),
                    bottom: px(0),
                    left: px(0),
                    right: px(0),
                ),
            )
            ForEach(
                each: move || {
                    let a = area.get();
                    (0..tab_labels(a).len()).map(|i| (a, i)).collect::<Vec<_>>()
                },
                key: |k: &(usize, usize)| *k,
                children: move |k: ReadSignal<(usize, usize)>| {
                    let (a, i) = k.get_untracked();
                    let label = signal(tab_labels(a)[i].clone());
                    render! {
                        View(
                            style: computed(move || {
                                let active = tab_for_page(area.get(), page.get()) == i;
                                css!(
                                    flex_grow: 1.0,
                                    flex_basis: px(0),
                                    height: px(48),
                                    display: Display::Flex,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    border_radius: px(24),
                                    background_color: if active {
                                        if is_dark() { Color::rgba(255, 255, 255, 0.12) } else { Color::rgba(0, 0, 0, 0.07) }
                                    } else {
                                        Color::rgba(0, 0, 0, 0.0)
                                    },
                                )
                            }),
                            on_tap: move |_| page.set(page_for_tab(a, i)),
                        ) {
                            Text(
                                value: computed(move || label.get()),
                                style: computed(move || {
                                    let active = tab_for_page(area.get(), page.get()) == i;
                                    css!(
                                        color: hex(if active { TINT } else { LABEL }),
                                        font_size: px(13),
                                        font_weight: FontWeight::Numeric(if active { 600 } else { 500 }),
                                    )
                                }),
                            )
                        }
                    }
                },
            )
        }
    }
}

/// メニューの 1 行：色付きの角丸アイコン、機能名、「›」。
#[component]
fn menu_row(label: &'static str, color: u32, separator: bool, on_tap: Callback<()>) -> Element {
    let glyph: String = label.chars().next().map(String::from).unwrap_or_default();
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                gap: px(14),
                padding_left: px(16),
            ),
            on_tap: move |_| on_tap.call(),
        ) {
            View(
                style: css!(
                    width: px(30),
                    height: px(30),
                    border_radius: px(7),
                    background_color: hex(color),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    flex_shrink: 0.0,
                ),
            ) {
                Text(
                    value: glyph.clone(),
                    style: css!(
                        color: Color::hex(0xFFFFFF),
                        font_size: px(15),
                        font_weight: FontWeight::Bold,
                    ),
                )
            }
            View(
                style: {
                    let s = css!(
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        flex_grow: 1.0,
                        padding: (px(12), px(16), px(12), px(0)),
                    );
                    if separator {
                        s.border_bottom(hairline())
                    } else {
                        s
                    }
                },
            ) {
                Text(
                    value: label,
                    style: css!(color: hex(LABEL), font_size: px(17), flex_grow: 1.0),
                )
                Text(value: "›", style: css!(color: hex(CHEVRON), font_size: px(26)))
            }
        }
    }
}

// ---------------------------------------------------------------- 機能の一覧

/// どの機能でも使う一覧画面。検索欄は入力するたびに絞り込む。
#[component]
fn feature_list(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    feature: Feature,
    back_label: &'static str,
    on_back: Callback<()>,
) -> Element {
    let query = signal(String::new());
    let items = signal(Vec::<Value>::new());
    let loading = signal(true);
    let error = signal(String::new());
    let selected = signal(None::<Value>);

    let visible = computed(move || {
        let q = query.get().trim().to_lowercase();
        items.with(|all| {
            all.iter()
                .filter(|item| {
                    q.is_empty()
                        || format!(
                            "{} {}",
                            features::title(&feature, item),
                            features::subtitle(&feature, item)
                        )
                        .to_lowercase()
                        .contains(&q)
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let refreshing = signal(false);
    load_list(token, tenant, feature, items, loading, error, refreshing);

    // 詳細は一覧の上に重ねて出す。一覧は消さずに後ろに残すので、
    // 詳細を右へ引くと、下から一覧が見えてくる（戻ったときもスクロール位置のまま）。
    // 詳細を出している間の一覧は、すこし左に寄せて暗くしておき、
    // 引いた幅に合わせて元の位置・明るさへ戻す。
    let back = use_context::<BackSwipe>();
    let dragging = computed(move || back.is_some_and(|b| b.drag.get().is_some()));
    let behind = computed(move || {
        if selected.with(Option::is_none) {
            return None;
        }
        let dx = back.and_then(|b| b.drag.get()).unwrap_or(0.0);
        Some((dx / BEHIND_TRAVEL).clamp(0.0, 1.0))
    });
    let ease = || {
        Transition::new(TransitionPropertyKind::name("transform"))
            .duration(ms(220))
            .timing(EasingFunction::EaseOut)
    };

    render! {
        View(
            style: css!(
                position: PositionKind::Relative,
                flex_grow: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            )
            .flex_basis(px(0))
            .min_height(px(0)),
        ) {
            View(
                style: computed(move || {
                    let x = behind.get().map_or(0.0, |p| -BEHIND_SHIFT * (1.0 - p));
                    let s = screen_style().transform(TransformFn::TranslateX(px(x).into()));
                    if dragging.get() { s } else { s.transition(ease()) }
                }),
            ) {
                NavBar(
                    title: feature.label,
                    back_label: back_label,
                    on_back: on_back,
                    action_label: "",
                    on_action: |_| {},
                )
                ListBody(
                    feature: feature,
                    query: query,
                    visible: visible,
                    loading: loading,
                    error: error,
                    selected: selected,
                    refreshing: refreshing,
                    on_refresh: move |_| {
                        refreshing.set(true);
                        load_list(token, tenant, feature, items, loading, error, refreshing);
                    },
                )
            }
            Show(when: move || behind.get().is_some()) {
                View(
                    style: computed(move || {
                        let p = behind.get().unwrap_or(1.0);
                        css!(
                            position: PositionKind::Absolute,
                            top: px(0),
                            bottom: px(0),
                            left: px(0),
                            right: px(0),
                        )
                        .background_color(Color::rgba(0, 0, 0, 0.12 * (1.0 - p)))
                    }),
                )
            }
            Show(when: move || selected.with(Option::is_some)) {
                View(
                    style: css!(
                        position: PositionKind::Absolute,
                        top: px(0),
                        bottom: px(0),
                        left: px(0),
                        right: px(0),
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                    ),
                ) {
                    ItemDetail(token: token, tenant: tenant, feature: feature, selected: selected)
                }
            }
        }
    }
}

/// 一覧を読み込む（最初の表示と、引っ張って再読み込みのとき）。
#[allow(clippy::too_many_arguments)]
fn load_list(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    feature: Feature,
    items: RwSignal<Vec<Value>>,
    loading: RwSignal<bool>,
    error: RwSignal<String>,
    refreshing: RwSignal<bool>,
) {
    let Some(current) = tenant.get_untracked() else {
        return;
    };
    spawn_local(async move {
        let result = authed(token, move |access| {
            let mut list = api::fetch_list(access, &current, feature.path, feature.list_key)?;
            if feature.path.starts_with("/v1/erp/deals") {
                api::resolve_pipelines(access, &current, &mut list);
            }
            Ok(list)
        })
        .await;
        loading.set(false);
        refreshing.set(false);
        match result {
            Ok(list) => {
                error.set(String::new());
                items.set(list);
            }
            Err(message) => error.set(message),
        }
    });
}

/// 一覧の中身。iOS は標準の一覧（検索欄・引っ張って再読み込み付き）。
#[cfg(target_os = "ios")]
#[allow(clippy::too_many_arguments)]
#[component]
fn list_body(
    feature: Feature,
    query: RwSignal<String>,
    visible: Signal<Vec<Value>>,
    loading: RwSignal<bool>,
    error: RwSignal<String>,
    selected: RwSignal<Option<Value>>,
    refreshing: RwSignal<bool>,
    on_refresh: Callback<()>,
) -> Element {
    // 標準の検索欄は作り直すと空になるので、絞り込みも空に戻してそろえる（詳細から戻ったとき）。
    query.set(String::new());
    let rows = computed(move || {
        with_list(visible, |list| {
            let rows: Vec<serde_json::Value> = list
                .iter()
                .map(|item| {
                    serde_json::json!({
                        "key": features::id(item),
                        "title": features::title(&feature, item),
                        "subtitle": features::subtitle(&feature, item),
                        "trailing": features::trailing(&feature, item),
                        "avatar": feature.avatar,
                    })
                })
                .collect();
            serde_json::Value::Array(rows).to_string()
        })
    });
    let header = computed(move || format!("{} 件", with_list(visible, Vec::len)));
    let message = computed(move || {
        if loading.get() {
            "読み込み中…".to_owned()
        } else if !error.with(String::is_empty) {
            error.get()
        } else if with_list(visible, Vec::is_empty) {
            "見つかりません".to_owned()
        } else {
            String::new()
        }
    });

    render! {
        SystemRecordList(
            rows: rows,
            header: header,
            message: message,
            placeholder: "検索",
            refreshing: computed(move || refreshing.get()),
            bottom_inset: TAB_BAR_SPACE,
            on_select: move |key: String| {
                let item = with_list(visible, |list| {
                    list.iter().find(|item| features::id(item) == key).cloned()
                });
                if item.is_some() {
                    selected.set(item);
                }
            },
            on_search: move |text: String| query.set(text),
            on_refresh: move |_| on_refresh.call(),
            style: css!(flex_grow: 1.0).flex_basis(px(0)).min_height(px(0)),
        )
    }
}

/// 一覧の中身。iOS 以外は自作の検索欄と行を並べる。
#[cfg(not(target_os = "ios"))]
#[allow(clippy::too_many_arguments)]
#[component]
fn list_body(
    feature: Feature,
    query: RwSignal<String>,
    visible: Signal<Vec<Value>>,
    loading: RwSignal<bool>,
    error: RwSignal<String>,
    selected: RwSignal<Option<Value>>,
    refreshing: RwSignal<bool>,
    on_refresh: Callback<()>,
) -> Element {
    let _ = (refreshing, on_refresh);
    let last_id = computed(move || with_list(visible, |v| v.last().map(features::id)));
    render! {
        ScrollView(style: scroll_style()) {
            View(style: scroll_content_style()) {
                View(
                    style: css!(
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        padding: (px(4), px(16), px(8), px(16)),
                    ),
                ) {
                    Input(
                        text: query,
                        placeholder: "検索",
                        return_key: ReturnKey::Search,
                        auto_capitalize: AutoCapitalize::None,
                        style: css!(
                            height: px(36),
                            font_size: px(17),
                            padding: (px(0), px(10)),
                            border_radius: px(10),
                            background_color: hex(FILL),
                            color: hex(LABEL),
                        ),
                    )
                }
                Show(when: move || loading.get()) {
                    Notice(message: "読み込み中…")
                }
                Show(when: move || !error.with(String::is_empty)) {
                    ErrorNotice(message: computed(move || error.get()))
                }
                Show(
                    when: move || !loading.get() && error.with(String::is_empty) && with_list(visible, Vec::is_empty),
                ) {
                    Notice(message: "見つかりません")
                }
                Show(when: move || !with_list(visible, Vec::is_empty)) {
                    SectionHeader(
                        title: computed(move || format!("{} 件", with_list(visible, Vec::len))),
                    )
                    View(style: group_style()) {
                        ForEach(
                            each: move || visible.get(),
                            key: |item: &Value| features::id(item),
                            children: move |item: ReadSignal<Value>| {
                                let item = item.get_untracked();
                                let picked = item.clone();
                                let id = features::id(&item);
                                render! {
                                    ListRow(
                                        title: features::title(&feature, &item),
                                        subtitle: features::subtitle(&feature, &item),
                                        subtitle_color: SECONDARY_LABEL,
                                        trailing: features::trailing(&feature, &item),
                                        avatar: feature.avatar,
                                        separator: computed(move || last_id.get().as_deref() != Some(id.as_str())),
                                        on_tap: move |_| selected.set(Some(picked.clone())),
                                    )
                                }
                            },
                        )
                    }
                }
                View(style: css!(height: px(TAB_BAR_SPACE)))
            }
        }
    }
}

/// 一覧の中身を借りて読む（`Signal` には `with` が無いので、取り出してから渡す）。
fn with_list<R>(list: Signal<Vec<Value>>, f: impl FnOnce(&Vec<Value>) -> R) -> R {
    f(&list.get())
}

// ---------------------------------------------------------------- 1 件の詳細

/// 詳細画面。一覧の応答に無い項目がある機能（販売契約の備考・明細など）は、
/// 開いたあとで 1 件を取り直し、届いたら描き直す。
#[component]
fn item_detail(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    feature: Feature,
    selected: RwSignal<Option<Value>>,
) -> Element {
    let record = signal(selected.get_untracked().unwrap_or_default());
    let version = signal(0_usize);
    let back = use_context::<BackSwipe>();
    if let Some(back) = back {
        back.target.set(Some(selected));
        on_cleanup(move || {
            back.target.set(None);
            back.drag.set(None);
        });
    }
    if let (Some(template), Some(current)) = (feature.detail_path, tenant.get_untracked()) {
        let id = features::id(&record.get_untracked());
        if !id.is_empty() {
            let path = template.replace("{id}", &id);
            spawn_local(async move {
                let result = authed(token, move |access| {
                    api::fetch_record(access, &current, &path)
                })
                .await;
                if let Ok(full) = result {
                    if full.is_object() {
                        record.set(full);
                        version.update(|v| *v += 1);
                    }
                }
            });
        }
    }
    render! {
        // 左から右へ引いている間は、指について右へずれる。
        View(
            style: computed(move || {
                let dx = back.and_then(|b| b.drag.get());
                let x = dx.unwrap_or(0.0);
                let shadow = if x > 0.0 { 0.18 } else { 0.0 };
                let s = screen_style()
                    .background_color(hex(SYSTEM_BACKGROUND))
                    .transform(TransformFn::TranslateX(px(x).into()))
                    .box_shadow(px(0), px(0), px(24), px(0), Color::rgba(0, 0, 0, shadow));
                if dx.is_some() {
                    s
                } else {
                    s.transition(
                        Transition::new(TransitionPropertyKind::name("transform"))
                            .duration(ms(220))
                            .timing(EasingFunction::EaseOut),
                    )
                }
            }),
        ) {
            ForEach(
                each: move || vec![version.get()],
                key: |v: &usize| *v,
                children: move |_: ReadSignal<usize>| render! {
                    DetailBody(feature: feature, item: record.get_untracked(), selected: selected)
                },
            )
        }
    }
}

/// 連絡先アプリのような詳細画面：上に名前、その下に項目と明細。
#[component]
fn detail_body(feature: Feature, item: Value, selected: RwSignal<Option<Value>>) -> Element {
    let title = signal(features::title(&feature, &item));
    let status = signal(features::trailing(&feature, &item));
    let rows = features::detail_rows(&feature, &item);
    let lines = features::line_rows(&item);
    let has_status = !status.with(String::is_empty);
    let has_rows = !rows.is_empty();
    let has_lines = !lines.is_empty();

    render! {
        View(style: screen_style()) {
            NavBar(
                title: "",
                back_label: feature.label,
                on_back: move |_| selected.set(None),
                action_label: "",
                on_action: |_| {},
            )
            ScrollView(style: scroll_style()) {
                View(style: scroll_content_style()) {
                    View(
                        style: css!(
                            display: Display::Flex,
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            gap: px(8),
                            padding: (px(8), px(24), px(24), px(24)),
                        ),
                    ) {
                        Show(when: move || feature.avatar) {
                            Avatar(name: computed(move || title.get()), size: 96.0_f32)
                        }
                        Text(
                            value: computed(move || title.get()),
                            style: css!(
                                color: hex(LABEL),
                                font_size: px(26),
                                font_weight: FontWeight::Numeric(600),
                                text_align: TextAlign::Center,
                            ),
                        )
                        Show(when: move || has_status) {
                            View(
                                style: css!(
                                    padding: (px(4), px(12)),
                                    border_radius: px(999),
                                    background_color: hex(FILL),
                                ),
                            ) {
                                Text(
                                    value: computed(move || status.get()),
                                    style: css!(color: hex(LABEL), font_size: px(13)),
                                )
                            }
                        }
                    }
                    Show(when: move || has_rows) {
                        InfoGroup(rows: rows.clone(), inline: false)
                        View(style: css!(height: px(24)))
                    }
                    Show(when: move || has_lines) {
                        SectionHeader(title: "明細")
                        InfoGroup(rows: lines.clone(), inline: true)
                        View(style: css!(height: px(24)))
                    }
                    Show(when: move || !has_rows && !has_lines) {
                        Notice(message: "詳しい情報はまだ登録されていません")
                    }
                    View(style: css!(height: px(TAB_BAR_SPACE)))
                }
            }
        }
    }
}

/// 項目のグループ。`inline` なら「ラベル … 値」を横並び、
/// そうでなければ連絡先アプリと同じ「小さなラベルの下に値」。
#[component]
fn info_group(rows: Signal<Vec<(String, String)>>, inline: bool) -> Element {
    let numbered = computed(move || {
        rows.get()
            .into_iter()
            .enumerate()
            .map(|(i, (label, value))| (i, label, value))
            .collect::<Vec<_>>()
    });
    let last = computed(move || numbered.with(Vec::len).saturating_sub(1));
    render! {
        View(style: group_style()) {
            ForEach(
                each: move || numbered.get(),
                key: |r: &(usize, String, String)| r.0,
                children: move |r: ReadSignal<(usize, String, String)>| {
                    let (index, label, value) = r.get_untracked();
                    render! {
                        View(style: css!(display: Display::Flex, padding_left: px(16))) {
                            View(
                                style: computed(move || {
                                    let s = if inline {
                                        css!(
                                            display: Display::Flex,
                                            flex_direction: FlexDirection::Row,
                                            align_items: AlignItems::Center,
                                            gap: px(12),
                                            flex_grow: 1.0,
                                            padding: (px(12), px(16), px(12), px(0)),
                                        )
                                    } else {
                                        css!(
                                            display: Display::Flex,
                                            flex_direction: FlexDirection::Column,
                                            gap: px(2),
                                            flex_grow: 1.0,
                                            padding: (px(10), px(16), px(10), px(0)),
                                        )
                                    };
                                    if index == last.get() {
                                        s
                                    } else {
                                        s.border_bottom(hairline())
                                    }
                                }),
                            ) {
                                Text(
                                    value: label,
                                    style: if inline {
                                        css!(
                                            color: hex(LABEL),
                                            font_size: px(17),
                                            flex_grow: 1.0,
                                            flex_shrink: 1.0,
                                        )
                                    } else {
                                        css!(color: hex(LABEL), font_size: px(13))
                                    },
                                )
                                Text(
                                    value: value,
                                    style: if inline {
                                        css!(
                                            color: hex(SECONDARY_LABEL),
                                            font_size: px(17),
                                            flex_shrink: 0.0,
                                        )
                                    } else {
                                        css!(color: hex(LABEL), font_size: px(17))
                                    },
                                )
                            }
                        }
                    }
                },
            )
        }
    }
}

// ---------------------------------------------------------------- 部品

/// 画面上部のバー。左に「☰」か青い「‹ 戻る」、真ん中に題名、右に青い文字のボタン。空文字なら出さない。
#[component]
fn nav_bar(
    title: &'static str,
    back_label: &'static str,
    on_back: Callback<()>,
    action_label: &'static str,
    on_action: Callback<()>,
) -> Element {
    let has_back = !back_label.is_empty();
    let has_action = !action_label.is_empty();
    // 戻る先が無い画面では、左上に裏のメニューを開く「☰」を出す。
    let drawer = if has_back {
        None
    } else {
        use_context::<Drawer>()
    };
    let has_menu = drawer.is_some();
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                height: px(if has_menu { 56 } else { 44 }),
                padding: (px(0), px(8)),
                position: PositionKind::Relative,
            ),
        ) {
            // 左右のボタンに重ならない幅で、帯の真ん中に置く。
            Text(
                value: title,
                max_lines: 1u32,
                style: css!(
                    position: PositionKind::Absolute,
                    left: px(96),
                    right: px(96),
                    text_align: TextAlign::Center,
                    color: hex(LABEL),
                    font_size: px(17),
                    font_weight: FontWeight::Numeric(600),
                )
                .text_overflow(TextOverflow::Ellipsis),
            )
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    gap: px(4),
                    padding: (px(6), px(8)),
                ),
                on_tap: move |_| match drawer {
                    Some(Drawer(open)) => open.set(true),
                    None => on_back.call(),
                },
            ) {
                Show(when: move || has_menu) {
                    // iOS の本物のガラス（Liquid Glass）の丸いボタン。ガラスを下敷きにして、線を上に重ねる。
                    View(
                        style: css!(
                            width: px(42),
                            height: px(42),
                            position: PositionKind::Relative,
                            display: Display::Flex,
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            gap: px(4),
                        ),
                    ) {
                        Glass(
                            corner_radius: 21.0_f32,
                            style: css!(
                                position: PositionKind::Absolute,
                                top: px(0),
                                left: px(0),
                                width: px(42),
                                height: px(42),
                            ),
                        )
                        View(style: menu_bar_style())
                        View(style: menu_bar_style())
                        View(style: menu_bar_style())
                    }
                }
                Show(when: move || has_back) {
                    Text(
                        value: "‹",
                        style: css!(
                            color: hex(TINT),
                            font_size: px(34),
                            font_weight: FontWeight::Numeric(300),
                        ),
                    )
                    Text(value: back_label, style: css!(color: hex(TINT), font_size: px(17)))
                }
            }
            View(style: css!(padding: (px(6), px(8))), on_tap: move |_| on_action.call()) {
                Show(when: move || has_action) {
                    Text(
                        value: action_label,
                        style: css!(color: hex(TINT), font_size: px(17)),
                    )
                }
            }
        }
    }
}

#[component]
fn large_title(title: &'static str) -> Element {
    render! {
        Text(
            value: title,
            style: css!(
                color: hex(LABEL),
                font_size: px(34),
                font_weight: FontWeight::Bold,
                padding: (px(0), px(16), px(8), px(16)),
            ),
        )
    }
}

/// グループの上に付く小さな灰色の見出し。
#[component]
fn section_header(title: Signal<String>) -> Element {
    render! {
        Text(
            value: title,
            style: css!(
                color: hex(SECONDARY_LABEL),
                font_size: px(13),
                padding: (px(16), px(32), px(6), px(32)),
            ),
        )
    }
}

/// 一覧の 1 行：丸いアイコン、名前、2 行目、右端の状態と「›」。
/// 区切り線はアイコンの右から引く（iOS と同じ）。最後の行には引かない。
#[component]
fn list_row(
    title: Signal<String>,
    subtitle: Signal<String>,
    subtitle_color: u32,
    trailing: Signal<String>,
    avatar: bool,
    separator: Signal<bool>,
    on_tap: Callback<()>,
) -> Element {
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                gap: px(12),
                padding_left: px(16),
            ),
            on_tap: move |_| on_tap.call(),
        ) {
            Show(when: move || avatar) {
                Avatar(name: title, size: 40.0_f32)
            }
            View(
                style: computed(move || {
                    let s = css!(
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        gap: px(8),
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                        padding: (px(10), px(14), px(10), px(0)),
                    );
                    if separator.get() {
                        s.border_bottom(hairline())
                    } else {
                        s
                    }
                }),
            ) {
                View(
                    style: css!(
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        gap: px(2),
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                    ),
                ) {
                    Text(
                        value: title,
                        max_lines: 1u32,
                        style: css!(
                            color: hex(LABEL),
                            font_size: px(17),
                        )
                        .text_overflow(TextOverflow::Ellipsis),
                    )
                    Show(when: move || !subtitle.get().is_empty()) {
                        Text(
                            value: subtitle,
                            max_lines: 1u32,
                            style: css!(
                                color: hex(subtitle_color),
                                font_size: px(15),
                            )
                            .text_overflow(TextOverflow::Ellipsis),
                        )
                    }
                }
                Text(
                    value: trailing,
                    max_lines: 1u32,
                    style: css!(color: hex(SECONDARY_LABEL), font_size: px(15), flex_shrink: 0.0),
                )
                Text(
                    value: "›",
                    style: css!(color: hex(CHEVRON), font_size: px(26), flex_shrink: 0.0),
                )
            }
        }
    }
}

/// 名前の 1 文字目を灰色の丸に白抜きで入れたアイコン（連絡先アプリと同じ）。
#[component]
fn avatar(name: Signal<String>, size: f32) -> Element {
    render! {
        View(
            style: css!(
                width: px(size),
                height: px(size),
                border_radius: px(size / 2.0),
                background_color: hex(AVATAR),
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_shrink: 0.0,
            ),
        ) {
            Text(
                value: computed(move || initial(&name.get())),
                style: css!(
                    color: Color::hex(0xFFFFFF),
                    font_size: px(size * 0.42),
                    font_weight: FontWeight::Numeric(600),
                ),
            )
        }
    }
}

#[component]
fn notice(message: &'static str) -> Element {
    render! {
        Text(
            value: message,
            style: css!(
                color: hex(SECONDARY_LABEL),
                font_size: px(15),
                padding: (px(32), px(16)),
                text_align: TextAlign::Center,
            ),
        )
    }
}

#[component]
fn error_notice(message: Signal<String>) -> Element {
    render! {
        Text(
            value: message,
            style: css!(
                color: hex(RED),
                font_size: px(15),
                padding: (px(16), px(32)),
                text_align: TextAlign::Center,
            ),
        )
    }
}

/// アイコンに入れる 1 文字。括弧などの記号は飛ばす。
fn initial(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into())
}

/// 1 画面。高さを画面に合わせる（中身に合わせて伸びないようにする）。
fn screen_style() -> Css {
    css!(
        flex_grow: 1.0,
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
    )
    .flex_basis(px(0))
    .min_height(px(0))
}

/// スクロールする部分。残りの高さぴったりに収め、はみ出た分をスクロールさせる。
/// これが無いと中身の長さまで伸びてしまい、スクロールできない。
fn scroll_style() -> Css {
    css!(flex_grow: 1.0).flex_basis(px(0)).min_height(px(0))
}

/// スクロールする中身をまとめる 1 枚。ScrollView の直下は 1 つにする
/// （直下に複数並べると、一部が表示されないことがあった）。
fn scroll_content_style() -> Css {
    css!(display: Display::Flex, flex_direction: FlexDirection::Column)
}

/// API を呼ぶ。アクセストークンが切れていたら、保存したリフレッシュトークンで
/// 取り直して 1 回だけやり直す。
async fn authed<T: Send + 'static>(
    token: RwSignal<Option<String>>,
    call: impl Fn(&str) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let access = token.get_untracked().unwrap_or_default();
    let (result, renewed) = run_blocking(move || match call(&access) {
        Err(error) if error == api::EXPIRED => {
            let Some(refresh_token) = storage::refresh_token() else {
                return (Err(error), None);
            };
            match api::refresh(&refresh_token) {
                Ok(session) => {
                    if let Some(rotated) = &session.refresh_token {
                        storage::save_refresh_token(rotated);
                    }
                    (call(&session.access_token), Some(session.access_token))
                }
                Err(error) => (Err(error), None),
            }
        }
        other => (other, None),
    })
    .await;
    if let Some(access_token) = renewed {
        token.set(Some(access_token));
    }
    result
}

/// サインアウト：端末に残したものを消し、Cognito 側でもリフレッシュトークンを無効にする。
fn sign_out(token: RwSignal<Option<String>>, tenant: RwSignal<Option<Tenant>>) {
    if let Some(refresh_token) = storage::refresh_token() {
        spawn_local(async move {
            run_blocking(move || api::revoke(&refresh_token)).await;
        });
    }
    storage::forget_all();
    tenant.set(None);
    token.set(None);
}

/// iOS の「角丸グループ」：枠線なしの白い角丸。左右 16pt あける。
/// 「☰」の 1 本の線。
fn menu_bar_style() -> Css {
    css!(
        width: px(16),
        height: px(2),
        border_radius: px(1),
        background_color: hex(LABEL),
    )
}

fn group_style() -> Css {
    css!(
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        margin: (px(0), px(16)),
        border_radius: px(10),
        background_color: cell_color(),
        overflow: Overflow::Hidden,
    )
}

fn form_row_style() -> Css {
    css!(
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        margin_left: px(16),
    )
}

/// ガラスの上の入力欄。背景は透かして、下の板の色を見せる。
fn glass_input_style() -> Css {
    css!(
        height: px(52),
        font_size: px(17),
        padding_right: px(16),
        background_color: Color::rgba(0, 0, 0, 0.0),
        color: hex(LABEL),
    )
}

/// ガラスの上の区切り線。白い板の線より薄くして、透けた色になじませる。
fn glass_hairline() -> Border {
    Border::new()
        .width(px(0.5))
        .color(if is_dark() { Color::rgba(84, 84, 88, 0.6) } else { Color::rgba(60, 60, 67, 0.25) })
        .style(BorderStyle::Solid)
}

/// iOS の区切り線（細い 1 本）。
fn hairline() -> Border {
    Border::new()
        .width(px(0.5))
        .color(hex(SEPARATOR))
        .style(BorderStyle::Solid)
}
