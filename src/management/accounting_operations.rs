//! Business-day calendars, sales provenance and scheduled report delivery.

use super::{model, schema::*};
use crate::api::{self, Tenant};
use serde_json::{Value, json};
use std::collections::HashSet;

const GROUP: &str = "会計・監査";
const SITES: Lookup = Lookup {
    path: "/v1/erp/master-control/organization",
    list_key: "sites",
    value_key: "id",
    label_keys: &["name", "code"],
};
const SUBSCRIPTIONS: Lookup = Lookup {
    path: "/v1/erp/master-control/organization/reports/delivery-subscriptions",
    list_key: "subscriptions",
    value_key: "id",
    label_keys: &["name"],
};
const SOURCE_KINDS: &[Choice] = &[
    Choice::new("work_report_revision", "日報"),
    Choice::new("external_aggregate_snapshot", "外部集計"),
    Choice::new("order_ledger", "注文・売上台帳"),
    Choice::new("pos_transactions", "POS取引明細"),
];
const SUBSCRIPTION_FILTER: &[Field] = &[Field::new(
    "subscriptionId",
    "配信設定",
    InputKind::Reference(SUBSCRIPTIONS),
)
.required()];
const EMAIL_FIELDS: &[Field] = &[
    Field::text("displayName", "宛先の名前").required(),
    Field::new("address", "メールアドレス", InputKind::Email).required(),
    Field::new("confirmed", "宛先と配信内容を確認しました", InputKind::Bool).required(),
];
const WEEKDAYS: &[(&str, &str)] = &[
    ("closedMon", "mon"),
    ("closedTue", "tue"),
    ("closedWed", "wed"),
    ("closedThu", "thu"),
    ("closedFri", "fri"),
    ("closedSat", "sat"),
    ("closedSun", "sun"),
];
const CALENDAR_FIELDS: &[Field] = &[
    Field::text("timezone", "タイムゾーン")
        .required()
        .default("Asia/Tokyo"),
    Field::text("dayCutover", "営業日の切替時刻")
        .required()
        .default("00:00")
        .help("24時間表記のHH:MM"),
    Field::new("closedMon", "月曜日を休業", InputKind::Bool),
    Field::new("closedTue", "火曜日を休業", InputKind::Bool),
    Field::new("closedWed", "水曜日を休業", InputKind::Bool),
    Field::new("closedThu", "木曜日を休業", InputKind::Bool),
    Field::new("closedFri", "金曜日を休業", InputKind::Bool),
    Field::new("closedSat", "土曜日を休業", InputKind::Bool),
    Field::new("closedSun", "日曜日を休業", InputKind::Bool),
    Field::new("effectiveFrom", "適用開始日", InputKind::Date)
        .read("newEffectiveFrom")
        .required()
        .default("@today"),
    Field::new("reason", "変更理由", InputKind::Multiline).required(),
];

