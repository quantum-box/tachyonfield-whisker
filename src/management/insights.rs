//! Home, strategy, reporting, integrations, and administration contracts.
//! Verified against quantum-box/tachyonfield at
//! 73e645adebe3b21dddb83b6d4556ce58b6bee08f. This is a native presentation of
//! the API, not a copy of the web client's implementation.

use serde_json::{Value, json};

use super::schema::{Choice, Empty, Field, InputKind, Lookup, Mutation, Pagination, Resource};

const SITES: Lookup = Lookup {
    path: "/v1/erp/work-reports/sites",
    list_key: "sites",
    value_key: "id",
    label_keys: &["name", "code"],
};
const ORGANIZATION_SITES: Lookup = Lookup {
    path: "/v1/erp/master-control/organization",
    list_key: "sites",
    value_key: "id",
    label_keys: &["name", "code"],
};
const PERIODS: Lookup = Lookup {
    path: "/v1/erp/strategy/periods",
    list_key: "periods",
    value_key: "id",
    label_keys: &["name"],
};
const DATE_RANGE: &[Field] = &[
    Field::new("from", "開始日", InputKind::Date)
        .required()
        .default("@monthStart"),
    Field::new("to", "終了日", InputKind::Date)
        .required()
        .default("@today"),
];
const AXES: &[Choice] = &[
    Choice::new("department", "部門"),
    Choice::new("store", "店舗"),
    Choice::new("warehouse", "倉庫"),
    Choice::new("channel", "チャネル"),
    Choice::new("brand", "ブランド"),
    Choice::new("area", "エリア"),
];
const KNOWLEDGE_KINDS: &[Choice] = &[
    Choice::new("brand", "ブランド"),
    Choice::new("product", "商品"),
    Choice::new("customer", "お客さま"),
    Choice::new("process", "業務の進め方"),
    Choice::new("quality", "品質"),
    Choice::new("legal", "法令・契約"),
    Choice::new("other", "その他"),
];
const KNOWLEDGE_FIELDS: &[Field] = &[
    Field::text("title", "タイトル").required(),
    Field::new("kind", "種類", InputKind::Select(KNOWLEDGE_KINDS))
        .required()
        .default("other"),
    Field::new("points", "知っておくこと", InputKind::StringLines).help("1行に1つずつ入力"),
    Field::new("follow", "守ること", InputKind::StringLines).help("1行に1つずつ入力"),
    Field::new("avoid", "避けること", InputKind::StringLines).help("1行に1つずつ入力"),
    Field::new("note", "補足", InputKind::Multiline).empty(Empty::String),
];
const NAME_FIELDS: &[Field] = &[Field::text("name", "名前").required()];
const SITE_KIND_FIELDS: &[Field] = &[
    Field::text("code", "管理コード").required(),
    Field::text("name", "種類名").required(),
    Field::new("active", "利用する", InputKind::Bool).default("true"),
];
const SITE_KIND_EDIT_FIELDS: &[Field] = &[
    Field::text("name", "種類名").required(),
    Field::new("active", "利用する", InputKind::Bool),
];
const SITE_FIELDS: &[Field] = &[
    Field::text("code", "管理コード").required(),
    Field::text("name", "拠点名").required(),
    Field::new(
        "siteKind",
        "拠点の種類",
        InputKind::Reference(Lookup {
            path: "/v1/erp/master-control/organization",
            list_key: "siteKinds",
            value_key: "code",
            label_keys: &["name", "code"],
        }),
    )
    .required(),
    Field::text("timezone", "タイムゾーン")
        .required()
        .default("Asia/Tokyo"),
    Field::new("openedOn", "開設日", InputKind::Date),
];
const ACCOUNTING_ENTITY_FIELDS: &[Field] = &[
    Field::text("code", "管理コード").required(),
    Field::text("legalName", "正式名称").empty(Empty::String),
    Field::new(
        "fiscalYearStartMonth",
        "会計年度の開始月",
        InputKind::Integer,
    )
    .required(),
    Field::text("functionalCurrency", "機能通貨")
        .required()
        .default("JPY"),
];
const REPORT_PERIOD: &[Field] = &[Field::new(
    "period",
    "対象期間",
    InputKind::Select(&[
        Choice::new("weekly", "週次"),
        Choice::new("monthly", "月次"),
    ]),
)
.required()
.default("monthly")];
const ANALYTICS_FILTERS: &[Field] = &[
    Field::new("from", "開始日", InputKind::Date)
        .required()
        .default("@monthStart"),
    Field::new("to", "終了日", InputKind::Date)
        .required()
        .default("@today"),
    Field::new(
        "channel",
        "販路",
        InputKind::Select(&[
            Choice::new("b2b", "法人取引"),
            Choice::new("online", "オンライン"),
            Choice::new("store", "店舗"),
            Choice::new("marketplace", "モール"),
        ]),
    ),
    Field::new(
        "customerSegment",
        "顧客区分",
        InputKind::Select(&[
            Choice::new("guest", "ゲスト"),
            Choice::new("member", "会員"),
        ]),
    ),
    Field::text("sku", "商品コード"),
];
const FX_FIELDS: &[Field] = &[
    Field::text("currency", "通貨コード")
        .required()
        .help("USD、EURなど"),
    Field::text("yearMonth", "対象月")
        .required()
        .default("@month")
        .help("YYYY-MM"),
    Field::new(
        "rateType",
        "用途",
        InputKind::Select(&[
            Choice::new("average", "期中平均"),
            Choice::new("closing", "期末"),
            Choice::new("spot", "取引日"),
        ]),
    )
    .required()
    .default("average"),
    Field::new("rateToJpy", "1通貨あたりの円", InputKind::Decimal).required(),
];

