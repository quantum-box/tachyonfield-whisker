//! Field Agent（AI に話しかける画面）。
//!
//! 頼みごとを送ると、返事の文と操作の経過が少しずつ届く（SSE）。Field の操作（道具）は
//! サーバー側で実行されるので、この画面は「送る・届いたものを並べる」だけ。
//! 自分の発言は右の青い吹き出し、Agent の返事は吹き出しなしの文、操作は小さな 1 行で出す。
//! 会話はホームが持っているので、タブを切り替えて戻っても消えない。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use whisker::css::{
    Animation, AnimationDirection, AnimationFillMode, AnimationIterationCount, Css, Keyframes,
    Number, Percentage,
};
use whisker::runtime::view::observe_layout;
use whisker::{ElementHandle, ScrollViewHandle};

use super::*;
use crate::cash_book_screen::{sleep, spawn_in};

/// Agent の印（✦）の色。Apple Intelligence の紫寄りの色。
const PURPLE: u32 = 0x8E5CF7;
/// 何も届かないまま、これだけ待ったら「時間がかかっています」と経過時間を出す。
const LONG_WAIT: Duration = Duration::from_secs(15);
/// 何も届かないまま、これだけ待ったら止まったものとして待つのをやめる。
/// 通信の読み取りは 150 秒で切れるので、ふつうはその前に失敗として届く。
const STALL_AFTER: Duration = Duration::from_secs(300);
/// 待ち時間を数え直す間隔。
const TICK: Duration = Duration::from_secs(1);
use api::AgentEvent;

/// 会話の 1 行の種類。
#[derive(Clone, Copy, PartialEq)]
enum Role {
    User,
    Assistant,
    /// Agent が使った操作。
    Tool(ToolState),
    /// 中止などの知らせ。
    Notice,
    /// 失敗の知らせ。最後の失敗には「再試行」を出す。
    Error,
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

/// 失敗した頼みを、もう一度送るための控え。
#[derive(Clone)]
struct Retry {
    typed: String,
    picked: Vec<field_glass::PickedFile>,
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
    /// この会話で Field へ上げたファイル。次の回からも Agent に渡す。
    files: RwSignal<Vec<api::AgentFile>>,
    /// 送る前の添付ファイル（入力欄の上に並ぶ）。
    pending: RwSignal<Vec<field_glass::PickedFile>>,
    /// 今流れている返事を止める旗。
    cancel: RwSignal<Option<Arc<AtomicBool>>>,
    /// 最後の失敗の知らせ（行の ID）と、送り直す頼み。
    retry: RwSignal<Option<(usize, Retry)>>,
    /// 最後に Agent から何か届いた（または送った）時刻。
    last_event: RwSignal<Instant>,
    /// 返事を待っている間、1 秒ごとに進める今の時刻。
    now: RwSignal<Instant>,
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
            files: signal(Vec::new()),
            pending: signal(Vec::new()),
            cancel: signal(None),
            retry: signal(None),
            last_event: signal(Instant::now()),
            now: signal(Instant::now()),
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

    /// 失敗を知らせ、再試行できるよう頼みを控えておく。
    fn fail(&self, message: String, again: Retry) {
        let id = self.push(Role::Error, message, String::new());
        self.retry.set(Some((id, again)));
    }

    /// 何も届かないまま長く経ったら、待つのをやめて失敗として知らせる。
    fn stall(&self, again: Retry) {
        if let Some(flag) = self.cancel.get_untracked() {
            flag.store(true, Ordering::Relaxed);
        }
        self.cancel.set(None);
        if self.sending.get_untracked() {
            self.sending.set(false);
            self.settle_tools();
            self.fold_turn();
            self.fail(
                "Field Agent から応答がないため、待つのをやめました".into(),
                again,
            );
        }
    }

    fn stop(&self) {
        self.stop_with("止めました");
    }