pub const RESOURCES: &[Resource] = &[
    Resource::new(
        "business-calendars",
        GROUP,
        "営業日・休業日の設定",
        "/v1/erp/master-control/organization/sites/{siteId}/business-calendar",
        "",
    )
    .route("accounting/operations")
    .id("siteId")
    .title(&["siteName"])
    .filters(&[Field::new("siteId", "対象拠点", InputKind::Reference(SITES)).required()])
    .load(load_calendar)
    .fields(&[
        ("拠点", "siteName"),
        ("タイムゾーン", "timezone"),
        ("営業日の切替時刻", "dayCutover"),
        ("定休日", "weekdayText"),
    ])
    .read_lines(&[
        Field::new(
            "versions",
            "設定の履歴",
            InputKind::Lines(&[
                Field::new("effectiveFrom", "適用開始日", InputKind::Date),
                Field::text("timezone", "タイムゾーン"),
                Field::text("dayCutover", "営業日の切替時刻"),
                Field::text("weekdayText", "定休日"),
            ]),
        ),
        Field::new(
            "schedules",
            "営業・休業予定",
            InputKind::Lines(&[
                Field::new("dateFrom", "開始日", InputKind::Date),
                Field::new("dateTo", "終了日", InputKind::Date),
                Field::new(
                    "status",
                    "予定",
                    InputKind::Select(&[
                        Choice::new("open", "営業"),
                        Choice::new("closed", "休業"),
                    ]),
                ),
                Field::text("note", "理由"),
            ]),
        ),
    ])
    .edit(
        Mutation::new(
            "営業カレンダーを更新",
            "POST",
            "/v1/erp/master-control/organization/sites/{siteId}/business-calendar",
            CALENDAR_FIELDS,
        )
        .prepare(calendar_payload)
        .explain("適用開始日以降の営業日設定を追加します。過去の設定は履歴に残ります。"),
    )
    .actions(&[Mutation::new(
        "臨時の営業・休業を登録",
        "POST",
        "/v1/erp/master-control/organization/sites/{siteId}/business-calendar/schedules",
        &[
            Field::new("dateFrom", "開始日", InputKind::Date)
                .required()
                .default("@today"),
            Field::new("dateTo", "終了日", InputKind::Date)
                .required()
                .default("@today"),
            Field::new(
                "status",
                "予定",
                InputKind::Select(&[Choice::new("closed", "休業"), Choice::new("open", "営業")]),
            )
            .required()
            .default("closed"),
            Field::new("note", "理由", InputKind::Multiline).required(),
        ],
    )
    .prepare(schedule_payload)]),
    Resource::new(
        "sales-source-policies",
        GROUP,
        "売上の集計元設定",
        "/v1/erp/master-control/organization/sales-source-policies",
        "",
    )
    .route("accounting/operations")
    .title(&["sourceLabel"])
    .subtitle(&["siteName", "effectiveFrom"])
    .load(load_policies)
    .fields(&[
        ("対象拠点", "siteName"),
        ("集計範囲", "scope"),
        ("売上の集計元", "sourceLabel"),
        ("適用開始日", "effectiveFrom"),
    ])
    .create(
        Mutation::new(
            "売上の集計元を設定",
            "POST",
            "/v1/erp/master-control/organization/sales-source-policies",
            &[
                Field::new("siteId", "対象拠点", InputKind::Reference(SITES)).required(),
                Field::text("scope", "集計範囲")
                    .required()
                    .default("all")
                    .help("全体の設定はall"),
                Field::new(
                    "sourceKind",
                    "採用する売上",
                    InputKind::Select(SOURCE_KINDS),
                )
                .required(),
                Field::new("effectiveFrom", "適用開始日", InputKind::Date)
                    .required()
                    .default("@today"),
            ],
        )
        .explain(
            "適用開始日以降に採用する売上の集計元を設定します。既存の取引そのものは変更しません。",
        ),
    ),
    Resource::new(
        "report-delivery-subscriptions",
        GROUP,
        "日次レポートの配信",
        "/v1/erp/master-control/organization/reports/delivery-subscriptions",
        "subscriptions",
    )
    .route("accounting/operations")
    .title(&["name"])
    .subtitle(&["sendAt", "timezone"])
    .status("status")
    .fields(&[
        ("配信名", "name"),
        ("状態", "status"),
        ("配信時刻", "sendAt"),
        ("タイムゾーン", "timezone"),
        ("適用開始日", "effectiveFrom"),
    ])
    .read_lines(&[Field::new(
        "recipients",
        "配信先",
        InputKind::Lines(&[
            Field::text("displayName", "名前"),
            Field::text("channel", "配信方法"),
            Field::text("address", "宛先"),
            Field::new("effectiveFrom", "開始日", InputKind::Date),
            Field::new("effectiveTo", "終了日", InputKind::Date),
            Field::new("invalidatedAt", "無効化日時", InputKind::DateTime),
        ]),
    )])
    .create(
        Mutation::new(
            "メール配信を登録",
            "POST",
            "/v1/erp/master-control/organization/reports/delivery-subscriptions",
            &[
                Field::text("name", "配信名").required(),
                Field::new(
                    "sites",
                    "対象拠点",
                    InputKind::Lines(&[
                        Field::new("siteId", "拠点", InputKind::Reference(SITES)).required()
                    ]),
                ),
                Field::text("timezone", "タイムゾーン")
                    .required()
                    .default("Asia/Tokyo"),
                Field::text("sendAt", "配信時刻")
                    .required()
                    .default("09:00")
                    .help("24時間表記のHH:MM"),
                Field::new("effectiveFrom", "開始日", InputKind::Date)
                    .required()
                    .default("@today"),
                Field::new(
                    "recipients",
                    "メール配信先",
                    InputKind::Lines(&[
                        Field::text("displayName", "名前").required(),
                        Field::new("address", "メールアドレス", InputKind::Email).required(),
                    ]),
                ),
                Field::new(
                    "confirmed",
                    "宛先・対象拠点・配信時刻を確認しました",
                    InputKind::Bool,
                )
                .required(),
            ],
        )
        .prepare(subscription_payload)
        .explain(
            "指定した宛先への自動レポート配信を開始します。宛先と対象拠点を確認してください。",
        ),
    )
    .actions(&[
        Mutation::new(
            "配信を停止",
            "POST",
            "/v1/erp/master-control/organization/reports/delivery-subscriptions/{id}/status",
            &[],
        )
        .prepare(pause_subscription)
        .when(|record| record["status"] == "active")
        .destructive(),
        Mutation::new(
            "配信を再開",
            "POST",
            "/v1/erp/master-control/organization/reports/delivery-subscriptions/{id}/status",
            &[Field::new(
                "confirmed",
                "配信先と配信内容を確認しました",
                InputKind::Bool,
            )
            .required()],
        )
        .prepare(resume_subscription)
        .when(|record| record["status"] == "paused")
        .explain("登録済みの有効な配信先への自動配信を再開します。"),
        Mutation::new(
            "メール配信先を追加",
            "POST",
            "/v1/erp/master-control/organization/reports/delivery-subscriptions/{id}/recipients",
            EMAIL_FIELDS,
        )
        .prepare(email_payload)
        .explain("新しい宛先へレポートの自動配信を追加します。"),
    ]),
    Resource::new(
        "report-delivery-recipients",
        GROUP,
        "レポート配信先の終了",
        "/v1/erp/master-control/organization/reports/delivery-subscriptions",
        "items",
    )
    .route("accounting/operations")
    .title(&["displayName", "address"])
    .subtitle(&["subscriptionName", "effectiveTo"])
    .filters(SUBSCRIPTION_FILTER)
    .load(load_recipients)
    .fields(&[
        ("配信設定", "subscriptionName"),
        ("名前", "displayName"),
        ("宛先", "address"),
        ("配信方法", "channel"),
        ("開始日", "effectiveFrom"),
        ("終了日", "effectiveTo"),
        ("無効化日時", "invalidatedAt"),
    ])
    .actions(&[Mutation::new(
        "この宛先への配信を終了",
        "POST",
        "/v1/erp/master-control/organization/reports/delivery-recipients/{id}/end",
        &[Field::new("effectiveTo", "終了日", InputKind::Date)
            .required()
            .default("@today")],
    )
    .prepare(end_recipient)
    .when(|record| record["effectiveTo"].is_null() && record["invalidatedAt"].is_null())
    .destructive()]),
    Resource::new(
        "report-delivery-history",
        GROUP,
        "レポート配信の履歴",
        "/v1/erp/master-control/organization/reports/deliveries",
        "items",
    )
    .route("accounting/operations")
    .title(&["businessDate"])
    .subtitle(&["deliveryLabel"])
    .status("status")
    .filters(SUBSCRIPTION_FILTER)
    .load(load_delivery_history)
    .fields(&[
        ("営業日", "businessDate"),
        ("状態", "status"),
        ("配信方法", "channel"),
        ("訂正版", "isCorrection"),
        ("試行回数", "attemptCount"),
        ("直近のエラー", "lastErrorCode"),
        ("配信しなかった理由", "reason"),
    ])
    .actions(&[Mutation::new(
        "失敗した配信を再試行",
        "POST",
        "/v1/erp/master-control/organization/reports/deliveries/{id}/retry",
        &[Field::new(
            "confirmed",
            "登録済みの宛先への再送を確認しました",
            InputKind::Bool,
        )
        .required()],
    )
    .prepare(retry_payload)
    .when(|record| record["status"] == "failed")
    .explain("失敗したレポート配信を再試行します。登録済みの宛先へ送信されます。")]),
];

