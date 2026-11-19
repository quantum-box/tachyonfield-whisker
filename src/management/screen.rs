use super::{
    access::Access,
    form::InputFields,
    model,
    schema::{Idempotency, Mutation, Resource},
};
use crate::cash_book_screen::spawn_in;
use crate::management::detail::{RecordLines, expanded_line_keys};
use crate::management::intake::DocumentInput;
use crate::management::jobcan::JobcanImportPanel;
use crate::management::resources;
use crate::*;
use whisker::Owner;

#[cfg(test)]
#[path = "ui_tests.rs"]
mod ui_tests;

#[derive(Clone, Copy)]
pub(crate) struct Connection {
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    access: RwSignal<Option<Access>>,
    loading: RwSignal<bool>,
    error: RwSignal<String>,
    owner: Option<Owner>,
}

pub(crate) fn provide_connection(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
) {
    provide_context(Connection::new(token, tenant));
}

fn connection_for(token: RwSignal<Option<String>>, tenant: RwSignal<Option<Tenant>>) -> Connection {
    use_context::<Connection>().unwrap_or_else(|| Connection::new(token, tenant))
}

impl Connection {
    fn new(token: RwSignal<Option<String>>, tenant: RwSignal<Option<Tenant>>) -> Self {
        let value = Self {
            token,
            tenant,
            access: signal(None),
            loading: signal(false),
            error: signal(String::new()),
            owner: Owner::current(),
        };
        value.load();
        value
    }
    fn load(self) {
        if self.loading.get_untracked() {
            return;
        }
        let Some(current) = self.tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        self.loading.set(true);
        self.error.set(String::new());
        self.access.set(None);
        spawn_in(self.owner, async move {
            let result = authed(self.token, move |token| Access::load(token, &current)).await;
            if !self.owner.is_none_or(Owner::is_alive)
                || self.tenant.get_untracked().as_ref() != Some(&context)
                || self.token.with_untracked(Option::is_none)
            {
                return;
            }
            self.loading.set(false);
            match result {
                Ok(access) => self.access.set(Some(access)),
                Err(message) => self.error.set(message),
            }
        });
    }
    fn allowed(self, resource: Resource) -> bool {
        self.tenant.with(|tenant| {
            tenant.as_ref().is_some_and(|tenant| {
                self.access
                    .with(|a| a.as_ref().is_some_and(|a| a.allows(resource, &tenant.id)))
            })
        })
    }
    fn can_mutate(self, operation: Mutation, collection: &Value, original: &Value) -> bool {
        self.access.with(|a| {
            a.as_ref()
                .is_some_and(|a| a.can_mutate(operation, collection, original))
        })
    }
}

