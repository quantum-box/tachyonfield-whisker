//! Native inputs, reference pickers and repeatable line editors.
use super::model;
use super::schema::{Field, InputKind, Lookup};
use crate::cash_book_screen::spawn_in;
use crate::management::files::ReceiptAttachments;
use crate::*;
use whisker::Owner;

#[component]
pub(crate) fn input_fields(
    fields: Vec<Field>,
    draft: RwSignal<Value>,
    prefix: String,
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
) -> Element {
    let prefix = signal(prefix.clone());
    let fields = signal(fields.clone());
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            ForEach(
                each: move || fields.get(),
                key: |field: &Field| field.key,
                children: move |field: ReadSignal<Field>| {
                    let field = field.get_untracked();
                    let key = if prefix.get_untracked().is_empty() { field.key.to_owned() } else { format!("{}.{}", prefix.get_untracked(), field.key) };
                    render! { FormField(field: field, draft: draft, path: key, prefix: prefix.get_untracked(), token: token, tenant: tenant) }
                },
            )
        }
    }
}

#[component]
fn form_field(
    field: Field,
    draft: RwSignal<Value>,
    path: String,
    prefix: String,
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
) -> Element {
    let path = signal(path.clone());
    let label = if field.required {
        format!("{}（必須）", field.label)
    } else {
        field.label.to_owned()
    };
    let content = match field.kind {
        InputKind::Hidden => render! { View() },
        InputKind::ReadOnly => render! {
            View(style: computed(move || form_row_style().padding(px(14)))) {
                Text(value: label.clone(), style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(13))))
                Text(value: computed(move || draft.with(|v| model::display(model::value_at(v, &path.get())))), style: computed(move || css!(color: hex(LABEL), font_size: px(16))))
            }
        },
        InputKind::Attachments(kind) => render! {
            ReceiptAttachments(label: field.label, kind: kind, draft: draft, path: path.get_untracked(), token: token, tenant: tenant)
        },
        InputKind::Lines(children) => {
            render! { LineFields(field: field, fields: children.to_vec(), draft: draft, path: path.get_untracked(), token: token, tenant: tenant) }
        }
        InputKind::Bool => render! {
            View(
                style: computed(move || form_row_style().padding(px(14))),
                on_tap: move |_| draft.update(|value| {
                    let key = path.get_untracked();
                    let old = model::value_at(value, &key).as_bool().unwrap_or(false);
                    model::set_at(value, &key, Value::Bool(!old));
                }),
            ) {
                Text(value: label.clone(), style: computed(move || css!(color: hex(LABEL), font_size: px(16), flex_grow: 1.0)))
                Text(value: computed(move || if draft.with(|v| model::value_at(v, &path.get()).as_bool() == Some(true)) { "オン".to_owned() } else { "オフ".to_owned() }), style: computed(move || css!(color: hex(TINT), font_size: px(16))))
            }
        },
        InputKind::Select(options) => {
            let options = options
                .iter()
                .map(|c| (c.value.to_owned(), c.label.to_owned()))
                .collect::<Vec<_>>();
            render! { Picker(label: label.clone(), options: signal(options), selected: computed(move || draft.with(|v| model::input_text(model::value_at(v, &path.get())))), on_pick: move |value: String| draft.update(|v| model::set_at(v, &path.get_untracked(), Value::String(value))), optional: !field.required, loading: signal(false), error: signal(String::new()), on_retry: |_| {}) }
        }
        InputKind::Reference(lookup) => {
            render! { ReferenceField(field: field, lookup: lookup, draft: draft, path: path.get_untracked(), parent_path: String::new(), children_key: "", token: token, tenant: tenant) }
        }
        InputKind::RecordReference {
            rows_key,
            value_key,
            label_keys,
        } => {
            let options = signal(Vec::<(String, String)>::new());
            effect(move || {
                options.set(draft.with(|value| {
                    model::value_at(value, rows_key)
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|row| {
                            let key = model::input_text(model::value_at(row, value_key));
                            let label = label_keys
                                .iter()
                                .map(|key| model::display(model::value_at(row, key)))
                                .filter(|text| !text.is_empty())
                                .collect::<Vec<_>>()
                                .join(" · ");
                            (!key.is_empty() && !label.is_empty()).then_some((key, label))
                        })
                        .collect()
                }));
            });
            render! { Picker(label: label.clone(), options: options, selected: computed(move || draft.with(|v| model::input_text(model::value_at(v, &path.get())))), on_pick: move |value: String| draft.update(|v| model::set_at(v, &path.get_untracked(), Value::String(value))), optional: !field.required, loading: signal(false), error: signal(String::new()), on_retry: |_| {}) }
        }
        InputKind::DependentReference {
            lookup,
            parent_key,
            children_key,
        } => {
            let parent_path = if prefix.is_empty() {
                parent_key.into()
            } else {
                format!("{prefix}.{parent_key}")
            };
            render! { ReferenceField(field: field, lookup: lookup, draft: draft, path: path.get_untracked(), parent_path: parent_path, children_key: children_key, token: token, tenant: tenant) }
        }
        _ => {
            let input = InputRef::new();
            let multiline = matches!(field.kind, InputKind::Multiline | InputKind::StringLines);
            let keyboard = match field.kind {
                InputKind::Integer
                | InputKind::IntegerString
                | InputKind::Decimal
                | InputKind::Money => KeyboardType::Default,
                InputKind::Email => KeyboardType::Email,
                _ => KeyboardType::Default,
            };
            let placeholder = match field.kind {
                InputKind::Date => "2026-11-19",
                InputKind::DateTime => "2026-11-19 09:00（日本時間）",
                InputKind::StringLines => "1行に1項目ずつ入力",
                InputKind::Decimal
                | InputKind::Integer
                | InputKind::IntegerString
                | InputKind::Money => "半角の数値",
                _ => "未入力",
            };
            render! {
                View(style: computed(move || css!(padding: (px(12), px(16)), display: Display::Flex, flex_direction: FlexDirection::Column, gap: px(6)).border_bottom(hairline()))) {
                    Text(value: label.clone(), style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(13))))
                    Input(
                        value: computed(move || draft.with(|v| model::input_text(model::value_at(v, &path.get())))),
                        input_ref: input,
                        on_input: move |value: String| draft.update(|v| model::set_at(v, &path.get_untracked(), Value::String(value))),
                        on_submit: move |_| { if !multiline { input.blur(); } },
                        placeholder: placeholder,
                        multiline: multiline,
                        lines: if multiline { 4_u32 } else { 1_u32 },
                        keyboard_type: keyboard,
                        auto_capitalize: AutoCapitalize::None,
                        autocorrect: false,
                        style: computed(move || css!(color: hex(LABEL), background_color: hex(SYSTEM_BACKGROUND), font_size: px(17), min_height: px(if multiline { 104 } else { 42 }), flex_grow: 1.0)),
                    )
                }
            }
        }
    };
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            {content}
            Show(when: move || !field.help.is_empty()) {
                Text(value: field.help, style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(4), px(16), px(12), px(16)))))
            }
        }
    }
}

