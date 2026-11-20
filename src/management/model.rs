use super::schema::*;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, TimeZone};
use serde_json::{Number, Value, json};

pub fn value_at<'a>(value: &'a Value, path: &str) -> &'a Value {
    if path.is_empty() {
        return value;
    }
    path.split('.').fold(value, |value, key| match value {
        Value::Array(items) => key
            .parse::<usize>()
            .ok()
            .and_then(|index| items.get(index))
            .unwrap_or(&Value::Null),
        _ => value.get(key).unwrap_or(&Value::Null),
    })
}

pub fn set_at(value: &mut Value, path: &str, replacement: Value) {
    if path.is_empty() {
        *value = replacement;
        return;
    }
    let (first, rest) = path
        .split_once('.')
        .map_or((path, None), |(a, b)| (a, Some(b)));
    if let Value::Array(items) = value {
        if let Ok(index) = first.parse::<usize>() {
            if let Some(item) = items.get_mut(index) {
                if let Some(rest) = rest {
                    set_at(item, rest, replacement);
                } else {
                    *item = replacement;
                }
            }
        }
        return;
    }
    if !value.is_object() {
        *value = json!({});
    }
    if let Some(rest) = rest {
        set_at(
            value
                .as_object_mut()
                .unwrap()
                .entry(first)
                .or_insert_with(|| json!({})),
            rest,
            replacement,
        );
    } else {
        value[first] = replacement;
    }
}

pub fn input_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => String::new(),
    }
}

pub fn display(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(true) => "はい".into(),
        Value::Bool(false) => "いいえ".into(),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Array(values) => values
            .iter()
            .map(display)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("、"),
        Value::Object(_) => {
            if let Some(label) = value["label"].as_str() {
                if !value["value"].is_null() {
                    return format!("{label}: {}", display(&value["value"]));
                }
            }
            for key in [
                "name",
                "label",
                "title",
                "displayName",
                "description",
                "body",
                "text",
                "outcome",
                "message",
                "reason",
            ] {
                if let Some(text) = value[key].as_str().filter(|v| !v.is_empty()) {
                    return text.into();
                }
            }
            let address = [
                "postalCode",
                "prefecture",
                "state",
                "city",
                "address1",
                "address2",
                "line1",
                "line2",
                "street",
                "building",
            ]
            .iter()
            .filter_map(|key| value[key].as_str())
            .collect::<Vec<_>>()
            .join(" ");
            if address.is_empty() {
                "登録済み".into()
            } else {
                address
            }
        }
    }
}

fn currency(record: &Value, fallback: &str) -> String {
    record["currency"]
        .as_str()
        .or_else(|| record["currencyCode"].as_str())
        .unwrap_or(fallback)
        .trim()
        .to_uppercase()
}

fn minor_to_decimal(value: &Value, currency: &str) -> Option<String> {
    let amount = input_text(value).parse::<i128>().ok()?;
    let exponent = crate::features::minor_unit_digits(currency);
    let divisor = 10_u128.pow(exponent as u32);
    let number = amount.unsigned_abs();
    let sign = if amount < 0 { "-" } else { "" };
    Some(if exponent == 0 {
        format!("{sign}{number}")
    } else {
        format!(
            "{sign}{}.{:0width$}",
            number / divisor,
            number % divisor,
            width = exponent
        )
    })
}

fn money_to_minor(text: &str, currency: &str) -> Result<Value, String> {
    if currency.len() != 3 || !currency.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err("通貨は3文字のコードで指定してください".into());
    }
    if !valid_decimal(text, false) {
        return Err("金額は半角の数値で入力してください".into());
    }
    let exponent = crate::features::minor_unit_digits(currency);
    let negative = text.starts_with('-');
    let (whole, fraction) = text
        .trim_start_matches('-')
        .split_once('.')
        .unwrap_or((text.trim_start_matches('-'), ""));
    if fraction.len() > exponent && fraction[exponent..].bytes().any(|digit| digit != b'0') {
        return Err(format!("{currency}の金額は小数点以下{exponent}桁までです"));
    }
    let fraction = &fraction[..fraction.len().min(exponent)];
    let whole = whole.parse::<i128>().map_err(|_| "金額が大きすぎます")?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<i128>()
            .map_err(|_| "金額を確認してください")?
            * 10_i128.pow((exponent - fraction.len()) as u32)
    };
    let amount = whole
        .checked_mul(10_i128.pow(exponent as u32))
        .and_then(|v| v.checked_add(fraction))
        .ok_or("金額が大きすぎます")?;
    let amount = if negative { -amount } else { amount };
    Ok(Value::Number(Number::from(
        i64::try_from(amount).map_err(|_| "金額が大きすぎます")?,
    )))
}

