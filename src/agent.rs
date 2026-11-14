//! Field Agent（AI に話しかける画面）。
//!
//! 頼みごとを送ると、返事の文と操作の経過が少しずつ届く（SSE）。Field の操作（道具）は
//! サーバー側で実行されるので、この画面は「送る・届いたものを並べる」だけ。
//! 自分の発言は右の青い吹き出し、Agent の返事は吹き出しなしの文、操作は小さな 1 行で出す。
//! 会話はホームが持っているので、タブを切り替えて戻っても消えない。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::StreamExt;
use whisker::runtime::view::observe_layout;
use whisker::{ElementHandle, ScrollViewHandle};

use super::*;

/// Agent の印（✦）の色。Apple Intelligence の紫寄りの色。
const PURPLE: u32 = 0x8E5CF7;
use api::AgentEvent;

/// 会話の 1 行の種類。
#[derive(Clone, Copy, PartialEq)]
enum Role {
    User,
    Assistant,
    /// Agent が使った操作。
    Tool(ToolState),
    /// 失敗や中止の知らせ。
    Notice,
    /// 返事が終わったあと、その回の操作をまとめた 1 行（押すと開く）。
    Summary,
}

#[derive(Clone, Copy, PartialEq)]
enum ToolState {
    Running,
    Done,
    Failed,
}

/// 会話の 1 行。
#[derive(Clone, PartialEq)]
pub struct ChatMessage {
    id: usize,
    role: Role,
    text: String,
    /// 操作の行だけ：サーバーが付けた操作の ID。
    tool_id: String,
    /// 何回目のやりとりか（送るたびに 1 増える）。
    turn: usize,
    /// 操作の行だけ：返事が終わってまとめ行にたたまれたか。
    folded: bool,
}

/// 会話の中身。ホームが 1 つ持ち、Agent 画面に渡す。
#[derive(Clone, Copy)]
pub struct AgentChat {
    messages: RwSignal<Vec<ChatMessage>>,
    /// 続きを送るための会話 ID。新しい会話では None。
    session: RwSignal<Option<String>>,
    sending: RwSignal<bool>,
    next_id: RwSignal<usize>,
    turn: RwSignal<usize>,
    /// まとめ行を押して開いている回。
    expanded: RwSignal<Vec<usize>>,
    /// 今流れている返事を止める旗。
    cancel: RwSignal<Option<Arc<AtomicBool>>>,
    /// 返事を受ける仕事の持ち主（ホーム）。画面の一部（案内やタブ）が消えても止まらないように。
    owner: Option<whisker::Owner>,
}

impl AgentChat {
    pub fn new() -> Self {
        Self {
            messages: signal(Vec::new()),
            session: signal(None),
            sending: signal(false),
            next_id: signal(0),
            turn: signal(0),
            expanded: signal(Vec::new()),
            cancel: signal(None),
            owner: whisker::Owner::current(),
        }
    }

    fn push(&self, role: Role, text: String, tool_id: String) -> usize {
        let id = self.next_id.get_untracked();
        self.next_id.set(id + 1);
        let turn = self.turn.get_untracked();
        self.messages.update(|list| {
            list.push(ChatMessage {
                id,
                role,
                text,
                tool_id,
                turn,
                folded: false,
            })
        });
        id
    }

    /// 返事が終わったら、その回の操作の行を「3 件の操作」の 1 行にたたむ。
    fn fold_turn(&self) {
        let turn = self.turn.get_untracked();
        let id = self.next_id.get_untracked();
        let mut placed = false;
        self.messages.update(|list| {
            let tools: Vec<usize> = list
                .iter()
                .enumerate()
                .filter(|(_, m)| m.turn == turn && matches!(m.role, Role::Tool(_)))
                .map(|(i, _)| i)
                .collect();
            let Some(&first) = tools.first() else {
                return;
            };
            let failed = tools
                .iter()
                .filter(|&&i| list[i].role == Role::Tool(ToolState::Failed))
                .count();
            for &i in &tools {
                list[i].folded = true;
            }
            let text = if failed > 0 {
                format!("{} 件の操作（{failed} 件失敗）", tools.len())
            } else {
                format!("{} 件の操作", tools.len())
            };
            list.insert(
                first,
                ChatMessage {
                    id,
                    role: Role::Summary,
                    text,
                    tool_id: String::new(),
                    turn,
                    folded: false,
                },
            );
            placed = true;
        });
        if placed {
            self.next_id.set(id + 1);
        }
    }

