//! Native adapters for version-pinned analysis and server-calculated profit scenarios.
use crate::api::{self, Tenant};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const ANALYSIS: &str = "/v1/erp/analytics/external-data";
pub const REFERENCE: &str = "/v1/erp/profit-summary/consultation-reference";
pub const WORKSPACES: &str = "/v1/field/profit-scenarios/workspaces";

pub fn request_key() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|_| "操作の準備ができませんでした".to_owned())?;
    bytes[6] = bytes[6] & 15 | 64;
    bytes[8] = bytes[8] & 63 | 128;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

/// Decimal scaling uses integer arithmetic. Inputs and outputs never pass through f64.
pub fn apply_percent(amount: &str, percent: i64) -> Result<String, String> {
    let factor = i128::from(percent)
        .checked_add(100)
        .ok_or("割合が大きすぎます")?;
    if factor < 0 {
        return Err("変化率は-100%以上にしてください".into());
    }
    let (negative, digits) = amount
        .strip_prefix('-')
        .map_or((false, amount), |v| (true, v));
    let parts: Vec<_> = digits.split('.').collect();
    if parts.is_empty()
        || parts.len() > 2
        || parts
            .iter()
            .any(|v| v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("実績金額の形式を確認できません".into());
    }
    let scale = parts.get(1).map_or(0, |s| s.len());
    if scale > 18 {
        return Err("実績金額の小数桁が計算可能な範囲を超えています".into());
    }
    let value = parts
        .concat()
        .parse::<i128>()
        .map_err(|_| "実績金額が大きすぎます")?;
    let units = value
        .checked_mul(factor)
        .and_then(|n| if negative { n.checked_neg() } else { Some(n) })
        .ok_or("試算額が計算可能な範囲を超えています")?;
    let digits = units.unsigned_abs().to_string();
    let digits = format!("{:0>width$}", digits, width = scale + 3);
    let split = digits.len() - scale - 2;
    let fraction = digits[split..].trim_end_matches('0');
    Ok(format!(
        "{}{}{}",
        if units < 0 { "-" } else { "" },
        &digits[..split],
        if fraction.is_empty() {
            String::new()
        } else {
            format!(".{fraction}")
        }
    ))
}

fn actual(snapshot: &Value, key: &str) -> Option<String> {
    let metric = snapshot["metrics"]
        .as_array()?
        .iter()
        .find(|metric| metric["key"] == key)?;
    if metric["status"] != "available" {
        return None;
    }
    if let (Some(basis), Some(required)) = (
        metric["tax_basis"].as_str(),
        snapshot["contract"]["tax_basis"].as_str(),
    ) {
        if basis != required {
            return None;
        }
    }
    metric["value"].as_str().map(str::to_owned)
}

pub fn reference_rows(snapshot: &Value) -> Vec<(String, String)> {
    let labels = [
        ("sales_total", "売上"),
        ("purchase_cost_total", "仕入"),
        ("gross_profit", "粗利"),
        ("menu_theoretical_cost", "メニューの理論原価"),
        ("food_loss_cost", "食品ロス"),
        ("labor_cost", "人件費"),
        ("fixed_cost", "固定費"),
        ("subscription_cost", "SaaS・サブスク"),
    ];
    labels
        .into_iter()
        .map(|(key, label)| {
            let metric = snapshot["metrics"]
                .as_array()
                .and_then(|rows| rows.iter().find(|metric| metric["key"] == key));
            let value = metric
                .map(|metric| {
                    if let Some(amount) = metric["value"]
                        .as_str()
                        .filter(|_| metric["status"] == "available")
                    {
                        format!(
                            "{amount} 円{}{}",
                            if metric["kind"] == "model_estimate" {
                                "（推計を含む）"
                            } else {
                                ""
                            },
                            if actual(snapshot, key).is_none() {
                                "（税の基準が異なるため試算には使いません）"
                            } else {
                                ""
                            }
                        )
                    } else {
                        format!("未算出：{}", missing_reason(metric))
                    }
                })
                .unwrap_or_else(|| "未算出：取得できませんでした".into());
            (label.into(), value)
        })
        .collect()
}

