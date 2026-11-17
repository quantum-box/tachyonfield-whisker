//! 戦略「思想と行動」の画面。会社の MVV から目標・KR・施策・行動までを 1 本のツリーで見せる。
//!
//! Web 版（戦略 → 思想と行動）と同じ見た目の決まり（段ごとの色、事業部のタブ、つなぎ線、
//! 状態の色分け）で、スマホの幅に合わせて 1 行を 2〜3 段に積む。読み取りだけで、編集はしない。
//! 行を押すと詳細を重ねて開き、右へ引くとツリーへ戻る。

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::*;
use crate::strategy::{self as model, Counts, LinkTone, Row, Tone, Workspace};

/// 字下げ 1 段の幅。縦のつなぎ線もこの幅で引く。
const INDENT: f32 = 14.0;
/// 行の左の余白。
const ROW_LEFT: f32 = 10.0;
/// ▼ のボタンの幅。
const TOGGLE: f32 = 24.0;

#[derive(Clone, Copy)]
struct Tree {
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    /// 事業部のタブで選んでいる組織。空なら全社。
    department: RwSignal<String>,
    workspace: RwSignal<Option<Workspace>>,
    /// タブに出す組織。絞り込んだ応答で減っても、タブが消えないよう最初に読んだものを残す。
    organizations: RwSignal<Vec<model::Organization>>,
    counts: RwSignal<HashMap<String, Counts>>,
    collapsed: RwSignal<HashSet<String>>,
    loading: RwSignal<bool>,
    error: RwSignal<String>,
    generation: RwSignal<u64>,
}

impl Tree {
    fn load(self) {
        self.fetch(false);
    }

    /// 引っ張って更新。読み込み中なら新しく頼まず、今の読み込みが終わるのを待つ。
    /// 読めなかったときは、今出しているツリーを残したままエラーを出す。
    fn refresh(self) {
        if !self.loading.get_untracked() {
            self.fetch(true);
        }
    }

    fn fetch(self, keep_on_error: bool) {
        let Some(current) = self.tenant.get_untracked() else {
            return;
        };
        self.generation.update(|g| *g += 1);
        let generation = self.generation.get_untracked();
        self.loading.set(true);
        self.error.set(String::new());
        let department = self.department.get_untracked();
        spawn_local(async move {
            let for_progress = (current.clone(), department.clone());
            let result = authed(self.token, move |access| {
                api::strategy_workspace(access, &current, &department)
            })
            .await;
            if self.generation.get_untracked() != generation {
                return;
            }
            self.loading.set(false);
            match result {
                Ok(workspace) => {
                    if self.organizations.with_untracked(Vec::is_empty)
                        || !workspace.organizations.is_empty()
                    {
                        self.organizations.set(workspace.organizations.clone());
                    }
                    self.workspace.set(Some(workspace));
                }
                Err(message) => {
                    if !keep_on_error {
                        self.workspace.set(None);
                    }
                    self.error.set(message);
                    return;
                }
            }
            // 行動の数は別に読む。読めなくてもツリーはそのまま出す。
            let (tenant, department) = for_progress;
            let progress = authed(self.token, move |access| {
                api::strategy_progress(access, &tenant, &department)
            })
            .await;
            if self.generation.get_untracked() != generation {
                return;
            }
            self.counts
                .set(progress.map(|p| model::goal_counts(&p)).unwrap_or_default());
        });
    }

    fn toggle(self, id: &str) {
        self.collapsed.update(|set| {
            if !set.remove(id) {
                set.insert(id.to_owned());
            }
        });
    }
}

