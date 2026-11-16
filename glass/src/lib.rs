//! `Glass` — ガラスの板。iOS では Apple の本物のガラス（Liquid Glass）、
//! それ以外（Android・Mac・Web）は白の半透明に後ろのぼかしを重ねた「すりガラス風」。
//!
//! 中身は持たない。ボタンなどの「下敷き」として、同じ大きさの部品の
//! 一番下に置き、その上に文字や線を重ねて使う。
//!
//! ```ignore
//! View(style: css!(width: px(42), height: px(42), position: PositionKind::Relative)) {
//!     Glass(corner_radius: 21.0, style: css!(position: PositionKind::Absolute, top: px(0), ...))
//!     Text(value: "☰")
//! }
//! ```
//!
//! iOS でも 26 より前の端末では、Apple の普通のすりガラスになる（Swift 側で切り替え）。

#[cfg(target_os = "ios")]
use whisker::Callback;
use whisker::prelude::*;
use whisker::{Signal, Style};

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:Glass", measurement = None)]
pub fn native_glass(corner_radius: Signal<String>, style: Style) {}

/// 生成されたホストの起動処理が読む、部品の一覧。
#[doc(hidden)]
pub fn __whisker_element_module_definition() -> whisker::ElementModuleDefinition {
    whisker::ElementModuleDefinition::new(
        env!("CARGO_PKG_NAME"),
        [
            native_glass_schema::element_provider(),
            native_tab_bar_schema::element_provider(),
            native_menu_list_schema::element_provider(),
            native_record_list_schema::element_provider(),
            native_keyboard_observer_schema::element_provider(),
            native_refresh_control_schema::element_provider(),
            native_appearance_observer_schema::element_provider(),
            native_nav_bar_schema::element_provider(),
            native_sheet_schema::element_provider(),
            native_backdrop_schema::element_provider(),
            native_attach_button_schema::element_provider(),
            native_print_button_schema::element_provider(),
            native_document_preview_schema::element_provider(),
        ],
    )
}

/// ガラスの板。`corner_radius` は角の丸み（pt）。
#[cfg(target_os = "ios")]
#[component]
pub fn glass(corner_radius: f32, style: Style) -> Element {
    NativeGlass::builder()
        .corner_radius(corner_radius.to_string())
        .style(style.clone())
        .build()
}

/// ガラスの板。iOS 以外は本物のガラスが無いので、すりガラス風で代わりにする。
#[cfg(not(target_os = "ios"))]
#[component]
pub fn glass(corner_radius: f32, style: Style) -> Element {
    use whisker::css::{BackdropFilter, Border, BorderStyle, Color, PositionKind};

    let outer = style.clone();
    render! {
        View(style: outer) {
            View(
                style: css!(
                    position: PositionKind::Absolute,
                    top: px(0),
                    bottom: px(0),
                    left: px(0),
                    right: px(0),
                    border_radius: px(corner_radius),
                    background_color: Color::rgba(255, 255, 255, 0.62),
                )
                .backdrop_filter(BackdropFilter::blur(px(24)))
                .border(
                    Border::new()
                        .width(px(0.5))
                        .color(Color::rgba(0, 0, 0, 0.12))
                        .style(BorderStyle::Solid),
                ),
            )
        }
    }
}

/// iOS 標準のタブバーが押されたときの知らせ。`detail.index` が押されたタブの番号。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct TabSelectEvent {
    #[serde(default)]
    pub detail: TabSelectDetail,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct TabSelectDetail {
    #[serde(default)]
    pub index: i64,
}

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:TabBar", measurement = None)]
pub fn native_tab_bar(
    items: Signal<String>,
    icons: Signal<String>,
    selected: Signal<String>,
    style: Style,
    on_select: TabSelectEvent,
) {
}

