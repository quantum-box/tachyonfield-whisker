//! Resource schedules and materialized reservation inventory use separate APIs.
//! A preview is bound to its resource, local dates, rules and day overrides.

use super::{model, schema::*};
use crate::api::{self, Tenant};
use chrono::{DateTime, LocalResult, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{Value, json};
use std::collections::HashSet;

const GROUP: &str = "予約管理";
const RESOURCE_LOOKUP: Lookup = Lookup {
    path: "/v1/erp/reservation-resources?includeInactive=true",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const RESOURCE_FIELD: Field = Field::new(
    "resourceId",
    "場所・設備",
    InputKind::Reference(RESOURCE_LOOKUP),
)
.required();
const FROM: Field = Field::new("from", "開始日", InputKind::Date)
    .required()
    .default("@today");
const TO: Field = Field::new("to", "終了日（この日を含む）", InputKind::Date)
    .required()
    .default("@today");
const TIMEZONE: Field = Field::text("timezone", "タイムゾーン")
    .required()
    .default("Asia/Tokyo")
    .help("日本は Asia/Tokyo。現地の日付・時刻をこのタイムゾーンで扱います。");
const CONFIRM: Field = Field::new("confirmed", "対象・内容を確認しました", InputKind::Bool);
const WEEKDAYS: &[Choice] = &[
    Choice::new("0", "月曜日"),
    Choice::new("1", "火曜日"),
    Choice::new("2", "水曜日"),
    Choice::new("3", "木曜日"),
    Choice::new("4", "金曜日"),
    Choice::new("5", "土曜日"),
    Choice::new("6", "日曜日"),
];
const WINDOWS: &[Field] = &[
    Field::text("startTime", "開始時刻")
        .required()
        .default("09:00")
        .help("現地時間。24時間表記の HH:MM または HH:MM:SS。"),
    Field::text("endTime", "終了時刻")
        .required()
        .default("17:00"),
    Field::new("capacity", "同時に予約できる組数", InputKind::Integer)
        .required()
        .default("1")
        .help("1組の人数ではなく、同じ開始時刻に受け付ける予約の組数です。"),
    Field::new(
        "slotIntervalMinutes",
        "開始枠の間隔（分）",
        InputKind::Integer,
    )
    .required()
    .default("30")
    .help("プランの利用時間とは別です。1〜1440分。"),
];
const RULE_FIELDS: &[Field] = &[
    Field::new("id", "", InputKind::Hidden),
    Field::new("dayOfWeek", "曜日", InputKind::Select(WEEKDAYS))
        .required()
        .default("0"),
    TIMEZONE,
    WINDOWS[0],
    WINDOWS[1],
    WINDOWS[2],
    WINDOWS[3],
    Field::new("effectiveFrom", "適用開始日（任意）", InputKind::Date).empty(Empty::Null),
    Field::new("effectiveTo", "適用終了日（任意）", InputKind::Date).empty(Empty::Null),
    Field::text("seasonStartMonthDay", "毎年の営業期間・開始（月-日）")
        .empty(Empty::Null)
        .help("例 04-01。終了と一緒に指定します。年をまたぐ期間も指定できます。"),
    Field::text("seasonEndMonthDay", "毎年の営業期間・終了（月-日）").empty(Empty::Null),
    Field::new("solarPolicyLabel", "日出・日没の制限", InputKind::ReadOnly),
];
const SCHEDULE_EDIT: &[Field] = &[
    Field::new("resourceName", "場所・設備", InputKind::ReadOnly),
    Field::new("rollingWindowDays", "自動生成する日数（既存設定）", InputKind::ReadOnly),
    Field::new("rules", "曜日ごとの営業時間", InputKind::Lines(RULE_FIELDS))
        .help("削除した行の営業時間は無効になります。保存後は生成プレビューで枠への影響を確認してください。"),
    CONFIRM,
];
const OVERRIDE_VALUES: &[Field] = &[
    TIMEZONE,
    Field::new(
        "kind",
        "この日の営業",
        InputKind::Select(&[
            Choice::new("closed", "休業"),
            Choice::new("replace", "臨時の営業時間"),
        ]),
    )
    .required()
    .default("closed"),
    Field::new("windows", "臨時の営業時間", InputKind::Lines(WINDOWS))
        .help("休業にする場合は時間帯を空にします。臨時営業では1件以上追加してください。"),
    CONFIRM,
];
const OVERRIDE_CREATE: &[Field] = &[
    RESOURCE_FIELD,
    Field::new("localDate", "対象日", InputKind::Date)
        .required()
        .default("@today"),
    OVERRIDE_VALUES[0],
    OVERRIDE_VALUES[1],
    OVERRIDE_VALUES[2],
    OVERRIDE_VALUES[3],
];
const GENERATE_FIELDS: &[Field] = &[
    Field::new("resourceName", "場所・設備", InputKind::ReadOnly),
    Field::new("from", "開始日", InputKind::ReadOnly),
    Field::new("to", "終了日", InputKind::ReadOnly),
    Field::new("created", "新しく作る枠", InputKind::ReadOnly),
    Field::new("updated", "変更する枠", InputKind::ReadOnly),
    Field::new("deactivated", "無効にする枠", InputKind::ReadOnly),
    Field::new("unchanged", "変更しない枠", InputKind::ReadOnly),
    CONFIRM,
];
const OVERRIDE_ACTIONS: &[Mutation] = &[
    Mutation::new("特例を解除して通常の営業へ戻す", "DELETE", "/v1/erp/reservation-resources/{resourceId}/schedule/overrides/{localDate}", &[CONFIRM])
        .prepare(prepare_override_delete).execute(delete_override).destructive()
        .explain("この日の休業・臨時営業設定を解除します。通常の曜日ルールに戻ります。予約枠へ反映するには生成プレビューを確認して再生成してください。"),
];
const GENERATION_ACTIONS: &[Mutation] = &[
    Mutation::new("確認した内容で予約枠を生成", "POST", "/v1/erp/reservation-resources/{resourceId}/time-slots/generate", GENERATE_FIELDS)
        .prepare(prepare_generation).execute(generate_slots).success_message(generation_message)
        .explain("表示された場所・期間の予約枠を作成・更新します。予約済み・仮押さえ済みの枠に矛盾する変更はサーバーが拒否します。営業時間・休業日・プレビュー件数が変わっていた場合は、もう一度プレビューしてください。"),
];

pub const RESOURCES: &[Resource] = &[
    Resource::new("reservation-slots", GROUP, "空き枠・予約枠", "/v1/erp/reservation-resources/{resourceId}/time-slots", "items")
        .route("reservations/resources").load(load_slots)
        .filters(&[RESOURCE_FIELD, FROM, TO, TIMEZONE,
            Field::new("availableOnly", "空きがある枠だけ", InputKind::Bool),
            Field::new("includeInactive", "無効な枠も含める", InputKind::Bool)])
        .title(&["startsAtLocal"]).subtitle(&["resourceName", "availableQuantity"]).status("availabilityLabel")
        .fields(&[("場所・設備", "resourceName"), ("開始", "startsAtLocal"), ("終了", "endsAtLocal"),
            ("タイムゾーン", "timezone"), ("受付組数", "capacity"), ("予約済みの組数", "reservedQuantity"),
            ("仮押さえの組数", "heldQuantity"), ("空き組数", "availableQuantity"), ("受付中", "active"),
            ("生成元の営業日", "sourceScheduleDate"), ("更新日時", "updatedAt")]),
    Resource::new("reservation-schedules", GROUP, "営業時間・予約枠の設定", "/v1/erp/reservation-resources/{resourceId}/schedule", "items")
        .route("reservations/resources").id("resourceId").load(load_schedule).filters(&[RESOURCE_FIELD])
        .title(&["resourceName"]).subtitle(&["ruleCount"])
        .fields(&[("場所・設備", "resourceName"), ("営業時間の件数", "ruleCount"), ("自動生成する日数", "rollingWindowDays")])
        .edit(Mutation::new("営業時間を保存", "PUT", "/v1/erp/reservation-resources/{resourceId}/schedule", SCHEDULE_EDIT)
            .prepare(prepare_schedule).execute(save_schedule)
            .explain("営業時間の保存だけでは公開予約枠は変わりません。保存後に「予約枠の生成プレビュー」から対象期間を確認し、生成してください。日出・日没の制限と自動生成設定は保持します。")),
    Resource::new("reservation-schedule-overrides", GROUP, "休業日・臨時営業", "/v1/erp/reservation-resources/{resourceId}/schedule/overrides", "items")
        .route("reservations/resources").id("resourceId|localDate").load(load_overrides).filters(&[RESOURCE_FIELD, FROM, TO])
        .title(&["localDate"]).subtitle(&["resourceName", "kindLabel"])
        .fields(&[("場所・設備", "resourceName"), ("対象日", "localDate"), ("営業区分", "kindLabel"), ("タイムゾーン", "timezone"), ("版", "revision")])
        .create(Mutation::new("休業・臨時営業を登録", "PUT", "/v1/erp/reservation-resources/{resourceId}/schedule/overrides/{localDate}", OVERRIDE_CREATE)
            .prepare(prepare_override_create).execute(save_override))
        .edit(Mutation::new("この日の営業を編集", "PUT", "/v1/erp/reservation-resources/{resourceId}/schedule/overrides/{localDate}", OVERRIDE_VALUES)
            .prepare(prepare_override_edit).execute(save_override))
        .actions(OVERRIDE_ACTIONS),
    Resource::new("reservation-slot-generation", GROUP, "予約枠の生成プレビュー", "/v1/erp/reservation-resources/{resourceId}/time-slots/generate", "items")
        .route("reservations/resources").id("resourceId").load(load_generation).filters(&[RESOURCE_FIELD, FROM, TO])
        .title(&["resourceName"]).subtitle(&["from", "to"])
        .fields(&[("場所・設備", "resourceName"), ("開始日", "from"), ("終了日", "to"),
            ("新しく作る枠", "created"), ("変更する枠", "updated"), ("無効にする枠", "deactivated"), ("変更しない枠", "unchanged")])
        .actions(GENERATION_ACTIONS),
];

fn required<'a>(value: &'a Value, key: &str, label: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{label}を指定してください"))
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    value[key]
        .as_array()
        .ok_or_else(|| "予約設定の応答形式を確認できません".into())
}
fn date(value: &Value, key: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(required(value, key, "日付")?, "%Y-%m-%d")
        .map_err(|_| "日付は YYYY-MM-DD で入力してください".into())
}
fn date_range(value: &Value) -> Result<(NaiveDate, NaiveDate), String> {
    let from = date(value, "from")?;
    let to = date(value, "to")?;
    if !(0..366).contains(&to.signed_duration_since(from).num_days()) {
        return Err("対象期間は開始日から366日以内で指定してください".into());
    }
    Ok((from, to))
}
fn timezone(value: &Value) -> Result<Tz, String> {
    required(value, "timezone", "タイムゾーン")?
        .parse::<Tz>()
        .map_err(|_| "タイムゾーンを確認してください（例 Asia/Tokyo）".into())
}
fn local_datetime(zone: Tz, local: chrono::NaiveDateTime) -> Result<DateTime<Utc>, String> {
    match zone.from_local_datetime(&local) {
        LocalResult::Single(value) => Ok(value.with_timezone(&Utc)),
        LocalResult::Ambiguous(_, _) => {
            Err("夏時間の切替で時刻が重複しています。対象日時を確認してください".into())
        }
        LocalResult::None => {
            Err("夏時間の切替などで存在しない現地日時です。対象日時を確認してください".into())
        }
    }
}
pub(crate) fn utc_date_range(from: &str, to: &str, zone: &str) -> Result<(String, String), String> {
    let input = json!({"from":from,"to":to,"timezone":zone});
    let (from, to) = date_range(&input)?;
    let zone = timezone(&input)?;
    let next = to.succ_opt().ok_or("終了日を確認してください")?;
    Ok((
        local_datetime(zone, from.and_hms_opt(0, 0, 0).unwrap())?.to_rfc3339(),
        local_datetime(zone, next.and_hms_opt(0, 0, 0).unwrap())?.to_rfc3339(),
    ))
}
fn resource_path(id: &str) -> String {
    format!("/v1/erp/reservation-resources/{}", api::encode_query(id))
}
fn fetch_resource(token: &str, tenant: &Tenant, id: &str) -> Result<Value, String> {
    let response = api::fetch_record(token, tenant, &resource_path(id))?;
    if response["id"].as_str() != Some(id) {
        return Err("選択した場所・設備の情報が一致しません".into());
    }
    required(&response, "name", "場所・設備の名称")?;
    Ok(response)
}
fn confirm(body: &mut Value) -> Result<(), String> {
    if body["confirmed"] != true {
        return Err("対象・内容を確認してチェックしてください".into());
    }
    body.as_object_mut()
        .ok_or("入力内容を確認できません")?
        .remove("confirmed");
    Ok(())
}
fn unchanged(original: &Value, fresh: &Value, label: &str) -> Result<(), String> {
    if original != fresh {
        Err(format!(
            "{label}が変更されています。読み直してから操作してください（409）"
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn query_slots(
    token: &str,
    tenant: &Tenant,
    resource_id: &str,
    from: &str,
    to: &str,
    zone: &str,
    include_inactive: bool,
) -> Result<Value, String> {
    let (start, end) = utc_date_range(from, to, zone)?;
    let resource = fetch_resource(token, tenant, resource_id)?;
    let path = format!(
        "{}/time-slots?from={}&to={}&includeInactive={include_inactive}",
        resource_path(resource_id),
        api::encode_query(&start),
        api::encode_query(&end)
    );
    let response = api::fetch_record(token, tenant, &path)?;
    if response["resourceId"].as_str() != Some(resource_id) {
        return Err("選択した場所の予約枠を確認できません".into());
    }
    let mut rows = array(&response, "slots")?.clone();
    let zone: Tz = zone.parse().map_err(|_| "タイムゾーンを確認してください")?;
    let mut ids = HashSet::new();
    for row in &mut rows {
        if row["resourceId"].as_str() != Some(resource_id) {
            return Err("別の場所の予約枠が含まれています".into());
        }
        if !ids.insert(required(row, "id", "予約枠")?.to_owned()) || !row["active"].is_boolean()
        {
            return Err("予約枠の応答形式を確認できません".into());
        }
        for key in [
            "capacity",
            "reservedQuantity",
            "heldQuantity",
            "availableQuantity",
        ] {
            if row[key].as_i64().is_none_or(|n| n < 0) {
                return Err("予約枠の組数を確認できません。再読み込みしてください".into());
            }
        }
        for key in ["startsAt", "endsAt"] {
            let time = DateTime::parse_from_rfc3339(required(row, key, "予約枠の日時")?)
                .map_err(|_| "予約枠の日時を読み取れません")?;
            row[format!("{key}Local")] = json!(
                time.with_timezone(&zone)
                    .format("%Y-%m-%d %H:%M %Z")
                    .to_string()
            );
        }
        row["resourceName"] = resource["name"].clone();
        row["timezone"] = json!(zone.name());
        row["availabilityLabel"] = json!(if row["active"] == false {
            "停止中"
        } else if row["availableQuantity"].as_i64() == Some(0) {
            "満枠"
        } else {
            "空きあり"
        });
    }
    Ok(json!({"items":rows}))
}
fn load_slots(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let mut result = query_slots(
        token,
        tenant,
        required(filters, "resourceId", "場所・設備")?,
        required(filters, "from", "開始日")?,
        required(filters, "to", "終了日")?,
        required(filters, "timezone", "タイムゾーン")?,
        filters["includeInactive"] == true,
    )?;
    if filters["availableOnly"] == true {
        result["items"].as_array_mut().unwrap().retain(|row| {
            row["active"] == true && row["availableQuantity"].as_i64().is_some_and(|n| n > 0)
        });
    }
    Ok(result)
}
fn get_schedule(token: &str, tenant: &Tenant, id: &str) -> Result<Value, String> {
    let response = api::fetch_record(token, tenant, &format!("{}/schedule", resource_path(id)))?;
    if response["resourceId"].as_str() != Some(id) {
        return Err("営業時間の対象が一致しません".into());
    }
    array(&response, "rules")?;
    Ok(response)
}
fn load_schedule(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let id = required(filters, "resourceId", "場所・設備")?;
    let resource = fetch_resource(token, tenant, id)?;
    let source = get_schedule(token, tenant, id)?;
    let mut view = source.clone();
    view["resourceName"] = resource["name"].clone();
    view["ruleCount"] = json!(array(&source, "rules")?.len());
    for rule in view["rules"].as_array_mut().unwrap() {
        rule["solarPolicyLabel"] = json!(if rule["solarWindow"].is_null() {
            "設定なし"
        } else {
            "既存設定を保持"
        });
    }
    view["_schedule"] = source;
    Ok(json!({"items":[view]}))
}
fn normalize_window(window: &mut Value) -> Result<(), String> {
    let mut times = Vec::new();
    for key in ["startTime", "endTime"] {
        let raw = required(window, key, "営業時間")?.trim();
        let parsed = NaiveTime::parse_from_str(raw, "%H:%M:%S")
            .or_else(|_| NaiveTime::parse_from_str(raw, "%H:%M"))
            .map_err(|_| "営業時間は HH:MM または HH:MM:SS で入力してください")?;
        window[key] = json!(parsed.format("%H:%M:%S").to_string());
        times.push(parsed);
    }
    if times[0] >= times[1] {
        return Err(
            "終了時刻は開始時刻より後にしてください。日をまたぐ営業時間は分けて登録してください"
                .into(),
        );
    }
    if window["capacity"]
        .as_i64()
        .is_none_or(|n| !(1..=i32::MAX.into()).contains(&n))
    {
        return Err("受付組数は1以上の整数で入力してください".into());
    }
    if window["slotIntervalMinutes"]
        .as_i64()
        .is_none_or(|n| !(1..=1440).contains(&n))
    {
        return Err("開始枠の間隔は1〜1440分で入力してください".into());
    }
    Ok(())
}
fn month_day(value: &Value) -> Result<Option<(u32, u32)>, String> {
    if value.is_null() || value.as_str() == Some("") {
        return Ok(None);
    }
    let raw = value.as_str().ok_or("営業期間を確認してください")?;
    let date = NaiveDate::parse_from_str(&format!("2000-{raw}"), "%Y-%m-%d")
        .map_err(|_| "毎年の営業期間は MM-DD で入力してください")?;
    if raw.len() != 5 {
        return Err("毎年の営業期間は MM-DD で入力してください".into());
    }
    use chrono::Datelike;
    Ok(Some((date.month(), date.day())))
}
fn prepare_schedule(body: &mut Value, original: &Value) -> Result<(), String> {
    confirm(body)?;
    let old = array(original, "rules")?;
    let mut seen = HashSet::new();
    for rule in body["rules"]
        .as_array_mut()
        .ok_or("営業時間を確認してください")?
    {
        if let Some(id) = rule["id"].as_str() {
            if !seen.insert(id.to_owned()) || !old.iter().any(|row| row["id"].as_str() == Some(id))
            {
                return Err("営業時間の識別情報が一致しません".into());
            }
        }
        timezone(rule)?;
        let day = model::input_text(&rule["dayOfWeek"])
            .parse::<i8>()
            .ok()
            .filter(|n| (0..=6).contains(n))
            .ok_or("曜日を選択してください")?;
        rule["dayOfWeek"] = json!(day);
        normalize_window(rule)?;
        let from = if rule["effectiveFrom"].is_null() {
            None
        } else {
            Some(date(rule, "effectiveFrom")?)
        };
        let to = if rule["effectiveTo"].is_null() {
            None
        } else {
            Some(date(rule, "effectiveTo")?)
        };
        if matches!((from,to),(Some(from),Some(to)) if from>to) {
            return Err("適用終了日は開始日以降にしてください".into());
        }
        if month_day(&rule["seasonStartMonthDay"])?.is_some()
            != month_day(&rule["seasonEndMonthDay"])?.is_some()
        {
            return Err("毎年の営業期間は開始と終了を両方指定してください".into());
        }
        // solarWindow is tri-state. Its omission preserves the existing policy.
        rule.as_object_mut().unwrap().remove("solarWindow");
    }
    *body = json!({"rules":body["rules"]});
    Ok(())
}
fn save_schedule(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let id = required(original, "resourceId", "場所・設備")?;
    let current = get_schedule(token, tenant, id)?;
    unchanged(&original["_schedule"], &current, "営業時間")?;
    let response = api::mutate_record(
        token,
        tenant,
        "PUT",
        &format!("{}/schedule", resource_path(id)),
        body,
        None,
    )?;
    if response["resourceId"].as_str() != Some(id) {
        return Err("保存した営業時間の対象を確認できません".into());
    }
    array(&response, "rules")?;
    Ok(response)
}
fn get_overrides(
    token: &str,
    tenant: &Tenant,
    id: &str,
    from: &str,
    to: &str,
) -> Result<Value, String> {
    date_range(&json!({"from":from,"to":to}))?;
    let response = api::fetch_record(
        token,
        tenant,
        &format!(
            "{}/schedule/overrides?from={}&to={}",
            resource_path(id),
            api::encode_query(from),
            api::encode_query(to)
        ),
    )?;
    if response["resourceId"].as_str() != Some(id) {
        return Err("休業・臨時営業の対象を確認できません".into());
    }
    array(&response, "overrides")?;
    Ok(response)
}
fn load_overrides(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let id = required(filters, "resourceId", "場所・設備")?;
    let resource = fetch_resource(token, tenant, id)?;
    let source = get_overrides(
        token,
        tenant,
        id,
        required(filters, "from", "開始日")?,
        required(filters, "to", "終了日")?,
    )?;
    let mut values = array(&source, "overrides")?.clone();
    for row in &mut values {
        let raw = row.clone();
        row["_override"] = raw;
        row["resourceName"] = resource["name"].clone();
        row["resourceId"] = json!(id);
        row["kindLabel"] = json!(if row["kind"] == "closed" {
            "休業"
        } else {
            "臨時営業"
        });
    }
    Ok(json!({"items":values}))
}
fn prepare_override(body: &mut Value) -> Result<(), String> {
    confirm(body)?;
    timezone(body)?;
    let windows = body["windows"]
        .as_array_mut()
        .ok_or("臨時の営業時間を確認してください")?;
    for window in windows.iter_mut() {
        normalize_window(window)?;
    }
    for (index, window) in windows.iter().enumerate() {
        for other in &windows[index + 1..] {
            if window["startTime"].as_str() < other["endTime"].as_str()
                && other["startTime"].as_str() < window["endTime"].as_str()
            {
                return Err("臨時の営業時間が重複しています".into());
            }
        }
    }
    match body["kind"].as_str() {
        Some("closed") => {
            if !body["windows"].as_array().unwrap().is_empty() {
                return Err("休業日は時間帯を空にしてください".into());
            }
            body.as_object_mut().unwrap().remove("windows");
        }
        Some("replace") if !body["windows"].as_array().unwrap().is_empty() => {}
        _ => return Err("休業または1件以上の臨時営業時間を設定してください".into()),
    }
    Ok(())
}
fn prepare_override_create(body: &mut Value, _: &Value) -> Result<(), String> {
    date(body, "localDate")?;
    required(body, "resourceId", "場所・設備")?;
    prepare_override(body)
}
fn prepare_override_edit(body: &mut Value, original: &Value) -> Result<(), String> {
    prepare_override(body)?;
    body["expectedRevision"] = json!(override_revision(original)?);
    Ok(())
}
fn override_revision(original: &Value) -> Result<i64, String> {
    original["revision"]
        .as_i64()
        .filter(|n| *n > 0)
        .ok_or_else(|| "休業・臨時営業の版を確認できません".into())
}
fn prepare_override_delete(body: &mut Value, original: &Value) -> Result<(), String> {
    confirm(body)?;
    body["expectedRevision"] = json!(override_revision(original)?);
    Ok(())
}
fn override_preflight(original: &Value, current: Option<&Value>) -> Result<(), String> {
    if original.is_null() {
        if current.is_some() {
            return Err("この日には営業設定が登録済みです。一覧から編集してください".into());
        }
    } else {
        unchanged(
            &original["_override"],
            current.ok_or("営業設定が削除されています。読み直してください")?,
            "休業・臨時営業",
        )?;
    }
    Ok(())
}
fn save_override(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let scope = if original.is_null() { body } else { original };
    let id = required(scope, "resourceId", "場所・設備")?;
    let day = required(scope, "localDate", "対象日")?;
    let current = get_overrides(token, tenant, id, day, day)?;
    let found = array(&current, "overrides")?
        .iter()
        .find(|row| row["localDate"].as_str() == Some(day));
    override_preflight(original, found)?;
    let mut payload = body.clone();
    payload.as_object_mut().unwrap().remove("resourceId");
    payload.as_object_mut().unwrap().remove("localDate");
    let response = api::mutate_record(
        token,
        tenant,
        "PUT",
        &format!(
            "{}/schedule/overrides/{}",
            resource_path(id),
            api::encode_query(day)
        ),
        &payload,
        None,
    )?;
    if response["resourceId"].as_str() != Some(id)
        || response["localDate"].as_str() != Some(day)
        || response["kind"] != body["kind"]
    {
        return Err("保存した営業設定を確認できません。再読み込みしてください".into());
    }
    override_revision(&response)?;
    Ok(response)
}
fn delete_override(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let id = required(original, "resourceId", "場所・設備")?;
    let day = required(original, "localDate", "対象日")?;
    let current = get_overrides(token, tenant, id, day, day)?;
    override_preflight(
        original,
        array(&current, "overrides")?
            .iter()
            .find(|row| row["localDate"].as_str() == Some(day)),
    )?;
    api::mutate_record(
        token,
        tenant,
        "DELETE",
        &format!(
            "{}/schedule/overrides/{}?expectedRevision={}",
            resource_path(id),
            api::encode_query(day),
            body["expectedRevision"]
        ),
        &json!({}),
        None,
    )
}

fn validate_generation(
    response: &Value,
    id: &str,
    from: &str,
    to: &str,
    dry_run: bool,
) -> Result<(), String> {
    if response["resourceId"].as_str() != Some(id)
        || response["from"].as_str() != Some(from)
        || response["to"].as_str() != Some(to)
        || response["dryRun"].as_bool() != Some(dry_run)
        || ["created", "updated", "deactivated", "unchanged"]
            .iter()
            .any(|key| response[*key].as_u64().is_none())
    {
        return Err("予約枠の生成結果を確認できません。対象期間を再読み込みしてください".into());
    }
    Ok(())
}
fn preview_call(
    token: &str,
    tenant: &Tenant,
    id: &str,
    from: &str,
    to: &str,
) -> Result<Value, String> {
    let result = api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{}/time-slots/generate", resource_path(id)),
        &json!({"from":from,"to":to,"dryRun":true}),
        None,
    )?;
    validate_generation(&result, id, from, to, true)?;
    Ok(result)
}
fn load_generation(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    date_range(filters)?;
    let id = required(filters, "resourceId", "場所・設備")?;
    let from = required(filters, "from", "開始日")?;
    let to = required(filters, "to", "終了日")?;
    let resource = fetch_resource(token, tenant, id)?;
    let schedule = get_schedule(token, tenant, id)?;
    let overrides = get_overrides(token, tenant, id, from, to)?;
    let mut result = preview_call(token, tenant, id, from, to)?;
    // Confirm that the evidence read before the preview still describes it.
    unchanged(&schedule, &get_schedule(token, tenant, id)?, "営業時間")?;
    unchanged(
        &overrides,
        &get_overrides(token, tenant, id, from, to)?,
        "休業・臨時営業",
    )?;
    result["_schedule"] = schedule;
    result["_overrides"] = overrides;
    result["resourceName"] = resource["name"].clone();
    Ok(
        json!({"items":[result],"notice":"この一覧はプレビューです。まだ予約枠は変更していません。表示された行を開き、件数を確認して生成してください。"}),
    )
}
fn prepare_generation(body: &mut Value, original: &Value) -> Result<(), String> {
    confirm(body)?;
    date_range(original)?;
    validate_generation(
        original,
        required(original, "resourceId", "場所・設備")?,
        required(original, "from", "開始日")?,
        required(original, "to", "終了日")?,
        true,
    )?;
    *body = json!({"from":original["from"],"to":original["to"],"dryRun":false});
    Ok(())
}
fn generation_preflight(
    original: &Value,
    schedule: &Value,
    overrides: &Value,
    preview: &Value,
) -> Result<(), String> {
    unchanged(&original["_schedule"], schedule, "営業時間")?;
    unchanged(&original["_overrides"], overrides, "休業・臨時営業")?;
    for key in [
        "resourceId",
        "from",
        "to",
        "created",
        "updated",
        "deactivated",
        "unchanged",
        "dryRun",
    ] {
        if original[key] != preview[key] {
            return Err(
                "生成予定の件数が変わっています。もう一度プレビューを確認してください（409）"
                    .into(),
            );
        }
    }
    Ok(())
}
fn generate_slots(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let id = required(original, "resourceId", "場所・設備")?;
    let from = required(original, "from", "開始日")?;
    let to = required(original, "to", "終了日")?;
    if body != &json!({"from":from,"to":to,"dryRun":false}) {
        return Err("プレビューと生成条件が一致しません".into());
    }
    let schedule = get_schedule(token, tenant, id)?;
    let overrides = get_overrides(token, tenant, id, from, to)?;
    generation_preflight(
        original,
        &schedule,
        &overrides,
        &preview_call(token, tenant, id, from, to)?,
    )?;
    // A rule could change while the second dry run was being calculated.
    unchanged(&schedule, &get_schedule(token, tenant, id)?, "営業時間")?;
    unchanged(
        &overrides,
        &get_overrides(token, tenant, id, from, to)?,
        "休業・臨時営業",
    )?;
    let result = api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{}/time-slots/generate", resource_path(id)),
        body,
        None,
    )?;
    validate_generation(&result, id, from, to, false)?;
    Ok(result)
}
fn generation_message(value: &Value) -> String {
    format!(
        "予約枠を生成しました。新規{}件・更新{}件・無効化{}件・変更なし{}件",
        value["created"], value["updated"], value["deactivated"], value["unchanged"]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_uses_inclusive_local_dates_and_dst_day_lengths() {
        assert_eq!(
            utc_date_range("2026-11-20", "2026-11-21", "Asia/Tokyo").unwrap(),
            (
                "2026-11-19T15:00:00+00:00".into(),
                "2026-11-21T15:00:00+00:00".into()
            )
        );
        for (day, hours) in [("2026-03-08", 23), ("2026-11-01", 25)] {
            let (a, b) = utc_date_range(day, day, "America/New_York").unwrap();
            assert_eq!(
                DateTime::parse_from_rfc3339(&b)
                    .unwrap()
                    .signed_duration_since(DateTime::parse_from_rfc3339(&a).unwrap())
                    .num_hours(),
                hours
            );
        }
        assert!(utc_date_range("2026-11-21", "2026-11-20", "Asia/Tokyo").is_err());
        assert!(utc_date_range("2026-01-01", "2027-01-02", "Asia/Tokyo").is_err());
        assert!(utc_date_range("2026-01-01", "2026-01-01", "Unknown/Zone").is_err());
    }
    #[test]
    fn ambiguous_and_missing_local_times_are_never_guessed() {
        let zone: Tz = "America/New_York".parse().unwrap();
        for local in ["2026-03-08T02:30:00", "2026-11-01T01:30:00"] {
            assert!(
                local_datetime(
                    zone,
                    chrono::NaiveDateTime::parse_from_str(local, "%Y-%m-%dT%H:%M:%S").unwrap()
                )
                .is_err()
            );
        }
    }
    fn schedule() -> Value {
        json!({"resourceId":"resource-one","rules":[{"id":"rule-a","dayOfWeek":0,"timezone":"Asia/Tokyo","startTime":"09:00:00","endTime":"12:00:00","capacity":2,"slotIntervalMinutes":30,"effectiveFrom":"2026-01-01","effectiveTo":null,"seasonStartMonthDay":"11-01","seasonEndMonthDay":"02-28","solarWindow":{"latestStart":{"event":"sunset","offsetMinutes":-30}}}],"rollingWindowDays":60})
    }
    #[test]
    fn schedule_roundtrip_preserves_effective_season_and_opaque_policies() {
        let original = schedule();
        let operation = RESOURCES[1].edit.unwrap();
        let mut draft = model::draft(operation.fields, &original);
        draft["confirmed"] = json!(true);
        draft["rules"][0]["capacity"] = json!("3");
        let body = model::request_body(operation, &draft, &original, "").unwrap();
        assert_eq!(body["rules"][0]["dayOfWeek"], 0);
        assert_eq!(body["rules"][0]["capacity"], 3);
        for key in [
            "id",
            "timezone",
            "effectiveFrom",
            "effectiveTo",
            "seasonStartMonthDay",
            "seasonEndMonthDay",
        ] {
            assert_eq!(body["rules"][0][key], original["rules"][0][key]);
        }
        assert!(body.get("rollingWindowDays").is_none());
        assert!(body["rules"][0].get("solarWindow").is_none());
        draft["rules"][0]["id"] = json!("other-rule");
        assert!(model::request_body(operation, &draft, &original, "").is_err());
    }
    #[test]
    fn schedule_rejects_partial_seasons_backwards_hours_and_bad_counts() {
        let original = schedule();
        let operation = RESOURCES[1].edit.unwrap();
        let base = model::draft(operation.fields, &original);
        for (key, value) in [
            ("seasonEndMonthDay", json!("")),
            ("endTime", json!("08:00")),
            ("capacity", json!("0")),
            ("slotIntervalMinutes", json!("1441")),
            ("timezone", json!("invalid")),
        ] {
            let mut draft = base.clone();
            draft["confirmed"] = json!(true);
            draft["rules"][0][key] = value;
            assert!(
                model::request_body(operation, &draft, &original, "").is_err(),
                "{key}"
            );
        }
    }
    #[test]
    fn override_create_and_update_have_distinct_revision_contracts() {
        let create = RESOURCES[2].create.unwrap();
        let mut draft = model::draft(create.fields, &Value::Null);
        draft["resourceId"] = json!("resource-one");
        draft["localDate"] = json!("2026-11-20");
        draft["confirmed"] = json!(true);
        let body = model::request_body(create, &draft, &Value::Null, "").unwrap();
        assert!(body.get("expectedRevision").is_none());
        assert!(body.get("windows").is_none());
        let original = json!({"resourceId":"resource-one","localDate":"2026-11-20","timezone":"Asia/Tokyo","kind":"closed","revision":8,"windows":[]});
        let edit = RESOURCES[2].edit.unwrap();
        let mut draft = model::draft(edit.fields, &original);
        draft["confirmed"] = json!(true);
        let body = model::request_body(edit, &draft, &original, "").unwrap();
        assert_eq!(body["expectedRevision"], 8);
        assert!(override_preflight(&Value::Null, Some(&original)).is_err());
        let wrapped = json!({"_override":original});
        let mut fresh = wrapped["_override"].clone();
        fresh["revision"] = json!(9);
        assert!(override_preflight(&wrapped, Some(&fresh)).is_err());
    }
    #[test]
    fn replacement_hours_require_windows_and_closed_day_omits_them() {
        assert!(
            prepare_override(
                &mut json!({"kind":"replace","timezone":"Asia/Tokyo","windows":[],"confirmed":true})
            )
            .is_err()
        );
        let mut closed =
            json!({"kind":"closed","timezone":"Asia/Tokyo","windows":[],"confirmed":true});
        prepare_override(&mut closed).unwrap();
        assert_eq!(closed, json!({"kind":"closed","timezone":"Asia/Tokyo"}));
        let window =
            json!({"startTime":"09:00","endTime":"10:00","capacity":1,"slotIntervalMinutes":10});
        assert!(prepare_override(&mut json!({"kind":"replace","timezone":"Asia/Tokyo","windows":[window.clone(),window],"confirmed":true})).is_err());
    }
    #[test]
    fn generation_binds_dates_and_invalidates_changed_evidence_or_plan() {
        let preview = json!({"resourceId":"resource-one","from":"2026-11-20","to":"2026-11-21","created":5,"updated":2,"deactivated":0,"unchanged":1,"dryRun":true});
        let mut original = preview.clone();
        original["_schedule"] = schedule();
        original["_overrides"] = json!({"overrides":[]});
        let operation = RESOURCES[3].actions[0];
        let mut draft = model::draft(operation.fields, &original);
        draft["from"] = json!("2030-01-01");
        draft["confirmed"] = json!(true);
        let body = model::request_body(operation, &draft, &original, "").unwrap();
        assert_eq!(
            body,
            json!({"from":"2026-11-20","to":"2026-11-21","dryRun":false})
        );
        generation_preflight(
            &original,
            &original["_schedule"],
            &original["_overrides"],
            &preview,
        )
        .unwrap();
        let mut changed = original["_schedule"].clone();
        changed["rules"][0]["capacity"] = json!(4);
        assert!(
            generation_preflight(&original, &changed, &original["_overrides"], &preview).is_err()
        );
        assert!(
            generation_preflight(
                &original,
                &original["_schedule"],
                &json!({"overrides":[{"kind":"closed"}]}),
                &preview
            )
            .is_err()
        );
        let mut changed = preview.clone();
        changed["updated"] = json!(3);
        assert!(
            generation_preflight(
                &original,
                &original["_schedule"],
                &original["_overrides"],
                &changed
            )
            .is_err()
        );
        assert!(
            validate_generation(&preview, "resource-one", "2026-11-20", "2026-11-21", false)
                .is_err()
        );
    }
}