#[component]
pub(crate) fn strategy_screen(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
) -> Element {
    // 前回選んでいた事業部と、閉じていた行から始める（同じ会社のときだけ）。
    let last = tenant
        .get_untracked()
        .and_then(|current| cache::last_screen(&current))
        .unwrap_or_default();
    let tree = Tree {
        token,
        tenant,
        department: signal(last.department),
        workspace: signal(None),
        organizations: signal(Vec::new()),
        counts: signal(HashMap::new()),
        collapsed: signal(last.collapsed.into_iter().collect()),
        loading: signal(true),
        error: signal(String::new()),
        generation: signal(0),
    };
    tree.load();
    effect(move || {
        let department = tree.department.get();
        let mut collapsed: Vec<String> = tree.collapsed.with(|set| set.iter().cloned().collect());
        collapsed.sort();
        if let Some(current) = tenant.get_untracked() {
            cache::update_last_screen(&current, |last| {
                last.department = department;
                last.collapsed = collapsed;
            });
        }
    });
    let selected = signal(None::<Value>);
    let rows = computed(move || {
        let collapsed = tree.collapsed.get();
        let counts = tree.counts.get();
        tree.workspace
            .with(|ws| ws.as_ref().map(|ws| model::rows(ws, &collapsed, &counts)))
            .unwrap_or_default()
    });

    // 詳細は一覧の上に重ねて出す（機能の一覧と同じ動き）。右へ引くと、下からツリーが見えてくる。
    let back = use_context::<BackSwipe>();
    let dragging = computed(move || back.is_some_and(|b| b.drag.get().is_some()));
    let behind = computed(move || {
        if selected.with(Option::is_none) {
            return None;
        }
        let dx = back.and_then(|b| b.drag.get()).unwrap_or(0.0);
        Some((dx / BEHIND_TRAVEL).clamp(0.0, 1.0))
    });

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
                    if dragging.get() {
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
                NavBar(
                    title: "思想と行動",
                    back_label: "",
                    on_back: |_| {},
                    action_label: "",
                    on_action: |_| {},
                )
                ScrollView(style: scroll_style()) {
                    View(style: scroll_content_style()) {
                        RefreshControl(
                            refreshing: computed(move || tree.loading.get()),
                            on_refresh: move |_| tree.refresh(),
                        )
                        DivisionTabs(tree: tree)
                        Legend(tree: tree)
                        Show(when: move || tree.loading.get() && tree.workspace.with(Option::is_none)) {
                            Notice(message: "読み込み中…")
                        }
                        Show(when: move || !tree.error.with(String::is_empty)) {
                            ErrorNotice(message: computed(move || tree.error.get()))
                        }
                        Show(when: move || !tree.loading.get() && tree.workspace.with(Option::is_some) && rows.with(Vec::is_empty)) {
                            Notice(message: "この範囲に MVV・目標・行動はありません。")
                        }
                        Show(when: move || !rows.with(Vec::is_empty)) {
                            View(style: computed(move || group_style())) {
                                ForEach(
                                    each: move || {
                                        let dark = is_dark();
                                        rows.get().into_iter().map(|row| (row, dark)).collect::<Vec<_>>()
                                    },
                                    // 行内の固定色を再計算する。選択・開閉状態は Tree に保持する。
                                    key: |item: &(Row, bool)| format!("{}:{}:{}:{}", item.0.id, item.0.open, item.0.depth, item.1),
                                    children: move |item: ReadSignal<(Row, bool)>| {
                                        let row = item.get_untracked().0;
                                        let last = rows.with_untracked(|all| all.last().map(|r| r.id.clone()));
                                        let separator = last.as_deref() != Some(row.id.as_str());
                                        render! {
                                            TreeRow(row: row, separator: separator, tree: tree, selected: selected)
                                        }
                                    },
                                )
                            }
                        }
                        View(style: css!(height: px(TAB_BAR_SPACE)))
                    }
                }
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
                    ForEach(
                        each: move || selected.get().and_then(|v| v.as_str().map(str::to_owned)).into_iter().collect::<Vec<_>>(),
                        key: |id: &String| id.clone(),
                        children: move |id: ReadSignal<String>| render! {
                            StrategyDetail(tree: tree, id: id.get_untracked(), selected: selected)
                        },
                    )
                }
            }
        }
    }
}