pub fn display_field(resource: Resource, row: &Value, key: &str) -> String {
    if matches!(resource.key, "reservations" | "reservation-waitlist") {
        if matches!(key, "status" | "paymentStatus" | "previousStatus") {
            return crate::features::status_label(&input_text(value_at(row, key)));
        }
        if matches!(
            key,
            "startsAt"
                | "endsAt"
                | "desiredStartsAt"
                | "desiredEndsAt"
                | "createdAt"
                | "updatedAt"
                | "cancelledAt"
                | "closedAt"
                | "enqueuedAt"
        ) {
            if let Some(time) = value_at(row, key)
                .as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            {
                return time
                    .with_timezone(&FixedOffset::east_opt(9 * 3600).unwrap())
                    .format("%Y-%m-%d %H:%M（日本時間）")
                    .to_string();
            }
        }
    }
    if let Some(key) = key
        .strip_prefix("@money:")
        .or_else(|| key.ends_with("Minor").then_some(key))
    {
        let currency = currency(row, "JPY");
        return minor_to_decimal(value_at(row, key), &currency)
            .map(|amount| {
                if currency == "JPY" {
                    format!("¥{amount}")
                } else {
                    format!("{amount} {currency}")
                }
            })
            .unwrap_or_default();
    }
    if key.ends_with("Nanodollar") {
        return super::insights::format_nanodollar(value_at(row, key)).unwrap_or_default();
    }
    let value = display(value_at(row, key));
    if resource.key.contains("dataset") {
        super::insights::readable_dataset_name(&value).to_owned()
    } else {
        value
    }
}

pub fn title(resource: Resource, row: &Value) -> String {
    resource
        .title
        .iter()
        .map(|key| display_field(resource, row, key))
        .find(|s| !s.is_empty())
        .unwrap_or_else(|| resource.label.into())
}