    fn append(&self, id: usize, fragment: &str) {
        self.messages.update(|list| {
            if let Some(m) = list.iter_mut().find(|m| m.id == id) {
                m.text.push_str(fragment);
            }
        });
    }

    fn finish_tool(&self, tool_id: &str, label: String, ok: bool) {
        self.messages.update(|list| {
            if let Some(m) = list.iter_mut().rev().find(|m| m.tool_id == tool_id) {
                m.role = Role::Tool(if ok {
                    ToolState::Done
                } else {
                    ToolState::Failed
                });
                m.text = label;
            }
        });
    }

    /// 止めたときなどに、動いたままの操作の印を「終わり」にする。
    fn settle_tools(&self) {
        self.messages.update(|list| {
            for m in list.iter_mut() {
                if m.role == Role::Tool(ToolState::Running) {
                    m.role = Role::Tool(ToolState::Done);
                }
            }
        });
    }

    fn stop(&self) {
        if let Some(flag) = self.cancel.get_untracked() {
            flag.store(true, Ordering::Relaxed);
        }
        self.cancel.set(None);
        if self.sending.get_untracked() {
            self.sending.set(false);
            self.settle_tools();
            self.fold_turn();
            self.push(Role::Notice, "止めました".into(), String::new());
        }
    }
}