/// 事業部で絞り込むタブ（全社＋事業部）。事業部が無い会社では出さない。
#[component]
fn division_tabs(tree: Tree) -> Element {
    let tabs = computed(move || {
        let divisions = tree.organizations.with(|orgs| model::divisions(orgs));
        if divisions.is_empty() {
            return Vec::new();
        }
        let mut tabs = vec![(String::new(), "全社".to_owned(), None)];
        tabs.extend(divisions.into_iter().map(|d| (d.id, d.name, Some(d.color))));
        tabs
    });
    render! {
        Show(when: move || !tabs.with(Vec::is_empty)) {
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    gap: px(6),
                    padding: (px(4), px(16), px(10), px(16)),
                ),
            ) {
                ForEach(
                    each: move || tabs.get(),
                    key: |t: &(String, String, Option<u32>)| t.0.clone(),
                    children: move |t: ReadSignal<(String, String, Option<u32>)>| {
                        let (id, name, color) = t.get_untracked();
                        let key = id.clone();
                        let on = computed(move || tree.department.get() == key);
                        render! {
                            View(
                                style: computed(move || {
                                    let s = css!(
                                        display: Display::Flex,
                                        flex_direction: FlexDirection::Row,
                                        align_items: AlignItems::Center,
                                        gap: px(6),
                                        padding: (px(6), px(12)),
                                        border_radius: px(999),
                                    );
                                    if on.get() {
                                        s.background_color(hex(LABEL))
                                    } else {
                                        s.background_color(hex(FILL))
                                    }
                                }),
                                on_tap: move |_| {
                                    if tree.department.get_untracked() != id {
                                        tree.department.set(id.clone());
                                        tree.collapsed.set(HashSet::new());
                                        tree.load();
                                    }
                                },
                            ) {
                                Show(when: move || color.is_some()) {
                                    View(
                                        style: computed(move || css!(
                                            width: px(8),
                                            height: px(8),
                                            border_radius: px(4),
                                            background_color: Color::hex(color.unwrap_or_default()),
                                        )),
                                    )
                                }
                                Text(
                                    value: name.clone(),
                                    style: computed(move || {
                                        css!(
                                            font_size: px(14),
                                            font_weight: FontWeight::Numeric(if on.get() { 600 } else { 400 }),
                                            color: if on.get() { hex(SYSTEM_BACKGROUND) } else { hex(LABEL) },
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
}

/// 段の凡例（MVV → 目標 → KR → 施策 → 行動）と、まとめて開閉するボタン。
#[component]
fn legend(tree: Tree) -> Element {
    let items: Vec<(usize, &'static str)> = model::LEGEND.iter().copied().enumerate().collect();
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                gap: px(8),
                padding: (px(0), px(16), px(10), px(16)),
            ),
        ) {
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    align_items: AlignItems::Center,
                    gap: px(4),
                ),
            ) {
                ForEach(
                    each: move || items.clone(),
                    key: |item: &(usize, &'static str)| item.0,
                    children: move |item: ReadSignal<(usize, &'static str)>| {
                        let (index, level) = item.get_untracked();
                        render! {
                            View(
                                style: css!(
                                    display: Display::Flex,
                                    flex_direction: FlexDirection::Row,
                                    align_items: AlignItems::Center,
                                    gap: px(4),
                                ),
                            ) {
                                Show(when: move || index > 0) {
                                    Text(value: "→", style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(12))))
                                }
                                LevelBadge(level: level)
                            }
                        }
                    },
                )
            }
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    gap: px(16),
                ),
            ) {
                Text(
                    value: "すべて開く",
                    style: computed(move || css!(color: hex(TINT), font_size: px(14))),
                    on_tap: move |_| tree.collapsed.set(HashSet::new()),
                )
                Text(
                    value: "目標まで閉じる",
                    style: computed(move || css!(color: hex(TINT), font_size: px(14))),
                    on_tap: move |_| {
                        let set = tree.workspace.with_untracked(|ws| {
                            ws.as_ref().map(model::collapse_to_objectives).unwrap_or_default()
                        });
                        tree.collapsed.set(set);
                    },
                )
            }
        }
    }
}

/// 段の札（MVV・目標など）。段ごとの色で塗る。
#[component]
fn level_badge(level: &'static str) -> Element {
    render! {
        View(style: computed(move || {
            let (bg, _, border) = model::level_colors(level, is_dark());
            let s = css!(padding: (px(2), px(5)), border_radius: px(4), background_color: Color::hex(bg), flex_shrink: 0.0);
            match border {
                Some(color) => s.border(Border::new().width(px(1)).color(Color::hex(color)).style(BorderStyle::Solid)),
                None => s,
            }
        })) {
            Text(
                value: level,
                max_lines: 1u32,
                style: computed(move || css!(color: Color::hex(model::level_colors(level, is_dark()).1), font_size: px(11), font_weight: FontWeight::Bold)),
            )
        }
    }
}

/// 色付きの小さな札（事業部・状態など）。
#[component]
fn pill(label: String, background: u32, color: u32, round: bool) -> Element {
    let label = signal(label.clone());
    render! {
        View(
            style: computed(move || css!(
                padding: (px(2), px(if round { 8 } else { 5 })),
                border_radius: px(if round { 999 } else { 4 }),
                background_color: Color::hex(background),
                flex_shrink: 0.0,
            )),
        ) {
            Text(
                value: computed(move || label.get()),
                max_lines: 1u32,
                style: computed(move || css!(color: Color::hex(color), font_size: px(11), font_weight: FontWeight::Numeric(600))),
            )
        }
    }
}

fn tone_pill_colors(tone: Tone) -> (u32, u32) {
    model::tone_colors(tone, is_dark())
}

/// 事業部・事業の札（番号, 名前, 地の色, 文字の色）。
type Chip = (usize, String, u32, u32);

/// ツリーの 1 行。左に親子をつなぐ縦線と ▼、段の札、名前。その下に、つながりの札と、責任者・状態・行動の数。
#[component]
fn tree_row(row: Row, separator: bool, tree: Tree, selected: RwSignal<Option<Value>>) -> Element {
    let dark = is_dark();
    let id = row.id.clone();
    let toggle_id = row.id.clone();
    let has_children = row.has_children;
    let open = row.open;
    let depth = row.depth;
    let (weight, size) = model::title_weight(row.level);
    let lines: Vec<usize> = (0..depth).collect();
    let chips: Vec<Chip> = {
        let mut chips = Vec::new();
        if let Some((name, color)) = &row.division_badge {
            chips.push((name.clone(), *color, 0xFFFFFF));
        }
        for (name, color) in &row.scope_names {
            let c = color.unwrap_or(if dark { 0xA1A1AA } else { 0x64748B });
            chips.push((name.clone(), if dark { 0x000000 } else { 0xFFFFFF }, c));
        }
        chips
            .into_iter()
            .enumerate()
            .map(|(i, (n, b, f))| (i, n, b, f))
            .collect()
    };
    let has_chips = !chips.is_empty();
    let links: Vec<(usize, String, LinkTone)> = row
        .links
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, (l, t))| (i, l, t))
        .collect();
    let notes: Vec<(usize, String)> = row.notes.iter().cloned().enumerate().collect();
    let has_links = !links.is_empty() || !notes.is_empty();
    let mut meta: Vec<(usize, String, u32, u32, bool)> = Vec::new();
    if let Some((status, tone)) = &row.status {
        let (bg, fg) = tone_pill_colors(*tone);
        meta.push((meta.len(), status.clone(), bg, fg, true));
    }
    if let Some(counts) = row.counts.filter(|c| c.held_up > 0) {
        let (bg, fg) = tone_pill_colors(Tone::Warn);
        meta.push((
            meta.len(),
            format!("止まり {}", counts.held_up),
            bg,
            fg,
            true,
        ));
    }
    let counts_text = row.counts.map(|c| {
        if c.total > 0 {
            format!("行動 {} 件", c.total)
        } else {
            "行動未設定".into()
        }
    });
    let owner = row.owner.clone().unwrap_or_default();
    let has_owner = !owner.is_empty();
    let has_meta = !meta.is_empty() || counts_text.is_some() || has_owner;
    let counts_text = counts_text.unwrap_or_default();
    let has_counts = !counts_text.is_empty();
    let stripe = row.stripe;
    let top = depth == 0;
    let title = row.title.clone();
    let (chips, links, notes, meta) = (signal(chips), signal(links), signal(notes), signal(meta));
    let (owner, counts_text, lines) = (signal(owner), signal(counts_text), signal(lines));

    render! {
        View(
            style: computed(move || {
                let s = css!(
                    position: PositionKind::Relative,
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::FlexStart,
                    gap: px(4),
                    padding: (px(8), px(12), px(8), px(ROW_LEFT + depth as f32 * INDENT)),
                );
                let s = if separator { s.border_bottom(hairline()) } else { s };
                let s = if top { s.background_color(Color::hex(if dark { 0x111113 } else { 0xF8FAFC })) } else { s };
                match stripe {
                    Some(color) => s.border_left(Border::new().width(px(4)).color(Color::hex(color)).style(BorderStyle::Solid)),
                    None => s,
                }
            }),
            on_tap: move |_| selected.set(Some(Value::String(id.clone()))),
        ) {
            // 親子をつなぐ縦の線。深さの数だけ、各段の ▼ の真ん中に引く。
            ForEach(
                each: move || lines.get(),
                key: |i: &usize| *i,
                children: move |i: ReadSignal<usize>| {
                    let i = i.get_untracked();
                    render! {
                        View(
                            style: computed(move || css!(
                                position: PositionKind::Absolute,
                                top: px(0),
                                bottom: px(0),
                                left: px(ROW_LEFT + TOGGLE / 2.0 + i as f32 * INDENT),
                                width: px(1),
                                background_color: hex(SEPARATOR),
                            )),
                        )
                    }
                },
            )
            View(
                style: css!(
                    width: px(TOGGLE),
                    height: px(TOGGLE),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    flex_shrink: 0.0,
                ),
                on_tap_catch: move |_| {
                    if has_children {
                        tree.toggle(&toggle_id);
                    }
                },
            ) {
                Show(when: move || has_children) {
                    Text(
                        value: if open { "▾" } else { "▸" },
                        style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(16))),
                    )
                }
            }
            View(style: css!(padding_top: px(3))) {
                LevelBadge(level: row.level)
            }
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    gap: px(4),
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                )
                .min_width(px(0)),
            ) {
                Show(when: move || has_chips) {
                    View(
                        style: css!(
                            display: Display::Flex,
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            gap: px(4),
                        ),
                    ) {
                        ForEach(
                            each: move || chips.get(),
                            key: |c: &Chip| c.0,
                            children: move |c: ReadSignal<Chip>| {
                                let (_, name, bg, fg) = c.get_untracked();
                                render! { Pill(label: name, background: bg, color: fg, round: false) }
                            },
                        )
                    }
                }
                Text(
                    value: title.clone(),
                    max_lines: 3u32,
                    style: computed(move || css!(
                        color: hex(LABEL),
                        font_size: px(size),
                        font_weight: FontWeight::Numeric(weight),
                    )
                    .text_overflow(TextOverflow::Ellipsis)),
                )
                Show(when: move || has_links) {
                    View(
                        style: css!(
                            display: Display::Flex,
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            gap: px(4),
                        ),
                    ) {
                        ForEach(
                            each: move || links.get(),
                            key: |l: &(usize, String, LinkTone)| l.0,
                            children: move |l: ReadSignal<(usize, String, LinkTone)>| {
                                let (_, label, tone) = l.get_untracked();
                                render! { LinkChip(label: label, tone: tone) }
                            },
                        )
                        ForEach(
                            each: move || notes.get(),
                            key: |n: &(usize, String)| n.0,
                            children: move |n: ReadSignal<(usize, String)>| {
                                let (_, note) = n.get_untracked();
                                let (bg, fg) = tone_pill_colors(Tone::Warn);
                                render! { Pill(label: format!("⚠ {note}"), background: bg, color: fg, round: false) }
                            },
                        )
                    }
                }
                Show(when: move || has_meta) {
                    View(
                        style: css!(
                            display: Display::Flex,
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            align_items: AlignItems::Center,
                            gap: px(6),
                        ),
                    ) {
                        Show(when: move || has_owner) {
                            Text(
                                value: computed(move || owner.get()),
                                max_lines: 1u32,
                                style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(12))),
                            )
                        }
                        ForEach(
                            each: move || meta.get(),
                            key: |m: &(usize, String, u32, u32, bool)| m.0,
                            children: move |m: ReadSignal<(usize, String, u32, u32, bool)>| {
                                let (_, label, bg, fg, round) = m.get_untracked();
                                render! { Pill(label: label, background: bg, color: fg, round: round) }
                            },
                        )
                        Show(when: move || has_counts) {
                            Text(
                                value: computed(move || counts_text.get()),
                                style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(12))),
                            )
                        }
                    }
                }
            }
        }
    }
}