    /// 今の返事を止めて、その旨を会話に残す。
    fn stop_with(&self, notice: &str) {
        if let Some(flag) = self.cancel.get_untracked() {
            flag.store(true, Ordering::Relaxed);
        }
        self.cancel.set(None);
        if self.sending.get_untracked() {
            self.sending.set(false);
            self.settle_tools();
            self.fold_turn();
            self.push(Role::Notice, notice.into(), String::new());
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
    // 一番下を見ているか・指で動かしているか。キーボードの開け閉めで見える高さが変わったときは、
    // 一番下を見ていたときだけ合わせ直す（上の会話を読んでいる途中や、引き下ろし中に勝手に動かさない）。
    let at_bottom = signal(true);
    let dragging = signal(false);
    let mut last_content = 0.0_f64;
    effect(move || {
        let content = content_height.get();
        let bottom = (content - viewport_height.get()).max(0.0);
        let grew = content > last_content;
        last_content = content;
        if grew || (at_bottom.get_untracked() && !dragging.get_untracked()) {
            scroller.scroll_to(bottom, false);
        }
    });
    let on_scroll = move |e: whisker::event::ScrollEvent| {
        let d = e.detail;
        dragging.set(d.is_dragging);
        at_bottom.set(d.scroll_top + d.viewport_height >= d.scroll_height - 24.0);
        // iOS は `KeyboardDismiss` が閉じる。ほかでは、会話を下へ引いたら入力欄を外す。
        if cfg!(not(target_os = "ios")) && d.is_dragging && d.delta_y < 0.0 {
            input.blur();
        }
    };

    let submit = move |typed: String, picked: Vec<field_glass::PickedFile>| {
        if sending.get_untracked() {
            return;
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let again = Retry {
            typed: typed.clone(),
            picked: picked.clone(),
        };
        // 文なしでファイルだけ送ったときは、中身の要点を聞く。
        // 「保存しないで」とは書かない。請求書や領収書は証憑OCR に上げないと Agent が読めないため。
        let task = if typed.is_empty() {
            "添付したファイルの中身を読んで、要点を教えてください。".to_owned()
        } else {
            typed.clone()
        };
        chat.retry.set(None);
        chat.turn.set(chat.turn.get_untracked() + 1);
        let mut shown = typed;
        for file in &picked {
            if !shown.is_empty() {
                shown.push('\n');
            }
            shown.push_str(&format!("📎 {}", file.name));
        }
        chat.push(Role::User, shown, String::new());
        sending.set(true);
        let cancel = Arc::new(AtomicBool::new(false));
        chat.cancel.set(Some(cancel.clone()));
        let started = Instant::now();
        chat.last_event.set(started);
        chat.now.set(started);

        // 待ち時間を数える。何も届かないまま長く経ったら、止まったものとして知らせる。
        let turn = chat.turn.get_untracked();
        let waiting = again.clone();
        spawn_in(chat.owner, async move {
            loop {
                sleep(TICK).await;
                if !sending.get_untracked() || chat.turn.get_untracked() != turn {
                    return;
                }
                let now = Instant::now();
                chat.now.set(now);
                if now.saturating_duration_since(chat.last_event.get_untracked()) >= STALL_AFTER {
                    chat.stall(waiting);
                    return;
                }
            }
        });

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
                                chat.cancel.set(None);
                                chat.fail(message, again);
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
            let earlier = chat.files.get_untracked();
            std::thread::spawn(move || {
                let mut access = access;
                let send = |event| {
                    let _ = sender.unbounded_send(event);
                };
                // 先にファイルを Field へ上げる。トークンが切れていたら 1 回だけ取り直す。
                let mut prepared = attachments::prepare(&access, &current, &picked);
                if matches!(&prepared, Err(e) if e == api::EXPIRED) {
                    if let Some(renewed) = renew(&send) {
                        access = renewed;
                        prepared = attachments::prepare(&access, &current, &picked);
                    }
                }
                let (added, inline) = match prepared {
                    Ok(done) => done,
                    Err(message) => {
                        send(AgentEvent::Failed(message));
                        return;
                    }
                };
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                if !added.is_empty() {
                    send(AgentEvent::Attached(added.clone()));
                }
                let mut files = earlier;
                files.extend(added.iter().cloned());
                let task = attachments::with_manifest(&task, &files, &added, &inline);
                let inline: Vec<serde_json::Value> = inline.into_iter().map(|i| i.body).collect();

                let mut result = api::agent_stream(
                    &access, &current, &session, &task, &files, &inline, &flag, &send,
                );
                // トークンが切れていたら取り直して 1 回だけ送り直す。期限切れは送った直後
                // （まだ何も流れていない時）に返るので、同じ頼みを二重に流すことはない。
                if matches!(&result, Err(e) if e == api::EXPIRED) {
                    if let Some(renewed) = renew(&send) {
                        // 取り直している間に止められていたら、送り直さない。
                        if flag.load(Ordering::Relaxed) {
                            return;
                        }
                        result = api::agent_stream(
                            &renewed, &current, &session, &task, &files, &inline, &flag, &send,
                        );
                    }
                }
                if let Err(message) = result {
                    send(AgentEvent::Failed(message));
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
                chat.last_event.set(Instant::now());
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
                        paragraph = None;
                        chat.fail(message, again.clone());
                    }
                    AgentEvent::Renewed(access) => token.set(Some(access)),
                    AgentEvent::Attached(added) => chat.files.update(|f| f.extend(added)),
                }
            }
            if !cancel.load(Ordering::Relaxed) {
                chat.settle_tools();
                chat.fold_turn();
                chat.cancel.set(None);
                sending.set(false);
            }
        };
        spawn_in(chat.owner, run);
    };

    let send = move || {
        let typed = draft.get_untracked().trim().to_owned();
        let picked = chat.pending.get_untracked();
        if (typed.is_empty() && picked.is_empty()) || tenant.with_untracked(Option::is_none) {
            return;
        }
        // 返事の途中で送ったら、今の返事を止めて、この頼みで続ける。
        if sending.get_untracked() {
            chat.stop_with("止めて、次の頼みに切り替えました");
        }
        draft.set(String::new());
        chat.pending.set(Vec::new());
        submit(typed, picked);
    };

    // 失敗した頼みを送り直す。上げ終わって消えた添付は、会話に残っているので付け直さない。
    let retry = move || {
        let Some((_, again)) = chat.retry.get_untracked() else {
            return;
        };
        let picked = again
            .picked
            .into_iter()
            .filter(|f| std::path::Path::new(&f.path).exists())
            .collect();
        submit(again.typed, picked);
    };

    // 考え中の印：送ったあと、まだ文が流れ始めていない（または操作が終わって次を待つ）間。
    let thinking = computed(move || {
        sending.get()
            && messages.with(|list| {
                !matches!(
                    list.last().map(|m| m.role),
                    Some(Role::Assistant)
                        | Some(Role::Tool(ToolState::Running))
                        | Some(Role::Error)
                )
            })
    });

    // 最後に何か届いてからの待ち時間（秒）。
    let waited = computed(move || {
        chat.now
            .get()
            .saturating_duration_since(chat.last_event.get())
            .as_secs()
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
                    chat.retry.set(None);
                    chat.expanded.set(Vec::new());
                    chat.session.set(None);
                    chat.files.set(Vec::new());
                },
            )
            KeyboardObserver(on_change: move |height: f32| keyboard.set(height))
            ScrollView(style: scroll_style(), element_ref: scroller.r(), on_scroll: on_scroll) {
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
                                Role::Error => {
                                    let id = m.get_untracked().id;
                                    let pending = chat.retry;
                                    render! {
                                        ErrorLine(
                                            text: text,
                                            can_retry: computed(move || {
                                                !sending.get()
                                                    && pending.with(|r| r.as_ref().is_some_and(|(n, _)| *n == id))
                                            }),
                                            on_retry: move |_| retry(),
                                        )
                                    }
                                }
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
                        ThinkingIndicator(waited: waited)
                    }
                }
            }
            Composer(
                draft: draft,
                input: input,
                sending: sending,
                pending: chat.pending,
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
    pending: RwSignal<Vec<field_glass::PickedFile>>,
    on_send: Callback<()>,
    on_stop: Callback<()>,
) -> Element {
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                gap: px(8),
                padding: (px(6), px(12), px(8), px(12)),
            ),
        ) {
            // 送る前の添付ファイル。× で外せる。
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    gap: px(6),
                ),
            ) {
                ForEach(
                    each: move || pending.get(),
                    key: |f: &field_glass::PickedFile| f.path.clone(),
                    children: move |f: ReadSignal<field_glass::PickedFile>| {
                        let file = f.get_untracked();
                        let path = file.path.clone();
                        render! {
                            AttachmentChip(
                                name: file.name.clone(),
                                on_remove: move |_| {
                                    let _ = std::fs::remove_file(&path);
                                    pending.update(|list| list.retain(|p| p.path != path));
                                },
                            )
                        }
                    },
                )
            }
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    gap: px(6),
                    min_height: px(50),
                    padding: (px(5), px(7), px(5), px(4)),
                    position: PositionKind::Relative,
                ),
            ) {
                // ガラスの丸い枠（iOS 26 のメッセージ入力欄の形）。
                Glass(corner_radius: 25.0_f32, style: glass_fill())
                // 枠の中（添付・送るボタンのすき間も）をタップしても、キーボードは閉じない。
                field_glass::KeyboardKeep()
                AttachButton(
                    on_pick: move |file: field_glass::PickedFile| {
                        pending.update(|list| list.push(file))
                    },
                    style: css!(width: px(40), height: px(40), flex_shrink: 0.0),
                )
                Input(
                    text: draft,
                    placeholder: "Field Agent に頼む",
                    return_key: ReturnKey::Send,
                    input_ref: input,
                    on_submit: move |_| on_send.run(()),
                    style: computed(move || css!(
                        flex_grow: 1.0,
                        height: px(40),
                        font_size: px(17),
                        color: hex(LABEL),
                        background_color: Color::rgba(0, 0, 0, 0.0),
                    )),
                )
                View(
                    style: computed(move || {
                        let typed = !draft.with(|d| d.trim().is_empty())
                            || pending.with(|p| !p.is_empty());
                        css!(
                            width: px(36),
                            height: px(36),
                            border_radius: px(18),
                            display: Display::Flex,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            background_color: hex(match send_button(sending.get(), typed) {
                                SendButton::Stop => LABEL,
                                SendButton::Send => TINT,
                                SendButton::Disabled => DISABLED,
                            }),
                        )
                    }),
                    on_tap: move |_| {
                        let typed = !draft.with_untracked(|d| d.trim().is_empty())
                            || pending.with_untracked(|p| !p.is_empty());
                        // 返事の途中でも、入力があれば送る（今の返事は止めて切り替える）。
                        if send_button(sending.get_untracked(), typed) == SendButton::Stop {
                            on_stop.run(());
                        } else {
                            on_send.run(());
                        }
                    },
                ) {
                    Text(
                        value: computed(move || {
                            let typed = !draft.with(|d| d.trim().is_empty())
                                || pending.with(|p| !p.is_empty());
                            if send_button(sending.get(), typed) == SendButton::Stop { "■" } else { "↑" }
                                .to_owned()
                        }),
                        style: computed(move || {
                            let typed = !draft.with(|d| d.trim().is_empty())
                                || pending.with(|p| !p.is_empty());
                            let busy = send_button(sending.get(), typed) == SendButton::Stop;
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

/// 入力欄の右の丸ボタンの役目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SendButton {
    /// 返事の途中で、入力が空：■ で止める。
    Stop,
    /// 入力がある：↑ で送る。返事の途中なら、止めてから送る。
    Send,
    /// 返事もなく入力も空。
    Disabled,
}

fn send_button(sending: bool, typed: bool) -> SendButton {
    match (sending, typed) {
        (_, true) => SendButton::Send,
        (true, false) => SendButton::Stop,
        (false, false) => SendButton::Disabled,
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
                Text(value: "✦", style: computed(move || css!(color: hex(PURPLE), font_size: px(34))))
            }
            Text(
                value: "何をしましょう？",
                style: computed(move || css!(color: hex(LABEL), font_size: px(28), font_weight: FontWeight::Bold)),
            )
            Text(
                value: "Field のデータについて聞いたり、作業を頼んだりできます。",
                style: computed(move || css!(
                    color: placeholder_color(),
                    font_size: px(16),
                    text_align: TextAlign::Center,
                )),
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
                                    style: computed(move || css!(color: hex(LABEL), font_size: px(15))),
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
                style: computed(move || css!(
                    max_width: percent(80),
                    padding: (px(10), px(14)),
                    border_radius: px(20),
                    background_color: hex(TINT),
                )),
            ) {
                Text(value: text, style: computed(move || css!(color: Color::hex(0xFFFFFF), font_size: px(16))))
            }
        }
    }
}

/// 入力欄の上に並ぶ、送る前の添付ファイル 1 つ。
#[component]
fn attachment_chip(name: String, on_remove: Callback<()>) -> Element {
    let label = format!("📎 {name}");
    render! {
        View(
            style: computed(move || css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                gap: px(6),
                max_width: px(240),
                padding: (px(6), px(6), px(6), px(12)),
                border_radius: px(16),
                background_color: hex(FILL),
            )),
        ) {
            Text(
                value: label.clone(),
                max_lines: 1u32,
                style: computed(move || css!(
                    color: hex(LABEL),
                    font_size: px(14),
                    flex_shrink: 1.0,
                    text_overflow: TextOverflow::Ellipsis,
                )),
            )
            View(
                on_tap: move |_| on_remove.run(()),
                style: computed(move || css!(
                    width: px(22),
                    height: px(22),
                    border_radius: px(11),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    background_color: hex(SECONDARY_LABEL),
                )),
            ) {
                Text(value: "✕", style: computed(move || css!(color: hex(FILL), font_size: px(11), font_weight: FontWeight::Bold)))
            }
        }
    }
}

/// リフレッシュトークンでアクセストークンを取り直す。取り直せたら画面にも知らせる。
fn renew(send: &impl Fn(AgentEvent)) -> Option<String> {
    let refresh_token = storage::refresh_token()?;
    let renewed = api::refresh(&refresh_token).ok()?;
    if let Some(rotated) = &renewed.refresh_token {
        storage::save_refresh_token(rotated);
    }
    send(AgentEvent::Renewed(renewed.access_token.clone()));
    Some(renewed.access_token)
}

/// Agent の返事：吹き出しなしで、画面の幅いっぱいの文。見出し・太字・箇条書き・表などを飾る。
/// 考え中の印と入れ替わるときに、ふわっと出す。
#[component]
fn assistant_text(text: Signal<String>) -> Element {
    render! {
        View(style: fade_in()) {
            crate::markdown::Markdown(text: text)
        }
    }
}

/// 考え中の印：紫の ✦ がゆっくり明滅しながら少し揺れ、「考えています」の後ろの点が順に灯る。
/// 動きを減らす設定では、揺れ・大きさの変化・点の動きを止め、✦ の明るさだけをゆっくり変える。
/// 何も届かないまま長く待っているときは、経過時間を下に添える。
#[component]
fn thinking_indicator(waited: Signal<u64>) -> Element {
    let long = computed(move || waited.get() >= LONG_WAIT.as_secs());
    let label = computed(move || waiting_label(waited.get()));
    let text_style = move || css!(color: hex(SECONDARY_LABEL), font_size: px(15));
    render! {
        View(
            style: fade_in()
                .display(Display::Flex)
                .flex_direction(FlexDirection::Column)
                .gap(px(2)),
        ) {
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    gap: px(6),
                    height: px(22),
                ),
            ) {
                // 揺れ（外）と明滅（内）を分け、同じ transform を取り合わないようにする。
                View(style: computed(move || sparkle_box().animation(sparkle_sway(reduce_motion())))) {
                    View(style: computed(move || sparkle_box().animation(sparkle_pulse(reduce_motion())))) {
                        Text(value: "✦", style: computed(move || css!(color: hex(PURPLE), font_size: px(15))))
                    }
                }
                View(
                    style: css!(
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                    ),
                ) {
                    Text(value: "考えています", style: computed(text_style))
                    View(style: computed(move || dot_style(0, reduce_motion()))) {
                        Text(value: ".", style: computed(text_style))
                    }
                    View(style: computed(move || dot_style(1, reduce_motion()))) {
                        Text(value: ".", style: computed(text_style))
                    }
                    View(style: computed(move || dot_style(2, reduce_motion()))) {
                        Text(value: ".", style: computed(text_style))
                    }
                }
            }
            Show(when: move || long.get()) {
                Text(
                    value: label,
                    style: computed(move || css!(
                        color: hex(SECONDARY_LABEL),
                        font_size: px(13),
                        padding_left: px(24),
                    )),
                )
            }
        }
    }
}