#[component]
pub fn agent_screen(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    chat: AgentChat,
) -> Element {
    let draft = signal(String::new());
    let input = InputRef::new();
    // 裏のメニューを開いたら、キーボードを閉じる（開いたままだとメニューが隠れる）。
    if let Some(Drawer(open)) = use_context::<Drawer>() {
        effect(move || {
            if open.get() {
                input.blur();
            }
        });
    }
    // キーボードの高さ（画面の下端から）。入力欄をその上へ持ち上げる。
    let keyboard = signal(0.0_f32);
    let scroller = ScrollViewHandle::new();
    let content = ElementHandle::new();
    let messages = chat.messages;
    let sending = chat.sending;

    // 会話の高さと見えている高さを測っておき、増えたら一番下まで送る。
    let content_height = signal(0.0_f64);
    let viewport_height = signal(0.0_f64);
    on_mount(move || {
        if let Some(element) = content.r().element() {
            observe_layout(
                element,
                Box::new(move |l| content_height.set(f64::from(l.geometry.border_box.height))),
            );
        }
        if let Some(element) = scroller.r().element() {
            observe_layout(
                element,
                Box::new(move |l| viewport_height.set(f64::from(l.geometry.border_box.height))),
            );
        }
    });
    effect(move || {
        let bottom = (content_height.get() - viewport_height.get()).max(0.0);
        scroller.scroll_to(bottom, false);
    });

    let send = move || {
        let task = draft.get_untracked().trim().to_owned();
        if task.is_empty() || sending.get_untracked() {
            return;
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        draft.set(String::new());
        chat.turn.set(chat.turn.get_untracked() + 1);
        chat.push(Role::User, task.clone(), String::new());
        sending.set(true);
        let cancel = Arc::new(AtomicBool::new(false));
        chat.cancel.set(Some(cancel.clone()));

        let run = async move {
            // 会話がまだなければ作る（期限切れのトークンはここで取り直す）。
            let session = match chat.session.get_untracked() {
                Some(id) => id,
                None => {
                    let for_session = current.clone();
                    match authed(token, move |access| {
                        api::agent_session(access, &for_session)
                    })
                    .await
                    {
                        Ok(id) => {
                            // 止めたあとに届いた会話 ID で、次の会話の ID を上書きしない。
                            if cancel.load(Ordering::Relaxed) {
                                return;
                            }
                            chat.session.set(Some(id.clone()));
                            id
                        }
                        Err(message) => {
                            if !cancel.load(Ordering::Relaxed) {
                                sending.set(false);
                                chat.push(Role::Notice, message, String::new());
                            }
                            return;
                        }
                    }
                }
            };
            if cancel.load(Ordering::Relaxed) {
                return;
            }

            // 読むのは別スレッド。届いたものを順に画面へ渡す。
            let (sender, mut events) = futures_channel::mpsc::unbounded::<AgentEvent>();
            let access = token.get_untracked().unwrap_or_default();
            let flag = cancel.clone();
            std::thread::spawn(move || {
                let mut result =
                    api::agent_stream(&access, &current, &session, &task, &flag, |event| {
                        let _ = sender.unbounded_send(event);
                    });
                // トークンが切れていたら取り直して 1 回だけ送り直す。期限切れは送った直後
                // （まだ何も流れていない時）に返るので、同じ頼みを二重に流すことはない。
                if matches!(&result, Err(e) if e == api::EXPIRED) {
                    if let Some(refresh_token) = storage::refresh_token() {
                        if let Ok(renewed) = api::refresh(&refresh_token) {
                            if let Some(rotated) = &renewed.refresh_token {
                                storage::save_refresh_token(rotated);
                            }
                            let _ = sender
                                .unbounded_send(AgentEvent::Renewed(renewed.access_token.clone()));
                            // 取り直している間に止められていたら、送り直さない。
                            if flag.load(Ordering::Relaxed) {
                                return;
                            }
                            result = api::agent_stream(
                                &renewed.access_token,
                                &current,
                                &session,
                                &task,
                                &flag,
                                |event| {
                                    let _ = sender.unbounded_send(event);
                                },
                            );
                        }
                    }
                }
                if let Err(message) = result {
                    let _ = sender.unbounded_send(AgentEvent::Failed(message));
                }
            });

            // 今書き足している返事の段落と、その種類（種類が変わったら次の段落）。
            let mut paragraph: Option<(usize, String)> = None;
            let mut raw = String::new();
            let mut shown = String::new();
            while let Some(event) = events.next().await {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                match event {
                    AgentEvent::Text { kind, text } => {
                        let id = match &paragraph {
                            Some((id, k)) if *k == kind => *id,
                            _ => {
                                raw.clear();
                                let id = chat.push(Role::Assistant, String::new(), String::new());
                                paragraph = Some((id, kind));
                                id
                            }
                        };
                        raw.push_str(&text);
                        shown.push_str(&text);
                        let display = raw.clone();
                        messages.update(|list| {
                            if let Some(m) = list.iter_mut().find(|m| m.id == id) {
                                m.text = display;
                            }
                        });
                    }
                    AgentEvent::ToolStart { id, label } => {
                        paragraph = None;
                        chat.push(Role::Tool(ToolState::Running), label, id);
                    }
                    AgentEvent::ToolDone { id, label, ok } => {
                        paragraph = None;
                        if messages.with_untracked(|list| list.iter().any(|m| m.tool_id == id)) {
                            chat.finish_tool(&id, label, ok);
                        } else {
                            let state = if ok {
                                ToolState::Done
                            } else {
                                ToolState::Failed
                            };
                            chat.push(Role::Tool(state), label, id);
                        }
                    }
                    AgentEvent::Completion(result) => {
                        if !shown.contains(result.trim()) {
                            paragraph = None;
                            let id = chat.push(Role::Assistant, String::new(), String::new());
                            chat.append(id, &result);
                            shown.push_str(&result);
                        }
                    }
                    AgentEvent::Failed(message) => {
                        chat.push(Role::Notice, message, String::new());
                    }
                    AgentEvent::Renewed(access) => token.set(Some(access)),
                }
            }
            if !cancel.load(Ordering::Relaxed) {
                chat.settle_tools();
                chat.fold_turn();
                chat.cancel.set(None);
                sending.set(false);
            }
        };
        match chat.owner {
            Some(owner) => owner.with(|| spawn_local(run)),
            None => spawn_local(run),
        }
    };

    // 考え中の印：送ったあと、まだ文が流れ始めていない（または操作が終わって次を待つ）間。
    let thinking = computed(move || {
        sending.get()
            && messages.with(|list| {
                !matches!(
                    list.last().map(|m| m.role),
                    Some(Role::Assistant) | Some(Role::Tool(ToolState::Running))
                )
            })
    });

    let insets = safe_area_insets();
    let empty = computed(move || messages.with(Vec::is_empty));

    render! {
        View(style: screen_style().position(PositionKind::Relative)) {
            // 会話を始める前だけ、ログイン画面と同じ淡い光を薄く敷く（Apple Intelligence の画面の雰囲気）。
            Show(when: move || empty.get()) {
                View(
                    style: computed(move || {
                        css!(
                            position: PositionKind::Absolute,
                            top: px(-(insets.get().top as f32)),
                            bottom: px(0),
                            left: px(0),
                            right: px(0),
                            opacity: 0.6,
                        )
                    }),
                ) {
                    Backdrop(style: glass_fill())
                }
            }
            NavBar(
                title: "Field Agent",
                back_label: "",
                on_back: |_| {},
                action_label: "新しい会話",
                on_action: move |_| {
                    chat.stop();
                    messages.set(Vec::new());
                    chat.expanded.set(Vec::new());
                    chat.session.set(None);
                },
            )
            KeyboardObserver(on_change: move |height: f32| keyboard.set(height))
            ScrollView(style: scroll_style(), element_ref: scroller.r()) {
                View(
                    element_ref: content.r(),
                    style: computed(move || {
                        // 会話が空のときは、案内を画面の真ん中に置く。
                        let base = scroll_content_style()
                            .padding((px(8), px(20), px(20), px(20)))
                            .gap(px(14));
                        if empty.get() {
                            base.flex_grow(1.0).justify_content(JustifyContent::Center)
                        } else {
                            base
                        }
                    }),
                ) {
                    Show(when: move || empty.get()) {
                        AgentWelcome(
                            on_pick: move |text: String| {
                                draft.set(text);
                                send();
                            },
                        )
                    }
                    ForEach(
                        // たたんだ操作の行は、まとめ行を開いているときだけ出す。
                        each: move || {
                            let open = chat.expanded.get();
                            messages.with(|list| {
                                list.iter()
                                    .filter(|m| !m.folded || open.contains(&m.turn))
                                    .cloned()
                                    .collect::<Vec<_>>()
                            })
                        },
                        key: |m: &ChatMessage| m.id,
                        children: move |m: ReadSignal<ChatMessage>| {
                            let role = m.get_untracked().role;
                            let text = computed(move || m.get().text);
                            match role {
                                Role::User => render! { UserBubble(text: text) },
                                Role::Assistant => render! { AssistantText(text: text) },
                                Role::Tool(_) => render! {
                                    ToolLine(text: text, state: computed(move || m.get().role))
                                },
                                Role::Notice => render! { NoticeLine(text: text) },
                                Role::Summary => {
                                    let turn = m.get_untracked().turn;
                                    let expanded = chat.expanded;
                                    render! {
                                        SummaryLine(
                                            text: text,
                                            open: computed(move || expanded.with(|e| e.contains(&turn))),
                                            on_tap: move |_| {
                                                expanded.update(|e| {
                                                    if let Some(i) = e.iter().position(|t| *t == turn) {
                                                        e.remove(i);
                                                    } else {
                                                        e.push(turn);
                                                    }
                                                })
                                            },
                                        )
                                    }
                                }
                            }
                        },
                    )
                    Show(when: move || thinking.get()) {
                        View(
                            style: css!(
                                display: Display::Flex,
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                gap: px(6),
                            ),
                        ) {
                            Text(value: "✦", style: css!(color: hex(PURPLE), font_size: px(15)))
                            Text(
                                value: "考えています…",
                                style: css!(color: hex(SECONDARY_LABEL), font_size: px(15)),
                            )
                        }
                    }
                }
            }
            Composer(
                draft: draft,
                input: input,
                sending: sending,
                on_send: move |_| send(),
                on_stop: move |_| chat.stop(),
            )
            // 下に浮かぶタブ、またはキーボードの分だけあける。
            View(
                style: computed(move || {
                    let k = keyboard.get();
                    css!(height: px(if k > 0.0 { k } else { TAB_BAR_SPACE - 14.0 }))
                }),
            )
        }
    }
}

/// 入力欄：丸い枠の中に、文字と送るボタン。返事を受けている間は ■（止める）。
#[component]
fn composer(
    draft: RwSignal<String>,
    input: InputRef,
    sending: RwSignal<bool>,
    on_send: Callback<()>,
    on_stop: Callback<()>,
) -> Element {
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                padding: (px(6), px(12), px(8), px(12)),
            ),
        ) {
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    gap: px(6),
                    min_height: px(50),
                    padding: (px(5), px(7), px(5), px(18)),
                    position: PositionKind::Relative,
                ),
            ) {
                // ガラスの丸い枠（iOS 26 のメッセージ入力欄の形）。
                Glass(corner_radius: 25.0_f32, style: glass_fill())
                Input(
                    text: draft,
                    placeholder: "Field Agent に頼む",
                    return_key: ReturnKey::Send,
                    input_ref: input,
                    on_submit: move |_| on_send.run(()),
                    style: css!(
                        flex_grow: 1.0,
                        height: px(40),
                        font_size: px(17),
                        color: hex(LABEL),
                        background_color: Color::rgba(0, 0, 0, 0.0),
                    ),
                )
                View(
                    style: computed(move || {
                        let busy = sending.get();
                        let ready = busy || !draft.with(|d| d.trim().is_empty());
                        css!(
                            width: px(36),
                            height: px(36),
                            border_radius: px(18),
                            display: Display::Flex,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            background_color: hex(match (busy, ready) {
                                (true, _) => LABEL,
                                (false, true) => TINT,
                                _ => DISABLED,
                            }),
                        )
                    }),
                    on_tap: move |_| {
                        if sending.get_untracked() {
                            on_stop.run(());
                        } else {
                            on_send.run(());
                        }
                    },
                ) {
                    Text(
                        value: computed(move || if sending.get() { "■" } else { "↑" }.to_owned()),
                        style: computed(move || {
                            let busy = sending.get();
                            css!(
                                // 止めるボタンは文字色の丸。暗い表示では白い丸になるので、■ は黒にする。
                                color: Color::hex(if busy && is_dark() { 0x000000 } else { 0xFFFFFF }),
                                font_size: px(if busy { 13.0 } else { 19.0 }),
                                font_weight: FontWeight::Bold,
                            )
                        }),
                    )
                }
            }
        }
    }
}