fn missing_reason(metric: &Value) -> &'static str {
    match metric["reason"]["code"].as_str() {
        Some("sales_not_imported") => "売上の実績がありません",
        Some("cost_unconfirmed") => "仕入が未計上です",
        Some("permission_denied") => "閲覧する権限がありません",
        Some("not_provided") => "集計元が未接続です",
        Some("no_records") => "対象期間の記録がありません",
        Some("cost_partially_unconfirmed") => "未確定の原価があります",
        Some("labor_accounts_undefined") => "人件費の勘定区分が未設定です",
        Some("period_not_whole_months") => "月全体を含む期間を指定してください",
        _ => "取得できませんでした",
    }
}

fn full_month(from: &str, to: &str) -> bool {
    use chrono::Datelike;
    let Some(start) = chrono::NaiveDate::parse_from_str(from, "%Y-%m-%d").ok() else {
        return false;
    };
    let Some(end) = chrono::NaiveDate::parse_from_str(to, "%Y-%m-%d").ok() else {
        return false;
    };
    start.day() == 1
        && start.year() == end.year()
        && start.month() == end.month()
        && end.succ_opt().is_some_and(|next| next.day() == 1)
}

pub fn scenario_input(draft: &Value, snapshot: &Value) -> Result<Value, String> {
    let from = draft["periodFrom"]
        .as_str()
        .ok_or("開始日を入力してください")?;
    let to = draft["periodTo"]
        .as_str()
        .ok_or("終了日を入力してください")?;
    if from > to {
        return Err("開始日は終了日以前にしてください".into());
    }
    if snapshot["contract"]["date_from"] != from || snapshot["contract"]["date_to"] != to {
        return Err("対象期間と実績の期間が一致しません。実績を読み直してください".into());
    }
    if snapshot["contract"]["currency"] != "JPY" {
        return Err("この利益試算は円の実績に対応しています".into());
    }
    let id = snapshot["snapshot_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("実績の参照元を確認できません")?;
    let sales = actual(snapshot, "sales_total")
        .ok_or("売上の実績が未取得、または税の基準が違うため試算できません")?;
    let definition = match draft["profitDefinition"].as_str() {
        Some("menu_margin") => "contributionMargin",
        Some("store_management_profit") => "storeManagementProfit",
        _ => return Err("利益の定義を選択してください".into()),
    };
    let rows = draft["scenarios"]
        .as_array()
        .filter(|rows| !rows.is_empty() && rows.len() <= 10)
        .ok_or("比較する案を1〜10件入力してください")?;
    let source = json!({"kind":"actual","reference":id});
    let assumed = json!({"kind":"userAssumption"});
    let mut cases = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let price = row["priceChangePct"].as_i64().unwrap_or(0);
        let volume = row["volumeChangePct"].as_i64().unwrap_or(0);
        let promo = row["promotionYen"].as_i64().unwrap_or(0);
        if price <= -100 || volume < -100 || promo < 0 {
            return Err(
                "価格変更は-100%より大きく、販売数変化は-100%以上、販促費は0円以上にしてください"
                    .into(),
            );
        }
        if promo > 0 && !full_month(from, to) {
            return Err(
                "月の販促費を含む試算は、開始日を月初・終了日を同じ月の末日にしてください".into(),
            );
        }
        let label = row["label"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or("比較する案の名前を入力してください")?;
        let mut changes = vec![
            json!({"id":"price", "label":format!("価格 {price}%・販売数 {volume}%"), "change":{"kind":"setPrice","menuId":"period_sales","price":apply_percent(&sales,price)?,"quantityChangeRate":decimal_percent(volume)},"source":assumed}),
        ];
        if promo != 0 {
            changes.push(json!({"id":"promotion","label":"販促費","change":{"kind":"setCost","cost":{"id":"promotion","name":"販促費","kind":"promotion","amount":promo.to_string(),"source":assumed}},"source":assumed}));
        }
        cases.push(json!({"id":format!("case-{}",index+1),"label":label,"changes":changes}));
    }
    let mut ingredients = vec![
        json!({"id":"period_purchases","name":"仕入（期間合計）","unit":"期間","unitPrice":actual(snapshot,"purchase_cost_total"),"priceSource":source,"yieldRate":"1"}),
    ];
    let loss = actual(snapshot, "food_loss_cost");
    if let Some(loss) = &loss {
        ingredients.push(json!({"id":"period_food_loss","name":"食品ロス（期間合計）","unit":"期間","unitPrice":loss,"priceSource":source,"yieldRate":"1"}));
    }
    let mut costs = vec![
        json!({"id":"labor","name":"人件費","kind":"existingLabor","amount":actual(snapshot,"labor_cost"),"source":source}),
        json!({"id":"fixed","name":"固定費","kind":"fixedOverhead","amount":actual(snapshot,"fixed_cost"),"source":source}),
    ];
    if let Some(amount) = actual(snapshot, "subscription_cost") {
        costs.push(json!({"id":"saas_subscriptions","name":"SaaS・サブスク","kind":"fixedOverhead","amount":amount,"source":{"kind":"estimate","reference":"saas_subscriptions"}}));
    }
    let days = snapshot["contract"]["business_days"]
        .as_u64()
        .or_else(|| snapshot["contract"]["calendar_days"].as_u64())
        .ok_or("営業日数を確認できません")?;
    Ok(json!({
        "context":{"storeId":"tenant_wide","periodStart":from,"periodEnd":to,"businessDays":days,"currency":"JPY","currencyMinorUnits":0,"taxBasis":"taxInclusive","rounding":"halfUp","quantityScale":4,"profitDefinition":definition,"snapshot":{"id":id,"takenAt":snapshot["captured_at"]}},
        "baseline":{"foodCostBasis":"lossInclusive","ingredients":ingredients,"menuItems":[{"id":"period_sales","name":"売上（期間合計）","price":sales,"quantity":{"kind":"fixed","quantity":"1","source":source},"recipe":[{"ingredientId":"period_purchases","edibleQuantity":"1"}]}],"waste":if loss.is_some(){json!([{"ingredientId":"period_food_loss","quantity":"1","source":source}])}else{json!([])},"costs":costs},"cases":cases
    }))
}

fn decimal_percent(value: i64) -> String {
    let number = i128::from(value);
    let unsigned = number.unsigned_abs();
    let fraction = format!("{:02}", unsigned % 100);
    let fraction = fraction.trim_end_matches('0');
    format!(
        "{}{}{}",
        if value < 0 { "-" } else { "" },
        unsigned / 100,
        if fraction.is_empty() {
            String::new()
        } else {
            format!(".{fraction}")
        }
    )
}

pub fn load_and_calculate(token: &str, tenant: &Tenant, draft: &Value) -> Result<Value, String> {
    let path = super::model::set_query(
        &super::model::set_query(
            REFERENCE,
            "date_from",
            draft["periodFrom"].as_str().unwrap_or(""),
        ),
        "date_to",
        draft["periodTo"].as_str().unwrap_or(""),
    );
    let snapshot = api::fetch_record(token, tenant, &path)?;
    let input = scenario_input(draft, &snapshot)?;
    let result = api::send_json(
        token,
        tenant,
        "POST",
        "/v1/erp/profit-summary/scenarios/calculate",
        &input,
        Duration::from_secs(45),
    )?;
    if !result["baseline"]["totals"].is_object()
        || !result["cases"].is_array()
        || result["inputFingerprint"].as_str().is_none()
    {
        return Err("試算結果の形式を確認できませんでした".into());
    }
    Ok(json!({"snapshot":snapshot,"result":result,"draft":draft}))
}

pub fn calculation_rows(calculation: &Value) -> Vec<(String, Vec<(String, String)>)> {
    let definition = if calculation["draft"]["profitDefinition"] == "menu_margin" {
        "contributionMargin"
    } else {
        "storeManagementProfit"
    };
    let mut plans = vec![(
        "現状維持".to_owned(),
        calculation["result"]["baseline"].clone(),
        Value::Null,
    )];
    for (index, scenario) in calculation["draft"]["scenarios"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let id = format!("case-{}", index + 1);
        let found = calculation["result"]["cases"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["id"] == id))
            .cloned()
            .unwrap_or(Value::Null);
        plans.push((
            scenario["label"].as_str().unwrap_or("比較案").into(),
            found["plan"].clone(),
            found,
        ));
    }
    plans
        .into_iter()
        .map(|(label, plan, result)| {
            let money = |value: &Value| {
                if value.is_null() {
                    "未算出（必要な実績を確認してください）".into()
                } else {
                    format!("{} 円", super::model::display(value))
                }
            };
            let mut rows = vec![
                ("利益".into(), money(&plan["totals"][definition])),
                (
                    "食品ロス".into(),
                    if actual(&calculation["snapshot"], "food_loss_cost").is_some() {
                        money(&plan["totals"]["wasteCost"])
                    } else {
                        "未算出（食品ロス額の実績がありません）".into()
                    },
                ),
            ];
            for line in result["attribution"].as_array().into_iter().flatten() {
                rows.push((
                    line["label"].as_str().unwrap_or("施策の寄与").into(),
                    money(&line["sequential"][definition]),
                ));
            }
            if !result["interaction"][definition].is_null() {
                rows.push((
                    "施策の重なり".into(),
                    money(&result["interaction"][definition]),
                ));
            }
            rows.push((
                "未算出の指標".into(),
                "必要客数・欠品リスク・作業時間は、必要な実績が未接続です。".into(),
            ));
            (label, rows)
        })
        .collect()
}

