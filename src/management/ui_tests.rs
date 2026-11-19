//! Headless native UI regressions. The renderer records the live tree and fires
//! the same input/tap callbacks as the native host. No test has an access token,
//! so asynchronous failures are exercised without contacting the Field API.

use super::*;
use crate::management::schema::{Choice, Field, InputKind, Lookup};
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use whisker::WhiskerValue;
use whisker::runtime::reactive::flush;
use whisker::runtime::tasks::run_until_stalled;
use whisker::runtime::view::{BindType, DynRenderer, install_renderer, uninstall_renderer};
use whisker::runtime::{RuntimeContext, RuntimeWakeHandle};
use whisker_engine::whisker_style::SpecifiedStyle;

type Listener = Rc<dyn Fn(WhiskerValue)>;

#[derive(Default)]
struct Node {
    tag: String,
    attributes: HashMap<String, String>,
    children: Vec<u32>,
    parent: Option<u32>,
    listeners: HashMap<String, Listener>,
    released: bool,
}

#[derive(Default)]
struct Tree {
    nodes: HashMap<u32, Node>,
    next: u32,
    root: Option<u32>,
}

impl Tree {
    fn allocate(&mut self, tag: String) -> Element {
        let id = self.next;
        self.next += 1;
        self.nodes.insert(
            id,
            Node {
                tag,
                ..Node::default()
            },
        );
        Element::from_raw(id)
    }

    fn attach(&mut self, parent: u32, child: u32, before: Option<u32>) {
        if let Some(old) = self.nodes[&child].parent {
            self.nodes
                .get_mut(&old)
                .unwrap()
                .children
                .retain(|id| *id != child);
        }
        self.nodes.get_mut(&child).unwrap().parent = Some(parent);
        let children = &mut self.nodes.get_mut(&parent).unwrap().children;
        let index = before
            .and_then(|id| children.iter().position(|child| *child == id))
            .unwrap_or(children.len());
        children.insert(index, child);
    }

    fn descendants(&self, root: u32, result: &mut Vec<u32>) {
        let Some(node) = self.nodes.get(&root).filter(|node| !node.released) else {
            return;
        };
        result.push(root);
        for child in &node.children {
            self.descendants(*child, result);
        }
    }

    fn visible(&self) -> Vec<u32> {
        let mut result = Vec::new();
        if let Some(root) = self.root {
            self.descendants(root, &mut result);
        }
        result
    }
}

struct Recorder(Rc<RefCell<Tree>>);

impl DynRenderer for Recorder {
    fn create_element(&self, tag: ElementTag) -> Element {
        self.0.borrow_mut().allocate(format!("{tag:?}"))
    }
    fn create_element_by_name(&self, tag: &str) -> Element {
        self.0.borrow_mut().allocate(tag.into())
    }
    fn release_element(&self, handle: Element) {
        if let Some(node) = self.0.borrow_mut().nodes.get_mut(&handle.id()) {
            node.released = true;
            node.listeners.clear();
        }
    }
    fn set_attribute(&self, handle: Element, key: &str, value: &str) {
        self.0
            .borrow_mut()
            .nodes
            .get_mut(&handle.id())
            .unwrap()
            .attributes
            .insert(key.into(), value.into());
    }
    fn set_specified_style(&self, _: Element, _: &SpecifiedStyle) -> bool {
        true
    }
    fn append_child(&self, parent: Element, child: Element) {
        self.0.borrow_mut().attach(parent.id(), child.id(), None);
    }
    fn remove_child(&self, parent: Element, child: Element) {
        let mut tree = self.0.borrow_mut();
        tree.nodes
            .get_mut(&parent.id())
            .unwrap()
            .children
            .retain(|id| *id != child.id());
        if tree.nodes[&child.id()].parent == Some(parent.id()) {
            tree.nodes.get_mut(&child.id()).unwrap().parent = None;
        }
    }
    fn supports_insert_before(&self) -> bool {
        true
    }
    fn insert_child_before(&self, parent: Element, child: Element, reference: Option<Element>) {
        self.0.borrow_mut().attach(
            parent.id(),
            child.id(),
            reference.map(|element| element.id()),
        );
    }
    fn set_event_listener(
        &self,
        handle: Element,
        event: &str,
        _: BindType,
        callback: Box<dyn Fn(WhiskerValue) + 'static>,
    ) {
        self.0
            .borrow_mut()
            .nodes
            .get_mut(&handle.id())
            .unwrap()
            .listeners
            .insert(event.into(), Rc::from(callback));
    }
    fn set_root(&self, root: Element) {
        self.0.borrow_mut().root = Some(root.id());
    }
    fn flush(&self) {}
}