/// 外のどこにつながっているかの札。種類ごとの色の印を付ける。
#[component]
fn link_chip(label: String, tone: LinkTone) -> Element {
    let (mark, color) = tone.mark();
    let label = signal(label.clone());
    render! {
        View(
            style: computed(move || css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                gap: px(4),
                padding: (px(2), px(5), px(2), px(3)),
                border_radius: px(4),
                border: Border::new().width(px(0.5)).color(hex(SEPARATOR)).style(BorderStyle::Solid),
                flex_shrink: 0.0,
            )),
        ) {
            View(
                style: computed(move || css!(
                    padding: (px(0), px(3)),
                    border_radius: px(2),
                    background_color: Color::hex(color),
                )),
            ) {
                Text(
                    value: mark,
                    style: computed(move || css!(color: Color::hex(0xFFFFFF), font_size: px(9), font_weight: FontWeight::Bold)),
                )
            }
            Text(value: computed(move || label.get()), max_lines: 1u32, style: computed(move || css!(color: hex(LABEL), font_size: px(11))))
        }
    }
}

// ---------------------------------------------------------------- 詳細

/// 1 項目の詳細。本文はツリーの応答に入っていないことがあるので、開いたあとで読み直す。
#[component]
fn strategy_detail(tree: Tree, id: String, selected: RwSignal<Option<Value>>) -> Element {
    let back = use_context::<BackSwipe>();
    if let Some(back) = back {
        back.target.set(Some(selected));
        on_cleanup(move || {
            back.target.set(None);
            back.drag.set(None);
        });
    }
    let node = signal(
        tree.workspace
            .with_untracked(|ws| ws.as_ref().and_then(|ws| ws.node(&id).cloned())),
    );
    let version = signal(0usize);
    if let Some(current) = tree.tenant.get_untracked() {
        let wanted = id.clone();
        spawn_local(async move {
            let lookup = wanted.clone();
            let result = authed(tree.token, move |access| {
                api::strategy_node(access, &current, &lookup)
            })
            .await;
            let Ok(value) = result else { return };
            let full = Workspace::from_value(&value);
            if let Some(fresh) = full.nodes.into_iter().find(|n| n.id == wanted) {
                // 読み直しに本文が無ければ、ツリーで読んだ方を残す。
                let keep = node.with_untracked(|old| {
                    old.as_ref()
                        .is_some_and(|o| fresh.content().is_none() && o.content().is_some())
                });
                if !keep {
                    node.set(Some(fresh));
                    version.update(|v| *v += 1);
                }
            }
        });
    }
    render! {
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
            NavBar(
                title: "",
                back_label: "思想と行動",
                on_back: move |_| selected.set(None),
                action_label: "",
                on_action: |_| {},
            )
            ForEach(
                each: move || vec![version.get()],
                key: |v: &usize| *v,
                children: move |_: ReadSignal<usize>| match node.get_untracked() {
                    Some(node) => render! { StrategyDetailBody(tree: tree, node: node, selected: selected) },
                    None => render! { Notice(message: "この項目は見つかりません") },
                },
            )
        }
    }
}