pub fn workspace_body(calculation: &Value, note: &str) -> Result<Value, String> {
    let draft = &calculation["draft"];
    let fingerprint = calculation["result"]["inputFingerprint"]
        .as_str()
        .ok_or("試算してから保存してください")?;
    let mut scenarios = vec![json!({"key":"baseline","label":"現状維持","assumptions":{}})];
    for (index, row) in draft["scenarios"]
        .as_array()
        .ok_or("比較する案を確認してください")?
        .iter()
        .enumerate()
    {
        let mut assumptions = json!({"price_change_pct":row["priceChangePct"],"volume_change_pct":row["volumeChangePct"]});
        if row["promotionYen"].as_i64().is_some_and(|value| value > 0) {
            assumptions["promo_cost_yen"] = row["promotionYen"].clone();
        }
        scenarios.push(json!({"key":format!("case-{}",index+1),"label":row["label"],"assumptions":assumptions}));
    }
    Ok(
        json!({"context":{"periodFrom":draft["periodFrom"],"periodTo":draft["periodTo"],"profitDefinition":draft["profitDefinition"]},"scenarios":scenarios,"conversation":[],"agentSessionId":null,"lastRunId":fingerprint,"note":if note.trim().is_empty(){Value::Null}else{json!(note.trim())}}),
    )
}