struct Ui(Rc<RefCell<Tree>>);

impl Ui {
    fn mount(&self, root: Element) {
        self.0.borrow_mut().root = Some(root.id());
        flush();
    }

    fn texts(&self) -> Vec<String> {
        let tree = self.0.borrow();
        tree.visible()
            .into_iter()
            .filter_map(|id| tree.nodes[&id].attributes.get("text").cloned())
            .collect()
    }

    fn has_text(&self, value: &str) -> bool {
        self.texts().iter().any(|text| text == value)
    }

    fn text_node(&self, label: &str, occurrence: usize) -> u32 {
        let tree = self.0.borrow();
        tree.visible()
            .into_iter()
            .filter(|id| {
                tree.nodes[id]
                    .attributes
                    .get("text")
                    .is_some_and(|text| text == label)
            })
            .nth(occurrence)
            .unwrap_or_else(|| {
                panic!(
                    "text {label:?} #{occurrence} not found; visible: {:?}",
                    self.texts()
                )
            })
    }

    fn tap(&self, label: &str) {
        self.tap_nth(label, 0);
    }

    fn tap_nth(&self, label: &str, occurrence: usize) {
        let mut target = self.text_node(label, occurrence);
        let callback = {
            let tree = self.0.borrow();
            loop {
                let node = &tree.nodes[&target];
                if let Some(callback) = node.listeners.get("tap") {
                    break callback.clone();
                }
                target = node
                    .parent
                    .unwrap_or_else(|| panic!("{label:?} has no tappable ancestor"));
            }
        };
        callback(WhiskerValue::Map(Default::default()));
        flush();
    }

    fn input(&self, label: &str, value: &str) {
        self.input_nth(label, 0, value);
    }

    fn input_nth(&self, label: &str, occurrence: usize, value: &str) {
        let mut target = self.text_node(label, occurrence);
        let input = {
            let tree = self.0.borrow();
            loop {
                let mut descendants = Vec::new();
                tree.descendants(target, &mut descendants);
                if let Some(input) = descendants
                    .into_iter()
                    .find(|id| tree.nodes[id].tag == "whisker-input:Input")
                {
                    break input;
                }
                target = tree.nodes[&target]
                    .parent
                    .unwrap_or_else(|| panic!("{label:?} has no input"));
            }
        };
        self.input_node(input, value);
    }

    fn input_placeholder(&self, placeholder: &str, value: &str) {
        let input = {
            let tree = self.0.borrow();
            tree.visible()
                .into_iter()
                .find(|id| {
                    tree.nodes[id]
                        .attributes
                        .get("placeholder")
                        .is_some_and(|text| text == placeholder)
                })
                .unwrap_or_else(|| panic!("input {placeholder:?} not found"))
        };
        self.input_node(input, value);
    }

    fn input_node(&self, input: u32, value: &str) {
        let callback = self.0.borrow().nodes[&input]
            .listeners
            .get("input")
            .expect("native input callback")
            .clone();
        callback(WhiskerValue::map([(
            "detail",
            WhiskerValue::map([("value", WhiskerValue::String(value.into()))]),
        )]));
        flush();
    }

    fn drain_tasks(&self) {
        run_until_stalled();
        flush();
    }
}

fn with_ui(test: impl FnOnce(&Ui)) {
    let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
    runtime.enter(|| {
        let tree = Rc::new(RefCell::new(Tree::default()));
        let previous = install_renderer(Box::new(Recorder(tree.clone())));
        let owner = Owner::new(None);
        owner.with(|| test(&Ui(tree)));
        owner.dispose();
        uninstall_renderer(previous);
    });
    runtime.shutdown();
}

fn tenant_fixture(id: &str) -> Tenant {
    Tenant {
        id: id.into(),
        name: format!("会社 {id}"),
        environment: "sandbox".into(),
        platform_id: "platform-test".into(),
    }
}

fn connection_fixture(access: Access) -> Connection {
    Connection {
        token: signal(None),
        tenant: signal(Some(tenant_fixture("tenant-a"))),
        access: signal(Some(access)),
        loading: signal(false),
        error: signal(String::new()),
        owner: Owner::current(),
    }
}