#[component]
pub(crate) fn management_screen(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
) -> Element {
    let connection = connection_for(token, tenant);
    let query = signal(String::new());
    let selected = signal(None::<&'static str>);
    effect(move || {
        if let Some(key) = selected.get() {
            if let Some(resource) = resources().into_iter().find(|resource| resource.key == key) {
                if !connection.allowed(resource) {
                    selected.set(None);
                }
            }
        }
    });
    let groups = computed(move || {
        let query = query.get().trim().to_lowercase();
        let mut groups: Vec<(&'static str, Vec<Resource>)> = Vec::new();
        for resource in resources()
            .into_iter()
            .filter(|resource| connection.allowed(*resource))
        {
            if !query.is_empty()
                && !format!("{} {}", resource.group, resource.label)
                    .to_lowercase()
                    .contains(&query)
            {
                continue;
            }
            match groups
                .iter_mut()
                .find(|(group, _)| *group == resource.group)
            {
                Some((_, resources)) => resources.push(resource),
                None => groups.push((resource.group, vec![resource])),
            }
        }
        let order = [
            "ホーム",
            "戦略",
            "販売管理",
            "顧客管理",
            "予約管理",
            "人事労務",
            "商品・在庫管理",
            "請求・債権管理",
            "会計・監査",
            "データ連携",
            "分析・レポート",
            "IT・SaaS管理",
            "管理",
        ];
        groups.sort_by_key(|(name, _)| {
            order
                .iter()
                .position(|group| group == name)
                .unwrap_or(order.len())
        });
        groups
    });
    render! {
        View(style: screen_style()) {
            Show(when: move || selected.with(Option::is_none)) {
                View(style: screen_style()) {
                    NavBar(title: "管理メニュー", back_label: "", on_back: |_| {}, action_label: "", on_action: |_| {})
                    ScrollView(style: scroll_style()) {
                        View(style: scroll_content_style()) {
                            View(style: css!(padding: (px(10), px(16)))) {
                                Input(text: query, placeholder: "機能を検索", auto_capitalize: AutoCapitalize::None, style: computed(move || css!(height: px(44), font_size: px(17), background_color: hex(FILL), color: hex(LABEL), padding: px(12), border_radius: px(10))))
                            }
                            AccessStatus(connection: connection)
                            ForEach(
                                each: move || groups.get(),
                                key: |group: &(&'static str, Vec<Resource>)| group.0,
                                children: move |group: ReadSignal<(&'static str, Vec<Resource>)>| {
                                    let name = group.get_untracked().0;
                                    render! {
                                        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
                                            SectionHeader(title: name)
                                            View(style: computed(move || group_style())) {
                                                ForEach(
                                                    each: move || group.get().1,
                                                    key: |resource: &Resource| resource.key,
                                                    children: move |resource: ReadSignal<Resource>| {
                                                        let resource = resource.get_untracked();
                                                        let description = if resource.create.is_some() || resource.edit.is_some() { "一覧・詳細・登録／編集" } else if !resource.actions.is_empty() { "一覧・詳細・操作" } else { "一覧・詳細" };
                                                        render! { ListRow(title: resource.label.to_owned(), subtitle: description.to_owned(), subtitle_color: SECONDARY_LABEL, trailing: String::new(), avatar: false, separator: true, on_tap: move |_| selected.set(Some(resource.key))) }
                                                    },
                                                )
                                            }
                                        }
                                    }
                                },
                            )
                            Show(when: move || connection.access.with(Option::is_some) && groups.with(Vec::is_empty)) { Notice(message: "利用できる機能が見つかりません") }
                            View(style: css!(height: px(TAB_BAR_SPACE)))
                        }
                    }
                }
            }
            ForEach(
                each: move || selected.get().into_iter().filter_map(|key| resources().into_iter().find(|r| r.key == key)).collect::<Vec<_>>(),
                key: |resource: &Resource| resource.key,
                children: move |resource: ReadSignal<Resource>| render! { ResourceScreen(connection: connection, resource: resource.get_untracked(), back_label: "管理メニュー", on_back: move |_| selected.set(None)) },
            )
        }
    }
}

/// Existing tabs also open the same editing screens; there is no second copy of a record.
#[component]
pub(crate) fn resource_entry(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    resource: Resource,
    back_label: &'static str,
    on_back: Callback<()>,
) -> Element {
    let connection = connection_for(token, tenant);
    render! {
        View(style: screen_style()) {
            Show(when: move || !connection.allowed(resource)) {
                NavBar(title: resource.label, back_label: back_label, on_back: on_back, action_label: "", on_action: |_| {})
                AccessStatus(connection: connection)
                Show(when: move || connection.access.with(Option::is_some)) { Notice(message: "この会社ではこの機能を利用できません") }
            }
            Show(when: move || connection.allowed(resource)) {
                ResourceScreen(connection: connection, resource: resource, back_label: back_label, on_back: on_back)
            }
        }
    }
}

#[component]
fn access_status(connection: Connection) -> Element {
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Show(when: move || connection.loading.get()) { Notice(message: "利用できる機能を確認中…") }
            Show(when: move || !connection.error.with(String::is_empty)) {
                ErrorNotice(message: computed(move || connection.error.get()))
                ActionButton(label: "もう一度確認", destructive: false, on_tap: move |_| connection.load())
            }
            Show(when: move || connection.access.with(|access| access.as_ref().is_some_and(|access| !access.notice.is_empty()))) {
                ErrorNotice(message: computed(move || connection.access.with(|access| access.as_ref().map(|access| access.notice.clone()).unwrap_or_default())))
                ActionButton(label: "利用可否を再確認", destructive: false, on_tap: move |_| connection.load())
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Data {
    resource: Resource,
    connection: Connection,
    collection: RwSignal<model::Collection>,
    filters: RwSignal<Value>,
    loading: RwSignal<bool>,
    loaded: RwSignal<bool>,
    error: RwSignal<String>,
    notice: RwSignal<String>,
    generation: RwSignal<u64>,
    selected: RwSignal<Option<Value>>,
    detail_loading: RwSignal<bool>,
    detail_error: RwSignal<String>,
    detail_ready: RwSignal<bool>,
    detail_generation: RwSignal<u64>,
    edit: RwSignal<Option<Mutation>>,
    editing_original: RwSignal<Value>,
    owner: Option<Owner>,
}

impl Data {
    fn load(self) {
        let Some(current) = self.connection.tenant.get_untracked() else {
            return;
        };
        if !self.connection.allowed(self.resource) {
            return;
        }
        let context = current.clone();
        let filters = self.filters.get_untracked();
        if let Err(message) = model::query_path(self.resource.path, self.resource.filters, &filters)
        {
            self.error.set(message);
            return;
        }
        self.generation.update(|g| *g += 1);
        let generation = self.generation.get_untracked();
        self.loading.set(true);
        self.error.set(String::new());
        spawn_in(self.owner, async move {
            let result = authed(self.connection.token, move |token| {
                model::fetch_collection(token, &current, self.resource, &filters)
            })
            .await;
            if !self.owner.is_none_or(Owner::is_alive)
                || self.connection.tenant.get_untracked().as_ref() != Some(&context)
                || self.generation.get_untracked() != generation
            {
                return;
            }
            self.loading.set(false);
            self.loaded.set(true);
            match result {
                Ok(collection) => self.collection.set(collection),
                Err(message) => {
                    self.collection.set(model::Collection::default());
                    self.error.set(message);
                }
            }
        });
    }

    fn open(self, row: Value) {
        self.detail_generation.update(|generation| *generation += 1);
        let generation = self.detail_generation.get_untracked();
        self.selected.set(Some(row.clone()));
        self.detail_loading.set(false);
        self.detail_error.set(String::new());
        self.detail_ready.set(self.resource.detail_path.is_none());
        let Some(template) = self.resource.detail_path else {
            return;
        };
        let path = match model::record_path(self.resource, template, &row) {
            Ok(path) => path,
            Err(message) => {
                self.detail_error.set(message);
                return;
            }
        };
        let Some(current) = self.connection.tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        let selected_key = record_key(self.resource, &row);
        self.detail_loading.set(true);
        spawn_in(self.owner, async move {
            let result = authed(self.connection.token, move |token| {
                api::fetch_record(token, &current, &path)
            })
            .await;
            if !self.owner.is_none_or(Owner::is_alive)
                || self.connection.tenant.get_untracked().as_ref() != Some(&context)
                || self.detail_generation.get_untracked() != generation
                || !self.selected.with_untracked(|s| {
                    s.as_ref()
                        .is_some_and(|s| record_key(self.resource, s) == selected_key)
                })
            {
                return;
            }
            self.detail_loading.set(false);
            match result {
                Ok(value) => {
                    let value = model::value_at(&value, self.resource.detail_key).clone();
                    if !value.is_object() {
                        self.detail_error
                            .set("詳細の応答を読み取れませんでした".into());
                        return;
                    }
                    // Preserve list-only display labels, but the detail response wins.
                    let mut merged = row;
                    if let (Some(base), Some(full)) = (merged.as_object_mut(), value.as_object()) {
                        base.extend(full.clone());
                    }
                    self.selected.set(Some(merged));
                    self.detail_ready.set(true);
                }
                Err(message) => self.detail_error.set(message),
            }
        });
    }

    fn can_mutate(self, operation: Mutation, original: &Value) -> bool {
        self.connection.allowed(self.resource)
            && self.loaded.get()
            && self.error.with(String::is_empty)
            && self
                .collection
                .with(|c| self.connection.can_mutate(operation, &c.meta, original))
    }

    fn begin(self, operation: Mutation, original: Value) {
        if !self.can_mutate(operation, &original) {
            return;
        }
        self.editing_original.set(original);
        self.edit.set(Some(operation));
    }
}

fn record_key(resource: Resource, row: &Value) -> String {
    resource
        .id_key
        .split('|')
        .map(|key| model::input_text(model::value_at(row, key)))
        .collect::<Vec<_>>()
        .join("|")
}

#[component]
fn resource_screen(
    connection: Connection,
    resource: Resource,
    back_label: &'static str,
    on_back: Callback<()>,
) -> Element {
    if let Some(view) = crate::management::analytics_screen::custom_screen(
        resource,
        connection.token,
        connection.tenant,
        back_label,
        on_back,
    ) {
        return view;
    }
    let data = Data {
        resource,
        connection,
        collection: signal(model::Collection::default()),
        filters: signal(model::draft(resource.filters, &Value::Null)),
        loading: signal(false),
        loaded: signal(false),
        error: signal(String::new()),
        notice: signal(String::new()),
        generation: signal(0),
        selected: signal(None),
        detail_loading: signal(false),
        detail_error: signal(String::new()),
        detail_ready: signal(false),
        detail_generation: signal(0),
        edit: signal(None),
        editing_original: signal(Value::Null),
        owner: Owner::current(),
    };
    let query = signal(String::new());
    let filter_open = signal(
        model::query_path(
            resource.path,
            resource.filters,
            &data.filters.get_untracked(),
        )
        .is_err(),
    );
    if !filter_open.get_untracked() {
        data.load();
    }
    let visible = computed(move || {
        let query = query.get().trim().to_lowercase();
        data.collection.with(|c| {
            c.rows
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    query.is_empty()
                        || format!(
                            "{} {} {}",
                            model::title(resource, row),
                            model::subtitle(resource, row),
                            model::status(resource, row)
                        )
                        .to_lowercase()
                        .contains(&query)
                })
                .map(|(index, row)| (index, row.clone()))
                .collect::<Vec<_>>()
        })
    });
    render! {
        View(style: screen_style()) {
            Show(when: move || data.selected.with(Option::is_none) && data.edit.with(Option::is_none)) {
                View(style: screen_style()) {
                    NavBar(title: resource.label, back_label: back_label, on_back: on_back, action_label: "", on_action: |_| {})
                    ScrollView(style: scroll_style()) {
                        View(style: scroll_content_style()) {
                            RefreshControl(refreshing: computed(move || data.loading.get()), on_refresh: move |_| data.load())
                            View(style: css!(padding: (px(8), px(16)))) {
                                Input(text: query, placeholder: "読み込んだ項目を検索", auto_capitalize: AutoCapitalize::None, style: computed(move || css!(height: px(42), background_color: hex(FILL), color: hex(LABEL), font_size: px(17), padding: px(10), border_radius: px(10))))
                            }
                            Show(when: move || !resource.filters.is_empty()) {
                                ActionButton(label: "絞り込み条件", destructive: false, on_tap: move |_| filter_open.update(|v| *v = !*v))
                                Show(when: move || filter_open.get()) {
                                    View(style: computed(move || group_style())) {
                                        InputFields(fields: resource.filters.to_vec(), draft: data.filters, prefix: String::new(), token: connection.token, tenant: connection.tenant)
                                        ActionButton(label: "この条件で表示", destructive: false, on_tap: move |_| { if model::query_path(resource.path, resource.filters, &data.filters.get_untracked()).is_ok() { filter_open.set(false); } data.load(); })
                                    }
                                }
                            }
                            Show(when: move || resource.create.is_some_and(|op| data.can_mutate(op, &Value::Null))) {
                                ActionButton(label: "新規登録", destructive: false, on_tap: move |_| { if let Some(operation) = resource.create { data.begin(operation, Value::Null); } })
                            }
                            Show(when: move || !data.notice.with(String::is_empty)) { Text(value: computed(move || data.notice.get()), style: computed(move || css!(color: hex(GREEN), font_size: px(15), padding: px(16)))) }
                            Show(when: move || data.loading.get()) { Notice(message: "読み込み中…") }
                            Show(when: move || !data.error.with(String::is_empty)) {
                                ErrorNotice(message: computed(move || data.error.get()))
                                ActionButton(label: "再読み込み", destructive: false, on_tap: move |_| data.load())
                            }
                            Show(when: move || data.collection.with(|c| c.truncated)) { Notice(message: "表示上限に達しました。条件を絞って読み直してください") }
                            CollectionNotice(collection: data.collection)
                            Show(when: move || resource.key == "jobcan-import-history") {
                                JobcanImportPanel(token: connection.token, tenant: connection.tenant,
                                    can_manage: computed(move || connection.allowed(resource) && connection.access.with(|a| a.as_ref().is_some_and(|a| a.capabilities["hrm"]["manage"].as_bool() == Some(true)))),
                                    on_imported: move |_| data.load())
                            }
                            SectionHeader(title: computed(move || format!("{} 件", visible.with(Vec::len))))
                            View(style: computed(move || group_style())) {
                                ForEach(
                                    each: move || visible.get(),
                                    key: move |(index, row): &(usize, Value)| format!("{}:{index}", record_key(resource, row)),
                                    children: move |item: ReadSignal<(usize, Value)>| render! {
                                        ListRow(
                                            title: computed(move || model::title(resource, &item.get().1)),
                                            subtitle: computed(move || model::subtitle(resource, &item.get().1)),
                                            subtitle_color: SECONDARY_LABEL,
                                            trailing: computed(move || model::status(resource, &item.get().1)),
                                            avatar: false,
                                            separator: true,
                                            on_tap: move |_| data.open(item.get_untracked().1),
                                        )
                                    },
                                )
                            }
                            Show(when: move || data.loaded.get() && !data.loading.get() && data.error.with(String::is_empty) && visible.with(Vec::is_empty)) { Notice(message: "該当する項目がありません") }
                            View(style: css!(height: px(TAB_BAR_SPACE)))
                        }
                    }
                }
            }
            Show(when: move || data.selected.with(Option::is_some) && data.edit.with(Option::is_none)) { RecordDetail(data: data) }
            ForEach(
                each: move || data.edit.get().into_iter().collect::<Vec<_>>(),
                key: |operation: &Mutation| (operation.method, operation.path, operation.label),
                children: move |operation: ReadSignal<Mutation>| render! { EditRecord(data: data, operation: operation.get_untracked()) },
            )
        }
    }
}

#[component]
fn record_detail(data: Data) -> Element {
    let resource = data.resource;
    let selected = computed(move || data.selected.get().unwrap_or(Value::Null));
    render! {
        View(style: screen_style()) {
            NavBar(title: resource.label, back_label: "一覧", on_back: move |_| data.selected.set(None), action_label: "", on_action: |_| {})
            ScrollView(style: scroll_style()) {
                View(style: scroll_content_style()) {
                    Text(value: computed(move || model::title(resource, &selected.get())), style: computed(move || css!(color: hex(LABEL), font_size: px(25), font_weight: FontWeight::Numeric(600), padding: (px(16), px(20)))))
                    Show(when: move || data.detail_loading.get()) { Notice(message: "詳細を読み込み中…") }
                    Show(when: move || !data.detail_error.with(String::is_empty)) {
                        ErrorNotice(message: computed(move || data.detail_error.get()))
                        ActionButton(label: "詳細を読み直す", destructive: false, on_tap: move |_| data.open(selected.get_untracked()))
                    }
                    InfoGroup(rows: computed(move || {
                        let expanded = expanded_line_keys(resource);
                        let labels: Vec<_> = resource.detail_fields.iter().filter(|(_, key)| expanded.contains(key)).map(|(label, _)| *label).collect();
                        model::detail_rows(resource, &selected.get()).into_iter().filter(|(label, _)| !labels.contains(&label.as_str())).collect::<Vec<_>>()
                    }), inline: false)
                    RecordLines(resource: resource, record: selected, token: data.connection.token, tenant: data.connection.tenant)
                    Show(when: move || data.detail_ready.get() && resource.edit.is_some_and(|op| data.can_mutate(op, &selected.get()))) {
                        ActionButton(label: "編集", destructive: false, on_tap: move |_| { if let Some(operation) = resource.edit { data.begin(operation, selected.get_untracked()); } })
                    }
                    ForEach(
                        each: move || if data.detail_ready.get() { resource.actions.iter().filter(|operation| data.can_mutate(**operation, &selected.get())).copied().collect::<Vec<_>>() } else { Vec::new() },
                        key: |operation: &Mutation| (operation.method, operation.path, operation.label),
                        children: move |operation: ReadSignal<Mutation>| {
                            let operation = operation.get_untracked();
                            render! { ActionButton(label: operation.label, destructive: operation.destructive, on_tap: move |_| data.begin(operation, selected.get_untracked())) }
                        },
                    )
                    View(style: css!(height: px(TAB_BAR_SPACE)))
                }
            }
        }
    }
}

#[component]
fn edit_record(data: Data, operation: Mutation) -> Element {
    let original = signal(data.editing_original.get_untracked());
    let initial = model::draft(operation.fields, &original.get_untracked());
    let draft = signal(initial.clone());
    let initial = signal(initial);
    let error = signal(String::new());
    let saving = signal(false);
    let discard = signal(false);
    let submitted = signal(None::<Value>);
    let key = signal(request_key().unwrap_or_default());
    let owner = Owner::current();
    let close = move || {
        if saving.get_untracked() {
            return;
        }
        if draft.get_untracked() != initial.get_untracked() && !discard.get_untracked() {
            discard.set(true);
            return;
        }
        data.edit.set(None);
    };
    let submit = move || {
        if saving.get_untracked() {
            return;
        }
        if draft.with_untracked(|value| value["_nativeUploadsInFlight"].as_u64().unwrap_or(0) > 0) {
            error.set("添付資料を送信中です。完了してから保存してください".into());
            return;
        }
        if draft.with_untracked(|value| value["_nativeDocumentPending"].as_bool() == Some(true)) {
            error.set(
                "選んだ書類はまだ入力へ反映されていません。読み取るか、手入力を選んでください"
                    .into(),
            );
            return;
        }
        if !data.can_mutate(operation, &original.get_untracked()) {
            error.set("この操作は現在できません。一覧を読み直してください".into());
            return;
        }
        if !matches!(operation.idempotency, Idempotency::None)
            && key.with_untracked(String::is_empty)
        {
            error.set("登録の準備ができませんでした。画面を開き直してください".into());
            return;
        }
        let mut body = match model::request_body(
            operation,
            &draft.get_untracked(),
            &original.get_untracked(),
            &key.get_untracked(),
        ) {
            Ok(body) => body,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        if let Some(field) = operation.actor_body_field {
            let actor = data.connection.access.with_untracked(|access| {
                access.as_ref().and_then(|access| access.actor_id.clone())
            });
            let Some(actor) = actor else {
                error.set("利用者情報を確認できません。画面を読み直してください".into());
                return;
            };
            model::set_at(&mut body, field, Value::String(actor));
        }
        if submitted.with_untracked(|old| old.as_ref().is_some_and(|old| *old != body)) {
            error.set("前の操作結果を確認できていません。内容を変えて再送する前に、一覧で登録結果を確認してください".into());
            return;
        }
        // Without server-side idempotency, never resend an ambiguously failed write.
        if matches!(operation.idempotency, Idempotency::None)
            && submitted.with_untracked(Option::is_some)
        {
            error.set(
                "重複登録を防ぐため、一覧を読み直して結果を確認してから操作してください".into(),
            );
            return;
        }
        let path = match model::request_path(
            data.resource,
            operation.path,
            &original.get_untracked(),
            &body,
        ) {
            Ok(path) => path,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let Some(current) = data.connection.tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        let request_key = key.get_untracked();
        let original_to_send = original.get_untracked();
        submitted.set(Some(body.clone()));
        saving.set(true);
        error.set(String::new());
        spawn_in(owner, async move {
            let result = authed(data.connection.token, move |token| {
                let value = if let Some(executor) = operation.executor {
                    executor(token, &current, &body, &original_to_send)?
                } else {
                    api::mutate_record(
                        token,
                        &current,
                        operation.method,
                        &path,
                        &body,
                        matches!(operation.idempotency, Idempotency::Header)
                            .then_some(request_key.as_str()),
                    )?
                };
                if let Some(check) = operation.response_check {
                    check(&value)?;
                }
                Ok(value)
            })
            .await;
            if !owner.is_none_or(Owner::is_alive)
                || data.connection.tenant.get_untracked().as_ref() != Some(&context)
            {
                return;
            }
            saving.set(false);
            match result {
                Ok(_) => {
                    data.notice.set(format!(
                        "{}しました",
                        if operation.method == "DELETE" {
                            "削除"
                        } else {
                            "保存"
                        }
                    ));
                    data.selected.set(None);
                    data.edit.set(None);
                    data.load();
                }
                Err(message) => {
                    // Validation/authorization rejections cannot have committed a mutation.
                    if message.contains("（400）")
                        || message.contains("（403）")
                        || message.contains("（422）")
                        || message == "この操作の権限がありません"
                    {
                        submitted.set(None);
                    }
                    error.set(if message.contains("（409）") { "別の更新または同じ操作が先に保存されています。一覧を読み直して内容を確認してください".into() } else { message });
                }
            }
        });
    };
    render! {
        View(style: screen_style()) {
            NavBar(title: operation.label, back_label: "閉じる", on_back: move |_| close(), action_label: "", on_action: |_| {})
            ScrollView(style: scroll_style()) {
                View(style: scroll_content_style()) {
                    Show(when: move || !operation.explanation.is_empty()) { Text(value: operation.explanation, style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(15), padding: px(16)))) }
                    Show(when: move || operation.destructive) { Text(value: "対象と内容を確認してから実行してください。", style: computed(move || css!(color: hex(RED), font_size: px(15), padding: px(16)))) }
                    Show(when: move || !original.get().is_null()) { Text(value: computed(move || model::title(data.resource, &original.get())), style: computed(move || css!(color: hex(LABEL), font_size: px(20), padding: px(16), font_weight: FontWeight::Numeric(600)))) }
                    Show(when: move || !saving.get()) {
                        DocumentInput(operation: operation, draft: draft, token: data.connection.token, tenant: data.connection.tenant)
                        View(style: computed(move || group_style())) {
                            InputFields(fields: operation.fields.to_vec(), draft: draft, prefix: String::new(), token: data.connection.token, tenant: data.connection.tenant)
                        }
                    }
                    Show(when: move || !error.with(String::is_empty)) { ErrorNotice(message: computed(move || error.get())) }
                    Show(when: move || saving.get()) { Notice(message: "保存中…") }
                    Show(when: move || !saving.get()) { ActionButton(label: operation.label, destructive: operation.destructive, on_tap: move |_| submit()) }
                    Show(when: move || discard.get()) {
                        Notice(message: "入力中の内容を破棄しますか？")
                        ActionButton(label: "入力を続ける", destructive: false, on_tap: move |_| discard.set(false))
                        ActionButton(label: "破棄して閉じる", destructive: true, on_tap: move |_| close())
                    }
                    View(style: css!(height: px(TAB_BAR_SPACE)))
                }
            }
        }
    }
}

fn request_key() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ))
}