pub fn dataset_description(
    token: &str,
    tenant: &Tenant,
    dataset: &str,
    version: &str,
) -> Result<Value, String> {
    let path = format!(
        "{ANALYSIS}/datasets/{}?version={}",
        api::encode_query(dataset),
        api::encode_query(version)
    );
    let response = api::fetch_record(token, tenant, &path)?;
    if response["version"] != version || !response["schema"]["columns"].is_array() {
        return Err("データの版または列を確認できません".into());
    }
    if response["sql_dialect"]["name"].as_str().is_none() {
        return Err("このデータはAIチャットでの分析に対応していません".into());
    }
    Ok(response)
}

fn query_id(value: &str) -> bool {
    value.len() == 29
        && value.starts_with("qj_")
        && value[3..].bytes().all(|b| b.is_ascii_alphanumeric())
}

pub fn parse_dialogue_event(event: &Value) -> Result<Option<Value>, String> {
    match event["type"].as_str() {
        Some("progress") => Ok(None),
        Some("answer") => {
            let answer = &event["answer"];
            let text = answer["text"]
                .as_str()
                .filter(|s| !s.is_empty() && s.chars().count() <= 32768)
                .ok_or("分析の回答を読み取れませんでした")?;
            let jobs = answer["query_jobs"]
                .as_array()
                .filter(|jobs| jobs.len() <= 12)
                .ok_or("分析結果の参照先を確認できません")?;
            if jobs
                .iter()
                .any(|id| id.as_str().is_none_or(|id| !query_id(id)))
            {
                return Err("分析結果の参照先を確認できません".into());
            }
            let mut checked = json!({"text":text,"query_jobs":jobs});
            if let Some(token) = answer["continuation"]["token"]
                .as_str()
                .filter(|t| !t.is_empty() && t.len() <= 90)
            {
                let turns = answer["continuation"]["turns"]
                    .as_u64()
                    .filter(|n| (1..=3).contains(n))
                    .ok_or("会話の継続情報を確認できません")?;
                checked["continuation"] = json!({"token":token,"turns":turns});
            }
            Ok(Some(checked))
        }
        Some("failed") => Err(match event["code"].as_str() {
            Some("DATA_FORBIDDEN") => "このデータを分析する権限がありません",
            Some("DATA_LIMIT") => "分析の利用上限に達しました",
            Some("DIALOGUE_RESTART_REQUIRED") => {
                "会話または閲覧条件が変わりました。新しい会話を始めてください"
            }
            Some("DIALOGUE_CANCELLED") => "分析を停止しました",
            Some("DATA_RESULT_UNAVAILABLE") => {
                "データの公開状態または保持期限が変わりました。読み直してください"
            }
            _ => "分析を完了できませんでした。実行状況を確認してください",
        }
        .into()),
        _ => Err("分析の応答形式を確認できませんでした".into()),
    }
}