/// iOS 標準のタブバー（iOS 26 以降は本物の Liquid Glass）。iOS 専用。
///
/// - `items` … タブの名前の並び（中身が変わればタブも差し替わる）
/// - `icons` … 各タブの SF Symbols の名前（`items` と同じ順）
/// - `selected` … 選んでいるタブの番号。`None` ならどれも選ばない
/// - `on_select` … タブが押されたら、その番号で呼ばれる
#[cfg(target_os = "ios")]
#[component]
pub fn tab_bar(
    items: Signal<Vec<String>>,
    icons: Signal<Vec<String>>,
    selected: Signal<Option<usize>>,
    on_select: Callback<usize>,
    style: Style,
) -> Element {
    NativeTabBar::builder()
        .items(Signal::from(computed(move || items.get().join("|"))))
        .icons(Signal::from(computed(move || icons.get().join("|"))))
        .selected(Signal::from(computed(move || {
            selected.get().map_or(-1, |i| i as i64).to_string()
        })))
        .style(style.clone())
        .on_select(move |e: TabSelectEvent| {
            if let Ok(index) = usize::try_from(e.detail.index) {
                on_select.run(index);
            }
        })
        .build()
}

/// iOS 標準の一覧の行が押されたときの知らせ。`detail.key` が押された行の鍵。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct MenuSelectEvent {
    #[serde(default)]
    pub detail: MenuSelectDetail,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct MenuSelectDetail {
    #[serde(default)]
    pub key: String,
}

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:MenuList", measurement = None)]
pub fn native_menu_list(sections: Signal<String>, style: Style, on_select: MenuSelectEvent) {}

/// iOS 標準の一覧（設定アプリと同じ見た目）。iOS 専用。
///
/// - `sections` … 見出しと行の JSON（形は Swift 側 `MenuListView.swift` の説明を参照）
/// - `on_select` … 行が押されたら、その行の `key` で呼ばれる
#[cfg(target_os = "ios")]
#[component]
pub fn menu_list(sections: Signal<String>, on_select: Callback<String>, style: Style) -> Element {
    NativeMenuList::builder()
        .sections(sections)
        .style(style.clone())
        .on_select(move |e: MenuSelectEvent| on_select.run(e.detail.key))
        .build()
}

/// iOS 標準のデータ一覧の検索欄が変わったときの知らせ。`detail.value` が今の文字。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct SearchEvent {
    #[serde(default)]
    pub detail: SearchDetail,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct SearchDetail {
    #[serde(default)]
    pub value: String,
}

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:RecordList", measurement = None)]
pub fn native_record_list(
    rows: Signal<String>,
    header: Signal<String>,
    message: Signal<String>,
    placeholder: Signal<String>,
    refreshing: Signal<bool>,
    bottom_inset: Signal<String>,
    style: Style,
    on_select: MenuSelectEvent,
    on_search: SearchEvent,
    on_refresh: (),
) {
}

/// iOS 標準のデータ一覧（検索欄・引っ張って再読み込み付き）。iOS 専用。
///
/// - `rows` … 行の JSON（形は Swift 側 `RecordListView.swift` の説明を参照）
/// - `header` … 一覧の上の小見出し
/// - `message` … 行が無いときに真ん中に出す文
/// - `refreshing` … 再読み込み中か。false になったらくるくるを止める
#[cfg(target_os = "ios")]
#[allow(clippy::too_many_arguments)]
#[component]
pub fn record_list(
    rows: Signal<String>,
    header: Signal<String>,
    message: Signal<String>,
    placeholder: &'static str,
    refreshing: Signal<bool>,
    bottom_inset: f32,
    on_select: Callback<String>,
    on_search: Callback<String>,
    on_refresh: Callback<()>,
    style: Style,
) -> Element {
    NativeRecordList::builder()
        .rows(rows)
        .header(header)
        .message(message)
        .placeholder(placeholder.to_string())
        .refreshing(refreshing)
        .bottom_inset(bottom_inset.to_string())
        .style(style.clone())
        .on_select(move |e: MenuSelectEvent| on_select.run(e.detail.key))
        .on_search(move |e: SearchEvent| on_search.run(e.detail.value))
        .on_refresh(move || on_refresh.run(()))
        .build()
}