const FORM_FIELDS: &[Field] = &[Field::text("name", "名称").required()];
const SAVE: Mutation = Mutation::new("保存する", "PATCH", "/v1/test/records/{id}", FORM_FIELDS);
const FIXTURE: Resource = Resource::new(
    "ui-test-records",
    "テスト",
    "テスト項目",
    "/v1/test/records",
    "items",
)
.title(&["name"])
.fields(&[("名称", "name"), ("金額", "totalMinor")])
.edit(SAVE)
.route("library/clients");

fn data_fixture(resource: Resource, original: Value) -> Data {
    Data {
        resource,
        connection: connection_fixture(Access::default()),
        collection: signal(model::Collection {
            rows: vec![original.clone()],
            meta: json!({}),
            truncated: false,
        }),
        filters: signal(json!({})),
        loading: signal(false),
        loaded: signal(true),
        error: signal(String::new()),
        notice: signal(String::new()),
        generation: signal(0),
        selected: signal(Some(original.clone())),
        detail_loading: signal(false),
        detail_generation: signal(0),
        detail_error: signal(String::new()),
        detail_ready: signal(true),
        edit: signal(None),
        editing_original: signal(original),
        owner: Owner::current(),
    }
}

#[test]
fn native_inputs_produce_precise_typed_payloads_without_exposing_hidden_values() {
    const CURRENCIES: &[Choice] = &[Choice::new("JPY", "日本円"), Choice::new("USD", "米ドル")];
    const FIELDS: &[Field] = &[
        Field::new("id", "内部識別子", InputKind::Hidden),
        Field::text("name", "名称").required(),
        Field::new("currency", "通貨", InputKind::Select(CURRENCIES)).required(),
        Field::new("amountMinor", "金額", InputKind::Money).required(),
        Field::new("quantity", "数量", InputKind::Decimal).required(),
        Field::new("notes", "箇条書き", InputKind::StringLines),
        Field::new("active", "有効", InputKind::Bool),
        Field::new("scheduledDate", "予定日", InputKind::Date),
        Field::new("startsAt", "開始日時", InputKind::DateTime),
    ];
    with_ui(|ui| {
        let original = json!({"id":"opaque-row-1","currency":"USD","amountMinor":12345});
        let draft = signal(model::draft(FIELDS, &original));
        let token = signal(None::<String>);
        let tenant = signal(None::<Tenant>);
        ui.mount(render! { InputFields(fields: FIELDS.to_vec(), draft: draft, prefix: String::new(), token: token, tenant: tenant) });
        assert!(!ui.has_text("内部識別子"));
        assert!(!ui.texts().iter().any(|text| text.contains("opaque-row-1")));
        ui.input("名称（必須）", "  定期便  ");
        ui.input("金額（必須）", "100.05");
        ui.input("数量（必須）", "123456789012345678.0000000001");
        ui.input("箇条書き", " 第一項目\n\n第二項目 ");
        ui.tap("有効");
        ui.input("予定日", "2026-11-19");
        ui.input("開始日時", "2026-11-19 09:15");
        let mutation = Mutation::new("保存", "PATCH", "/v1/test/records/{id}", FIELDS);
        let payload =
            model::request_body(mutation, &draft.get_untracked(), &original, "unused").unwrap();
        assert_eq!(
            payload,
            json!({
                "id":"opaque-row-1", "name":"定期便", "currency":"USD", "amountMinor":10005,
                "quantity":"123456789012345678.0000000001", "notes":["第一項目","第二項目"], "active":true,
                "scheduledDate":"2026-11-19", "startsAt":"2026-11-19T09:15:00+09:00"
            })
        );
    });
}

#[test]
fn line_deletion_and_addition_preserve_surviving_identity_and_input_binding() {
    const LINE: &[Field] = &[
        Field::new("id", "明細の識別子", InputKind::Hidden),
        Field::text("description", "内容").required(),
        Field::new("amountMinor", "金額", InputKind::Money).required(),
    ];
    const FIELDS: &[Field] = &[Field::new("lines", "明細", InputKind::Lines(LINE)).required()];
    with_ui(|ui| {
        let original = json!({"lines":[{"id":"line-a","description":"削除前","amountMinor":100},{"id":"line-b","description":"残す行","amountMinor":200}]});
        let draft = signal(model::draft(FIELDS, &original));
        let token = signal(None::<String>);
        let tenant = signal(None::<Tenant>);
        ui.mount(render! { InputFields(fields: FIELDS.to_vec(), draft: draft, prefix: String::new(), token: token, tenant: tenant) });
        ui.tap_nth("削除", 0);
        ui.input("内容（必須）", "残す行を編集");
        ui.input("金額（必須）", "321");
        ui.tap("＋ 明細を追加");
        ui.input_nth("内容（必須）", 1, "追加した行");
        ui.input_nth("金額（必須）", 1, "654");
        let payload = model::request_body(
            Mutation::new("保存", "PATCH", "/v1/test", FIELDS),
            &draft.get_untracked(),
            &original,
            "unused",
        )
        .unwrap();
        assert_eq!(
            payload,
            json!({"lines":[{"id":"line-b","description":"残す行を編集","amountMinor":321},{"description":"追加した行","amountMinor":654}]})
        );
        assert!(!ui.has_text("明細の識別子"));
    });
}