/// 会話が空のときの案内と、すぐ送れる例。真ん中寄せで、例はガラスの丸い札。
#[component]
fn agent_welcome(on_pick: Callback<String>) -> Element {
    const EXAMPLES: &[&str] = &[
        "取引先は何件ある？",
        "今月の請求書で未入金のものを教えて",
        "最近の受注を 5 件見せて",
    ];
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                gap: px(8),
                padding_bottom: px(24),
            ),
        ) {
            View(
                style: css!(
                    width: px(72),
                    height: px(72),
                    position: PositionKind::Relative,
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    margin_bottom: px(12),
                ),
            ) {
                Glass(corner_radius: 36.0_f32, style: glass_fill())
                Text(value: "✦", style: css!(color: hex(PURPLE), font_size: px(34)))
            }
            Text(
                value: "何をしましょう？",
                style: css!(color: hex(LABEL), font_size: px(28), font_weight: FontWeight::Bold),
            )
            Text(
                value: "Field のデータについて聞いたり、作業を頼んだりできます。",
                style: css!(
                    color: placeholder_color(),
                    font_size: px(16),
                    text_align: TextAlign::Center,
                ),
            )
            View(style: css!(height: px(20)))
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    gap: px(10),
                ),
            ) {
                ForEach(
                    each: || EXAMPLES.to_vec(),
                    key: |e: &&'static str| *e,
                    children: move |e: ReadSignal<&'static str>| {
                        let example = e.get_untracked();
                        render! {
                            View(
                                style: css!(
                                    position: PositionKind::Relative,
                                    padding: (px(11), px(18)),
                                ),
                                on_tap: move |_| on_pick.run(example.to_owned()),
                            ) {
                                Glass(corner_radius: 21.0_f32, style: glass_fill())
                                Text(
                                    value: example,
                                    style: css!(color: hex(LABEL), font_size: px(15)),
                                )
                            }
                        }
                    },
                )
            }
        }
    }
}