/// 長く待っているときの添え書き。「時間がかかっています（1 分 05 秒）・■ で止められます」。
fn waiting_label(secs: u64) -> String {
    let elapsed = if secs < 60 {
        format!("{secs} 秒")
    } else {
        format!("{} 分 {:02} 秒", secs / 60, secs % 60)
    };
    format!("時間がかかっています（{elapsed}）・■ で止められます")
}

/// ✦ を回したり大きさを変えたりしても、周りが動かないよう大きさを決めた箱。
fn sparkle_box() -> Css {
    css!(
        width: px(18),
        height: px(18),
        display: Display::Flex,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
    )
}

/// 繰り返し、行って戻る動き。
fn breathing(frames: Keyframes, duration_ms: f32) -> Animation {
    Animation::new(frames)
        .duration(ms(duration_ms))
        .timing(EasingFunction::EaseInOut)
        .iteration_count(AnimationIterationCount::Infinite)
        .direction(AnimationDirection::Alternate)
}

fn keyframes(from: Css, to: Css) -> Keyframes {
    Keyframes::builder()
        .from(from)
        .to(to)
        .build()
        .expect("考え中の印の動きが組めません")
}

/// ✦ をゆっくり左右に少し傾ける。動きを減らす設定では傾けない。
fn sparkle_sway(reduce: bool) -> Animation {
    if reduce {
        return Animation::new("none");
    }
    let tilt = |angle: f32| Css::new().transform(TransformFn::Rotate(deg(angle).into()));
    breathing(keyframes(tilt(-12.0), tilt(12.0)), 1800.0)
}