/// 詳細に出す項目（ラベルと値）。
fn detail_rows(node: &model::Node, ws: Option<&Workspace>) -> Vec<(String, String)> {
    let orgs = ws.map(|w| w.organizations.as_slice()).unwrap_or_default();
    let mut rows = vec![("段".to_owned(), model::level_of(node, orgs).to_owned())];
    rows.push(("状態".into(), model::status_of(node)));
    rows.push((
        "責任者".into(),
        node.owner_name().unwrap_or_else(|| "責任者が未定".into()),
    ));
    if let Some(name) = ws
        .and_then(|w| w.periods.iter().find(|(id, _)| *id == node.period_id))
        .map(|(_, n)| n.clone())
    {
        rows.push(("期間".into(), name));
    }
    if let Some(due) = node.raw["dueDate"]
        .as_str()
        .or_else(|| node.raw["external"]["target_date"].as_str())
    {
        rows.push(("期日".into(), due.chars().take(10).collect()));
    }
    let measurement = &node.raw["measurement"];
    if measurement.is_object() {
        let unit = measurement["unit"].as_str().unwrap_or_default();
        rows.push((
            "測定".into(),
            format!(
                "{} {unit} → {} {unit}",
                measurement["baseline"].as_str().unwrap_or("-"),
                measurement["target"].as_str().unwrap_or("-")
            ),
        ));
    }
    let scope: Vec<String> = node
        .department_ids
        .iter()
        .filter_map(|id| orgs.iter().find(|o| &o.id == id).map(|o| o.name.clone()))
        .collect();
    rows.push((
        "範囲".into(),
        if scope.is_empty() {
            "全社".into()
        } else {
            scope.join("、")
        },
    ));
    let links = model::link_chips(node);
    if !links.is_empty() {
        rows.push((
            "つながり".into(),
            links
                .into_iter()
                .map(|(l, _)| l)
                .collect::<Vec<_>>()
                .join("、"),
        ));
    }
    if let Some(name) = node.raw["external"]["name"].as_str() {
        rows.push(("外の名前".into(), name.to_owned()));
    }
    rows
}