#[test]
fn picker_search_and_clear_do_not_silently_replace_an_unknown_enum() {
    const CHOICES: &[Choice] = &[
        Choice::new("personal", "個人"),
        Choice::new("business", "法人"),
        Choice::new("partner", "販売代理店"),
    ];
    const FIELDS: &[Field] = &[Field::new("type", "区分", InputKind::Select(CHOICES))];
    with_ui(|ui| {
        let draft = signal(model::draft(FIELDS, &json!({"type":"future-server-value"})));
        let token = signal(None::<String>);
        let tenant = signal(None::<Tenant>);
        ui.mount(render! { InputFields(fields: FIELDS.to_vec(), draft: draft, prefix: String::new(), token: token, tenant: tenant) });
        assert_eq!(draft.get_untracked()["type"], "future-server-value");
        assert!(model::body(FIELDS, &draft.get_untracked()).is_err());
        assert!(
            !ui.texts()
                .iter()
                .any(|text| text.contains("future-server-value"))
        );
        ui.tap("区分");
        ui.input_placeholder("候補を検索", "法人");
        assert!(ui.has_text("法人"));
        assert!(!ui.has_text("個人"));
        ui.tap("法人");
        assert_eq!(
            model::body(FIELDS, &draft.get_untracked()).unwrap(),
            json!({"type":"business"})
        );
        ui.tap("区分");
        ui.tap("選択を解除");
        assert_eq!(
            model::body(FIELDS, &draft.get_untracked()).unwrap(),
            json!({})
        );
    });
}

#[test]
fn changing_a_parent_selection_only_clears_its_own_dependent_field() {
    const PARENTS: &[Choice] = &[
        Choice::new("pipeline-a", "第一パイプライン"),
        Choice::new("pipeline-b", "第二パイプライン"),
    ];
    const LOOKUP: Lookup = Lookup {
        path: "/v1/test/pipelines",
        list_key: "items",
        value_key: "id",
        label_keys: &["name"],
    };
    const LINE: &[Field] = &[
        Field::new("pipelineId", "パイプライン", InputKind::Select(PARENTS)),
        Field::new(
            "stageId",
            "ステージ",
            InputKind::DependentReference {
                lookup: LOOKUP,
                parent_key: "pipelineId",
                children_key: "stages",
            },
        ),
        Field::text("note", "メモ"),
    ];
    const FIELDS: &[Field] = &[Field::new("lines", "商談", InputKind::Lines(LINE))];
    with_ui(|ui| {
        let draft = signal(model::draft(
            FIELDS,
            &json!({"lines":[
                {"pipelineId":"pipeline-a","stageId":"stage-a","note":""},
                {"pipelineId":"pipeline-a","stageId":"stage-b","note":""}
            ]}),
        ));
        let token = signal(None::<String>);
        let tenant = signal(None::<Tenant>);
        ui.mount(render! { InputFields(fields: FIELDS.to_vec(), draft: draft, prefix: String::new(), token: token, tenant: tenant) });
        assert_eq!(
            draft.get_untracked()["lines"][0]["stageId"],
            "stage-a",
            "mounting must preserve existing selections"
        );
        ui.input_nth("メモ", 0, "入力中");
        assert_eq!(draft.get_untracked()["lines"][0]["stageId"], "stage-a");
        ui.tap_nth("パイプライン", 0);
        ui.tap("第二パイプライン");
        assert_eq!(draft.get_untracked()["lines"][0]["stageId"], "");
        assert_eq!(
            draft.get_untracked()["lines"][1]["stageId"],
            "stage-b",
            "another line's selection must survive"
        );
    });
}