pub fn ask_question(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    cancel: &AtomicBool,
) -> Result<Value, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("分析を停止しました".into());
    }
    let response = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(90))
        .build()
        .post(&format!(
            "https://field-agent-stream.txcloud.app{ANALYSIS}/dialogue"
        ))
        .set("Authorization", &format!("Bearer {token}"))
        .set("x-operator-id", &tenant.id)
        .set("x-platform-id", &tenant.platform_id)
        .set("Accept", "text/event-stream")
        .send_json(body.clone())
        .map_err(|error| match error {
            ureq::Error::Status(401, _) => api::EXPIRED.into(),
            ureq::Error::Status(403, _) => "このデータを分析する権限がありません".into(),
            _ => "分析サービスに接続できませんでした。実行状況を確認してください".to_owned(),
        })?;
    let mut reader = BufReader::new(response.into_reader());
    let mut data = String::new();
    let mut total = 0usize;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("分析を停止しました".into());
        }
        let mut line = String::new();
        let read = (&mut reader)
            .take(262145)
            .read_line(&mut line)
            .map_err(|_| "分析の接続が切れました。実行状況を確認してください")?;
        total += read;
        if read == 0 {
            return Err("回答の前に接続が切れました。質問は自動で再送されません。実行状況を確認してください".into());
        }
        if total > 8 * 1024 * 1024 || data.len() + line.len() > 262144 {
            return Err("分析の応答が大きすぎます".into());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some(part) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(part.strip_prefix(' ').unwrap_or(part));
            continue;
        }
        if !line.is_empty() || data.is_empty() {
            continue;
        }
        let event: Value =
            serde_json::from_str(&data).map_err(|_| "分析の応答を読み取れませんでした")?;
        data.clear();
        if let Some(answer) = parse_dialogue_event(&event)? {
            return Ok(answer);
        }
    }
}