#[component]
fn action_button(label: &'static str, destructive: bool, on_tap: Callback<()>) -> Element {
    render! {
        View(style: computed(move || css!(padding: (px(14), px(18)), margin: (px(6), px(16)), min_height: px(48), border_radius: px(12), background_color: hex(FILL))), on_tap: move |_| on_tap.run(())) {
            Text(value: label, style: computed(move || css!(color: hex(if destructive { RED } else { TINT }), font_size: px(17), font_weight: FontWeight::Numeric(500), text_align: TextAlign::Center)))
        }
    }
}

#[component]
fn collection_notice(collection: RwSignal<model::Collection>) -> Element {
    let notices = computed(move || {
        collection.with(|collection| {
            [
                ("notice", ""),
                ("report.notice", ""),
                ("excludedSources", "集計対象外"),
                ("currencyExclusions", "集計対象外の通貨"),
                ("unconvertedCurrencies", "換算されていない通貨"),
                ("report.coverage", "集計範囲"),
            ]
            .iter()
            .filter_map(|(key, label)| {
                let value = model::display(model::value_at(&collection.meta, key));
                (!value.is_empty()).then(|| {
                    if label.is_empty() {
                        value
                    } else {
                        format!("{label}: {value}")
                    }
                })
            })
            .collect::<Vec<_>>()
        })
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            ForEach(each: move || notices.get().into_iter().enumerate().collect::<Vec<_>>(), key: |row: &(usize, String)| row.0, children: move |row: ReadSignal<(usize, String)>| render! {
                Text(value: computed(move || row.get().1), style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(14), padding: (px(8), px(16)))))
            })
        }
    }
}