#[component]
fn line_fields(
    field: Field,
    fields: Vec<Field>,
    draft: RwSignal<Value>,
    path: String,
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
) -> Element {
    let path = signal(path.clone());
    let revision = signal(0_u64);
    let fields = signal(fields.clone());
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            SectionHeader(title: field.label)
            ForEach(
                each: move || {
                    let generation = revision.get();
                    let count = draft.with(|v| model::value_at(v, &path.get()).as_array().map_or(0, Vec::len));
                    (0..count).map(|index| (generation, index)).collect::<Vec<_>>()
                },
                key: |key: &(u64, usize)| *key,
                children: move |key: ReadSignal<(u64, usize)>| {
                    let index = key.get_untracked().1;
                    render! {
                        View(style: computed(move || group_style().margin_top(px(8)))) {
                            View(style: css!(padding: (px(10), px(16)), display: Display::Flex, flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween)) {
                                Text(value: format!("{} {}", field.label, index + 1), style: computed(move || css!(color: hex(LABEL), font_size: px(15), font_weight: FontWeight::Numeric(600))))
                                Show(when: move || !field.fixed_rows) { Text(value: "削除", style: computed(move || css!(color: hex(RED), font_size: px(15), padding: px(8))), on_tap: move |_| {
                                    draft.update(|v| {
                                        let mut rows = model::value_at(v, &path.get_untracked()).as_array().cloned().unwrap_or_default();
                                        if index < rows.len() { rows.remove(index); }
                                        model::set_at(v, &path.get_untracked(), Value::Array(rows));
                                    });
                                    revision.update(|r| *r += 1);
                                }) }
                            }
                            InputFields(fields: fields.get_untracked(), draft: draft, prefix: format!("{}.{index}", path.get_untracked()), token: token, tenant: tenant)
                        }
                    }
                },
            )
            Show(when: move || !field.fixed_rows) { Text(value: format!("＋ {}を追加", field.label), style: computed(move || css!(color: hex(TINT), font_size: px(16), padding: px(16))), on_tap: move |_| {
                draft.update(|v| {
                    let mut rows = model::value_at(v, &path.get_untracked()).as_array().cloned().unwrap_or_default();
                    rows.push(model::draft(&fields.get_untracked(), &Value::Null));
                    model::set_at(v, &path.get_untracked(), Value::Array(rows));
                });
            }) }
        }
    }
}