#[component]
fn strategy_detail_body(
    tree: Tree,
    node: model::Node,
    selected: RwSignal<Option<Value>>,
) -> Element {
    let (rows, parents, children) = tree.workspace.with_untracked(|ws| {
        let ws = ws.as_ref();
        let pick = |list: Vec<&model::Node>| -> Vec<(String, String, &'static str)> {
            list.into_iter()
                .map(|n| (n.id.clone(), model::display_title(n), model::level(&n.kind)))
                .collect()
        };
        (
            detail_rows(&node, ws),
            ws.map(|w| pick(w.parents_of(&node.id))).unwrap_or_default(),
            ws.map(|w| pick(w.children_of(&node.id)))
                .unwrap_or_default(),
        )
    });
    let level = tree.workspace.with_untracked(|ws| {
        model::level_of(
            &node,
            ws.as_ref()
                .map(|w| w.organizations.as_slice())
                .unwrap_or_default(),
        )
    });
    let title = model::display_title(&node);
    let full_title = node.title.clone();
    let show_full_title = !full_title.trim().is_empty() && full_title != title;
    let body = node
        .content()
        .map(|text| model::strip_frontmatter(&text).trim().to_owned())
        .filter(|t| !t.is_empty())
        .map(|t| {
            if t.chars().count() > 4000 {
                format!("{}…", t.chars().take(4000).collect::<String>())
            } else {
                t
            }
        });
    let has_body = body.is_some();
    let body = signal(body.unwrap_or_default());
    let documents: Vec<(String, String)> = model::document_links(&node);
    let has_documents = !documents.is_empty();
    let has_parents = !parents.is_empty();
    let has_children = !children.is_empty();

    render! {
        ScrollView(style: scroll_style()) {
            View(style: scroll_content_style()) {
                View(
                    style: css!(
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::FlexStart,
                        gap: px(8),
                        padding: (px(8), px(20), px(20), px(20)),
                    ),
                ) {
                    LevelBadge(level: level)
                    Text(
                        value: title.clone(),
                        style: computed(move || css!(color: hex(LABEL), font_size: px(22), font_weight: FontWeight::Numeric(700))),
                    )
                    Show(when: move || show_full_title) {
                        Text(value: full_title.clone(), style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(14))))
                    }
                }
                InfoGroup(rows: rows.clone(), inline: false)
                Show(when: move || has_documents) {
                    SectionHeader(title: computed(|| "つないだ文書".to_owned()))
                    InfoGroup(rows: documents.clone(), inline: false)
                }
                Show(when: move || has_parents) {
                    SectionHeader(title: computed(|| "上の段".to_owned()))
                    RelatedGroup(items: parents.clone(), selected: selected)
                }
                Show(when: move || has_children) {
                    SectionHeader(title: computed(|| "下の段".to_owned()))
                    RelatedGroup(items: children.clone(), selected: selected)
                }
                Show(when: move || has_body) {
                    SectionHeader(title: computed(|| "本文".to_owned()))
                    View(style: computed(move || group_style())) {
                        Text(
                            value: computed(move || body.get()),
                            style: computed(move || css!(color: hex(LABEL), font_size: px(15), padding: (px(12), px(16)))),
                        )
                    }
                }
                View(style: css!(height: px(TAB_BAR_SPACE)))
            }
        }
    }
}

/// 上・下の段の一覧。押すとその項目の詳細に移る。
#[component]
fn related_group(
    items: Vec<(String, String, &'static str)>,
    selected: RwSignal<Option<Value>>,
) -> Element {
    let last = signal(items.last().map(|i| i.0.clone()));
    let items = signal(items.clone());
    render! {
        View(style: computed(move || group_style())) {
            ForEach(
                each: move || items.get(),
                key: |i: &(String, String, &'static str)| i.0.clone(),
                children: move |i: ReadSignal<(String, String, &'static str)>| {
                    let (id, title, level) = i.get_untracked();
                    let separator = last.with_untracked(|l| l.as_deref() != Some(id.as_str()));
                    render! {
                        ListRow(
                            title: title,
                            subtitle: level.to_owned(),
                            subtitle_color: SECONDARY_LABEL,
                            trailing: String::new(),
                            avatar: false,
                            separator: computed(move || separator),
                            on_tap: move |_| selected.set(Some(Value::String(id.clone()))),
                        )
                    }
                },
            )
        }
    }
}
