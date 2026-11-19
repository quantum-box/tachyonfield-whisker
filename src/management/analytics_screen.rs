//! Dedicated native workflows for server-side profit calculation and data questions.
use crate::cash_book_screen::spawn_in;
use crate::management::{
    analytics_engine as engine,
    form::InputFields,
    model,
    schema::{Choice, Field, InputKind, Lookup, Resource},
};
use crate::*;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use whisker::Owner;

const CASE_FIELDS: &[Field] = &[
    Field::text("label", "案の名前").required(),
    Field::new("priceChangePct", "価格変更（%・整数）", InputKind::Integer)
        .required()
        .default("0"),
    Field::new(
        "volumeChangePct",
        "販売数の変化（%・整数）",
        InputKind::Integer,
    )
    .required()
    .default("0"),
    Field::new("promotionYen", "月の販促費（円）", InputKind::Money)
        .required()
        .default("0"),
];
const PROFIT_FIELDS: &[Field] = &[
    Field::new("periodFrom", "開始日", InputKind::Date)
        .required()
        .default("@monthStart"),
    Field::new("periodTo", "終了日（この日を含む）", InputKind::Date)
        .required()
        .default("@monthEnd"),
    Field::new(
        "profitDefinition",
        "利益の定義",
        InputKind::Select(&[
            Choice::new("menu_margin", "メニュー限界利益"),
            Choice::new("store_management_profit", "店舗の管理利益"),
        ]),
    )
    .required()
    .default("menu_margin"),
    Field::new("scenarios", "比較する案", InputKind::Lines(CASE_FIELDS)).required(),
];

pub(crate) fn custom_screen(
    resource: Resource,
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    back_label: &'static str,
    on_back: Callback<()>,
) -> Option<Element> {
    match resource.key {
        "profit-calculator" => Some(
            render! { ProfitScenarioScreen(token: token, tenant: tenant, back_label: back_label, on_back: on_back) },
        ),
        "analysis-dialogue" => Some(
            render! { DataQuestionScreen(token: token, tenant: tenant, back_label: back_label, on_back: on_back) },
        ),
        _ => None,
    }
}

fn active(
    owner: Option<Owner>,
    tenant: RwSignal<Option<Tenant>>,
    token: RwSignal<Option<String>>,
    expected: &Tenant,
) -> bool {
    owner.is_none_or(Owner::is_alive)
        && tenant.try_get_untracked().flatten().as_ref() == Some(expected)
        && token.try_get_untracked().flatten().is_some()
}

#[component]
fn workflow_button(label: &'static str, on_tap: Callback<()>) -> Element {
    render! {
        View(style: computed(move || css!(padding: (px(14), px(18)), margin: (px(6), px(16)), min_height: px(48), border_radius: px(12), background_color: hex(FILL))), on_tap: move |_| on_tap.run(())) {
            Text(value: label, style: computed(move || css!(color: hex(TINT), font_size: px(17), font_weight: FontWeight::Numeric(500))))
        }
    }
}