#[test]
fn a_failed_reference_lookup_can_be_retried_without_revealing_an_opaque_id() {
    const LOOKUP: Lookup = Lookup {
        path: "/v1/test/clients",
        list_key: "items",
        value_key: "id",
        label_keys: &["name"],
    };
    const FIELDS: &[Field] = &[Field::new(
        "clientId",
        "取引先",
        InputKind::Reference(LOOKUP),
    )];
    with_ui(|ui| {
        let draft = signal(model::draft(
            FIELDS,
            &json!({"clientId":"opaque-client-id"}),
        ));
        let token = signal(None::<String>);
        let tenant = signal(Some(tenant_fixture("tenant-a")));
        ui.mount(render! { InputFields(fields: FIELDS.to_vec(), draft: draft, prefix: String::new(), token: token, tenant: tenant) });
        ui.tap("取引先");
        assert!(ui.has_text("候補を読み込み中…"));
        ui.drain_tasks();
        assert!(ui.has_text(api::EXPIRED));
        ui.tap("もう一度読み込む");
        assert!(ui.has_text("候補を読み込み中…"));
        ui.drain_tasks();
        assert!(ui.has_text(api::EXPIRED));
        assert_eq!(draft.get_untracked()["clientId"], "opaque-client-id");
        assert!(
            !ui.texts()
                .iter()
                .any(|text| text.contains("opaque-client-id"))
        );
    });
}

#[test]
fn detail_values_update_in_place_when_the_same_record_receives_fresh_data() {
    with_ui(|ui| {
        let resource = FIXTURE.fields(&[("備考", "note"), ("金額", "totalMinor")]);
        let data = data_fixture(
            resource,
            json!({"id":"row-1","name":"請求テスト","note":"一覧取得時の備考","currency":"USD","totalMinor":12345}),
        );
        ui.mount(render! { RecordDetail(data: data) });
        let note_node = ui.text_node("一覧取得時の備考", 0);
        let old_amount = ui
            .texts()
            .into_iter()
            .find(|text| text.contains("123.45"))
            .expect("initial detail amount");
        let amount_node = ui.text_node(&old_amount, 0);
        let created = ui.0.borrow().next;

        data.selected.set(Some(json!({
            "id":"row-1", "name":"請求テスト", "note":"詳細取得後の備考", "currency":"USD", "totalMinor":67890
        })));
        flush();

        assert!(!ui.has_text("一覧取得時の備考"));
        assert!(!ui.has_text(&old_amount));
        assert_eq!(ui.text_node("詳細取得後の備考", 0), note_node);
        let new_amount = ui
            .texts()
            .into_iter()
            .find(|text| text.contains("678.90"))
            .expect("updated detail amount");
        assert_eq!(ui.text_node(&new_amount, 0), amount_node);
        assert_eq!(
            ui.0.borrow().next,
            created,
            "fresh detail values must update retained rows without remounting"
        );
    });
}

#[test]
fn native_profit_workflow_rejects_invalid_periods_and_fractional_yen_before_requesting() {
    with_ui(|ui| {
        let resource = crate::management::analytics::RESOURCES
            .iter()
            .find(|resource| resource.key == "profit-calculator")
            .copied()
            .unwrap();
        let token = signal(None::<String>);
        let tenant = signal(Some(tenant_fixture("tenant-a")));
        let back = Callback::new(|_| {});
        ui.mount(
            crate::management::analytics_screen::custom_screen(
                resource,
                token,
                tenant,
                "管理メニュー",
                back,
            )
            .unwrap(),
        );
        ui.input("開始日（必須）", "2026-12-01");
        ui.input("終了日（この日を含む）（必須）", "2026-11-30");
        ui.tap("実績を読み直して試算");
        assert!(ui.has_text("開始日は終了日以前にしてください"));
        assert!(!ui.has_text("処理中…"));
        ui.input("開始日（必須）", "2026-11-01");
        ui.input("月の販促費（円）（必須）", "1.5");
        ui.tap("実績を読み直して試算");
        assert!(
            ui.texts()
                .iter()
                .any(|text| text.contains("JPYの金額は小数点以下0桁までです"))
        );
        assert!(!ui.has_text("処理中…"));
    });
}

#[test]
fn analysis_results_render_exact_decimal_strings_and_update_retained_rows() {
    with_ui(|ui| {
        let output = signal(
            json!({"columns":[{"name":"売上","type":"decimal"},{"name":"商品","type":"string"}],"rows":[["9007199254740993.01","小麦粉"]],"truncated":true}),
        );
        ui.mount(render! { crate::management::analytics_screen::AnalysisResult(output:output) });
        assert!(ui.has_text("9007199254740993.01"));
        assert!(ui.has_text("小麦粉"));
        assert!(
            ui.has_text("結果の一部を表示しています。質問の条件を絞ると、より詳しく確認できます。")
        );
        output.update(|value| {
            value["rows"] = json!([["9007199254740994.02", "薄力粉"]]);
            value["truncated"] = json!(false);
        });
        flush();
        assert!(ui.has_text("9007199254740994.02"));
        assert!(!ui.has_text("9007199254740993.01"));
        assert!(ui.has_text("薄力粉"));
    });
}