/// キーボードの高さが変わったときの知らせ。`detail.height` は画面の下端から測った高さ（pt）。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct KeyboardEvent {
    #[serde(default)]
    pub detail: KeyboardDetail,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct KeyboardDetail {
    #[serde(default)]
    pub height: f64,
}

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:KeyboardObserver", measurement = None)]
pub fn native_keyboard_observer(style: Style, on_change: KeyboardEvent) {}

/// 見えない部品。iOS のキーボードが出たり引っ込んだりするたびに、その高さ（pt）を知らせる。
/// 入力欄をキーボードの上へ持ち上げるのに使う。iOS 以外は何も知らせない。
#[cfg(target_os = "ios")]
#[component]
pub fn keyboard_observer(on_change: Callback<f32>) -> Element {
    use whisker::css::PositionKind;
    NativeKeyboardObserver::builder()
        .style(css!(position: PositionKind::Absolute, width: px(0), height: px(0)))
        .on_change(move |e: KeyboardEvent| on_change.run(e.detail.height as f32))
        .build()
}

#[cfg(not(target_os = "ios"))]
#[component]
pub fn keyboard_observer(on_change: Callback<f32>) -> Element {
    let _ = on_change;
    render! { View() }
}

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:RefreshControl", measurement = None)]
pub fn native_refresh_control(refreshing: Signal<bool>, style: Style, on_refresh: ()) {}

/// 見えない部品。置いた `ScrollView` に iOS 標準の「引っ張って更新」（UIRefreshControl）を付ける。
/// `ScrollView` の中身の中に置く。iOS 以外は何もしない。
///
/// - `refreshing` … 更新中か。false になったらくるくるを止める
/// - `on_refresh` … 最上部から引っ張って離したとき
#[cfg(target_os = "ios")]
#[component]
pub fn refresh_control(refreshing: Signal<bool>, on_refresh: Callback<()>) -> Element {
    use whisker::css::PositionKind;
    NativeRefreshControl::builder()
        .refreshing(refreshing)
        .style(css!(position: PositionKind::Absolute, width: px(0), height: px(0)))
        .on_refresh(move || on_refresh.run(()))
        .build()
}

