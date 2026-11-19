//! Contracts for register analysis, finance reports, vendor spend, and Agent documents.
//! Reference: Field client 73e645ad; UI implementation is native and independent.
use super::{model, schema::*};
use crate::api::{self, Tenant};
use serde_json::{Value, json};

pub const SUBSCRIPTIONS: Lookup = Lookup {
    path: "/v1/erp/saas-subscriptions",
    list_key: "items",
    value_key: "id",
    label_keys: &["serviceName", "currentPlan"],
};
const SITES: Lookup = Lookup {
    path: "/v1/erp/work-reports/sites",
    list_key: "sites",
    value_key: "id",
    label_keys: &["name"],
};
const REG_FILTERS: &[Field] = &[
    Field::new("from", "開始日", InputKind::Date)
        .required()
        .default("@monthStart"),
    Field::new("to", "終了日（この日は含まない）", InputKind::Date)
        .required()
        .default("@nextMonthStart"),
    Field::new("siteId", "拠点", InputKind::Reference(SITES)),
];
const MONTH: &[Field] = &[Field::text("yearMonth", "対象月（年-月）")
    .required()
    .default("@month")];
const REG_ROWS: &[Field] = &[
    Field::text("label", "項目"),
    Field::new("quantity", "数量", InputKind::Decimal),
    Field::new("amountMinor", "売上（税込）", InputKind::Money),
    Field::new("days", "営業日数", InputKind::Integer),
];
const SPEND_FIELDS: &[Field] = &[
    Field::new(
        "subscriptionId",
        "SaaS契約",
        InputKind::Reference(SUBSCRIPTIONS),
    )
    .required(),
    Field::text("yearMonth", "使用月（年-月）")
        .required()
        .default("@month"),
    Field::text("currency", "通貨（JPY・USDなど3文字）")
        .required()
        .default("JPY"),
    Field::new("amountMinor", "支出額", InputKind::Money).required(),
    Field::new("isFinal", "確定した実績", InputKind::Bool),
];
const SPEND_FILTERS: &[Field] = &[
    Field::new(
        "subscriptionId",
        "SaaS契約",
        InputKind::Reference(SUBSCRIPTIONS),
    ),
    Field::text("from", "開始月（年-月）"),
    Field::text("to", "終了月（年-月）"),
    Field::new(
        "status",
        "状態",
        InputKind::Select(&[
            Choice::new("active", "有効"),
            Choice::new("voided", "取消済み"),
        ]),
    )
    .default("active"),
];
const CONNECTION_FIELDS: &[Field] = &[
    Field::new(
        "subscriptionId",
        "SaaS契約",
        InputKind::Reference(SUBSCRIPTIONS),
    )
    .required(),
    Field::new(
        "provider",
        "接続先",
        InputKind::Select(&[
            Choice::new("aws", "AWS"),
            Choice::new("gcp", "Google Cloud"),
            Choice::new("github", "GitHub"),
            Choice::new("anthropic", "Anthropic"),
            Choice::new("openai", "OpenAI"),
        ]),
    )
    .required(),
    Field::text("externalAccountId", "接続先のアカウント・組織").required(),
    Field::text("credentialRef", "シークレットの参照先").help(
        "保存済みシークレットの参照先を指定してください。APIキーの値を入力する欄ではありません。",
    ),
    Field::text("currency", "通貨（JPY・USDなど3文字）")
        .required()
        .default("JPY"),
    Field::new(
        "status",
        "接続状態",
        InputKind::Select(&[Choice::new("active", "有効"), Choice::new("paused", "停止")]),
    )
    .required()
    .default("active"),
];
const CORRECTION_FIELDS: &[Field] = &[
    Field::text("currency", "通貨（JPY・USDなど3文字）").required(),
    Field::new("amountMinor", "訂正後の金額", InputKind::Money).required(),
    Field::new(
        "bookedFxRate",
        "計上レート（1通貨単位あたりの円）",
        InputKind::Decimal,
    )
    .read("fxRate"),
    Field::new("effectiveOn", "訂正日", InputKind::Date)
        .required()
        .default("@today"),
    Field::new("reason", "訂正理由", InputKind::Multiline).required(),
];
const VOID_FIELDS: &[Field] = &[
    Field::new("effectiveOn", "取消日", InputKind::Date)
        .required()
        .default("@today"),
    Field::new("reason", "取消理由", InputKind::Multiline).required(),
];
const PROFIT_KIND: &[Choice] = &[
    Choice::new("menu_margin", "メニュー限界利益"),
    Choice::new("store_management_profit", "店舗の管理利益"),
];