#[test]
fn register_analysis_renders_every_aggregation_with_human_money() {
    with_ui(|ui| {
        let resource = crate::management::analytics::RESOURCES
            .iter()
            .find(|resource| resource.key == "register-analysis")
            .copied()
            .unwrap();
        let data = data_fixture(
            resource,
            json!({"reportTitle":"レジ明細の分析","totals":{"days":1,"slips":2,"netSalesMinor":5000},"byHour":[{"label":"9時","amountMinor":1234,"quantity":2}],"byItem":[{"label":"モーニング","amountMinor":1000}],"byDepartment":[{"label":"朝食部門","amountMinor":2000}],"byPayment":[{"label":"現金","amountMinor":3000}],"byWeekday":[{"weekdayLabel":"月曜日","days":1,"netSalesMinor":5000,"averageNetSalesMinor":5000}]}),
        );
        ui.mount(render! { RecordDetail(data:data) });
        for label in [
            "時間帯別",
            "商品別",
            "部門別",
            "決済別",
            "曜日別",
            "9時",
            "モーニング",
            "朝食部門",
            "現金",
            "月曜日",
        ] {
            assert!(ui.has_text(label), "missing {label}");
        }
        assert!(ui.has_text("¥1234"));
    });
}

#[test]
fn detail_actions_follow_loaded_state_capabilities_and_current_menu_access() {
    with_ui(|ui| {
        let mutation = SAVE.capability("collection.canManage");
        let data = data_fixture(
            FIXTURE.edit(mutation),
            json!({"id":"row-1","name":"請求テスト","currency":"USD","totalMinor":12345,"internalSecret":"never-visible"}),
        );
        data.detail_ready.set(false);
        data.collection
            .update(|collection| collection.meta = json!({"canManage":true}));
        ui.mount(render! { RecordDetail(data: data) });
        assert!(ui.has_text("請求テスト"));
        assert!(
            ui.texts().iter().any(|text| text.contains("123.45")),
            "money must be rendered in human units"
        );
        assert!(!ui.has_text("編集"));
        assert!(!ui.texts().iter().any(|text| text.contains("never-visible")));
        data.detail_ready.set(true);
        flush();
        assert!(ui.has_text("編集"));
        data.collection
            .update(|collection| collection.meta = json!({"canManage":false}));
        flush();
        assert!(!ui.has_text("編集"));
        data.collection
            .update(|collection| collection.meta = json!({"canManage":true}));
        data.connection.access.update(|access| {
            access.as_mut().unwrap().capabilities =
                json!({"deniedMenuActions":["order:ListAllClients"]})
        });
        flush();
        assert!(
            !ui.has_text("編集"),
            "revoked menu access must also revoke actions on an open detail"
        );
    });
}

#[test]
fn management_search_filters_permitted_menus_and_revocation_returns_to_the_hub() {
    with_ui(|ui| {
        let connection = connection_fixture(Access::default());
        provide_context(connection);
        let clients = resources()
            .into_iter()
            .find(|resource| resource.key == "clients")
            .expect("client catalog");
        ui.mount(render! { ManagementScreen(token: connection.token, tenant: connection.tenant) });
        ui.input_placeholder("機能を検索", clients.label);
        assert!(ui.has_text(clients.label));
        ui.tap(clients.label);
        assert!(ui.has_text("読み込み中…"));
        connection.access.update(|access| {
            access.as_mut().unwrap().capabilities =
                json!({"deniedMenuActions":["order:ListAllClients"]})
        });
        flush();
        assert!(ui.has_text("管理メニュー"));
        assert!(!ui.has_text(clients.label));
        ui.drain_tasks();
        assert!(
            !ui.has_text(api::EXPIRED),
            "a disposed resource request must not overwrite the menu"
        );
    });
}

#[test]
fn an_unknown_route_does_not_mount_a_resource_or_send_a_request() {
    with_ui(|ui| {
        let connection = connection_fixture(Access::default());
        provide_context(connection);
        let resource = FIXTURE.route("not-a-known-route");
        ui.mount(render! { ResourceEntry(token: connection.token, tenant: connection.tenant, resource: resource, back_label: "管理メニュー", on_back: |_| {}) });
        assert!(ui.has_text("この会社ではこの機能を利用できません"));
        assert!(!ui.has_text("読み込み中…"));
        ui.drain_tasks();
        assert!(!ui.has_text(api::EXPIRED));
    });
}