pub fn subtitle(resource: Resource, row: &Value) -> String {
    resource
        .subtitle
        .iter()
        .map(|key| display_field(resource, row, key))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

pub fn status(resource: Resource, row: &Value) -> String {
    resource
        .status
        .map(|key| {
            let value = value_at(row, key);
            if value.is_boolean() {
                display(value)
            } else {
                crate::features::status_label(&input_text(value))
            }
        })
        .unwrap_or_default()
}

pub fn detail_rows(resource: Resource, row: &Value) -> Vec<(String, String)> {
    resource
        .detail_fields
        .iter()
        .filter_map(|(label, key)| {
            let value = display_field(resource, row, key);
            (!value.is_empty()).then(|| ((*label).into(), value))
        })
        .collect()
}

pub fn expanded_default(default: &str) -> String {
    let now: DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    let local = now.with_timezone(&FixedOffset::east_opt(9 * 3600).unwrap());
    match default.trim_start_matches('@') {
        "today" => local.format("%Y-%m-%d").to_string(),
        "now" => local.to_rfc3339(),
        "month" | "currentMonth" => local.format("%Y-%m").to_string(),
        "monthStart" => local.format("%Y-%m-01").to_string(),
        "nextMonthStart" | "monthEnd" => {
            let first =
                NaiveDate::parse_from_str(&local.format("%Y-%m-01").to_string(), "%Y-%m-%d")
                    .unwrap();
            let next = first.checked_add_months(chrono::Months::new(1)).unwrap();
            if default.trim_start_matches('@') == "monthEnd" {
                next.pred_opt().unwrap().format("%Y-%m-%d").to_string()
            } else {
                next.format("%Y-%m-%d").to_string()
            }
        }
        _ => default.into(),
    }
}

/// Preserve exact original values, including unknown enum values. Never silently
/// substitute the first choice and overwrite a value the app does not understand.
pub fn draft(fields: &[Field], original: &Value) -> Value {
    draft_with_currency(fields, original, "JPY")
}

pub fn mutation_draft(operation: Mutation, original: &Value) -> Result<Value, String> {
    let visible = match operation.prefill {
        Some(prefill) => prefill(original)?,
        None => original.clone(),
    };
    Ok(draft(operation.fields, &visible))
}

fn draft_with_currency(fields: &[Field], original: &Value, parent_currency: &str) -> Value {
    let currency = currency(original, parent_currency);
    let mut result = json!({});
    for field in fields {
        let value = value_at(original, field.read_key);
        let initial = match field.kind {
            InputKind::Hidden => value.clone(),
            InputKind::ReadOnly if field.read_key.starts_with("@money:") => {
                let amount = value_at(original, &field.read_key[7..]);
                Value::String(
                    minor_to_decimal(amount, &currency)
                        .map(|amount| {
                            if currency == "JPY" {
                                format!("¥{amount}")
                            } else {
                                format!("{amount} {currency}")
                            }
                        })
                        .unwrap_or_default(),
                )
            }
            InputKind::Attachments(_) => value
                .as_array()
                .cloned()
                .map(Value::Array)
                .unwrap_or_else(|| json!([])),
            InputKind::Lines(children) => Value::Array(
                value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|row| draft_with_currency(children, row, &currency))
                    .collect(),
            ),
            InputKind::Money => Value::String(
                minor_to_decimal(value, &currency)
                    .unwrap_or_else(|| field.default.map(expanded_default).unwrap_or_default()),
            ),
            InputKind::StringLines => Value::String(
                value
                    .as_array()
                    .map(|rows| {
                        rows.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default(),
            ),
            InputKind::Bool => {
                if value.is_boolean() {
                    value.clone()
                } else {
                    Value::Bool(field.default == Some("true"))
                }
            }
            _ => Value::String(if !value.is_null() {
                input_text(value)
            } else {
                field.default.map(expanded_default).unwrap_or_default()
            }),
        };
        set_at(&mut result, field.key, initial);
    }
    result
}

fn valid_decimal(text: &str, integer: bool) -> bool {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    if unsigned.is_empty() {
        return false;
    }
    let parts: Vec<_> = unsigned.split('.').collect();
    if parts.len() > if integer { 1 } else { 2 } {
        return false;
    }
    parts
        .iter()
        .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

pub fn body(fields: &[Field], draft: &Value) -> Result<Value, String> {
    body_with_currency(fields, draft, "JPY")
}

fn body_with_currency(
    fields: &[Field],
    draft: &Value,
    parent_currency: &str,
) -> Result<Value, String> {
    let currency = currency(draft, parent_currency);
    let mut result = json!({});
    for field in fields {
        let raw = value_at(draft, field.key);
        if matches!(field.kind, InputKind::ReadOnly) {
            continue;
        }
        let text = input_text(raw);
        let text = text.trim();
        let empty = match field.kind {
            InputKind::Lines(_) | InputKind::Attachments(_) => {
                raw.as_array().is_none_or(Vec::is_empty)
            }
            InputKind::Bool => !raw.is_boolean(),
            _ => text.is_empty(),
        };
        if empty {
            if field.required {
                return Err(format!("{}を入力してください", field.label));
            }
            match field.kind {
                InputKind::Lines(_) | InputKind::StringLines | InputKind::Attachments(_) => {
                    set_at(&mut result, field.key, json!([]))
                }
                _ => match field.empty {
                    Empty::Omit => {}
                    Empty::Null => set_at(&mut result, field.key, Value::Null),
                    Empty::String => set_at(&mut result, field.key, Value::String(String::new())),
                },
            }
            continue;
        }
        let value = match field.kind {
            InputKind::Hidden => raw.clone(),
            InputKind::Attachments(_) => Value::Array(
                raw.as_array()
                    .ok_or("添付資料を確認してください")?
                    .iter()
                    .map(|file| {
                        let id = file["receiptId"]
                            .as_str()
                            .filter(|id| !id.is_empty())
                            .ok_or("添付資料の送信が完了していません")?;
                        Ok(json!({"receiptId":id,"fileName":file["fileName"].as_str()}))
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            ),
            InputKind::Money => money_to_minor(text, &currency)
                .map_err(|message| format!("{}: {message}", field.label))?,
            InputKind::Integer => {
                if !valid_decimal(text, true) {
                    return Err(format!("{}は整数で入力してください", field.label));
                }
                let number = text
                    .parse::<i64>()
                    .map_err(|_| format!("{}の数値が大きすぎます", field.label))?;
                Value::Number(Number::from(number))
            }
            InputKind::IntegerString | InputKind::Decimal => {
                if !valid_decimal(text, matches!(field.kind, InputKind::IntegerString)) {
                    return Err(format!("{}は半角の数値で入力してください", field.label));
                }
                Value::String(text.into())
            }
            InputKind::Bool => Value::Bool(
                raw.as_bool()
                    .ok_or_else(|| format!("{}を選択してください", field.label))?,
            ),
            InputKind::Date => {
                let parsed = NaiveDate::parse_from_str(text, "%Y-%m-%d")
                    .map_err(|_| format!("{}は年-月-日で入力してください", field.label))?;
                if parsed.format("%Y-%m-%d").to_string() != text {
                    return Err(format!("{}は年-月-日で入力してください", field.label));
                }
                Value::String(text.into())
            }
            InputKind::DateTime => {
                let parsed = DateTime::parse_from_rfc3339(text)
                    .ok()
                    .or_else(|| {
                        let local = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").ok()?;
                        FixedOffset::east_opt(9 * 3600)?
                            .from_local_datetime(&local)
                            .single()
                    })
                    .ok_or_else(|| {
                        format!(
                            "{}は年-月-日 時:分で入力してください（日本時間）",
                            field.label
                        )
                    })?;
                Value::String(parsed.to_rfc3339())
            }
            InputKind::Select(choices) => {
                if !choices.iter().any(|choice| choice.value == text) {
                    return Err(format!("{}を選び直してください", field.label));
                }
                Value::String(text.into())
            }
            InputKind::Lines(children) => Value::Array(
                raw.as_array()
                    .ok_or_else(|| format!("{}を入力してください", field.label))?
                    .iter()
                    .enumerate()
                    .map(|(i, row)| {
                        body_with_currency(children, row, &currency)
                            .map_err(|message| format!("{} {}行目: {message}", field.label, i + 1))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            InputKind::StringLines => Value::Array(
                text.lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| Value::String(s.into()))
                    .collect(),
            ),
            InputKind::Email => {
                if !text.contains('@') || text.chars().any(char::is_whitespace) {
                    return Err(format!("{}を確認してください", field.label));
                }
                Value::String(text.into())
            }
            _ => Value::String(text.into()),
        };
        set_at(&mut result, field.key, value);
    }
    Ok(result)
}

pub fn request_body(
    operation: Mutation,
    draft_value: &Value,
    original: &Value,
    key: &str,
) -> Result<Value, String> {
    if !operation.is_available(original) {
        return Err("状態が変わったため、この操作はできません。一覧を読み直してください".into());
    }
    let mut payload = body(operation.fields, draft_value)?;
    validate_hidden(operation.fields, &payload, original)?;
    if let Some(prepare) = operation.prepare {
        prepare(&mut payload, original)?;
    }
    if let Idempotency::Body(field) = operation.idempotency {
        payload[field] = Value::String(key.into());
    }
    Ok(payload)
}

/// A line keeps its own identity through deletion/reordering. Never recover an
/// ID by array position: that would attach another line's ID after a removal.
fn validate_hidden(fields: &[Field], payload: &Value, original: &Value) -> Result<(), String> {
    for field in fields {
        match field.kind {
            InputKind::Hidden => {
                let value = value_at(payload, field.key);
                if !value.is_null() && value != value_at(original, field.read_key) {
                    return Err("明細の識別情報が変わっています。読み直してください".into());
                }
            }
            InputKind::Lines(children) => {
                let prior = value_at(original, field.read_key)
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if field.fixed_rows
                    && value_at(payload, field.key).as_array().map_or(0, Vec::len) != prior.len()
                {
                    return Err(format!(
                        "{}の項目が変わっています。読み直してください",
                        field.label
                    ));
                }
                let identity = children
                    .iter()
                    .find(|child| matches!(child.kind, InputKind::Hidden));
                let mut seen = std::collections::HashSet::new();
                for (index, row) in value_at(payload, field.key)
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    let old = if let Some(identity) = identity {
                        let key = value_at(row, identity.key);
                        if key.is_null() {
                            &Value::Null
                        } else {
                            if !seen.insert(key.to_string()) {
                                return Err("同じ明細が重複しています。読み直してください".into());
                            }
                            prior
                                .iter()
                                .find(|prior| value_at(prior, identity.read_key) == key)
                                .ok_or("明細の識別情報を確認できません。読み直してください")?
                        }
                    } else {
                        prior.get(index).unwrap_or(&Value::Null)
                    };
                    validate_hidden(children, row, old)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn record_path(resource: Resource, template: &str, original: &Value) -> Result<String, String> {
    interpolate_path(template, |key| {
        input_text(value_at(
            original,
            if key == "id" { resource.id_key } else { key },
        ))
    })
}

/// A create form can select the parent of a nested resource. Existing identity
/// always wins over an editable value; `{id}` never falls back to form input.
pub fn request_path(
    resource: Resource,
    template: &str,
    original: &Value,
    payload: &Value,
) -> Result<String, String> {
    interpolate_path(template, |key| {
        let source_key = if key == "id" { resource.id_key } else { key };
        let original = input_text(value_at(original, source_key));
        if !original.is_empty() || key == "id" {
            original
        } else {
            input_text(value_at(payload, source_key))
        }
    })
}

fn interpolate_path(template: &str, resolve: impl Fn(&str) -> String) -> Result<String, String> {
    if !template.starts_with('/') || template.starts_with("//") {
        return Err("操作の接続先を確認できません".into());
    }
    let mut path = String::new();
    let mut rest = template;
    while let Some((prefix, after)) = rest.split_once('{') {
        path.push_str(prefix);
        let (key, tail) = after
            .split_once('}')
            .ok_or("接続先の形式を確認できません")?;
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            return Err("接続先の項目を確認できません".into());
        }
        let value = resolve(key);
        if value.is_empty() {
            return Err("対象を特定できません。必要な条件を選んで読み直してください".into());
        }
        // A literal dot segment has URL semantics even after standard percent encoding.
        if value == "." || value == ".." {
            return Err("対象の識別情報を確認できません".into());
        }
        path.push_str(&crate::api::encode_query(&value));
        rest = tail;
    }
    if rest.contains('}') {
        return Err("接続先の形式を確認できません".into());
    }
    path.push_str(rest);
    Ok(path)
}

pub fn query_path(path: &str, fields: &[Field], draft_value: &Value) -> Result<String, String> {
    let filters = body(fields, draft_value)?;
    let template = path;
    let mut path = interpolate_path(template, |key| input_text(value_at(&filters, key)))?;
    for (key, value) in filters.as_object().into_iter().flatten() {
        if !value.is_null() && !template.contains(&format!("{{{key}}}")) {
            path = set_query(&path, key, &input_text(value));
        }
    }
    Ok(path)
}

/// Replace an existing parameter instead of appending a second limit/offset.
pub fn set_query(path: &str, key: &str, value: &str) -> String {
    let (base, query) = path.split_once('?').unwrap_or((path, ""));
    let encoded_key = crate::api::encode_query(key);
    let mut parts: Vec<String> = query
        .split('&')
        .filter(|part| !part.is_empty() && part.split('=').next() != Some(encoded_key.as_str()))
        .map(str::to_owned)
        .collect();
    parts.push(format!(
        "{}={}",
        encoded_key,
        crate::api::encode_query(value)
    ));
    format!("{base}?{}", parts.join("&"))
}

fn explicit_limit_reached(path: &str, count: usize) -> bool {
    path.split_once('?')
        .into_iter()
        .flat_map(|(_, query)| query.split('&'))
        .filter_map(|part| part.split_once('='))
        .find(|(key, _)| *key == "limit")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .is_some_and(|limit| limit > 0 && count >= limit)
}

#[derive(Clone, Debug, Default)]
pub struct Collection {
    pub rows: Vec<Value>,
    pub meta: Value,
    pub truncated: bool,
}

pub fn extract_rows(response: &Value, key: &str) -> Result<Vec<Value>, String> {
    if let Some(key) = key.strip_prefix("@object:") {
        let result = value_at(response, key);
        return if result.is_object() {
            Ok(vec![result.clone()])
        } else {
            Err("詳細の応答を読み取れませんでした".into())
        };
    }
    let result = if response.is_array() {
        response
    } else {
        value_at(response, key)
    };
    if let Some(rows) = result.as_array() {
        return Ok(rows.clone());
    }
    // Explicitly empty key also represents a report/settings document.
    if key.is_empty() && result.is_object() {
        return Ok(vec![result.clone()]);
    }
    Err("応答の形式が想定と異なります。しばらくしてから読み直してください".into())
}

pub fn fetch_collection(
    token: &str,
    tenant: &crate::api::Tenant,
    resource: Resource,
    filters: &Value,
) -> Result<Collection, String> {
    query_path(resource.path, resource.filters, filters)?;
    if resource.admin_only && resource.path != "/v1/field/iam/users" {
        let access = crate::api::fetch_record(token, tenant, "/v1/field/iam/users")?;
        if !access["users"].is_array() {
            return Err("管理設定へのアクセス権を確認できませんでした".into());
        }
    }
    if let Some(loader) = resource.loader {
        // Validate required filters before invoking an aggregate, just as for simple lists.
        query_path(resource.path, resource.filters, filters)?;
        let response = loader(token, tenant, filters)?;
        return Ok(Collection {
            rows: extract_rows(&response, resource.list_key)?,
            truncated: response["truncated"].as_bool().unwrap_or(false),
            meta: response,
        });
    }
    collect_pages(resource, filters, |path| {
        crate::api::fetch_record(token, tenant, path)
    })
}

fn collect_pages(
    resource: Resource,
    filters: &Value,
    mut fetch: impl FnMut(&str) -> Result<Value, String>,
) -> Result<Collection, String> {
    let base = query_path(resource.path, resource.filters, filters)?;
    let mut result = Collection::default();
    let mut cursor = String::new();
    let mut previous_page = Value::Null;
    let mut seen = std::collections::HashSet::new();
    for page in 0..20 {
        let path = match resource.pagination {
            Pagination::None => base.clone(),
            Pagination::Offset { limit } => set_query(
                &set_query(&base, "limit", &limit.to_string()),
                "offset",
                &result.rows.len().to_string(),
            ),
            Pagination::Page { size } => set_query(
                &set_query(&base, "pageSize", &size.to_string()),
                "page",
                &(page + 1).to_string(),
            ),
            Pagination::Cursor { limit } => {
                let path = set_query(&base, "limit", &limit.to_string());
                if cursor.is_empty() {
                    path
                } else {
                    set_query(&path, "cursor", &cursor)
                }
            }
        };
        let response = fetch(&path)?;
        let rows = extract_rows(&response, resource.list_key)?;
        let count = rows.len();
        let page_rows = Value::Array(rows.clone());
        if page > 0 && count > 0 && page_rows == previous_page {
            result.truncated = true;
            break;
        }
        previous_page = page_rows;
        if !matches!(resource.pagination, Pagination::None) {
            for row in &rows {
                let parts: Vec<_> = resource
                    .id_key
                    .split('|')
                    .map(|key| input_text(value_at(row, key)))
                    .collect();
                if parts.iter().all(|part| !part.is_empty()) && !seen.insert(parts) {
                    return Err(
                        "取得中に一覧が変わり、同じ項目が重複しました。再読み込みしてください"
                            .into(),
                    );
                }
            }
        }
        result.rows.extend(rows);
        if page == 0 {
            result.meta = response.clone();
        }
        let has_more = response["hasMore"]
            .as_bool()
            .or_else(|| response["has_more"].as_bool())
            .or_else(|| response["pagination"]["hasMore"].as_bool())
            .or_else(|| response["hasNextPage"].as_bool())
            .or_else(|| response["pageInfo"]["hasNextPage"].as_bool());
        let total = response["total"]
            .as_u64()
            .or_else(|| response["totalCount"].as_u64())
            .or_else(|| response["total_count"].as_u64())
            .or_else(|| response["pagination"]["total"].as_u64());
        let next = response["nextCursor"]
            .as_str()
            .or_else(|| response["next_cursor"].as_str())
            .unwrap_or("");
        if count == 0
            && (has_more == Some(true)
                || total.is_some_and(|total| (result.rows.len() as u64) < total))
        {
            return Err("一覧の続きを取得できませんでした。再読み込みしてください".into());
        }
        let more = match resource.pagination {
            Pagination::None => has_more.unwrap_or_else(|| {
                total.map_or_else(
                    || explicit_limit_reached(&base, count),
                    |total| (result.rows.len() as u64) < total,
                )
            }),
            Pagination::Cursor { .. } => !next.is_empty() || has_more == Some(true),
            Pagination::Offset { limit } => has_more.unwrap_or_else(|| {
                total.map_or(count >= limit, |total| (result.rows.len() as u64) < total)
            }),
            Pagination::Page { size } => has_more.unwrap_or_else(|| {
                total.map_or(count >= size, |total| (result.rows.len() as u64) < total)
            }),
        };
        if matches!(resource.pagination, Pagination::None) {
            result.truncated = more;
            break;
        }
        if count == 0 || !more {
            break;
        }
        if matches!(resource.pagination, Pagination::Cursor { .. }) {
            if next.is_empty() || next == cursor {
                result.truncated = has_more == Some(true) || !next.is_empty();
                break;
            }
            cursor = next.into();
        }
        if page == 19 {
            result.truncated = true;
        }
    }
    Ok(result)
}

pub fn capability_allowed(operation: Mutation, capabilities: &Value, collection: &Value) -> bool {
    operation.capability.is_none_or(|key| {
        if let Some(key) = key.strip_prefix("collection.") {
            value_at(collection, key).as_bool() == Some(true)
        } else {
            value_at(capabilities, key).as_bool() == Some(true)
        }
    })
}

pub fn lookup_options(
    lookup: Lookup,
    data: &Value,
    parent: Option<(&str, &str)>,
) -> Result<Vec<(String, String)>, String> {
    let rows = extract_rows(data, lookup.list_key)?;
    let rows = if let Some((parent, children)) = parent {
        rows.into_iter()
            .find(|row| lookup_id(row, lookup.value_key) == parent)
            .and_then(|row| value_at(&row, children).as_array().cloned())
            .unwrap_or_default()
    } else {
        rows
    };
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let id = lookup_id(&row, lookup.value_key);
            let label = lookup
                .label_keys
                .iter()
                .map(|key| display(value_at(&row, key)))
                .find(|s| !s.is_empty());
            if id.is_empty() {
                None
            } else {
                label.map(|label| (id, label))
            }
        })
        .collect())
}

fn lookup_id(row: &Value, keys: &str) -> String {
    keys.split('|')
        .map(|key| input_text(value_at(row, key)))
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

pub fn fetch_lookup(
    token: &str,
    tenant: &crate::api::Tenant,
    lookup: Lookup,
) -> Result<(Value, bool), String> {
    let mut resource = crate::management::for_path(lookup.path)
        .unwrap_or_else(|| Resource::new("lookup", "", "", lookup.path, lookup.list_key));
    resource.path = lookup.path;
    resource.list_key = lookup.list_key;
    resource.filters = &[];
    // Lookups address their declared endpoint directly, not another screen's aggregate.
    resource.loader = None;
    resource.admin_only = false;
    let collection = fetch_collection(token, tenant, resource, &json!({}))?;
    let mut data = collection.meta;
    if data.is_array() || lookup.list_key.is_empty() {
        data = Value::Array(collection.rows);
    } else {
        set_at(&mut data, lookup.list_key, Value::Array(collection.rows));
    }
    Ok((data, collection.truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_create_paths_escape_parent_values_and_existing_identity_wins() {
        let resource = Resource::new("shifts", "", "", "/staff", "items");
        assert_eq!(
            request_path(
                resource,
                "/staff/{staffId}/shifts",
                &Value::Null,
                &json!({"staffId":"a/b?#"})
            )
            .unwrap(),
            "/staff/a%2Fb%3F%23/shifts"
        );
        assert_eq!(
            request_path(
                resource,
                "/staff/{staffId}/shifts/{id}",
                &json!({"id":"old","staffId":"original"}),
                &json!({"id":"new","staffId":"changed"})
            )
            .unwrap(),
            "/staff/original/shifts/old"
        );
        assert!(
            request_path(
                resource,
                "/shifts/{id}",
                &Value::Null,
                &json!({"id":"forged"})
            )
            .is_err()
        );
        assert!(record_path(resource, "/shifts/{id}", &json!({"id":".."})).is_err());
        let fields = &[
            Field::text("month", "月").required(),
            Field::text("site", "拠点"),
        ];
        assert_eq!(
            query_path(
                "/months/{month}",
                fields,
                &json!({"month":"2026-11","site":"a/b"})
            )
            .unwrap(),
            "/months/2026-11?site=a%2Fb"
        );
    }

    #[test]
    fn fixed_template_rows_and_attachment_metadata_do_not_become_arbitrary_payloads() {
        const CHILDREN: &[Field] = &[
            Field::new("id", "ID", InputKind::Hidden),
            Field::new("label", "説明", InputKind::ReadOnly),
            Field::new("amount", "金額", InputKind::Integer),
        ];
        const FIELDS: &[Field] = &[
            Field::new("rows", "項目", InputKind::Lines(CHILDREN)).fixed_rows(),
            Field::new("files", "資料", InputKind::Attachments("sales")),
        ];
        let original = json!({"rows":[{"id":"a","label":"元の項目","amount":2}],"files":[{"receiptId":"r","fileName":"receipt.pdf","secret":"do not send"}]});
        let operation = Mutation::new("保存", "POST", "/records", FIELDS);
        let mut draft = draft(FIELDS, &original);
        let body = request_body(operation, &draft, &original, "").unwrap();
        assert!(body["rows"][0].get("label").is_none());
        assert!(body["files"][0].get("secret").is_none());
        draft["rows"][0]["id"] = json!("foreign");
        assert!(request_body(operation, &draft, &original, "").is_err());
        draft["rows"] = json!([]);
        assert!(request_body(operation, &draft, &original, "").is_err());
    }

    #[test]
    fn dependent_template_lookup_uses_parent_id_and_child_key() {
        let lookup = Lookup {
            path: "/sites",
            list_key: "sites",
            value_key: "key|id",
            label_keys: &["name"],
        };
        let options=lookup_options(lookup,&json!({"sites":[{"id":"site","name":"店","templates":[{"id":null,"key":"daily","name":"日報"}]}]}),Some(("site","templates"))).unwrap();
        assert_eq!(options, vec![("daily".into(), "日報".into())]);
    }

    #[test]
    fn decimals_and_large_integer_strings_remain_exact() {
        let fields = [
            Field::new("amount", "金額", InputKind::IntegerString).required(),
            Field::new("quantity", "数量", InputKind::Decimal),
        ];
        let payload = body(
            &fields,
            &json!({"amount":"9007199254740993","quantity":"0.00000000000001"}),
        )
        .unwrap();
        assert_eq!(
            payload,
            json!({"amount":"9007199254740993","quantity":"0.00000000000001"})
        );
        for invalid in ["NaN", "inf", "1e3", "12,345", "+12", "1.2.3"] {
            assert!(body(&fields, &json!({"amount": invalid})).is_err());
        }
    }

    #[test]
    fn dates_and_unknown_enums_fail_closed() {
        const OPTIONS: &[Choice] = &[Choice::new("active", "有効")];
        let fields = [
            Field::new("status", "状態", InputKind::Select(OPTIONS)).required(),
            Field::new("date", "日付", InputKind::Date).required(),
        ];
        assert!(
            body(
                &fields,
                &json!({"status":"new_future_state","date":"2026-11-19"})
            )
            .is_err()
        );
        assert!(body(&fields, &json!({"status":"active","date":"2026-02-30"})).is_err());
        assert_eq!(
            draft(&fields, &json!({"status":"new_future_state"}))["status"],
            "new_future_state"
        );
    }

    #[test]
    fn nested_fields_clear_explicitly_and_arrays_validate() {
        const LINES: &[Field] = &[
            Field::text("name", "品名").required(),
            Field::new("quantity", "数量", InputKind::Decimal).required(),
        ];
        let fields = [
            Field::text("address.city", "市区町村")
                .read("headOfficeAddress.city")
                .empty(Empty::Null),
            Field::new("items", "明細", InputKind::Lines(LINES)).required(),
        ];
        let mut values = draft(
            &fields,
            &json!({"headOfficeAddress":{"city":"札幌市"},"items":[{"name":"食材","quantity":"1.25"}]}),
        );
        set_at(&mut values, "address.city", json!(""));
        assert_eq!(
            body(&fields, &values).unwrap(),
            json!({"address":{"city":null},"items":[{"name":"食材","quantity":"1.25"}]})
        );
        set_at(&mut values, "items.0.quantity", json!("bad"));
        assert!(body(&fields, &values).unwrap_err().contains("1行目"));
    }

    #[test]
    fn ids_and_queries_are_encoded_without_duplicates() {
        let resource = Resource::new("sample", "管理", "項目", "/items", "items");
        assert_eq!(
            record_path(resource, "/items/{id}", &json!({"id":"a/b?#"})).unwrap(),
            "/items/a%2Fb%3F%23"
        );
        assert!(record_path(resource, "/items/{id}", &json!({})).is_err());
        assert_eq!(
            set_query("/items?limit=20&offset=0", "offset", "40"),
            "/items?limit=20&offset=40"
        );
    }

    #[test]
    fn explicit_grants_and_request_identity_are_required() {
        let operation = Mutation::new("追加", "POST", "/items", &[])
            .capability("inventory.manage")
            .idempotency_body("idempotencyKey");
        assert!(!capability_allowed(operation, &Value::Null, &Value::Null));
        assert!(!capability_allowed(
            operation,
            &json!({"inventory":{"manage":false}}),
            &Value::Null
        ));
        assert!(capability_allowed(
            operation,
            &json!({"inventory":{"manage":true}}),
            &Value::Null
        ));
        assert_eq!(
            request_body(operation, &json!({}), &Value::Null, "same-key").unwrap()["idempotencyKey"],
            "same-key"
        );
    }

    #[test]
    fn missing_collections_are_errors_not_empty_results() {
        assert!(extract_rows(&json!({"error":"denied"}), "items").is_err());
        assert_eq!(
            extract_rows(&json!({"data":{"items":[{"id":"one"}]}}), "data.items")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            extract_rows(&json!({"balance":"100"}), "").unwrap().len(),
            1
        );
    }

    #[test]
    fn bounded_collections_without_total_still_report_possible_truncation() {
        assert!(explicit_limit_reached("/waitlist?limit=100", 100));
        assert!(!explicit_limit_reached("/waitlist?limit=100", 99));
        assert!(!explicit_limit_reached("/all", 100));
        assert!(!explicit_limit_reached("/all?limit=0", 0));
    }

    #[test]
    fn jst_inputs_preserve_time_zone_and_currency_precision() {
        let fields = [
            Field::new("at", "日時", InputKind::DateTime),
            Field::text("currency", "通貨"),
            Field::new("amount", "金額", InputKind::Money),
        ];
        let input = json!({"at":"2026-11-19 09:30", "currency":"USD", "amount":"12.35"});
        let result = body(&fields, &input).unwrap();
        assert_eq!(result["at"], "2026-11-19T09:30:00+09:00");
        assert_eq!(result["amount"], 1235);
        assert_eq!(draft(&fields, &result)["amount"], "12.35");
        assert!(body(&fields, &json!({"currency":"JPY", "amount":"1.01"})).is_err());
        assert_eq!(money_to_minor("1.001", "BHD").unwrap(), 1001);
    }

    #[test]
    fn pagination_rejects_partial_overlap_and_missing_promised_rows() {
        let resource = Resource::new("items", "管理", "項目", "/items", "items")
            .pagination(Pagination::Offset { limit: 2 });
        for second_page in [
            json!({"items":[],"total":4}),
            json!({"items":[{"id":"b"},{"id":"c"}],"total":4}),
        ] {
            let mut pages = vec![
                json!({"items":[{"id":"a"},{"id":"b"}],"total":4}),
                second_page,
            ]
            .into_iter();
            assert!(collect_pages(resource, &json!({}), |_| Ok(pages.next().unwrap())).is_err());
        }
        let mut paths = Vec::new();
        let mut pages = vec![
            json!({"items":[{"id":"a"},{"id":"b"}],"total":3}),
            json!({"items":[{"id":"c"}],"total":3}),
        ]
        .into_iter();
        let result = collect_pages(resource, &json!({}), |path| {
            paths.push(path.to_owned());
            Ok(pages.next().unwrap())
        })
        .unwrap();
        assert_eq!(result.rows.len(), 3);
        assert!(!result.truncated);
        assert_eq!(
            paths,
            ["/items?limit=2&offset=0", "/items?limit=2&offset=2"]
        );
    }

    #[test]
    fn cursor_progress_is_required_even_without_has_more() {
        let resource = Resource::new("audit", "管理", "履歴", "/audit", "items")
            .pagination(Pagination::Cursor { limit: 100 });
        let mut pages = vec![
            json!({"items":[{"id":"a"}],"nextCursor":"cursor-1"}),
            json!({"items":[{"id":"b"}],"nextCursor":"cursor-1"}),
        ]
        .into_iter();
        let result = collect_pages(resource, &json!({}), |_| Ok(pages.next().unwrap())).unwrap();
        assert_eq!(result.rows.len(), 2);
        assert!(result.truncated);
    }
}