fn object(value: &mut Value) -> Result<&mut serde_json::Map<String, Value>, String> {
    value
        .as_object_mut()
        .ok_or_else(|| "入力の形式を確認してください".into())
}

fn expected_version(body: &mut Value, record: &Value) -> Result<(), String> {
    let version = record
        .get("version")
        .and_then(Value::as_u64)
        .ok_or("更新前の版を確認できません。再読み込みしてください")?;
    object(body)?.insert("expectedVersion".into(), json!(version));
    Ok(())
}

fn knowledge_arrays(body: &mut Value) -> Result<(), String> {
    let map = object(body)?;
    for key in ["points", "follow", "avoid"] {
        map.entry(key).or_insert_with(|| json!([]));
        if !map[key]
            .as_array()
            .is_some_and(|rows| rows.iter().all(Value::is_string))
        {
            return Err("知っておくこと・守ること・避けることは1行ずつ入力してください".into());
        }
    }
    Ok(())
}

fn create_knowledge(body: &mut Value, _record: &Value) -> Result<(), String> {
    knowledge_arrays(body)?;
    let map = object(body)?;
    map.insert("departmentIds".into(), json!([]));
    map.insert("sources".into(), json!([]));
    Ok(())
}

fn edit_knowledge(body: &mut Value, record: &Value) -> Result<(), String> {
    expected_version(body, record)?;
    knowledge_arrays(body)?;
    // Scope and document links have no editor on this screen. Sending an empty
    // array would silently broaden an existing knowledge record's audience.
    for key in ["departmentIds", "sources"] {
        let original = record
            .get(key)
            .filter(|value| value.is_array())
            .ok_or("ナレッジの適用範囲を確認できません。再読み込みしてください")?;
        object(body)?.insert(key.into(), original.clone());
    }
    Ok(())
}

fn active_knowledge(record: &Value) -> bool {
    record.get("active").and_then(Value::as_bool) == Some(true)
}

fn accounting_entity(body: &mut Value, record: &Value) -> Result<(), String> {
    let month = body.get("fiscalYearStartMonth").and_then(Value::as_u64);
    if !month.is_some_and(|month| (1..=12).contains(&month)) {
        return Err("会計年度の開始月は1〜12で入力してください".into());
    }
    let stamp = record
        .get("updatedAt")
        .and_then(Value::as_str)
        .filter(|stamp| !stamp.is_empty())
        .ok_or("更新日時を確認できません。再読み込みしてください")?;
    object(body)?.insert("expectedUpdatedAt".into(), json!(stamp));
    // The book-of-record policy is independently configured and must survive
    // an edit to the legal name or fiscal year.
    if let Some(policy) = record.get("bookOfRecordPolicyId") {
        object(body)?.insert("bookOfRecordPolicyId".into(), policy.clone());
    }
    Ok(())
}

fn fx_rate(body: &mut Value, _record: &Value) -> Result<(), String> {
    let currency = body
        .get("currency")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err("通貨コードはUSDなど3文字で入力してください".into());
    }
    let month = body.get("yearMonth").and_then(Value::as_str).unwrap_or("");
    if month.len() != 7
        || chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").is_err()
    {
        return Err("対象月はYYYY-MMで入力してください".into());
    }
    let rate = body.get("rateToJpy").and_then(Value::as_str).unwrap_or("");
    if rate.starts_with('-') || !rate.bytes().any(|byte| matches!(byte, b'1'..=b'9')) {
        return Err("為替レートは0より大きい値を入力してください".into());
    }
    object(body)?.insert("currency".into(), json!(currency));
    Ok(())
}

fn edit_site_kind(body: &mut Value, record: &Value) -> Result<(), String> {
    let code = record
        .get("code")
        .and_then(Value::as_str)
        .filter(|code| !code.is_empty())
        .ok_or("拠点の種類を確認できません。再読み込みしてください")?;
    object(body)?.insert("code".into(), json!(code));
    Ok(())
}