/// ✦ をゆっくり明るく・暗くする。動きを減らす設定では大きさを変えず、さらにゆっくり。
fn sparkle_pulse(reduce: bool) -> Animation {
    if reduce {
        return breathing(
            keyframes(Css::new().opacity(0.55), Css::new().opacity(1.0)),
            1600.0,
        );
    }
    let frame = |opacity: f32, scale: f32| {
        Css::new().opacity(opacity).transform(TransformFn::Scale(
            Number::new(scale).into(),
            Number::new(scale).into(),
        ))
    };
    breathing(keyframes(frame(0.5, 0.86), frame(1.0, 1.0)), 900.0)
}

/// 「考えています」の後ろの点を、左から順に灯す。動きを減らす設定では点を灯したままにする。
fn dot_style(index: usize, reduce: bool) -> Css {
    if reduce {
        return Css::new().opacity(1.0);
    }
    let dim = || Css::new().opacity(0.2);
    let frames = Keyframes::builder()
        .from(dim())
        .at(Percentage::new(35.0), Css::new().opacity(1.0))
        .at(Percentage::new(70.0), dim())
        .to(dim())
        .build()
        .expect("考え中の点の動きが組めません");
    Css::new().opacity(0.2).animation(
        Animation::new(frames)
            .duration(ms(1400))
            .delay(ms(index as f32 * 200.0))
            .timing(EasingFunction::EaseInOut)
            .iteration_count(AnimationIterationCount::Infinite)
            .fill_mode(AnimationFillMode::Both),
    )
}