pub const RESOURCES: &[Resource] = &[
    Resource::new("register-analysis", "ホーム", "レジ明細の分析", "/v1/erp/work-reports/register-reports/analysis", "")
        .route("work-reports/register-analysis").filters(REG_FILTERS).load(load_register).title(&["reportTitle"]).subtitle(&["from", "to"])
        .fields(&[("開始日", "from"), ("終了日（含まない）", "to"), ("営業日", "totals.days"), ("伝票数", "totals.slips"), ("純売上（税込）", "totals.netSalesMinor"), ("客数", "totals.customerCount")])
        .read_lines(&[
            Field::new("byHour", "時間帯別", InputKind::Lines(REG_ROWS)), Field::new("byItem", "商品別", InputKind::Lines(REG_ROWS)),
            Field::new("byDepartment", "部門別", InputKind::Lines(REG_ROWS)), Field::new("byPayment", "決済別", InputKind::Lines(REG_ROWS)),
            Field::new("byWeekday", "曜日別", InputKind::Lines(&[Field::text("weekdayLabel", "曜日"), Field::new("days", "営業日数", InputKind::Integer), Field::new("netSalesMinor", "純売上", InputKind::Money), Field::new("averageNetSalesMinor", "1日あたりの純売上", InputKind::Money), Field::new("customerCount", "客数", InputKind::Integer)])),
        ]),
    Resource::new("finance-reports", "分析・レポート", "会計レポート", "/v1/erp/cash-accrual-bridge", "items")
        .route("erp/finance-reports").filters(MONTH).load(load_finance).title(&["reportTitle"]).subtitle(&["yearMonth"])
        .fields(&[("対象月", "yearMonth"), ("MRR", "@money:mrr"), ("ARR", "@money:arr"), ("受注残（未請求）", "@money:backlogAmount"), ("売掛残", "@money:receivableOutstanding"), ("DSO（日）", "daysSalesOutstanding"), ("突合結果", "balanceLabel"), ("当期純利益", "@money:accrualNetIncome"), ("売掛金の増加（控除）", "@money:receivableIncrease"), ("買掛・未払の増加（加算）", "@money:payableIncrease"), ("前受収益の増加（加算）", "@money:deferredRevenueIncrease"), ("前払費用の増加（控除）", "@money:prepaidExpenseIncrease"), ("現金主義の純増減", "@money:cashNetChange"), ("注意", "note")])
        .read_lines(&[Field::new("sides", "補助元帳と統制勘定", InputKind::Lines(&[
            Field::text("kindLabel", "区分"), Field::new("subledgerBalance", "補助元帳（円）", InputKind::Money), Field::new("controlAccountBalance", "統制勘定（円）", InputKind::Money), Field::new("fxRevaluation", "評価替え（内訳・円）", InputKind::Money), Field::new("difference", "差異（円）", InputKind::Money), Field::new("itemsWithoutJournal", "仕訳のない件数", InputKind::Integer),
        ]))]),
    Resource::new("vendor-spend", "分析・レポート", "ベンダー支出", "/v1/erp/vendor-spend", "items")
        .route("erp/vendor-spend").filters(SPEND_FILTERS).load(load_vendor_spend).title(&["periodLabel"]).subtitle(&["amountMinor", "sourceLabel"]).status("finalLabel")
        .detail("/v1/erp/vendor-spend/{id}/history", "")
        .fields(&[("SaaS契約", "subscriptionName"), ("使用月", "yearMonth"), ("支出額", "amountMinor"), ("円換算額", "amountJpy"), ("計上レート", "fxRate"), ("取得元", "sourceLabel"), ("確定状態", "finalLabel"), ("取消日", "voidedOn")])
        .read_lines(&[
            Field::new("lines", "支出の内訳", InputKind::Lines(&[Field::text("category", "区分"), Field::text("subcategory", "内訳"), Field::text("unit", "単位"), Field::new("amountMinor", "金額", InputKind::Money)])),
            Field::new("actions", "訂正・取消履歴", InputKind::Lines(&[Field::new("action", "操作", InputKind::Select(&[Choice::new("correction", "訂正"), Choice::new("void", "取消")])), Field::text("effectiveOn", "適用日"), Field::text("reason", "理由"), Field::text("createdAt", "記録日時"), Field::text("beforeState.currency", "変更前の通貨"), Field::new("beforeState.amountMinor", "変更前の金額", InputKind::Money), Field::text("afterState.currency", "変更後の通貨"), Field::new("afterState.amountMinor", "変更後の金額", InputKind::Money)])),
        ])
        .create(Mutation::new("手入力の支出を登録", "POST", "/v1/erp/vendor-spend", SPEND_FIELDS).prepare(prepare_spend).explain("費用が発生した使用月の実績として登録します。請求月や支払月ではありません。"))
        .actions(&[
            Mutation::new("支出を訂正", "POST", "/v1/erp/vendor-spend/{id}/correction", CORRECTION_FIELDS).when(can_correct).prepare(prepare_correction).idempotency_body("clientRequestId").explain("元の仕訳を残し、取消仕訳と訂正仕訳を記録します。金額・日付・理由を確認してください。"),
            Mutation::new("支出を取消", "POST", "/v1/erp/vendor-spend/{id}/void", VOID_FIELDS).when(can_correct).prepare(prepare_void).idempotency_body("clientRequestId").destructive().explain("取消日を指定し、元の実績と仕訳の履歴を残して取り消します。"),
        ]),
    Resource::new("vendor-spend-csv", "分析・レポート", "ベンダー支出のCSV取込", "/v1/erp/vendor-spend", "items")
        .route("erp/vendor-spend").load(load_vendor_csv).title(&["periodLabel"]).subtitle(&["amountMinor"]).status("finalLabel")
        .fields(&[("SaaS契約", "subscriptionName"), ("使用月", "yearMonth"), ("支出額", "amountMinor"), ("状態", "finalLabel")])
        .create(Mutation::new("CSVを取り込む", "POST", "/v1/erp/vendor-spend/imports", &[
            Field::new("subscriptionId", "SaaS契約", InputKind::Reference(SUBSCRIPTIONS)).required(),
            Field::new("csv", "CSV本文", InputKind::Multiline).required().help("ヘッダー: year_month,category,subcategory,quantity,unit,currency,amount。金額は通常の通貨単位です。"),
            Field::new("isFinal", "確定した実績", InputKind::Bool),
        ]).prepare(prepare_csv).explain("対象契約とCSVの内容を確認して取り込みます。仕訳済みの金額変更には、支出詳細の訂正操作を使ってください。")),
    Resource::new("vendor-card-payments", "分析・レポート", "ベンダー支出のカード明細", "/v1/erp/vendor-spend/card-payments", "items")
        .route("erp/vendor-spend").filters(&[Field::new("subscriptionId", "SaaS契約", InputKind::Reference(SUBSCRIPTIONS)).required(), Field::new("from", "開始日", InputKind::Date).required().default("@monthStart"), Field::new("to", "終了日", InputKind::Date).required().default("@today")])
        .title(&["merchantName", "usedOn"]).subtitle(&["usedOn", "amountYen"]).fields(&[("利用先", "merchantName"), ("利用日", "usedOn"), ("支払額（円）", "amountYen"), ("手数料（円）", "feeYen"), ("元の通貨", "originalCurrency"), ("元の通貨の金額", "originalAmount"), ("実際の換算レート", "exchangeRate")]),
    Resource::new("vendor-connections", "分析・レポート", "ベンダー支出の連携設定", "/v1/erp/vendor-spend/connections", "items")
        .route("erp/vendor-spend").load(load_connections).title(&["subscriptionName", "provider"]).subtitle(&["provider", "externalAccountId"]).status("status")
        .fields(&[("SaaS契約", "subscriptionName"), ("接続先", "provider"), ("アカウント", "externalAccountId"), ("通貨", "currency"), ("最終同期月", "lastSyncedMonth"), ("最終同期日時", "lastSyncedAt"), ("最終エラー", "lastErrorCode")])
        .create(Mutation::new("連携を登録", "POST", "/v1/erp/vendor-spend/connections", CONNECTION_FIELDS).prepare(prepare_connection))
        .edit(Mutation::new("連携を更新", "POST", "/v1/erp/vendor-spend/connections", CONNECTION_FIELDS).prepare(prepare_connection))
        .actions(&[Mutation::new("連携を削除", "DELETE", "/v1/erp/vendor-spend/connections/{id}", &[]).destructive().explain("以後の同期に使う連携設定を削除します。登録済みの支出は残ります。")]),
    Resource::new("agent-quotations", "販売管理", "Agent見積書リクエスト", "/api/agent/quotations", "items")
        .route("agent").read_capability("agentDocuments.quotations.list").pagination(Pagination::Offset { limit: 20 })
        .title(&["quotationNumber"]).subtitle(&["clientName", "@money:totalAmount"]).status("status")
        .fields(&[("見積番号", "quotationNumber"), ("取引先", "clientName"), ("合計（円）", "@money:totalAmount"), ("有効期限", "validUntil"), ("状態", "status")])
        .actions(&[Mutation::new("見積書を送付", "POST", "/api/agent/quotations/{id}/send", &[]).capability("agentDocuments.quotations.send").when(can_send_quotation).explain("この見積書を取引先へ送付します。取引先・金額・有効期限を確認してください。")]),
    Resource::new("agent-invoices", "販売管理", "Agent請求書リクエスト", "/api/agent/invoices", "items")
        .route("agent").read_capability("agentDocuments.invoices.list").pagination(Pagination::Offset { limit: 20 })
        .title(&["invoiceNumber"]).subtitle(&["clientName", "@money:totalAmount"]).status("status")
        .fields(&[("請求番号", "invoiceNumber"), ("取引先", "clientName"), ("合計（円）", "@money:totalAmount"), ("支払期限", "dueDate"), ("状態", "status"), ("メール配信状態", "emailDeliveryStatus")])
        .actions(&[Mutation::new("請求書を送付", "POST", "/api/agent/invoices/{id}/send", &[]).capability("agentDocuments.invoices.send").when(can_send_invoice).validate_response(check_invoice_sent).explain("この請求書を取引先へ送付します。取引先・金額・支払期限を確認してください。")]),
    Resource::new("profit-workspaces", "分析・レポート", "保存した利益シナリオ", "/v1/field/profit-scenarios/workspaces", "items")
        .route("erp/profit-scenarios").load(load_workspaces)
        .title(&["context.periodFrom"]).subtitle(&["context.periodTo", "note"]).detail("/v1/field/profit-scenarios/workspaces/{id}", "")
        .fields(&[("開始日", "context.periodFrom"), ("終了日", "context.periodTo"), ("利益の定義", "context.profitDefinition"), ("保存版", "version"), ("メモ", "note")])
        .read_lines(&[
            Field::new("scenarios", "比較シナリオ", InputKind::Lines(&[Field::text("label", "シナリオ"), Field::text("assumptions.price_change_pct", "価格変更（%）"), Field::text("assumptions.volume_change_pct", "販売数の変化（%）"), Field::text("assumptions.promo_cost_yen", "販促費（月・円）"), Field::text("assumptions.set_attach_rate_pct", "セット注文率（%）"), Field::text("assumptions.set_discount_yen", "セット割引（円）"), Field::text("assumptions.set_substitution_pct", "単品からの置換（%）"), Field::text("assumptions.waste_reduction_pct", "廃棄削減（%）"), Field::text("assumptions.extra_labor_hours", "追加作業時間"), Field::new("assumptions.weekday_only", "平日のみ", InputKind::Bool)])),
            Field::new("conversation", "相談の履歴", InputKind::Lines(&[Field::new("role", "発言者", InputKind::Select(&[Choice::new("user", "自分"), Choice::new("assistant", "AI")])), Field::text("text", "内容")])),
        ])
        .edit(Mutation::new("期間とメモを更新", "POST", "/v1/field/profit-scenarios/workspaces/{id}/versions", &[
            Field::new("context.periodFrom", "開始日", InputKind::Date).required(), Field::new("context.periodTo", "終了日", InputKind::Date).required(), Field::new("context.profitDefinition", "利益の定義", InputKind::Select(PROFIT_KIND)).required(), Field::new("note", "メモ", InputKind::Multiline).empty(Empty::Null),
        ]).prepare(prepare_workspace_edit).idempotency_header().explain("保存済みのシナリオ前提と会話を保ち、期間・利益の定義・メモを更新します。計算結果は引き継ぎません。"))
        .actions(&[Mutation::new("保存した下書きを削除", "DELETE", "/v1/field/profit-scenarios/workspaces/{id}", &[]).prepare(prepare_workspace_delete).idempotency_header().destructive()]),
    Resource::new("profit-calculator", "分析・レポート", "利益シナリオを試算", "/v1/erp/profit-summary/consultation-reference", "").route("erp/profit-scenarios"),
    Resource::new("analysis-dialogue", "分析・レポート", "データに質問する", "/v1/erp/analytics/external-data/datasets", "items").route("analysis"),
];