fn edit_fx_rate(body: &mut Value, record: &Value) -> Result<(), String> {
    for key in ["currency", "yearMonth", "rateType"] {
        let original = record
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or("為替レートの対象を確認できません。再読み込みしてください")?;
        object(body)?.insert(key.into(), json!(original));
    }
    fx_rate(body, record)
}

fn can_enable_extension(record: &Value) -> bool {
    record.pointer("/validation/valid").and_then(Value::as_bool) == Some(true)
        && record.get("tenantStatus").and_then(Value::as_str) != Some("enabled")
}
fn can_disable_extension(record: &Value) -> bool {
    record.get("tenantStatus").and_then(Value::as_str) == Some("enabled")
}

/// Exact USD display for analytics amounts. One cent is 10,000,000 units.
/// Round half away from zero, with no floating-point financial conversion.
pub fn format_nanodollar(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let amount = text.parse::<i128>().ok()?;
    let cents = (amount.unsigned_abs() + 5_000_000) / 10_000_000;
    let dollars = (cents / 100).to_string();
    let mut grouped = String::new();
    for (index, character) in dollars.chars().enumerate() {
        if index > 0 && (dollars.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(character);
    }
    Some(format!(
        "{}${grouped}.{:02}",
        if amount < 0 { "-" } else { "" },
        cents % 100
    ))
}

/// Dataset names can include an internal 64-character tenant key. It is not
/// part of the human-readable dataset name and must not reach the picker.
pub fn readable_dataset_name(name: &str) -> &str {
    if let Some((label, key)) = name.rsplit_once(' ') {
        if key.len() == 64
            && key
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return label;
        }
    }
    name
}