#[cfg(not(target_os = "ios"))]
#[component]
pub fn refresh_control(refreshing: Signal<bool>, on_refresh: Callback<()>) -> Element {
    let _ = (refreshing, on_refresh);
    render! { View() }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct AppearanceEvent {
    #[serde(default)]
    pub detail: AppearanceDetail,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct AppearanceDetail {
    /// 暗い表示なら 1、明るい表示なら 0。
    #[serde(default)]
    pub dark: f64,
}

/// 画面の幅が変わったときの知らせ。`wide` は iPad の広い表示（横の大きさが regular で幅 700 以上）なら 1。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct LayoutEvent {
    #[serde(default)]
    pub detail: LayoutDetail,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct LayoutDetail {
    #[serde(default)]
    pub wide: f64,
    /// 画面（ウィンドウ）の幅（pt）。
    #[serde(default)]
    pub width: f64,
}

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:AppearanceObserver", measurement = None)]
pub fn native_appearance_observer(style: Style, on_change: AppearanceEvent, on_layout: LayoutEvent) {}

/// 見えない部品。iPhone が暗い表示（ダークモード）かどうかを、置かれたときと
/// 切り替わるたびに知らせる（暗いなら true）。あわせて、iPad の広い表示か（true）と
/// 画面の幅を `on_layout` で知らせる。iOS 以外はすぐに false と (false, 0) を 1 回だけ知らせる。
#[cfg(target_os = "ios")]
#[component]
pub fn appearance_observer(on_change: Callback<bool>, on_layout: Callback<(bool, f32)>) -> Element {
    use whisker::css::PositionKind;
    NativeAppearanceObserver::builder()
        .style(css!(position: PositionKind::Absolute, width: px(0), height: px(0)))
        .on_change(move |e: AppearanceEvent| on_change.run(e.detail.dark > 0.5))
        .on_layout(move |e: LayoutEvent| on_layout.run((e.detail.wide > 0.5, e.detail.width as f32)))
        .build()
}

#[cfg(not(target_os = "ios"))]
#[component]
pub fn appearance_observer(on_change: Callback<bool>, on_layout: Callback<(bool, f32)>) -> Element {
    on_change.run(false);
    on_layout.run((false, 0.0));
    render! { View() }
}

/// 上の帯のボタンが押されたときの知らせ。`detail.which` は "back" / "menu" / "action"。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct NavPressEvent {
    #[serde(default)]
    pub detail: NavPressDetail,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct NavPressDetail {
    #[serde(default)]
    pub which: String,
}

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:NavBar", measurement = None)]
pub fn native_nav_bar(
    title: Signal<String>,
    back: Signal<String>,
    menu: Signal<bool>,
    menu_icon: Signal<String>,
    action: Signal<String>,
    style: Style,
    on_press: NavPressEvent,
) {
}

/// 画面の上の帯。iOS 標準の UINavigationBar（設定アプリなどと同じ帯）。iOS 専用。
///
/// - `title` … 真ん中の題名（空なら出さない）
/// - `back` … 戻る先の名前。空でなければ左に iOS の「‹ 名前」の戻るボタンを出す
/// - `menu` … true なら左に「☰」を出す（戻るボタンがあるときは出さない）
/// - `action` … 右のボタンの文字（空なら出さない）
/// - `on_press` … ボタンが押されたら "back" / "menu" / "action" で呼ばれる
#[cfg(target_os = "ios")]
#[component]
pub fn nav_bar(
    title: String,
    back: String,
    menu: Signal<bool>,
    menu_icon: Signal<String>,
    action: String,
    on_press: Callback<String>,
    style: Style,
) -> Element {
    NativeNavBar::builder()
        .title(title.clone())
        .back(back.clone())
        .menu(menu)
        .menu_icon(menu_icon)
        .action(action.clone())
        .style(style.clone())
        .on_press(move |e: NavPressEvent| on_press.run(e.detail.which))
        .build()
}

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:Sheet", measurement = None)]
pub fn native_sheet(
    open: Signal<bool>,
    title: Signal<String>,
    sections: Signal<String>,
    search: Signal<String>,
    style: Style,
    on_select: MenuSelectEvent,
    on_dismiss: (),
) {
}

/// 下から出る iOS 標準のシート。中身は設定アプリ風の一覧。iOS 専用。
///
/// - `open` … true の間シートを出す。false にすると閉じる
/// - `title` … シートの上の題（中身を差し替えるときは題も変えられる）
/// - `sections` … 一覧の JSON（形は `MenuList` と同じ）
/// - `search` … 検索欄の「例」の文字（空なら検索欄を出さない）
/// - `on_select` … 行が押されたら、その行の `key` で呼ばれる（閉じるのは呼ぶ側が `open` で）
/// - `on_dismiss` … 利用者が引っ張る・「閉じる」で閉じたとき
#[cfg(target_os = "ios")]
#[component]
pub fn sheet(
    open: Signal<bool>,
    title: Signal<String>,
    sections: Signal<String>,
    search: Signal<String>,
    on_select: Callback<String>,
    on_dismiss: Callback<()>,
) -> Element {
    use whisker::css::PositionKind;
    NativeSheet::builder()
        .open(open)
        .title(title)
        .sections(sections)
        .search(search)
        .style(css!(position: PositionKind::Absolute, width: px(0), height: px(0)))
        .on_select(move |e: MenuSelectEvent| on_select.run(e.detail.key))
        .on_dismiss(move || on_dismiss.run(()))
        .build()
}

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:Backdrop", measurement = None)]
pub fn native_backdrop(style: Style) {}

/// ログイン画面の背景。淡い色の光の玉がゆっくり漂う絵。上にガラスを重ねて使う。
#[cfg(target_os = "ios")]
#[component]
pub fn backdrop(style: Style) -> Element {
    NativeBackdrop::builder().style(style.clone()).build()
}

/// ログイン画面の背景。iOS 以外は、淡い青から紫への色の移り変わりで代わりにする。
#[cfg(not(target_os = "ios"))]
#[component]
pub fn backdrop(style: Style) -> Element {
    use whisker::css::{Color, ColorStop, Gradient, PositionKind};
    let outer = style.clone();
    let painted = css!(
        position: PositionKind::Absolute,
        top: px(0),
        bottom: px(0),
        left: px(0),
        right: px(0),
    )
    .background_image(Gradient::linear_to_bottom([
        ColorStop::new(Color::hex(0xD6E8FF)),
        ColorStop::new(Color::hex(0xF4F6FB)),
        ColorStop::new(Color::hex(0xE9DEFF)),
    ]));
    render! {
        View(style: outer) {
            View(style: painted)
        }
    }
}

/// 選ばれたファイル 1 つ。`path` は一時フォルダに書き出された写し。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct PickedFile {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "type")]
    pub content_type: String,
    #[serde(default)]
    pub size: f64,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct AttachPickEvent {
    #[serde(default)]
    pub detail: PickedFile,
}

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:AttachButton", measurement = None)]
pub fn native_attach_button(style: Style, on_pick: AttachPickEvent) {}

/// 「＋」ボタン。押すと「写真」「カメラ」「ファイル」を選ぶメニューが出て、
/// 選ばれたファイルごとに `on_pick` が呼ばれる。iOS 以外は何も出さない。
#[cfg(target_os = "ios")]
#[component]
pub fn attach_button(on_pick: Callback<PickedFile>, style: Style) -> Element {
    NativeAttachButton::builder()
        .style(style.clone())
        .on_pick(move |e: AttachPickEvent| on_pick.run(e.detail))
        .build()
}

#[cfg(not(target_os = "ios"))]
#[component]
pub fn attach_button(on_pick: Callback<PickedFile>, style: Style) -> Element {
    let _ = (&on_pick, &style);
    render! { View() }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct PrintDoneEvent {
    #[serde(default)]
    pub detail: PrintDoneDetail,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct PrintDoneDetail {
    #[serde(default)]
    pub ok: bool,
}

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:PrintButton", measurement = None)]
pub fn native_print_button(
    label: Signal<String>,
    html: Signal<String>,
    name: Signal<String>,
    landscape: Signal<bool>,
    style: Style,
    on_done: PrintDoneEvent,
) {
}

/// 「印刷・PDF」ボタン。押すと `html` を A4 に組んで iOS の印刷画面を出す（PDF 保存・共有もできる）。
/// `html` が空のときは押せない。iOS 以外は何も出さない。
#[cfg(target_os = "ios")]
#[component]
pub fn print_button(
    label: String,
    html: Signal<String>,
    name: Signal<String>,
    landscape: Signal<bool>,
    style: Style,
) -> Element {
    NativePrintButton::builder()
        .label(label.clone())
        .html(html)
        .name(name)
        .landscape(landscape)
        .style(style.clone())
        .on_done(|_e: PrintDoneEvent| {})
        .build()
}

#[cfg(not(target_os = "ios"))]
#[component]
pub fn print_button(
    label: String,
    html: Signal<String>,
    name: Signal<String>,
    landscape: Signal<bool>,
    style: Style,
) -> Element {
    let _ = (&label, &html, &name, &landscape, &style);
    render! { View() }
}

#[doc(hidden)]
#[whisker::module_element(name = "field-glass:DocumentPreview", measurement = None)]
pub fn native_document_preview(url: Signal<String>, style: Style) {}

/// 証憑の原本（写真・PDF）を出す枠。`url` が https のときだけ読む。iOS 以外は何も出さない。
#[cfg(target_os = "ios")]
#[component]
pub fn document_preview(url: Signal<String>, style: Style) -> Element {
    NativeDocumentPreview::builder()
        .url(url)
        .style(style.clone())
        .build()
}

#[cfg(not(target_os = "ios"))]
#[component]
pub fn document_preview(url: Signal<String>, style: Style) -> Element {
    let _ = (&url, &style);
    render! { View() }
}