/// ガラスや背景の絵を、親いっぱいに敷くときの置き方。
fn glass_fill() -> Css {
    css!(
        position: PositionKind::Absolute,
        top: px(0),
        bottom: px(0),
        left: px(0),
        right: px(0),
    )
}

/// 自分の発言：右に青い吹き出し。
#[component]
fn user_bubble(text: Signal<String>) -> Element {
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                padding: (px(6), px(0), px(0), px(0)),
            ),
        ) {
            View(
                style: css!(
                    max_width: percent(80),
                    padding: (px(10), px(14)),
                    border_radius: px(20),
                    background_color: hex(TINT),
                ),
            ) {
                Text(value: text, style: css!(color: Color::hex(0xFFFFFF), font_size: px(16)))
            }
        }
    }
}

/// Agent の返事：吹き出しなしで、画面の幅いっぱいの文。見出し・太字・箇条書き・表などを飾る。
#[component]
fn assistant_text(text: Signal<String>) -> Element {
    render! { crate::markdown::Markdown(text: text) }
}

/// Agent が使った操作：小さな 1 行。動いている間は「…」、終わったら ✓、失敗は !。
#[component]
fn tool_line(text: Signal<String>, state: Signal<Role>) -> Element {
    let mark = computed(move || {
        match state.get() {
            Role::Tool(ToolState::Running) => "…",
            Role::Tool(ToolState::Failed) => "!",
            _ => "✓",
        }
        .to_owned()
    });
    let mark_color = computed(move || {
        let color = match state.get() {
            Role::Tool(ToolState::Running) => SECONDARY_LABEL,
            Role::Tool(ToolState::Failed) => 0xFF3B30,
            _ => 0x34C759,
        };
        css!(
            color: hex(color),
            font_size: px(13),
            font_weight: FontWeight::Bold,
            width: px(14),
        )
    });
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                gap: px(6),
                margin: (px(-6), px(0), px(-6), px(0)),
            ),
        ) {
            Text(value: mark, style: mark_color)
            Text(
                value: text,
                style: css!(
                    flex_shrink: 1.0,
                    color: hex(SECONDARY_LABEL),
                    font_size: px(13),
                    text_overflow: TextOverflow::Ellipsis,
                ),
            )
        }
    }
}

/// 失敗・中止の知らせ：灰色の小さな文。
#[component]
fn notice_line(text: Signal<String>) -> Element {
    render! {
        Text(value: text, style: css!(color: hex(SECONDARY_LABEL), font_size: px(13)))
    }
}

/// たたんだ操作のまとめ行：「✓ 3 件の操作 ›」。押すと中身の行が開く（⌄）。
#[component]
fn summary_line(text: Signal<String>, open: Signal<bool>, on_tap: Callback<()>) -> Element {
    let label = computed(move || format!("{} {}", text.get(), if open.get() { "⌄" } else { "›" }));
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                gap: px(6),
                margin: (px(-6), px(0), px(-6), px(0)),
            ),
            on_tap: move |_| on_tap.run(()),
        ) {
            Text(
                value: "✓",
                style: css!(
                    color: hex(SECONDARY_LABEL),
                    font_size: px(13),
                    font_weight: FontWeight::Bold,
                    width: px(14),
                ),
            )
            Text(
                value: label,
                style: css!(color: hex(SECONDARY_LABEL), font_size: px(13)),
            )
        }
    }
}