pub fn valid_month(value: &str) -> bool {
    value.len() == 7
        && chrono::NaiveDate::parse_from_str(&format!("{value}-01"), "%Y-%m-%d").is_ok()
        && value.as_bytes().get(4) == Some(&b'-')
}
fn normalize_currency(payload: &mut Value) -> Result<(), String> {
    let code = payload["currency"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err("通貨を確認してください".into());
    }
    payload["currency"] = json!(code);
    Ok(())
}
fn prepare_spend(payload: &mut Value, _: &Value) -> Result<(), String> {
    normalize_currency(payload)?;
    if !valid_month(payload["yearMonth"].as_str().unwrap_or("")) {
        return Err("使用月は年-月で入力してください".into());
    }
    if !payload["amountMinor"].as_i64().is_some_and(|n| n >= 0) {
        return Err("支出額は0以上の金額にしてください".into());
    }
    Ok(())
}
fn prepare_csv(payload: &mut Value, _: &Value) -> Result<(), String> {
    let csv = payload["csv"].as_str().unwrap_or("");
    if csv.len() > 5_000_000 {
        return Err("CSVが大きすぎます。分割して取り込んでください".into());
    }
    let header = csv
        .trim_start_matches('\u{feff}')
        .lines()
        .next()
        .unwrap_or("");
    if !["year_month", "category", "currency", "amount"]
        .iter()
        .all(|required| {
            header
                .split(',')
                .any(|column| column.trim().trim_matches('"') == *required)
        })
    {
        return Err("CSVのヘッダーに year_month,category,currency,amount が必要です".into());
    }
    Ok(())
}
fn prepare_connection(payload: &mut Value, original: &Value) -> Result<(), String> {
    normalize_currency(payload)?;
    if !original.is_null()
        && (payload["subscriptionId"] != original["subscriptionId"]
            || payload["provider"] != original["provider"])
    {
        return Err("契約や接続先を変える場合は、新しい連携として登録してください".into());
    }
    if payload["provider"] != "aws" && payload["credentialRef"].as_str().is_none_or(str::is_empty) {
        return Err("保存済みシークレットの参照先を指定してください".into());
    }
    Ok(())
}
fn can_correct(record: &Value) -> bool {
    record["source"] == "manual"
        && record["isVoided"] != true
        && record["voidedOn"].is_null()
        && record["journalEntryId"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
}
fn prepare_void(payload: &mut Value, record: &Value) -> Result<(), String> {
    if !can_correct(record) {
        return Err("この実績は現在、訂正・取消できません。読み直してください".into());
    }
    let reason = payload["reason"].as_str().unwrap_or("").trim();
    if reason.is_empty() || reason.len() > 255 {
        return Err("理由は255バイト以内で入力してください".into());
    }
    payload["expectedJournalEntryId"] = record["journalEntryId"].clone();
    Ok(())
}
fn prepare_correction(payload: &mut Value, record: &Value) -> Result<(), String> {
    prepare_void(payload, record)?;
    normalize_currency(payload)?;
    if !payload["amountMinor"].as_i64().is_some_and(|n| n > 0) {
        return Err("訂正後の金額は0より大きくしてください".into());
    }
    let currency = payload["currency"].as_str().unwrap_or("");
    let rate = payload["bookedFxRate"].as_str().unwrap_or("");
    if currency == "JPY" {
        payload.as_object_mut().unwrap().remove("bookedFxRate");
        return Ok(());
    }
    if rate.is_empty() {
        if record["currency"] != currency || record["fxRate"].as_str().is_none_or(str::is_empty) {
            return Err("外貨の訂正には計上レートを入力してください".into());
        }
    } else {
        let parts: Vec<_> = rate.split('.').collect();
        if parts.len() > 2
            || parts[0].is_empty()
            || parts
                .iter()
                .any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
            || parts.get(1).is_some_and(|s| s.len() > 6)
            || !rate.bytes().any(|b| matches!(b, b'1'..=b'9'))
        {
            return Err("計上レートは正の数値を小数6桁以内で入力してください".into());
        }
    }
    Ok(())
}
fn can_send_quotation(record: &Value) -> bool {
    record["status"].as_str().is_some_and(|status| {
        !matches!(
            status,
            "Sent" | "Accepted" | "Rejected" | "Expired" | "Cancelled"
        )
    })
}
fn can_send_invoice(record: &Value) -> bool {
    record["status"]
        .as_str()
        .is_some_and(|status| !matches!(status, "Sent" | "Paid" | "Void"))
}
fn check_invoice_sent(response: &Value) -> Result<(), String> {
    if response["status"] == "SendFailed" || response["emailDeliveryStatus"] == "Failed" {
        return Err(
            "請求書メールの配信に失敗しました。一覧を読み直して配信状態を確認してください".into(),
        );
    }
    if !response.is_object()
        || response["id"].as_str().is_none()
        || response["status"].as_str().is_none()
    {
        return Err("送付結果を確認できません。一覧を読み直してください".into());
    }
    Ok(())
}
fn workspace_version(record: &Value) -> Result<u64, String> {
    record["version"]
        .as_u64()
        .filter(|v| *v > 0)
        .ok_or("保存版を確認できません。読み直してください".into())
}
fn prepare_workspace_delete(payload: &mut Value, record: &Value) -> Result<(), String> {
    payload["expectedVersion"] = json!(workspace_version(record)?);
    Ok(())
}
fn prepare_workspace_edit(payload: &mut Value, record: &Value) -> Result<(), String> {
    let version = workspace_version(record)?;
    let context = payload["context"].clone();
    if context["periodFrom"].as_str() > context["periodTo"].as_str() {
        return Err("開始日は終了日以前にしてください".into());
    }
    let workspace = json!({"context":context, "scenarios":record["scenarios"], "conversation":record.get("conversation").cloned().unwrap_or(json!([])), "agentSessionId":record.get("agentSessionId").cloned().unwrap_or(Value::Null), "lastRunId":Value::Null, "note":payload["note"]});
    if !workspace["scenarios"].is_array() {
        return Err("保存済みシナリオを確認できません".into());
    }
    *payload = json!({"expectedVersion":version, "workspace":workspace});
    Ok(())
}

fn load_register(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    if filters["from"].as_str() >= filters["to"].as_str() {
        return Err("開始日は終了日より前の日にしてください".into());
    }
    let path = model::query_path(
        "/v1/erp/work-reports/register-reports/analysis",
        REG_FILTERS,
        filters,
    )?;
    let mut response = api::fetch_record(token, tenant, &path)?;
    response["reportTitle"] = json!("レジ明細の分析");
    let weekdays = [
        "月曜日",
        "火曜日",
        "水曜日",
        "木曜日",
        "金曜日",
        "土曜日",
        "日曜日",
    ];
    for row in response["byWeekday"].as_array_mut().into_iter().flatten() {
        row["weekdayLabel"] = json!(
            row["weekday"]
                .as_u64()
                .and_then(|day| weekdays.get(day as usize))
                .copied()
                .unwrap_or("曜日不明")
        );
    }
    Ok(response)
}
fn load_finance(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let month = filters["yearMonth"].as_str().unwrap_or("");
    if !valid_month(month) {
        return Err("対象月は年-月で入力してください".into());
    }
    let mut bridge = api::fetch_record(
        token,
        tenant,
        &model::set_query("/v1/erp/cash-accrual-bridge", "yearMonth", month),
    )?;
    let mut tie = api::fetch_record(
        token,
        tenant,
        &model::set_query("/v1/erp/ar-ap/tie-out", "yearMonth", month),
    )?;
    let mut metrics = api::fetch_record(token, tenant, "/v1/erp/sales-contracts/metrics")?;
    for (value, id, title) in [
        (&mut metrics, "contracts", "契約指標（現在）"),
        (&mut tie, "tie-out", "補助元帳の突合"),
        (&mut bridge, "cash-accrual", "発生主義と現金主義"),
    ] {
        if !value.is_object() {
            return Err("会計レポートの応答を読み取れませんでした".into());
        }
        value["id"] = json!(id);
        value["reportTitle"] = json!(title);
        value["currency"] = json!("JPY");
        if id != "contracts" {
            value["yearMonth"] = json!(month);
        }
    }
    if metrics["daysSalesOutstanding"].is_null() {
        metrics["note"] = json!("直近3か月の月商が0の場合、DSOは算出されません。");
    }
    tie["balanceLabel"] = json!(if tie["balanced"] == true {
        "一致"
    } else {
        "差異あり"
    });
    for side in tie["sides"].as_array_mut().into_iter().flatten() {
        side["kindLabel"] = json!(if side["kind"] == "receivable" {
            "売掛金"
        } else {
            "買掛金・未払"
        });
    }
    Ok(json!({"items":[metrics,tie,bridge]}))
}
fn attach_subscription_names(token: &str, tenant: &Tenant, response: &mut Value) {
    let names = model::fetch_lookup(token, tenant, SUBSCRIPTIONS)
        .and_then(|(value, _)| model::lookup_options(SUBSCRIPTIONS, &value, None))
        .unwrap_or_default();
    for row in response["items"].as_array_mut().into_iter().flatten() {
        let name = names
            .iter()
            .find(|(id, _)| row["subscriptionId"].as_str() == Some(id.as_str()))
            .map(|(_, name)| name.as_str())
            .unwrap_or("契約名を確認できません");
        row["subscriptionName"] = json!(name);
        row["periodLabel"] = json!(format!(
            "{} · {}",
            name,
            row["yearMonth"].as_str().unwrap_or("")
        ));
        row["sourceLabel"] = json!(match row["source"].as_str() {
            Some("api") => "API同期",
            Some("csv") => "CSV取込",
            Some("manual") => "手入力",
            _ => "",
        });
        row["finalLabel"] = json!(if row["isVoided"] == true {
            "取消済み"
        } else if !row["voidedOn"].is_null() {
            "取消予定"
        } else if row["isFinal"] == true {
            "確定"
        } else {
            "暫定"
        });
    }
}
fn load_vendor_spend(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    for key in ["from", "to"] {
        let value = filters[key].as_str().unwrap_or("");
        if !value.is_empty() && !valid_month(value) {
            return Err("支出の期間は年-月で入力してください".into());
        }
    }
    let path = model::query_path("/v1/erp/vendor-spend", SPEND_FILTERS, filters)?;
    let mut response = api::fetch_record(token, tenant, &path)?;
    attach_subscription_names(token, tenant, &mut response);
    Ok(response)
}
fn load_vendor_csv(token: &str, tenant: &Tenant, _: &Value) -> Result<Value, String> {
    let mut response = load_vendor_spend(token, tenant, &json!({}))?;
    if let Some(rows) = response["items"].as_array_mut() {
        rows.retain(|row| row["source"] == "csv");
    }
    Ok(response)
}
fn load_connections(token: &str, tenant: &Tenant, _: &Value) -> Result<Value, String> {
    let mut response = api::fetch_record(token, tenant, "/v1/erp/vendor-spend/connections")?;
    attach_subscription_names(token, tenant, &mut response);
    Ok(response)
}

fn load_workspaces(token: &str, tenant: &Tenant, _: &Value) -> Result<Value, String> {
    let mut items = Vec::new();
    let mut cursor = String::new();
    let mut seen = std::collections::HashSet::new();
    let mut truncated = false;
    for page in 0..20 {
        let base = "/v1/field/profit-scenarios/workspaces";
        let path = if cursor.is_empty() {
            base.into()
        } else {
            model::set_query(base, "before", &cursor)
        };
        let response = api::fetch_record(token, tenant, &path)?;
        let rows = response
            .as_array()
            .ok_or("保存したシナリオの一覧を読み取れませんでした")?;
        for row in rows {
            let id = row["id"]
                .as_str()
                .ok_or("保存したシナリオを特定できません")?;
            if !seen.insert(id.to_owned()) {
                return Err("保存したシナリオのページを読み進められませんでした".into());
            }
            items.push(row.clone());
        }
        if rows.len() < 100 {
            break;
        }
        cursor = rows.last().unwrap()["id"].as_str().unwrap().to_owned();
        if page == 19 {
            truncated = true;
        }
    }
    Ok(json!({"items":items,"truncated":truncated}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn posted_spend_requires_the_current_journal_and_preserves_exact_fx() {
        let original = json!({"source":"manual","isVoided":false,"voidedOn":null,"journalEntryId":"journal-1","currency":"USD","fxRate":"150.123456"});
        let mut body = json!({"currency":"USD","amountMinor":12345,"bookedFxRate":"150.123456","effectiveOn":"2026-11-19","reason":"訂正"});
        prepare_correction(&mut body, &original).unwrap();
        assert_eq!(body["expectedJournalEntryId"], "journal-1");
        assert_eq!(body["bookedFxRate"], "150.123456");
        body["bookedFxRate"] = json!("150.1234567");
        assert!(prepare_correction(&mut body, &original).is_err());
        assert!(!can_correct(
            &json!({"source":"api","journalEntryId":"journal-1"})
        ));
        assert!(!can_correct(
            &json!({"source":"manual","journalEntryId":"journal-1","voidedOn":"2026-12-01"})
        ));
    }
    #[test]
    fn invoice_send_checks_delivery_failure_even_with_a_successful_http_response() {
        assert!(check_invoice_sent(&json!({"id":"i1","status":"SendFailed"})).is_err());
        assert!(
            check_invoice_sent(&json!({"id":"i1","status":"Sent","emailDeliveryStatus":"Failed"}))
                .is_err()
        );
        assert!(
            check_invoice_sent(&json!({"id":"i1","status":"Sent","emailDeliveryStatus":"Sent"}))
                .is_ok()
        );
        assert!(!can_send_invoice(&json!({"status":"Paid"})));
    }
    #[test]
    fn workspace_edits_keep_scenarios_and_conversation_and_invalidate_old_results() {
        let original = json!({"version":3,"scenarios":[{"key":"a","assumptions":{"price_change_pct":1.5}}],"conversation":[{"role":"user","text":"相談"}],"lastRunId":"old-run"});
        let mut payload = json!({"context":{"periodFrom":"2026-11-01","periodTo":"2026-11-30","profitDefinition":"menu_margin"},"note":"更新"});
        prepare_workspace_edit(&mut payload, &original).unwrap();
        assert_eq!(payload["expectedVersion"], 3);
        assert_eq!(payload["workspace"]["scenarios"], original["scenarios"]);
        assert_eq!(
            payload["workspace"]["conversation"],
            original["conversation"]
        );
        assert!(payload["workspace"]["lastRunId"].is_null());
        assert!(prepare_workspace_edit(&mut json!({}), &json!({})).is_err());
        assert!(valid_month("2026-11"));
        assert!(!valid_month("2026-13"));
        assert!(!valid_month("2026-1"));
    }
}