#[component]
fn profit_scenario_screen(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    back_label: &'static str,
    on_back: Callback<()>,
) -> Element {
    let mut initial = model::draft(PROFIT_FIELDS, &Value::Null);
    initial["scenarios"] =
        json!([{"label":"改善案","priceChangePct":"0","volumeChangePct":"0","promotionYen":"0"}]);
    let draft = signal(initial);
    let calculation = signal(Value::Null);
    let busy = signal(false);
    let error = signal(String::new());
    let notice = signal(String::new());
    let note = signal(String::new());
    let saved = signal(false);
    let pending = signal(None::<(String, Value)>);
    let owner = Owner::current();
    let calculate = move || {
        if busy.get_untracked() {
            return;
        }
        let body = match model::body(PROFIT_FIELDS, &draft.get_untracked()) {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        if body["periodFrom"].as_str() > body["periodTo"].as_str() {
            error.set("開始日は終了日以前にしてください".into());
            return;
        }
        if pending.with_untracked(Option::is_some) {
            error.set("前の保存結果を確認できていません。同じ内容で再送するか、保存した利益シナリオの一覧を確認してください".into());
            return;
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        busy.set(true);
        error.set(String::new());
        notice.set(String::new());
        spawn_in(owner, async move {
            let result = authed(token, move |access| {
                engine::load_and_calculate(access, &current, &body)
            })
            .await;
            if !crate::management::analytics_screen::active(owner, tenant, token, &context) {
                return;
            }
            busy.set(false);
            match result {
                Ok(value) => {
                    calculation.set(value);
                    saved.set(false);
                }
                Err(message) => error.set(message),
            }
        });
    };
    let save = move || {
        if busy.get_untracked() || saved.get_untracked() {
            return;
        }
        let current_draft = match model::body(PROFIT_FIELDS, &draft.get_untracked()) {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        if calculation.with_untracked(|value| value["draft"] != current_draft) {
            error.set("前提を変更したため、もう一度試算してから保存してください".into());
            return;
        }
        let body = match engine::workspace_body(&calculation.get_untracked(), &note.get_untracked())
        {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let (key, body) = match pending.get_untracked() {
            Some((key, old)) if old == body => (key, old),
            Some(_) => {
                error.set("前の保存結果が未確認です。内容を変えて再送する前に保存したシナリオを確認してください".into());
                return;
            }
            None => match engine::request_key() {
                Ok(key) => (key, body),
                Err(message) => {
                    error.set(message);
                    return;
                }
            },
        };
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        pending.set(Some((key.clone(), body.clone())));
        busy.set(true);
        error.set(String::new());
        spawn_in(owner, async move {
            let result = authed(token, move |access| {
                api::mutate_record(
                    access,
                    &current,
                    "POST",
                    engine::WORKSPACES,
                    &body,
                    Some(&key),
                )
            })
            .await;
            if !crate::management::analytics_screen::active(owner, tenant, token, &context) {
                return;
            }
            busy.set(false);
            match result{
                Ok(value)if value["id"].as_str().is_some()&&value["version"].as_u64().is_some()=>{pending.set(None);saved.set(true);notice.set(format!("下書きを保存しました（版 {}）。保存した利益シナリオの一覧で確認できます。",value["version"]));},
                Ok(_)=>error.set("保存結果を確認できません。同じ内容で再送するか、保存したシナリオの一覧を確認してください".into()),
                Err(message)=>{if message.contains("（400）")||message.contains("（422）")||message=="この操作の権限がありません"{pending.set(None);}error.set(message);},
            }
        });
    };
    let reports = computed(move || engine::calculation_rows(&calculation.get()));
    render! {
        View(style: screen_style()) {
            NavBar(title: "利益シナリオを試算", back_label: back_label, on_back: on_back, action_label: "", on_action: |_| {})
            ScrollView(style: scroll_style()) {
                View(style: scroll_content_style()) {
                    Text(value: "価格・販売数・販促費を変えた場合を、会社全体の実績と比較します。価格・発注・シフト・在庫・会計は変更しません。", style: computed(move || css!(padding: px(16), font_size: px(15), color: hex(SECONDARY_LABEL))))
                    Show(when: move || !busy.get()) {
                        View(style: computed(move || group_style())) { InputFields(fields: PROFIT_FIELDS.to_vec(), draft: draft, prefix: String::new(), token: token, tenant: tenant) }
                        WorkflowButton(label: "実績を読み直して試算", on_tap: move |_| calculate())
                    }
                    Show(when: move || busy.get()) { Notice(message: "処理中…") }
                    Show(when: move || !error.with(String::is_empty)) { ErrorNotice(message: computed(move || error.get())) }
                    Show(when: move || !notice.with(String::is_empty)) { Text(value: computed(move || notice.get()), style: computed(move || css!(padding: px(16), font_size: px(15), color: hex(GREEN)))) }
                    Show(when: move || !calculation.get().is_null()) {
                        SectionHeader(title: "試算に使った実績")
                        InfoGroup(rows: computed(move || engine::reference_rows(&calculation.get()["snapshot"])), inline: false)
                        ForEach(each: move || reports.get().into_iter().enumerate().collect::<Vec<_>>(), key: |row: &(usize,(String,Vec<(String,String)>))| row.0, children: move |row: ReadSignal<(usize,(String,Vec<(String,String)>))>| render! {
                            View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
                                SectionHeader(title: computed(move || row.get().1.0))
                                InfoGroup(rows: computed(move || row.get().1.1), inline: false)
                            }
                        })
                        Show(when: move || !busy.get() && !saved.get()) {
                            View(style: computed(move || group_style().padding(px(16)))) {
                                Text(value: "保存するメモ", style: computed(move || css!(font_size: px(13), color: hex(SECONDARY_LABEL))))
                                Input(text: note, placeholder: "検討したいこと、判断の前提", multiline: true, lines: 3_u32, style: computed(move || css!(min_height: px(88), font_size: px(17), color: hex(LABEL), background_color: hex(SYSTEM_BACKGROUND))))
                            }
                            WorkflowButton(label: "新しい下書きとして保存", on_tap: move |_| save())
                        }
                    }
                    View(style: css!(height: px(TAB_BAR_SPACE)))
                }
            }
        }
    }
}

#[component]
fn native_choices(
    label: &'static str,
    choices: ReadSignal<Vec<(String, String)>>,
    selected: RwSignal<String>,
) -> Element {
    let open = signal(false);
    let query = signal(String::new());
    let current = computed(move || {
        choices.with(|rows| {
            rows.iter()
                .find(|(id, _)| *id == selected.get())
                .map(|(_, label)| label.clone())
                .unwrap_or_else(|| "選択してください".into())
        })
    });
    render! {
        View(style: computed(move || group_style())) {
            View(style: css!(padding: px(16), display: Display::Flex, flex_direction: FlexDirection::Column, gap: px(4)), on_tap: move |_| open.update(|v|*v=!*v)) {
                Text(value: label, style: computed(move || css!(font_size: px(13), color: hex(SECONDARY_LABEL))))
                Text(value: computed(move || format!("{} {}",current.get(),if open.get(){"⌃"}else{"⌄"})), style: computed(move || css!(font_size: px(17),color:hex(TINT))))
            }
            Show(when: move || open.get()) {
                Input(text: query, placeholder: "候補を検索", style: computed(move || css!(margin:px(12),padding:px(10),height:px(44),font_size:px(16),color:hex(LABEL),background_color:hex(FILL))))
                ForEach(each: move || {let q=query.get().to_lowercase();choices.with(|rows|rows.iter().filter(|(_,name)|name.to_lowercase().contains(&q)).cloned().collect::<Vec<_>>())}, key: |row:&(String,String)|row.0.clone(), children:move|row:ReadSignal<(String,String)>|render!{
                    ListRow(title:computed(move||row.get().1),subtitle:String::new(),subtitle_color:SECONDARY_LABEL,trailing:computed(move||if selected.get()==row.get().0{"✓".into()}else{String::new()}),avatar:false,separator:true,on_tap:move |_| {selected.set(row.get_untracked().0);open.set(false);query.set(String::new());})
                })
            }
        }
    }
}

fn load_datasets(token: &str, tenant: &Tenant) -> Result<Value, String> {
    let flags = api::send_json(
        token,
        tenant,
        "POST",
        "/v1/feature-flags/evaluate",
        &json!({"keys":["feature.field.external-data-dialogue"]}),
        Duration::from_secs(15),
    )?;
    let enabled = flags["values"].as_array().is_some_and(|flags| {
        flags.iter().any(|flag| {
            flag["key"] == "feature.field.external-data-dialogue" && flag["enabled"] == true
        })
    });
    if !enabled {
        return Ok(json!({"enabled":false,"items":[]}));
    }
    let lookup = Lookup {
        path: "/v1/erp/analytics/external-data/datasets",
        list_key: "items",
        value_key: "id",
        label_keys: &["name"],
    };
    let (mut response, truncated) = model::fetch_lookup(token, tenant, lookup)?;
    response["enabled"] = json!(true);
    response["truncated"] = json!(truncated);
    Ok(response)
}

fn load_versions(token: &str, tenant: &Tenant, dataset: &str) -> Result<Value, String> {
    let mut items = Vec::<Value>::new();
    let mut previous = Value::Null;
    let mut truncated = false;
    for page in 0..20 {
        let path = format!(
            "{}/datasets/{}/versions?offset={}",
            engine::ANALYSIS,
            api::encode_query(dataset),
            page * 50
        );
        let response = api::fetch_record(token, tenant, &path)?;
        let rows = response["items"]
            .as_array()
            .ok_or("公開版の一覧を読み取れませんでした")?;
        if !rows.is_empty() && response["items"] == previous {
            return Err("公開版のページを読み進められませんでした".into());
        }
        previous = response["items"].clone();
        items.extend(rows.iter().cloned());
        let more = response["total_count"]
            .as_u64()
            .is_some_and(|n| n > items.len() as u64)
            || rows.len() >= 50;
        if rows.is_empty() || !more {
            break;
        }
        if page == 19 {
            truncated = true;
        }
    }
    Ok(json!({"items":items,"truncated":truncated}))
}

#[component]
fn data_question_screen(
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
    back_label: &'static str,
    on_back: Callback<()>,
) -> Element {
    let datasets = signal(Vec::<Value>::new());
    let versions = signal(Vec::<Value>::new());
    let dataset = signal(String::new());
    let version = signal(String::new());
    let description = signal(Value::Null);
    let enabled = signal(false);
    let busy = signal(false);
    let phase = signal(String::new());
    let error = signal(String::new());
    let notice = signal(String::new());
    let question = signal(String::new());
    let turns = signal(Vec::<Value>::new());
    let output = signal(Value::Null);
    let run = signal(String::new());
    let run_state = signal(Value::Null);
    let answering = signal(false);
    let cancel = signal(Arc::new(AtomicBool::new(false)));
    let generation = signal(0_u64);
    let exploring = signal(false);
    let chat = crate::agent::AgentChat::new();
    let owner = Owner::current();
    on_cleanup(move || cancel.get_untracked().store(true, Ordering::Relaxed));
    let load = move || {
        if busy.get_untracked() {
            return;
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        dataset.set(String::new());
        version.set(String::new());
        busy.set(true);
        phase.set("利用できるデータを確認中…".into());
        error.set(String::new());
        spawn_in(owner, async move {
            let result = authed(token, move |access| {
                crate::management::analytics_screen::load_datasets(access, &current)
            })
            .await;
            if !crate::management::analytics_screen::active(owner, tenant, token, &context) {
                return;
            }
            busy.set(false);
            match result {
                Ok(value) => {
                    enabled.set(value["enabled"] == true);
                    datasets.set(value["items"].as_array().cloned().unwrap_or_default());
                    notice.set(if value["truncated"] == true {
                        "データ一覧の表示上限に達しました。".into()
                    } else {
                        String::new()
                    });
                }
                Err(message) => error.set(message),
            }
        });
    };
    load();
    let observed_dataset = signal(String::new());
    effect(move || {
        let selected = dataset.get();
        if selected == observed_dataset.get_untracked() {
            return;
        }
        observed_dataset.set(selected.clone());
        generation.update(|g| *g += 1);
        let request = generation.get_untracked();
        versions.set(Vec::new());
        version.set(String::new());
        description.set(Value::Null);
        turns.set(Vec::new());
        output.set(Value::Null);
        run.set(String::new());
        run_state.set(Value::Null);
        error.set(String::new());
        if selected.is_empty() {
            return;
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        let selected_id = selected.clone();
        busy.set(true);
        phase.set("公開版を確認中…".into());
        spawn_in(owner, async move {
            let result = authed(token, move |access| {
                crate::management::analytics_screen::load_versions(access, &current, &selected)
            })
            .await;
            if !crate::management::analytics_screen::active(owner, tenant, token, &context)
                || generation.get_untracked() != request
                || dataset.get_untracked() != selected_id
            {
                return;
            }
            busy.set(false);
            match result {
                Ok(value) => {
                    let rows = value["items"].as_array().cloned().unwrap_or_default();
                    let latest = datasets.with_untracked(|rows| {
                        rows.iter()
                            .find(|r| r["id"] == selected_id)
                            .and_then(|r| r["latest_version"].as_str())
                            .map(str::to_owned)
                    });
                    let picked = latest
                        .filter(|latest| rows.iter().any(|row| row["id"] == *latest))
                        .or_else(|| {
                            rows.first()
                                .and_then(|row| row["id"].as_str())
                                .map(str::to_owned)
                        });
                    versions.set(rows);
                    if let Some(id) = picked {
                        version.set(id);
                    }
                    if value["truncated"] == true {
                        notice.set("公開版の表示上限に達しました。".into());
                    }
                }
                Err(message) => error.set(message),
            }
        });
    });
    let observed_version = signal(String::new());
    effect(move || {
        let selected = version.get();
        if selected == observed_version.get_untracked() {
            return;
        }
        observed_version.set(selected.clone());
        description.set(Value::Null);
        turns.set(Vec::new());
        output.set(Value::Null);
        run.set(String::new());
        run_state.set(Value::Null);
        error.set(String::new());
        if selected.is_empty() {
            return;
        }
        generation.update(|g| *g += 1);
        let request = generation.get_untracked();
        let id = dataset.get_untracked();
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        busy.set(true);
        phase.set("データの列を確認中…".into());
        spawn_in(owner, async move {
            let result = authed(token, move |access| {
                engine::dataset_description(access, &current, &id, &selected)
            })
            .await;
            if !crate::management::analytics_screen::active(owner, tenant, token, &context)
                || generation.get_untracked() != request
            {
                return;
            }
            busy.set(false);
            match result {
                Ok(value) => description.set(value),
                Err(message) => error.set(message),
            }
        });
    });
    let dataset_choices = computed(move || {
        datasets.with(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some((
                        row["id"].as_str()?.to_owned(),
                        crate::management::insights::readable_dataset_name(
                            row["name"].as_str().unwrap_or("名称未設定"),
                        )
                        .to_owned(),
                    ))
                })
                .collect::<Vec<_>>()
        })
    });
    let version_choices = computed(move || {
        versions.with(|rows| {
            rows.iter()
                .enumerate()
                .filter_map(|(i, row)| {
                    Some((
                        row["id"].as_str()?.to_owned(),
                        format!(
                            "公開 {} · 版 {}",
                            row["published_at"].as_str().unwrap_or("日時不明"),
                            i + 1
                        ),
                    ))
                })
                .collect::<Vec<_>>()
        })
    });
    let ask = move || {
        if busy.get_untracked() || !enabled.get_untracked() || description.get_untracked().is_null()
        {
            return;
        }
        let text = question.get_untracked().trim().to_owned();
        if text.is_empty() {
            return;
        }
        if text.chars().count() > 8000 {
            error.set("質問は8,000文字以内にしてください".into());
            return;
        }
        let id = match engine::request_key() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let mut body = json!({"dataset_id":dataset.get_untracked(),"version_id":version.get_untracked(),"question":text,"run_id":id,"allow_continuation":true});
        let prior = turns.get_untracked();
        if let Some(latest) = prior.last() {
            if let (Some(token), Some(count)) = (
                latest["answer"]["continuation"]["token"].as_str(),
                latest["answer"]["continuation"]["turns"].as_u64(),
            ) {
                body["history"] = json!({"token":token,"turns":prior.iter().skip(prior.len().saturating_sub(count as usize)).map(|turn|json!({"question":turn["question"],"answer":turn["answer"]["text"]})).collect::<Vec<_>>()});
            }
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        cancel.set(stopped.clone());
        generation.update(|g| *g += 1);
        let request = generation.get_untracked();
        run.set(id.clone());
        run_state.set(Value::Null);
        busy.set(true);
        answering.set(true);
        phase.set("データを確認して回答を作成中…".into());
        error.set(String::new());
        output.set(Value::Null);
        spawn_in(owner, async move {
            let result = authed(token, move |access| {
                engine::ask_question(access, &current, &body, &stopped)
            })
            .await;
            if !crate::management::analytics_screen::active(owner, tenant, token, &context)
                || generation.get_untracked() != request
            {
                return;
            }
            busy.set(false);
            answering.set(false);
            match result {
                Ok(answer) => {
                    turns.update(|rows| {
                        rows.push(json!({"id":id,"question":text,"answer":answer}));
                        if rows.len() > 10 {
                            rows.remove(0);
                        }
                    });
                    question.set(String::new());
                }
                Err(message) => error.set(message),
            }
        });
    };
    let inspect = move |stop: bool| {
        let id = run.get_untracked();
        if id.is_empty() {
            return;
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        if stop {
            cancel.get_untracked().store(true, Ordering::Relaxed);
            generation.update(|g| *g += 1);
            busy.set(false);
            answering.set(false);
        }
        let inspected = id.clone();
        spawn_in(owner, async move {
            let result = authed(token, move |access| {
                engine::dialogue_status(access, &current, &id, stop)
            })
            .await;
            if !crate::management::analytics_screen::active(owner, tenant, token, &context)
                || run.get_untracked() != inspected
            {
                return;
            }
            match result {
                Ok(value) => run_state.set(value),
                Err(message) => error.set(message),
            }
        });
    };
    let jobs = computed(move || {
        let mut result = Vec::<String>::new();
        for turn in turns.get() {
            for id in turn["answer"]["query_jobs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !result.iter().any(|value| value == id) {
                    result.push(id.into());
                }
            }
        }
        for id in run_state.get()["query_jobs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !result.iter().any(|value| value == id) {
                result.push(id.into());
            }
        }
        result
    });
    let read_result = move |job: String| {
        if busy.get_untracked() {
            return;
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        let selected_version = version.get_untracked();
        generation.update(|g| *g += 1);
        let request = generation.get_untracked();
        busy.set(true);
        phase.set("集計結果を読み込み中…".into());
        error.set(String::new());
        spawn_in(owner, async move {
            let result = authed(token, move |access| {
                engine::analysis_result(access, &current, &job, &selected_version)
            })
            .await;
            if !crate::management::analytics_screen::active(owner, tenant, token, &context)
                || generation.get_untracked() != request
            {
                return;
            }
            busy.set(false);
            match result {
                Ok(value) => output.set(value),
                Err(message) => error.set(message),
            }
        });
    };
    render! {
        View(style: screen_style()) {
            NavBar(title: "データに質問する", back_label: back_label, on_back: on_back, action_label: "", on_action: |_| {})
            Show(when: move || exploring.get()) {
                WorkflowButton(label: "固定データの分析へ戻る", on_tap: move |_| exploring.set(false))
                crate::agent::AgentScreen(token: token, tenant: tenant, chat: chat)
            }
            Show(when: move || !exploring.get()) {
                ScrollView(style: scroll_style()) {
                    View(style: scroll_content_style()) {
                        Show(when: move || enabled.get()&&!busy.get()) {
                            WorkflowButton(label: "使えるデータや分析の進め方を相談", on_tap: move |_| exploring.set(true))
                            NativeChoices(label: "分析するデータ", choices: dataset_choices, selected: dataset)
                            Show(when: move || !versions.with(Vec::is_empty)) { NativeChoices(label: "固定する公開版", choices: version_choices, selected: version) }
                            Show(when: move || !description.get().is_null()) {
                                InfoGroup(rows: computed(move || description.get()["schema"]["columns"].as_array().into_iter().flatten().filter_map(|column|Some((column["name"].as_str()?.into(),column["type"].as_str().unwrap_or("").into()))).collect::<Vec<_>>()), inline: false)
                                View(style: computed(move || group_style().padding(px(16)))) {
                                    Input(text: question, placeholder: "このデータの傾向や比較したいこと", multiline: true, lines: 4_u32, style: computed(move || css!(min_height:px(112),font_size:px(17),color:hex(LABEL),background_color:hex(SYSTEM_BACKGROUND))))
                                }
                                WorkflowButton(label: "この版のデータに質問", on_tap: move |_| ask())
                                Show(when: move || !turns.with(Vec::is_empty)) { WorkflowButton(label: "新しい会話を始める", on_tap: move |_| {turns.set(Vec::new());output.set(Value::Null);run.set(String::new());run_state.set(Value::Null);error.set(String::new());}) }
                            }
                        }
                        Show(when: move || busy.get()) {
                            Text(value: computed(move || phase.get()), style: computed(move || css!(padding:px(16),font_size:px(16),color:hex(SECONDARY_LABEL))))
                            Show(when: move || answering.get()) { WorkflowButton(label: "分析を停止", on_tap: move |_| inspect(true)) }
                        }
                        Show(when: move || !busy.get()&&!enabled.get()&&error.with(String::is_empty)) { Notice(message: "この会社ではAIによるデータ相談が有効になっていません") }
                        Show(when: move || !error.with(String::is_empty)) {
                            ErrorNotice(message: computed(move || error.get()))
                            Show(when: move || !busy.get()&&run.with(String::is_empty)) { WorkflowButton(label: "利用できるデータを再確認", on_tap: move |_| load()) }
                        }
                        Show(when: move || !notice.with(String::is_empty)) { Text(value: computed(move || notice.get()), style: computed(move || css!(padding:px(16),font_size:px(14),color:hex(SECONDARY_LABEL)))) }
                        Show(when: move || !busy.get()&&!run.with(String::is_empty)) {
                            WorkflowButton(label: "実行状況を確認", on_tap: move |_| inspect(false))
                            Show(when: move || !run_state.get().is_null()) { InfoGroup(rows: computed(move || vec![("実行状況".into(),match run_state.get()["state"].as_str(){Some("running")=>"実行中",Some("cancel_requested")=>"停止を依頼済み",Some("succeeded")=>"完了",Some("failed")=>"失敗",Some("interrupted")=>"接続が中断",_=>"確認が必要"}.into())]), inline: false) }
                        }
                        ForEach(each: move || turns.get(), key: |turn:&Value|turn["id"].as_str().unwrap_or("").to_owned(), children:move|turn:ReadSignal<Value>|render!{
                            View(style:computed(move||group_style().padding(px(16)).margin_top(px(12)))) {
                                Text(value:computed(move||turn.get()["question"].as_str().unwrap_or("").to_owned()),style:computed(move||css!(font_size:px(17),color:hex(TINT),margin_bottom:px(12))))
                                crate::markdown::Markdown(text:computed(move||turn.get()["answer"]["text"].as_str().unwrap_or("").to_owned()))
                            }
                        })
                        Show(when: move || !busy.get()) {
                            ForEach(each:move||jobs.get().into_iter().enumerate().collect::<Vec<_>>(),key:|job:&(usize,String)|job.1.clone(),children:move|job:ReadSignal<(usize,String)>|render!{
                                ListRow(title:computed(move||format!("集計結果 {} を開く",job.get().0+1)),subtitle:"参照版を確認して最大200行を表示".to_owned(),subtitle_color:SECONDARY_LABEL,trailing:String::new(),avatar:false,separator:true,on_tap:move |_| read_result(job.get_untracked().1))
                            })
                        }
                        Show(when: move || !output.get().is_null()) { AnalysisResult(output: output) }
                        View(style: css!(height: px(TAB_BAR_SPACE)))
                    }
                }
            }
        }
    }
}

#[component]
pub(crate) fn analysis_result(output: RwSignal<Value>) -> Element {
    render! {
        View(style:css!(display:Display::Flex,flex_direction:FlexDirection::Column)) {
            SectionHeader(title:"集計結果")
            Show(when:move||output.get()["truncated"]==true){Notice(message:"結果の一部を表示しています。質問の条件を絞ると、より詳しく確認できます。")}
            ForEach(each:move||output.get()["rows"].as_array().cloned().unwrap_or_default().into_iter().enumerate().collect::<Vec<_>>(),key:|row:&(usize,Value)|row.0,children:move|row:ReadSignal<(usize,Value)>|render!{
                View(style:css!(display:Display::Flex,flex_direction:FlexDirection::Column)) {
                    SectionHeader(title:computed(move||format!("{}行目",row.get().0+1)))
                    InfoGroup(rows:computed(move||{
                        let value=output.get();let row=row.get().1;
                        value["columns"].as_array().into_iter().flatten().enumerate().map(|(i,column)|(column["name"].as_str().unwrap_or("項目").to_owned(),model::display(&row[i]))).collect::<Vec<_>>()
                    }),inline:false)
                }
            })
        }
    }
}