#[test]
fn required_filters_validate_before_loading_and_allow_a_corrected_request() {
    const FILTERS: &[Field] = &[Field::new("from", "開始日", InputKind::Date).required()];
    with_ui(|ui| {
        let connection = connection_fixture(Access::default());
        let resource = FIXTURE.filters(FILTERS);
        ui.mount(render! { ResourceScreen(connection: connection, resource: resource, back_label: "管理メニュー", on_back: |_| {}) });
        assert!(ui.has_text("開始日（必須）"));
        assert!(!ui.has_text("読み込み中…"));
        ui.input("開始日（必須）", "2026-02-31");
        ui.tap("この条件で表示");
        assert!(ui.has_text("開始日は年-月-日で入力してください"));
        assert!(!ui.has_text("読み込み中…"));
        ui.input("開始日（必須）", "2026-02-28");
        ui.tap("この条件で表示");
        assert!(!ui.has_text("開始日（必須）"));
        assert!(ui.has_text("読み込み中…"));
        ui.drain_tasks();
        assert!(ui.has_text(api::EXPIRED));
        assert!(ui.has_text("再読み込み"));
    });
}

#[test]
fn an_actor_restricted_action_only_appears_for_its_owner_in_the_allowed_state() {
    fn pending(record: &Value) -> bool {
        record["status"].as_str() == Some("pending")
    }
    const ACTIONS: &[Mutation] = &[Mutation::new(
        "申請を取り下げる",
        "POST",
        "/v1/test/records/{id}/withdraw",
        &[],
    )
    .actor_match("requestedBy")
    .when(pending)];
    with_ui(|ui| {
        let data = data_fixture(
            Resource {
                edit: None,
                actions: ACTIONS,
                ..FIXTURE
            },
            json!({"id":"row-1","name":"休暇申請","requestedBy":"us_owner","status":"pending"}),
        );
        ui.mount(render! { RecordDetail(data: data) });
        assert!(!ui.has_text("申請を取り下げる"));
        data.connection
            .access
            .update(|access| access.as_mut().unwrap().actor_id = Some("us_other".into()));
        flush();
        assert!(!ui.has_text("申請を取り下げる"));
        data.connection
            .access
            .update(|access| access.as_mut().unwrap().actor_id = Some("us_owner".into()));
        flush();
        assert!(ui.has_text("申請を取り下げる"));
        data.selected
            .update(|record| record.as_mut().unwrap()["status"] = json!("approved"));
        flush();
        assert!(!ui.has_text("申請を取り下げる"));
    });
}

#[test]
fn report_exclusions_and_currency_notices_are_visible_and_update_with_the_collection() {
    with_ui(|ui| {
        let collection = signal(model::Collection {
            rows: vec![],
            meta: json!({
                "notice":"確定済みのデータだけを集計しています",
                "excludedSources":[{"message":"予約集計を取得できません"}],
                "unconvertedCurrencies":["EUR"]
            }),
            truncated: false,
        });
        ui.mount(render! { CollectionNotice(collection: collection) });
        assert!(ui.has_text("確定済みのデータだけを集計しています"));
        assert!(ui.has_text("集計対象外: 予約集計を取得できません"));
        assert!(ui.has_text("換算されていない通貨: EUR"));
        collection.set(model::Collection::default());
        flush();
        assert!(ui.texts().iter().all(String::is_empty));
    });
}

#[test]
fn failed_write_keeps_the_draft_and_blocks_an_ambiguous_non_idempotent_retry() {
    with_ui(|ui| {
        let data = data_fixture(FIXTURE, json!({"id":"row-1","name":""}));
        data.edit.set(Some(SAVE));
        ui.mount(render! { EditRecord(data: data, operation: SAVE) });
        ui.tap_nth("保存する", 1);
        assert!(ui.has_text("名称を入力してください"));
        assert!(!ui.has_text("保存中…"));
        ui.input("名称（必須）", "失敗後も残る入力");
        ui.tap_nth("保存する", 1);
        assert!(ui.has_text("保存中…"));
        ui.drain_tasks();
        assert!(ui.has_text(api::EXPIRED));
        assert!(data.edit.get_untracked().is_some());
        ui.tap_nth("保存する", 1);
        assert!(
            ui.has_text("重複登録を防ぐため、一覧を読み直して結果を確認してから操作してください")
        );
        assert!(!ui.has_text("保存中…"));
        ui.tap("閉じる");
        assert!(
            ui.has_text("入力中の内容を破棄しますか？"),
            "failed writes must preserve the edited draft"
        );
    });
}