pub const RESOURCES: &[Resource] = &[
    Resource::new(
        "strategy-actions",
        "ホーム",
        "行動の一覧",
        "/v1/erp/strategy/actions",
        "actions",
    )
    .title(&["title"])
    .subtitle(&["action.outcome"])
    .status("action.stage")
    .filters(&[Field::new(
        "periodId",
        "期間",
        InputKind::Reference(PERIODS),
    )])
    .fields(&[
        ("行動", "title"),
        ("目指す結果", "action.outcome"),
        ("実行する人", "action.execution"),
        ("進行状況", "action.stage"),
        ("合意した期限", "action.plan.dueOn.agreed.value"),
        ("提案された期限", "action.plan.dueOn.suggestion.value"),
    ])
    .route("strategy"),
    Resource::new(
        "sales-dashboard",
        "ホーム",
        "売上ダッシュボード",
        "/v1/erp/sales-dashboard",
        "",
    )
    .title(&["period"])
    .filters(&[
        Field::new("date_from", "開始日", InputKind::Date)
            .required()
            .default("@monthStart"),
        Field::new("date_to", "終了日", InputKind::Date)
            .required()
            .default("@today"),
    ])
    .fields(&[
        ("対象期間", "period"),
        ("総売上", "summary.totalSales"),
        ("請求書の売上", "summary.invoiceSales"),
        ("Squareの売上", "summary.squareSales"),
        ("未入金額", "summary.unpaidInvoiceAmount"),
        ("未入金件数", "summary.unpaidInvoiceCount"),
        ("今月の新規顧客", "summary.newCustomersThisMonth"),
    ])
    .route("dashboard"),
    Resource::new(
        "work-report-sites",
        "ホーム",
        "日報の提出先",
        "/v1/erp/work-reports/sites",
        "sites",
    )
    .title(&["name"])
    .subtitle(&["currentBusinessDate"])
    .fields(&[
        ("拠点", "name"),
        ("種類", "siteKind"),
        ("営業日", "currentBusinessDate"),
        ("タイムゾーン", "timezone"),
    ])
    .route("work-reports"),
    Resource::new(
        "work-report-overview",
        "ホーム",
        "日報の提出状況",
        "/v1/erp/work-reports/overview",
        "days",
    )
    .id("report.id")
    .title(&["siteName"])
    .subtitle(&["businessDate"])
    .status("indicator")
    .filters(&[
        Field::new("from", "開始日", InputKind::Date)
            .required()
            .default("@monthStart"),
        Field::new("to", "終了日", InputKind::Date)
            .required()
            .default("@today"),
        Field::new("siteId", "拠点", InputKind::Reference(SITES)),
    ])
    .fields(&[
        ("拠点", "siteName"),
        ("営業日", "businessDate"),
        ("提出状況", "indicator"),
        ("再確認が必要", "needsRecheck"),
        ("報告の状態", "report.status"),
        ("売上", "report.netSalesMinor"),
        ("客数", "report.customerCount"),
        ("実績への採用", "report.adoptionStatus"),
    ])
    .route("work-reports/overview"),
    Resource::new(
        "work-report-entry",
        "ホーム",
        "日報を確認",
        "/v1/erp/work-reports/entry",
        "",
    )
    .title(&["template.name"])
    .status("report.report.status")
    .filters(&[
        Field::new("siteId", "拠点", InputKind::Reference(SITES)).required(),
        Field::new("businessDate", "営業日", InputKind::Date)
            .required()
            .default("@today"),
    ])
    .fields(&[
        ("日報の種類", "template.name"),
        ("営業予定", "plannedStatus"),
        ("状態", "report.report.status"),
        ("営業日", "report.report.businessDate"),
        (
            "下書きの売上",
            "report.report.draft.measures.grossSalesMinor",
        ),
        ("下書きの客数", "report.report.draft.measures.customerCount"),
        ("メモ", "report.report.draft.note"),
        ("再確認が必要", "report.needsRecheck"),
    ])
    .route("work-reports"),
    Resource::new(
        "strategy-plans",
        "戦略",
        "AIの計画案",
        "/v1/erp/strategy/plans",
        "proposals",
    )
    .title(&["purpose"])
    .subtitle(&["createdAt"])
    .status("status")
    .detail("/v1/erp/strategy/plans/{id}", "proposal")
    .fields(&[
        ("目的", "purpose"),
        ("状態", "status"),
        ("作成日時", "createdAt"),
        ("提案理由", "plan.rationale"),
        ("前提", "plan.assumptions"),
    ])
    .route("strategy/plans"),
    Resource::new(
        "strategy-knowledge",
        "戦略",
        "ナレッジ",
        "/v1/erp/strategy/knowledge",
        "knowledge",
    )
    .title(&["title"])
    .subtitle(&["kind"])
    .status("active")
    .fields(&[
        ("タイトル", "title"),
        ("種類", "kind"),
        ("知っておくこと", "points"),
        ("守ること", "follow"),
        ("避けること", "avoid"),
        ("補足", "note"),
        ("適用する部署", "departments"),
        ("出典", "sources"),
        ("利用中", "active"),
        ("更新日時", "updatedAt"),
    ])
    .create(
        Mutation::new(
            "ナレッジを登録",
            "POST",
            "/v1/erp/strategy/knowledge",
            KNOWLEDGE_FIELDS,
        )
        .prepare(create_knowledge)
        .idempotency_body("idempotencyKey")
        .capability("collection.canManage")
        .explain("会社全体で使うナレッジとして登録します。"),
    )
    .edit(
        Mutation::new(
            "編集",
            "PUT",
            "/v1/erp/strategy/knowledge/{id}",
            KNOWLEDGE_FIELDS,
        )
        .prepare(edit_knowledge)
        .when(active_knowledge)
        .capability("collection.canManage"),
    )
    .actions(&[Mutation::new(
        "利用を終了",
        "POST",
        "/v1/erp/strategy/knowledge/{id}/archive",
        &[],
    )
    .prepare(expected_version)
    .when(active_knowledge)
    .capability("collection.canManage")
    .destructive()
    .explain("このナレッジの利用を終了します。過去の参照は残ります。")])
    .route("strategy/knowledge"),
    Resource::new(
        "organization-sites",
        "戦略",
        "組織の拠点",
        "/v1/erp/master-control/organization",
        "sites",
    )
    .title(&["name"])
    .subtitle(&["siteKind", "code"])
    .fields(&[
        ("拠点名", "name"),
        ("管理コード", "code"),
        ("種類", "siteKind"),
        ("開設日", "openedOn"),
        ("閉鎖日", "closedOn"),
        ("タイムゾーン", "timezone"),
    ])
    .create(Mutation::new(
        "拠点を登録",
        "POST",
        "/v1/erp/master-control/organization/sites",
        SITE_FIELDS,
    ))
    .route("erp/organization-chart"),
    Resource::new(
        "organization-site-kinds",
        "戦略",
        "拠点の種類",
        "/v1/erp/master-control/organization",
        "siteKinds",
    )
    .id("code")
    .title(&["name"])
    .subtitle(&["code"])
    .status("active")
    .fields(&[
        ("種類名", "name"),
        ("管理コード", "code"),
        ("利用中", "active"),
    ])
    .create(Mutation::new(
        "種類を登録",
        "POST",
        "/v1/erp/master-control/organization/site-kinds",
        SITE_KIND_FIELDS,
    ))
    .edit(
        Mutation::new(
            "編集",
            "POST",
            "/v1/erp/master-control/organization/site-kinds",
            SITE_KIND_EDIT_FIELDS,
        )
        .prepare(edit_site_kind),
    )
    .route("erp/organization-chart"),
    Resource::new(
        "organization-accounting-entity",
        "戦略",
        "会計主体",
        "/v1/erp/master-control/organization",
        "@object:accountingEntity",
    )
    .title(&["legalName", "code"])
    .subtitle(&["functionalCurrency"])
    .fields(&[
        ("正式名称", "legalName"),
        ("管理コード", "code"),
        ("会計年度の開始月", "fiscalYearStartMonth"),
        ("機能通貨", "functionalCurrency"),
        ("更新日時", "updatedAt"),
    ])
    .edit(
        Mutation::new(
            "編集",
            "POST",
            "/v1/erp/master-control/organization/accounting-entity",
            ACCOUNTING_ENTITY_FIELDS,
        )
        .prepare(accounting_entity),
    )
    .route("erp/organization-chart"),
    Resource::new(
        "ai-execution-status",
        "戦略",
        "AI実行の準備状況",
        "/v1/erp/strategy/macro-harness",
        "businesses",
    )
    .id("key")
    .title(&["name"])
    .status("enabled")
    .fields(&[
        ("業務", "name"),
        ("有効", "enabled"),
        ("準備完了", "readiness.ready"),
        ("確認項目", "readiness.checks"),
        ("仕事の出どころ", "source.label"),
        ("レビュー", "review.decision"),
    ])
    .route("settings/ai-execution"),
    Resource::new(
        "ai-evaluations",
        "戦略",
        "AI活用の評価",
        "/v1/erp/strategy/evaluations",
        "evaluations",
    )
    .id("evaluation.id")
    .title(&["evaluation.title"])
    .subtitle(&["evaluation.business"])
    .status("decision.outcome")
    .detail("/v1/erp/strategy/evaluations/{id}", "")
    .fields(&[
        ("評価名", "evaluation.title"),
        ("対象業務", "businessName"),
        ("作成日時", "evaluation.createdAt"),
        ("判定基準", "evaluation.criteria.text"),
        ("通常運用の開始", "evaluation.windows.baselineFrom"),
        ("通常運用の終了", "evaluation.windows.baselineTo"),
        ("AI活用の開始", "evaluation.windows.templateFrom"),
        ("AI活用の終了", "evaluation.windows.templateTo"),
        ("決定", "decisions"),
    ])
    .route("settings/ai-evaluation"),
    Resource::new(
        "import-definitions",
        "データ連携",
        "データ取込の定義",
        "/v1/bridge/definitions",
        "items",
    )
    .title(&["name"])
    .subtitle(&["description"])
    .status("status")
    .fields(&[
        ("名前", "name"),
        ("説明", "description"),
        ("ファイル形式", "sourceType"),
        ("状態", "status"),
        ("作成日時", "createdAt"),
    ])
    .route("imports"),
    Resource::new(
        "import-runs",
        "データ連携",
        "データ取込の履歴",
        "/v1/bridge/runs",
        "items",
    )
    .pagination(Pagination::Offset { limit: 50 })
    .title(&["sourceFileName"])
    .subtitle(&["createdAt"])
    .status("status")
    .detail("/v1/bridge/runs/{id}", "")
    .fields(&[
        ("ファイル", "sourceFileName"),
        ("状態", "status"),
        ("作成件数", "createdCount"),
        ("エラー件数", "errorCount"),
        ("確認日時", "createdAt"),
        ("実行日時", "executedAt"),
        ("注意事項", "normalized.warnings"),
        ("エラー", "normalized.errors"),
    ])
    .route("imports"),
    Resource::new(
        "export-definitions",
        "データ連携",
        "データ出力の定義",
        "/v1/bridge/exports/definitions",
        "items",
    )
    .title(&["name"])
    .subtitle(&["description"])
    .status("status")
    .fields(&[
        ("名前", "name"),
        ("説明", "description"),
        ("ファイル形式", "destinationType"),
        ("状態", "status"),
        ("作成日時", "createdAt"),
    ])
    .route("exports"),
    Resource::new(
        "report-definitions",
        "分析・レポート",
        "レポートの一覧",
        "/v1/erp/master-control/organization/reports/definitions",
        "reports",
    )
    .id("key")
    .title(&["title"])
    .subtitle(&["notice"])
    .status("availability.status")
    .fields(&[
        ("レポート", "title"),
        ("速報・確報", "finality"),
        ("集計上の注意", "notice"),
        ("利用可否", "availability.status"),
        ("準備に必要なもの", "availability.blockedBy"),
    ])
    .route("reports"),
    Resource::new(
        "daily-flash",
        "分析・レポート",
        "日次速報",
        "/v1/erp/master-control/organization/reports/daily-flash",
        "rows",
    )
    .id("siteCode")
    .title(&["label"])
    .subtitle(&["axisValue"])
    .status("state")
    .filters(&[
        Field::new("businessDate", "営業日", InputKind::Date)
            .required()
            .default("@today"),
        Field::new("groupBy", "集計単位", InputKind::Select(AXES)),
        Field::new("siteIds", "拠点", InputKind::Reference(ORGANIZATION_SITES)),
    ])
    .fields(&[
        ("拠点・集計", "label"),
        ("状態", "state"),
        ("当日売上", "dayNetSales"),
        ("当日客数", "dayCustomerCount"),
        ("当日客単価", "dayAverageSpend"),
        ("月累計売上", "mtdNetSales"),
        ("月累計客数", "mtdCustomerCount"),
        ("未提出日数", "unenteredDays"),
        ("休業日数", "closedDays"),
        ("集計対象外", "excluded"),
    ])
    .route("reports"),
    Resource::new(
        "sales-analytics",
        "分析・レポート",
        "売上分析",
        "/v1/erp/analytics/sales-dashboard",
        "@object:summary",
    )
    .filters(ANALYTICS_FILTERS)
    .fields(&[
        ("売上（USD）", "totalRevenueNanodollar"),
        ("粗利益（USD）", "grossProfitNanodollar"),
        ("粗利率（%）", "grossMarginPercent"),
        ("受注件数", "orderCount"),
        ("販売数量", "unitsSold"),
        ("平均受注額（USD）", "averageOrderValueNanodollar"),
        ("在庫回転率", "inventoryTurnover"),
    ])
    .route("reports/sales"),
    Resource::new(
        "daily-sales",
        "分析・レポート",
        "日別の売上",
        "/v1/erp/analytics/daily-sales?lineScope=true",
        "",
    )
    .id("date")
    .title(&["date"])
    .subtitle(&["orderCount"])
    .filters(ANALYTICS_FILTERS)
    .fields(&[
        ("日付", "date"),
        ("売上（USD）", "totalNanodollar"),
        ("受注件数", "orderCount"),
    ])
    .route("reports/sales"),
    Resource::new(
        "customer-sales",
        "分析・レポート",
        "顧客別の売上",
        "/v1/erp/analytics/customer-sales?lineScope=true",
        "",
    )
    .pagination(Pagination::Offset { limit: 50 })
    .id("customerKey")
    .title(&["customerName"])
    .subtitle(&["salesChannel", "orderCount"])
    .filters(ANALYTICS_FILTERS)
    .fields(&[
        ("お客さま", "customerName"),
        ("販路", "salesChannel"),
        ("売上（USD）", "totalNanodollar"),
        ("受注件数", "orderCount"),
        ("初回注文日", "firstOrderDate"),
        ("最終注文日", "lastOrderDate"),
    ])
    .route("reports/sales"),
    Resource::new(
        "automated-reports",
        "分析・レポート",
        "自動レポート",
        "/v1/field/agent/reports?limit=50",
        "items",
    )
    .title(&["period"])
    .subtitle(&["generatedAt"])
    .detail("/v1/field/agent/reports/{id}", "")
    .fields(&[
        ("対象期間", "period"),
        ("生成日時", "generatedAt"),
        ("レポート", "content"),
        ("アーカイブ日時", "archivedAt"),
    ])
    .create(
        Mutation::new(
            "レポートを生成",
            "POST",
            "/v1/field/agent/reports/generate",
            REPORT_PERIOD,
        )
        .explain("選択した期間のレポートを生成します。"),
    )
    .actions(&[
        Mutation::new("アーカイブ", "DELETE", "/v1/field/agent/reports/{id}", &[])
            .destructive()
            .explain("このレポートを一覧から取り下げます。"),
    ])
    .route("reports/automated"),
    Resource::new(
        "cancellation-fee-report",
        "分析・レポート",
        "キャンセル料レポート",
        "/v1/erp/reservation-reports/cancellation-fees",
        "items",
    )
    .id("reservationId")
    .title(&["customerName", "reservationNumber"])
    .status("paymentStatus")
    .filters(DATE_RANGE)
    .fields(&[
        ("予約番号", "reservationNumber"),
        ("お客さま", "customerName"),
        ("通貨", "currency"),
        ("キャンセル料", "cancellationFeeAmount"),
        ("入金額", "paidAmount"),
        ("返金額", "refundAmount"),
        ("請求状態", "cancellationStatus"),
        ("入金状態", "paymentStatus"),
        ("キャンセル日時", "cancelledAt"),
    ])
    .route("reports/cancellation-fees"),
    Resource::new(
        "budget-forecast",
        "分析・レポート",
        "予実と見込み",
        "/v1/erp/budget-forecast",
        "months",
    )
    .id("yearMonth")
    .title(&["yearMonth"])
    .filters(&[Field::new(
        "basis",
        "集計基準",
        InputKind::Select(&[
            Choice::new("cash", "現金主義"),
            Choice::new("accrual", "発生主義"),
        ]),
    )
    .required()
    .default("cash")])
    .fields(&[
        ("対象月", "yearMonth"),
        ("収入予算", "income.budgetAmount"),
        ("収入実績", "income.actualAmount"),
        ("確定収入", "income.committedAmount"),
        ("収入の見込み", "income.projectedAmount"),
        ("支出予算", "spend.budgetAmount"),
        ("支出実績", "spend.actualAmount"),
        ("確定支出", "spend.committedAmount"),
        ("支出の見込み", "spend.projectedAmount"),
        ("差引の見込み", "netProjectedAmount"),
    ])
    .route("erp/budget-forecast"),
    Resource::new(
        "monthly-budgets",
        "分析・レポート",
        "月次予算",
        "/v1/erp/monthly-budgets",
        "items",
    )
    .id("yearMonth")
    .title(&["yearMonth"])
    .subtitle(&["kind", "basis"])
    .filters(&[Field::new(
        "basis",
        "集計基準",
        InputKind::Select(&[
            Choice::new("cash", "現金主義"),
            Choice::new("accrual", "発生主義"),
        ]),
    )
    .required()
    .default("cash")])
    .fields(&[
        ("対象月", "yearMonth"),
        ("区分", "kind"),
        ("集計基準", "basis"),
        ("予算額", "targetAmount"),
        ("勘定科目", "accountCode"),
        ("備考", "notes"),
    ])
    .route("erp/budget-forecast"),
    Resource::new(
        "cash-schedule",
        "分析・レポート",
        "資金繰り",
        "/v1/erp/cash-schedule",
        "items",
    )
    .id("sourceId")
    .title(&["counterparty"])
    .subtitle(&["scheduledOn", "currency"])
    .status("confidence")
    .filters(&[
        Field::new(
            "kind",
            "入出金",
            InputKind::Select(&[
                Choice::new("receivable", "入金予定"),
                Choice::new("payable", "支払予定"),
            ]),
        )
        .default("receivable"),
        Field::new("from", "開始日", InputKind::Date).default("@monthStart"),
        Field::new("to", "終了日", InputKind::Date),
    ])
    .fields(&[
        ("相手先", "counterparty"),
        ("予定日", "scheduledOn"),
        ("確定・見込み", "confidence"),
        ("通貨", "currency"),
        ("予定額", "amountMinor"),
        ("残額", "outstandingMinor"),
        ("超過日数", "daysOverdue"),
    ])
    .route("erp/cash-schedule"),
    Resource::new(
        "credit-aging",
        "分析・レポート",
        "売掛の滞留状況",
        "/v1/erp/ar-ap/customer-aging",
        "rows",
    )
    .id("clientId")
    .title(&["clientName"])
    .subtitle(&["maxDaysOverdue"])
    .filters(&[Field::new("asOf", "基準日", InputKind::Date).default("@today")])
    .fields(&[
        ("取引先", "clientName"),
        ("期日前", "current"),
        ("1〜30日超過", "days1To30"),
        ("31〜60日超過", "days31To60"),
        ("61〜90日超過", "days61To90"),
        ("90日超過", "over90"),
        ("売掛残高", "receivableOutstanding"),
        ("未請求残", "unbilledBacklog"),
        ("与信対象額", "exposure"),
        ("与信限度", "creditLimit"),
    ])
    .route("erp/credit-control"),
    Resource::new(
        "fx-rates",
        "分析・レポート",
        "為替レート",
        "/v1/erp/fx-rates",
        "items",
    )
    .id("currency")
    .title(&["currency"])
    .subtitle(&["yearMonth", "rateType"])
    .fields(&[
        ("通貨", "currency"),
        ("対象月", "yearMonth"),
        ("用途", "rateType"),
        ("1通貨あたりの円", "rateToJpy"),
        ("取得方法", "source"),
    ])
    .create(Mutation::new("レートを登録", "POST", "/v1/erp/fx-rates", FX_FIELDS).prepare(fx_rate))
    .edit(
        Mutation::new(
            "レートを更新",
            "POST",
            "/v1/erp/fx-rates",
            &[Field::new("rateToJpy", "1通貨あたりの円", InputKind::Decimal).required()],
        )
        .prepare(edit_fx_rate),
    )
    .route("erp/finance-settings"),
    Resource::new(
        "analysis-datasets",
        "分析・レポート",
        "分析データ",
        "/v1/erp/analytics/external-data/datasets",
        "items",
    )
    .pagination(Pagination::Offset { limit: 50 })
    .title(&["name"])
    .subtitle(&["updated_at"])
    .fields(&[("名前", "name"), ("更新日時", "updated_at")])
    .route("analysis/data"),
    Resource::new(
        "analysis-sources",
        "分析・レポート",
        "分析データの取込元",
        "/v1/erp/analytics/external-data/sources",
        "items",
    )
    .pagination(Pagination::Offset { limit: 50 })
    .title(&["name"])
    .subtitle(&["kind"])
    .fields(&[("名前", "name"), ("種類", "kind")])
    .route("analysis/data"),
    Resource::new(
        "saas-accounts",
        "IT・SaaS管理",
        "アカウント一覧",
        "/v1/erp/it-governance/accounts",
        "items",
    )
    .pagination(Pagination::Page { size: 25 })
    .title(&["observation.displayName", "observation.primaryEmail"])
    .subtitle(&["observation.primaryEmail"])
    .filters(&[Field::text("query", "名前・メールアドレス")])
    .detail("/v1/erp/it-governance/accounts/{id}", "")
    .fields(&[
        ("名前", "observation.displayName"),
        ("メールアドレス", "observation.primaryEmail"),
        ("別名", "observation.aliases"),
        ("停止中", "observation.suspended"),
        ("管理者", "observation.isAdmin"),
        ("2段階認証", "observation.isEnrolledIn2Sv"),
        ("最終ログイン", "observation.lastLoginTime"),
        ("確認日時", "observedAt"),
        ("直近の同期に存在しない", "missing"),
    ])
    .route("it-governance/accounts"),
    Resource::new(
        "saas-connections",
        "IT・SaaS管理",
        "接続の状態",
        "/v1/erp/it-governance/connections",
        "items",
    )
    .title(&["displayName"])
    .status("status")
    .fields(&[
        ("接続先", "displayName"),
        ("状態", "status"),
        ("最終同期成功", "lastSuccessAt"),
        ("最終確認", "lastAttemptAt"),
    ])
    .route("it-governance/accounts"),
    Resource::new(
        "extensions",
        "管理",
        "アプリ",
        "/v1/erp/extensions/status",
        "items",
    )
    .id("extensionKey")
    .title(&["name"])
    .subtitle(&["description"])
    .status("tenantStatus")
    .fields(&[
        ("アプリ名", "name"),
        ("説明", "description"),
        ("業種", "industry"),
        ("バージョン", "version"),
        ("利用状態", "tenantStatus"),
        ("確認済み", "validation.valid"),
        ("確認が必要な内容", "validation.errors"),
    ])
    .actions(&[
        Mutation::new("有効にする", "POST", "/v1/erp/extensions/{id}/enable", &[])
            .when(can_enable_extension),
        Mutation::new("無効にする", "POST", "/v1/erp/extensions/{id}/disable", &[])
            .when(can_disable_extension)
            .destructive()
            .explain("このテナントでアプリの利用を停止します。"),
    ])
    .route("extensions"),
    Resource::new(
        "billing-accounts",
        "管理",
        "請求アカウント",
        "/v1/field/billing-accounts",
        "",
    )
    .title(&["name"])
    .subtitle(&["created_at"])
    .detail("/v1/field/billing-accounts/{id}", "")
    .fields(&[
        ("名前", "name"),
        ("作成日時", "created_at"),
        ("更新日時", "updated_at"),
    ])
    .create(Mutation::new(
        "請求アカウントを登録",
        "POST",
        "/v1/field/billing-accounts",
        NAME_FIELDS,
    ))
    .edit(Mutation::new(
        "名前を編集",
        "PUT",
        "/v1/field/billing-accounts/{id}",
        NAME_FIELDS,
    ))
    .route("billing-accounts"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_edit_preserves_scope_and_requires_the_read_version() {
        let original = json!({"version": 7, "departmentIds": ["department-a"], "sources": [{"kind":"url", "title":"方針", "url":"https://example.test/policy"}]});
        let mut input = json!({"title":"修正", "kind":"process", "follow":["確認する"]});
        edit_knowledge(&mut input, &original).unwrap();
        assert_eq!(input["expectedVersion"], 7);
        assert_eq!(input["departmentIds"], original["departmentIds"]);
        assert_eq!(input["sources"], original["sources"]);
        assert_eq!(input["avoid"], json!([]));
        assert!(edit_knowledge(&mut input, &json!({"departmentIds":[],"sources":[]})).is_err());
        assert!(edit_knowledge(&mut input, &json!({"version":8})).is_err());
    }

    #[test]
    fn fx_rate_validation_does_not_convert_decimal_strings_to_floats() {
        let mut value =
            json!({"currency":"usd", "yearMonth":"2026-11", "rateToJpy":"123.1234567890123456789"});
        fx_rate(&mut value, &Value::Null).unwrap();
        assert_eq!(value["currency"], "USD");
        assert_eq!(value["rateToJpy"], "123.1234567890123456789");
        for month in ["2026-13", "2026-1", "invalid"] {
            value["yearMonth"] = json!(month);
            assert!(fx_rate(&mut value, &Value::Null).is_err());
        }
        value["yearMonth"] = json!("2026-11");
        for rate in ["0", "0.000", "-1.2"] {
            value["rateToJpy"] = json!(rate);
            assert!(fx_rate(&mut value, &Value::Null).is_err());
        }
    }

    #[test]
    fn analytics_currency_rounding_is_exact_for_large_values_and_negatives() {
        assert_eq!(format_nanodollar(&json!("15000000")), Some("$0.02".into()));
        assert_eq!(
            format_nanodollar(&json!("-15000000")),
            Some("-$0.02".into())
        );
        assert_eq!(
            format_nanodollar(&json!("9007199254740993999999999")),
            Some("$9,007,199,254,740,994.00".into())
        );
        assert_eq!(format_nanodollar(&json!("1.25")), None);
        assert_eq!(format_nanodollar(&Value::Null), None);
    }
}