fn clock_time(value: &Value) -> Result<(), String> {
    let text = value.as_str().ok_or("時刻を入力してください")?;
    if text.len() != 5 || chrono::NaiveTime::parse_from_str(text, "%H:%M").is_err() {
        return Err("時刻は00:00〜23:59の形式で入力してください".into());
    }
    Ok(())
}
fn weekday_text(value: &Value) -> String {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|day| match day.as_str() {
            Some("mon") => Some("月"),
            Some("tue") => Some("火"),
            Some("wed") => Some("水"),
            Some("thu") => Some("木"),
            Some("fri") => Some("金"),
            Some("sat") => Some("土"),
            Some("sun") => Some("日"),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("・")
}
fn site_name(organization: &Value, id: &Value) -> String {
    if id.is_null() {
        return "全拠点".into();
    }
    organization["sites"]
        .as_array()
        .and_then(|sites| sites.iter().find(|site| site["id"] == *id))
        .and_then(|site| site["name"].as_str())
        .unwrap_or("拠点（名称未取得）")
        .into()
}
fn load_calendar(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let resource = RESOURCES[0];
    let path = model::query_path(resource.path, resource.filters, filters)?;
    let mut result = api::fetch_record(token, tenant, &path)?;
    if result["siteId"] != filters["siteId"] {
        return Err("営業カレンダーの拠点が一致しません".into());
    }
    let organization = api::fetch_record(token, tenant, SITES.path).unwrap_or(Value::Null);
    result["siteName"] = json!(site_name(&organization, &result["siteId"]));
    let versions = result["versions"]
        .as_array_mut()
        .ok_or("営業カレンダーの履歴を確認できません")?;
    for version in versions.iter_mut() {
        version["weekdayText"] = json!(weekday_text(&version["regularClosedWeekdays"]));
    }
    let latest=versions.iter().max_by_key(|version|version["effectiveFrom"].as_str().unwrap_or("")).cloned().unwrap_or_else(||json!({"timezone":result["siteTimezone"],"dayCutover":"00:00","regularClosedWeekdays":[]}));
    for key in [
        "timezone",
        "dayCutover",
        "regularClosedWeekdays",
        "weekdayText",
    ] {
        result[key] = latest[key].clone();
    }
    for (field, day) in WEEKDAYS {
        result[*field] = json!(
            latest["regularClosedWeekdays"]
                .as_array()
                .is_some_and(|days| days.iter().any(|value| value == day))
        );
    }
    Ok(result)
}
fn calendar_payload(body: &mut Value, _: &Value) -> Result<(), String> {
    clock_time(&body["dayCutover"])?;
    let closed = WEEKDAYS
        .iter()
        .filter(|(field, _)| body[*field] == true)
        .map(|(_, day)| *day)
        .collect::<Vec<_>>();
    for (field, _) in WEEKDAYS {
        body.as_object_mut()
            .ok_or("入力を確認できません")?
            .remove(*field);
    }
    body["regularClosedWeekdays"] = json!(closed);
    Ok(())
}
fn schedule_payload(body: &mut Value, _: &Value) -> Result<(), String> {
    if body["dateFrom"]
        .as_str()
        .zip(body["dateTo"].as_str())
        .is_some_and(|(from, to)| from > to)
    {
        return Err("終了日は開始日以降を選んでください".into());
    }
    Ok(())
}
fn load_policies(token: &str, tenant: &Tenant, _: &Value) -> Result<Value, String> {
    let mut policies = api::fetch_record(
        token,
        tenant,
        "/v1/erp/master-control/organization/sales-source-policies",
    )?;
    let organization = api::fetch_record(token, tenant, SITES.path).unwrap_or(Value::Null);
    for row in policies
        .as_array_mut()
        .ok_or("売上の集計元設定を確認できません")?
    {
        row["siteName"] = json!(site_name(&organization, &row["siteId"]));
        row["sourceLabel"] = json!(
            SOURCE_KINDS
                .iter()
                .find(|choice| row["sourceKind"] == choice.value)
                .map_or("その他の集計元", |choice| choice.label)
        );
    }
    Ok(policies)
}
fn confirm(body: &mut Value) -> Result<(), String> {
    if body["confirmed"] != true {
        return Err("宛先と配信内容を確認してください".into());
    }
    body.as_object_mut()
        .ok_or("入力を確認できません")?
        .remove("confirmed");
    Ok(())
}
fn subscription_payload(body: &mut Value, _: &Value) -> Result<(), String> {
    confirm(body)?;
    clock_time(&body["sendAt"])?;
    let sites = body["sites"].as_array().ok_or("対象拠点を選んでください")?;
    if sites.is_empty() {
        return Err("対象拠点を1つ以上選んでください".into());
    }
    let mut seen = HashSet::new();
    let mut site_ids = Vec::new();
    for row in sites {
        let id = row["siteId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("対象拠点を選んでください")?;
        if !seen.insert(id) {
            return Err("同じ拠点が重複しています".into());
        }
        site_ids.push(id.to_owned());
    }
    let recipients = body["recipients"]
        .as_array_mut()
        .ok_or("配信先を登録してください")?;
    if recipients.is_empty() {
        return Err("配信先を1件以上登録してください".into());
    }
    let mut addresses = HashSet::new();
    for recipient in recipients {
        let address = recipient["address"]
            .as_str()
            .ok_or("メールアドレスを入力してください")?
            .trim()
            .to_lowercase();
        if !addresses.insert(address) {
            return Err("同じメールアドレスが重複しています".into());
        }
        recipient["channel"] = json!("email");
    }
    body.as_object_mut()
        .ok_or("入力を確認できません")?
        .remove("sites");
    body["siteIds"] = json!(site_ids);
    body["format"] = json!("link");
    Ok(())
}
fn email_payload(body: &mut Value, _: &Value) -> Result<(), String> {
    confirm(body)?;
    body["channel"] = json!("email");
    Ok(())
}
fn pause_subscription(body: &mut Value, _: &Value) -> Result<(), String> {
    *body = json!({"status":"paused"});
    Ok(())
}
fn resume_subscription(body: &mut Value, _: &Value) -> Result<(), String> {
    confirm(body)?;
    *body = json!({"status":"active"});
    Ok(())
}
fn retry_payload(body: &mut Value, _: &Value) -> Result<(), String> {
    confirm(body)?;
    *body = json!({});
    Ok(())
}
fn load_recipients(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let response = api::fetch_record(token, tenant, SUBSCRIPTIONS.path)?;
    let subscription = response["subscriptions"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["id"] == filters["subscriptionId"])
        })
        .ok_or("選んだ配信設定を確認できません")?;
    let mut rows = subscription["recipients"]
        .as_array()
        .ok_or("配信先を確認できません")?
        .clone();
    for row in &mut rows {
        row["subscriptionName"] = subscription["name"].clone();
    }
    Ok(json!({"items":rows}))
}
fn end_recipient(body: &mut Value, original: &Value) -> Result<(), String> {
    if body["effectiveTo"]
        .as_str()
        .zip(original["effectiveFrom"].as_str())
        .is_some_and(|(end, start)| end < start)
    {
        return Err("終了日は配信開始日以降を選んでください".into());
    }
    Ok(())
}
fn load_delivery_history(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let path = model::query_path(
        "/v1/erp/master-control/organization/reports/deliveries",
        SUBSCRIPTION_FILTER,
        filters,
    )?;
    let response = api::fetch_record(token, tenant, &path)?;
    let mut rows = response["deliveries"]
        .as_array()
        .ok_or("配信履歴を確認できません")?
        .clone();
    for row in &mut rows {
        row["deliveryLabel"] = json!("配信履歴");
    }
    for skip in response["skips"]
        .as_array()
        .ok_or("配信を見送った履歴を確認できません")?
    {
        let mut row = skip.clone();
        row["deliveryLabel"] = json!("配信見送り");
        row["status"] = json!("skipped");
        rows.push(row);
    }
    rows.sort_by(|a, b| b["businessDate"].as_str().cmp(&a["businessDate"].as_str()));
    Ok(json!({"items":rows}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calendar_builds_weekdays_without_leaking_ui_fields() {
        let mut body = json!({"timezone":"Asia/Tokyo","dayCutover":"04:30","effectiveFrom":"2026-11-01","reason":"営業時間変更","closedMon":true,"closedSun":true});
        calendar_payload(&mut body, &Value::Null).unwrap();
        assert_eq!(body["regularClosedWeekdays"], json!(["mon", "sun"]));
        assert!(body.get("closedMon").is_none());
        assert!(calendar_payload(&mut json!({"dayCutover":"25:00"}), &Value::Null).is_err());
    }
    #[test]
    fn delivery_creation_requires_explicit_scope_recipients_and_confirmation() {
        let mut body = json!({"name":"朝のレポート","sendAt":"09:00","sites":[{"siteId":"site_a"}],"recipients":[{"displayName":"担当","address":"staff@example.test"}],"confirmed":true});
        subscription_payload(&mut body, &Value::Null).unwrap();
        assert_eq!(body["siteIds"], json!(["site_a"]));
        assert_eq!(body["recipients"][0]["channel"], "email");
        assert_eq!(body["format"], "link");
        assert!(body.get("sites").is_none());
        assert!(body.get("confirmed").is_none());
        assert!(subscription_payload(&mut json!({"confirmed":false}), &Value::Null).is_err());
        assert!(
            subscription_payload(
                &mut json!({"confirmed":true,"sendAt":"09:00","sites":[],"recipients":[]}),
                &Value::Null
            )
            .is_err()
        );
    }
}