pub fn dialogue_status(
    token: &str,
    tenant: &Tenant,
    run: &str,
    cancel: bool,
) -> Result<Value, String> {
    api::send_json(
        token,
        tenant,
        "POST",
        &format!(
            "{ANALYSIS}/dialogue/{}",
            if cancel { "cancel" } else { "status" }
        ),
        &json!({"run_id":run}),
        Duration::from_secs(25),
    )
}

pub fn validate_result_page(
    page: &Value,
    job: &str,
    version: &str,
    columns: Option<&Value>,
) -> Result<(), String> {
    let schema = &page["schema"]["columns"];
    let versions = page["read_versions"]
        .as_object()
        .ok_or("分析結果の参照版を確認できません")?;
    if page["job_id"] != job
        || versions.len() != 1
        || !versions.values().all(|v| v.as_str() == Some(version))
        || !schema.is_array()
        || columns.is_some_and(|before| before != schema)
        || page["total_rows"].as_u64().is_none()
        || page["truncated"].as_bool().is_none()
        || page
            .get("next_page_token")
            .is_some_and(|next| !next.is_null() && !next.is_string())
    {
        return Err("分析結果の参照版または列が変わりました。読み直してください".into());
    }
    let width = schema.as_array().unwrap().len();
    if schema
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["name"].as_str().is_none())
        || page["rows"].as_array().is_none_or(|rows| {
            rows.iter()
                .any(|row| row.as_array().is_none_or(|cells| cells.len() != width))
        })
    {
        return Err("分析結果の行を読み取れませんでした".into());
    }
    Ok(())
}