/// 出てくるときに、短くふわっと濃くする（位置は動かさないので、動きを減らす設定でもそのまま）。
fn fade_in() -> Css {
    let frames = keyframes(Css::new().opacity(0.0), Css::new().opacity(1.0));
    Css::new().animation(
        Animation::new(frames)
            .duration(ms(220))
            .timing(EasingFunction::EaseOut)
            .fill_mode(AnimationFillMode::Both),
    )
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
                style: computed(move || css!(
                    flex_shrink: 1.0,
                    color: hex(SECONDARY_LABEL),
                    font_size: px(13),
                    text_overflow: TextOverflow::Ellipsis,
                )),
            )
        }
    }
}

/// 失敗・中止の知らせ：灰色の小さな文。
#[component]
fn notice_line(text: Signal<String>) -> Element {
    render! {
        Text(value: text, style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(13))))
    }
}

/// 失敗の知らせ：赤い「!」と灰色の文。最後の失敗には「再試行」の札を出す。
#[component]
fn error_line(text: Signal<String>, can_retry: Signal<bool>, on_retry: Callback<()>) -> Element {
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                gap: px(8),
            ),
        ) {
            View(
                style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::FlexStart,
                    gap: px(6),
                ),
            ) {
                Text(
                    value: "!",
                    style: computed(move || css!(
                        color: hex(RED),
                        font_size: px(13),
                        font_weight: FontWeight::Bold,
                        width: px(14),
                    )),
                )
                Text(
                    value: text,
                    style: computed(move || css!(flex_shrink: 1.0, color: hex(SECONDARY_LABEL), font_size: px(13))),
                )
            }
            Show(when: move || can_retry.get()) {
                View(
                    style: computed(move || css!(
                        margin_left: px(20),
                        padding: (px(6), px(14)),
                        border_radius: px(15),
                        background_color: hex(FILL),
                    )),
                    on_tap: move |_| on_retry.run(()),
                ) {
                    Text(
                        value: "再試行",
                        style: computed(move || css!(color: hex(TINT), font_size: px(14), font_weight: FontWeight::Numeric(600))),
                    )
                }
            }
        }
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
                style: computed(move || css!(
                    color: hex(SECONDARY_LABEL),
                    font_size: px(13),
                    font_weight: FontWeight::Bold,
                    width: px(14),
                )),
            )
            Text(
                value: label,
                style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(13))),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_label_shows_seconds_then_minutes() {
        assert_eq!(
            waiting_label(15),
            "時間がかかっています（15 秒）・■ で止められます"
        );
        assert_eq!(
            waiting_label(65),
            "時間がかかっています（1 分 05 秒）・■ で止められます"
        );
    }

    #[test]
    fn reduce_motion_stops_tilt_scale_and_dots() {
        assert!(sparkle_sway(false).keyframes.is_some());
        assert!(sparkle_sway(true).keyframes.is_none());
        assert_ne!(sparkle_pulse(true).name, sparkle_pulse(false).name);
        assert!(dot_style(1, false).to_string().contains("animation"));
        assert!(!dot_style(1, true).to_string().contains("animation"));
    }

    #[test]
    fn typing_during_a_reply_turns_stop_into_send() {
        assert_eq!(send_button(true, false), SendButton::Stop);
        assert_eq!(send_button(true, true), SendButton::Send);
        assert_eq!(send_button(false, true), SendButton::Send);
        assert_eq!(send_button(false, false), SendButton::Disabled);
    }

    #[test]
    fn stall_waits_longer_than_the_read_timeout() {
        assert!(LONG_WAIT < STALL_AFTER);
        assert!(STALL_AFTER > Duration::from_secs(150));
    }
}