#[component]
fn reference_field(
    field: Field,
    lookup: Lookup,
    draft: RwSignal<Value>,
    path: String,
    parent_path: String,
    children_key: &'static str,
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
) -> Element {
    let path = signal(path.clone());
    let parent_path = signal(parent_path.clone());
    let data = signal(Value::Null);
    let loading = signal(false);
    let error = signal(String::new());
    let options = signal(Vec::<(String, String)>::new());
    let owner = Owner::current();
    let generation = signal(0_u64);
    let load = move || {
        if loading.get_untracked() {
            return;
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        loading.set(true);
        error.set(String::new());
        generation.update(|g| *g += 1);
        let request = generation.get_untracked();
        spawn_in(owner, async move {
            let result = authed(token, move |access| {
                model::fetch_lookup(access, &current, lookup)
            })
            .await;
            if !owner.is_none_or(Owner::is_alive)
                || tenant.get_untracked().as_ref() != Some(&context)
                || generation.get_untracked() != request
            {
                return;
            }
            loading.set(false);
            match result {
                Ok((value, truncated)) => {
                    data.set(value);
                    if truncated {
                        error.set("候補の表示上限に達しました。見つからない項目は、この画面では選べません".into());
                    }
                }
                Err(message) => error.set(message),
            }
        });
    };
    load();
    let previous_parent = signal(None::<String>);
    effect(move || {
        let parent = if parent_path.with(String::is_empty) {
            None
        } else {
            Some(draft.with(|v| model::input_text(model::value_at(v, &parent_path.get()))))
        };
        if let Some(parent) = &parent {
            if previous_parent
                .get_untracked()
                .as_ref()
                .is_some_and(|old| old != parent)
            {
                draft.update(|v| {
                    model::set_at(v, &path.get_untracked(), Value::String(String::new()))
                });
            }
            previous_parent.set(Some(parent.clone()));
        }
        let value = data.get();
        if value.is_null() {
            return;
        }
        match model::lookup_options(lookup, &value, parent.as_deref().map(|p| (p, children_key))) {
            Ok(values) => options.set(values),
            Err(message) => error.set(message),
        }
    });
    render! {
        Picker(
            label: if field.required { format!("{}（必須）", field.label) } else { field.label.into() },
            options: options,
            selected: computed(move || draft.with(|v| model::input_text(model::value_at(v, &path.get())))),
            on_pick: move |value: String| draft.update(|v| model::set_at(v, &path.get_untracked(), Value::String(value))),
            optional: !field.required,
            loading: loading,
            error: error,
            on_retry: move |_| load(),
        )
    }
}

#[component]
pub(crate) fn picker(
    label: String,
    options: RwSignal<Vec<(String, String)>>,
    selected: ReadSignal<String>,
    on_pick: Callback<String>,
    optional: bool,
    loading: RwSignal<bool>,
    error: RwSignal<String>,
    on_retry: Callback<()>,
) -> Element {
    let open = signal(false);
    let query = signal(String::new());
    let label = signal(label.clone());
    let current = computed(move || {
        let selected = selected.get();
        if selected.is_empty() {
            return "選択してください".to_owned();
        }
        options
            .with(|rows| {
                rows.iter()
                    .find(|(key, _)| *key == selected)
                    .map(|(_, label)| label.clone())
            })
            .unwrap_or_else(|| {
                if loading.get() {
                    "読み込み中…".into()
                } else {
                    "現在の選択を確認できません".into()
                }
            })
    });
    render! {
        View(style: computed(move || css!(display: Display::Flex, flex_direction: FlexDirection::Column).border_bottom(hairline()))) {
            View(style: css!(padding: (px(12), px(16)), min_height: px(54), display: Display::Flex, flex_direction: FlexDirection::Column, gap: px(6)), on_tap: move |_| open.update(|v| *v = !*v)) {
                Text(value: label, style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(13))))
                Text(value: computed(move || format!("{} {}", current.get(), if open.get() { "⌃" } else { "⌄" })), style: computed(move || css!(color: hex(TINT), font_size: px(17))))
            }
            Show(when: move || open.get()) {
                View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column, padding: (px(0), px(16), px(12), px(16)))) {
                    Input(text: query, placeholder: "候補を検索", auto_capitalize: AutoCapitalize::None, style: computed(move || css!(background_color: hex(FILL), color: hex(LABEL), font_size: px(16), height: px(44), padding: px(10), border_radius: px(10))))
                    Show(when: move || loading.get()) { Notice(message: "候補を読み込み中…") }
                    Show(when: move || !error.with(String::is_empty)) {
                        ErrorNotice(message: computed(move || error.get()))
                        Text(value: "もう一度読み込む", style: computed(move || css!(color: hex(TINT), font_size: px(16), padding: px(12))), on_tap: move |_| on_retry.run(()))
                    }
                    Show(when: move || optional) {
                        Text(value: "選択を解除", style: computed(move || css!(color: hex(TINT), font_size: px(16), padding: px(12))), on_tap: move |_| { on_pick.run(String::new()); open.set(false); })
                    }
                    ForEach(
                        each: move || {
                            let q = query.get().trim().to_lowercase();
                            options.with(|rows| rows.iter().filter(|(_, name)| name.to_lowercase().contains(&q)).cloned().collect::<Vec<_>>())
                        },
                        key: |row: &(String, String)| row.0.clone(),
                        children: move |row: ReadSignal<(String, String)>| {
                            let (key, label) = row.get_untracked();
                            let checked_key = key.clone();
                            render! {
                                View(style: computed(move || form_row_style().padding(px(12))), on_tap: move |_| { on_pick.run(key.clone()); open.set(false); query.set(String::new()); }) {
                                    Text(value: label, style: computed(move || css!(color: hex(LABEL), font_size: px(16), flex_grow: 1.0)))
                                    Text(value: computed(move || if selected.get() == checked_key { "✓".to_owned() } else { String::new() }), style: computed(move || css!(color: hex(TINT), font_size: px(17))))
                                }
                            }
                        },
                    )
                    Show(when: move || !loading.get() && options.with(Vec::is_empty) && error.with(String::is_empty)) { Notice(message: "選択できる候補がありません") }
                }
            }
        }
    }
}