pub fn analysis_result(
    token: &str,
    tenant: &Tenant,
    job: &str,
    version: &str,
) -> Result<Value, String> {
    if !query_id(job) {
        return Err("分析結果を特定できません".into());
    }
    let mut rows = Vec::new();
    let mut columns = None;
    let mut cursor = String::new();
    let mut seen = HashSet::new();
    let mut truncated = false;
    for page_number in 0..8 {
        let base = format!("{ANALYSIS}/dialogue/jobs/{}/result", api::encode_query(job));
        let path = if cursor.is_empty() {
            base
        } else {
            super::model::set_query(&base, "page_token", &cursor)
        };
        let page = api::fetch_record(token, tenant, &path)?;
        validate_result_page(&page, job, version, columns.as_ref())?;
        columns = Some(page["schema"]["columns"].clone());
        let incoming = page["rows"].as_array().unwrap();
        let available = 200usize.saturating_sub(rows.len());
        truncated |= incoming.len() > available;
        rows.extend(incoming.iter().take(available).cloned());
        let next = page["next_page_token"].as_str().unwrap_or("");
        truncated |= page["truncated"] == true
            || page["total_rows"].as_u64().is_some_and(|count| count > 200);
        if next.is_empty() {
            break;
        }
        if incoming.is_empty() || !seen.insert(next.to_owned()) {
            return Err("分析結果のページを正しく読み取れませんでした".into());
        }
        if rows.len() >= 200 || page_number == 7 {
            truncated = true;
            break;
        }
        cursor = next.into();
    }
    Ok(json!({"columns":columns,"rows":rows,"truncated":truncated}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Value {
        json!({"snapshot_id":"snapshot","captured_at":"2026-11-19T00:00:00Z","contract":{"date_from":"2026-11-01","date_to":"2026-11-30","currency":"JPY","calendar_days":30,"tax_basis":"tax_inclusive"},"metrics":[{"key":"sales_total","status":"available","value":"9007199254740993.01"},{"key":"purchase_cost_total","status":"available","value":"123.4567890123456789"}]})
    }
    fn draft() -> Value {
        json!({"periodFrom":"2026-11-01","periodTo":"2026-11-30","profitDefinition":"menu_margin","scenarios":[{"label":"案A","priceChangePct":10,"volumeChangePct":-20,"promotionYen":1000}]})
    }
    #[test]
    fn exact_scenario_money_and_percentages_never_use_floats() {
        assert_eq!(
            apply_percent("9007199254740993.01", 10).unwrap(),
            "9907919180215092.311"
        );
        assert_eq!(apply_percent("10.001", -100).unwrap(), "0");
        let input = scenario_input(&draft(), &snapshot()).unwrap();
        assert_eq!(
            input["baseline"]["ingredients"][0]["unitPrice"],
            "123.4567890123456789"
        );
        assert_eq!(
            input["cases"][0]["changes"][0]["change"]["quantityChangeRate"],
            "-0.2"
        );
        assert_eq!(
            input["cases"][0]["changes"][1]["change"]["cost"]["amount"],
            "1000"
        );
        assert_eq!(input["baseline"]["foodCostBasis"], "lossInclusive");
    }
    #[test]
    fn missing_or_incompatible_actuals_do_not_become_zero_cost() {
        let input = scenario_input(&draft(), &snapshot()).unwrap();
        assert!(input["baseline"]["costs"][0]["amount"].is_null());
        let mut snapshot = snapshot();
        snapshot["metrics"][0]["tax_basis"] = json!("tax_exclusive");
        assert!(scenario_input(&draft(), &snapshot).is_err());
        let mut draft = draft();
        draft["periodTo"] = json!("2026-11-19");
        snapshot["contract"]["date_to"] = draft["periodTo"].clone();
        snapshot["metrics"][0]["tax_basis"] = json!("tax_inclusive");
        assert!(scenario_input(&draft, &snapshot).is_err());
    }
    #[test]
    fn absent_food_loss_remains_uncalculated_and_zero_promotion_does_not_add_a_monthly_constraint()
    {
        let mut draft = draft();
        draft["scenarios"][0]["promotionYen"] = json!(0);
        let calculation = json!({"snapshot":snapshot(),"draft":draft,"result":{"inputFingerprint":"run-1","baseline":{"totals":{"contributionMargin":"10","wasteCost":"0"}},"cases":[]}});
        let rows = calculation_rows(&calculation);
        assert!(
            rows[0]
                .1
                .iter()
                .any(|(label, value)| label == "食品ロス" && value.starts_with("未算出"))
        );
        let body = workspace_body(&calculation, "").unwrap();
        assert!(
            body["scenarios"][1]["assumptions"]
                .get("promo_cost_yen")
                .is_none()
        );
    }
    #[test]
    fn dialogue_answers_and_result_versions_are_validated() {
        let id = "qj_00000000000000000000000001";
        assert!(
            parse_dialogue_event(
                &json!({"type":"answer","answer":{"text":"答え","query_jobs":[id]}})
            )
            .unwrap()
            .is_some()
        );
        assert!(
            parse_dialogue_event(
                &json!({"type":"answer","answer":{"text":"答え","query_jobs":["../other"]}})
            )
            .is_err()
        );
        let mut page = json!({"job_id":id,"read_versions":{"source_data":"version-a"},"schema":{"columns":[{"name":"売上","type":"decimal"}]},"rows":[["9007199254740993.01"]],"total_rows":1,"truncated":false});
        validate_result_page(&page, id, "version-a", None).unwrap();
        assert!(validate_result_page(&page, id, "version-b", None).is_err());
        page["total_rows"] = Value::Null;
        assert!(validate_result_page(&page, id, "version-a", None).is_err());
        page["total_rows"] = json!(1);
        page["rows"] = json!([[1, 2]]);
        assert!(validate_result_page(&page, id, "version-a", None).is_err());
    }
}