#[test]
fn idempotent_retry_allows_the_same_payload_but_blocks_changed_content() {
    with_ui(|ui| {
        let operation = SAVE.idempotency_body("idempotencyKey");
        let data = data_fixture(
            FIXTURE.edit(operation),
            json!({"id":"row-1","name":"元の値"}),
        );
        data.edit.set(Some(operation));
        ui.mount(render! { EditRecord(data: data, operation: operation) });
        ui.input("名称（必須）", "同じ操作");
        ui.tap_nth("保存する", 1);
        ui.drain_tasks();
        assert!(ui.has_text(api::EXPIRED));
        ui.tap_nth("保存する", 1);
        assert!(
            ui.has_text("保存中…"),
            "a matching idempotent retry must remain possible"
        );
        ui.drain_tasks();
        ui.input("名称（必須）", "別の操作");
        ui.tap_nth("保存する", 1);
        assert!(ui.has_text("前の操作結果を確認できていません。内容を変えて再送する前に、一覧で登録結果を確認してください"));
        assert!(!ui.has_text("保存中…"));
    });
}

#[test]
fn metadata_prefill_failure_cannot_be_bypassed_by_typing_a_replacement() {
    fn invalid_metadata(_: &Value) -> Result<Value, String> {
        Err("保存済みの補足情報を読み取れません。読み直してください。".into())
    }
    with_ui(|ui| {
        let operation = SAVE.prefill(invalid_metadata);
        let original = json!({"id":"row-1", "name":"内部データを含む元の値"});
        let data = data_fixture(FIXTURE.edit(operation), original.clone());
        data.edit.set(Some(operation));
        ui.mount(render! { EditRecord(data: data, operation: operation) });
        assert!(ui.has_text("保存済みの補足情報を読み取れません。読み直してください。"));
        ui.input("名称（必須）", "新しく入力しても保護する");
        ui.tap_nth("保存する", 1);
        assert!(!ui.has_text("保存中…"));
        ui.drain_tasks();
        assert!(!ui.has_text(api::EXPIRED));
        assert_eq!(data.editing_original.get_untracked(), original);
        assert!(data.edit.get_untracked().is_some());
    });
}

#[test]
fn closing_an_edited_form_requires_explicit_discard_and_can_be_cancelled() {
    with_ui(|ui| {
        let data = data_fixture(FIXTURE, json!({"id":"row-1","name":"保存済み"}));
        data.edit.set(Some(SAVE));
        ui.mount(render! { EditRecord(data: data, operation: SAVE) });
        ui.input("名称（必須）", "編集中");
        ui.tap("閉じる");
        assert!(ui.has_text("入力中の内容を破棄しますか？"));
        assert!(data.edit.get_untracked().is_some());
        ui.tap("入力を続ける");
        assert!(!ui.has_text("入力中の内容を破棄しますか？"));
        assert!(data.edit.get_untracked().is_some());
        ui.tap("閉じる");
        ui.tap("破棄して閉じる");
        assert!(data.edit.get_untracked().is_none());
    });
}

#[test]
fn tenant_switch_and_newer_detail_requests_ignore_stale_async_completions() {
    with_ui(|ui| {
        let original = json!({"id":"row-1","name":"前の会社の表示"});
        let data = data_fixture(
            FIXTURE.detail("/v1/test/records/{id}", "item"),
            original.clone(),
        );
        data.load();
        data.open(original.clone());
        data.connection.tenant.set(Some(tenant_fixture("tenant-b")));
        ui.drain_tasks();
        assert!(
            data.error.get_untracked().is_empty(),
            "old tenant list errors must not land in the new context"
        );
        assert!(
            data.detail_error.get_untracked().is_empty(),
            "old tenant detail errors must not land in the new context"
        );
        assert_eq!(data.collection.get_untracked().rows, vec![original.clone()]);
        data.open(original);
        let first = data.detail_generation.get_untracked();
        data.open(json!({"name":"IDのない行"}));
        assert!(data.detail_generation.get_untracked() > first);
        assert!(
            !data.detail_loading.get_untracked(),
            "an invalid new row must clear the old loading indicator"
        );
        let validation_error = data.detail_error.get_untracked();
        ui.drain_tasks();
        assert_eq!(data.detail_error.get_untracked(), validation_error);
        assert_eq!(data.selected.get_untracked().unwrap()["name"], "IDのない行");
    });
}
