//! Native reservation desk: calendar dates, scoped slots, plans and operational history.
use super::*;
use crate::management::schema::{Field, InputKind, Lookup};
use crate::management::{
    form::Picker, reservation_handoff as handoff, reservation_slots, reservation_waitlist,
    workforce,
};
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, TimeZone};
use serde_json::json;

const DAY_FIELD: Field = Field::new("day", "予約日（日本時間）", InputKind::Date)
    .required()
    .default("@today");
const RELEASE_HOLDS: Mutation = Mutation::new("期限切れの仮押さえを解放", "POST", "/v1/erp/reservations/payment-holds/release-expired", &[
    Field::new("confirmed", "期限切れの仮押さえと支払リンクを解放する", InputKind::Bool),
]).prepare(release_payload).validate_response(check_release).success_message(release_message)
    .explain("支払期限が切れた仮押さえを最大100件解放し、対象の支払リンクを停止します。実行後に予約と空き枠を読み直してください。").destructive();
fn release_payload(body: &mut Value, _: &Value) -> Result<(), String> {
    if body["confirmed"] != true {
        return Err("対象の操作を確認してチェックしてください".into());
    }
    *body = json!({"limit":100});
    Ok(())
}
fn check_release(value: &Value) -> Result<(), String> {
    if value["releasedCount"].as_u64().is_none()
        || !value["reservationIds"].is_array()
        || !value["paymentIntentIds"].is_array()
    {
        return Err("解放した件数を確認できません。予約と空き枠を読み直してください".into());
    }
    Ok(())
}
fn release_message(value: &Value) -> String {
    format!(
        "期限切れの仮押さえを{}件解放しました",
        value["releasedCount"]
    )
}
fn jst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).unwrap()
}
fn today() -> String {
    crate::management::reservation_workflows::now_jst()
        .format("%Y-%m-%d")
        .to_string()
}
fn local_time(text: &str) -> String {
    DateTime::parse_from_rfc3339(text)
        .map(|d| d.with_timezone(&jst()).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| text.into())
}
fn display_time(value: &Value) -> String {
    value.as_str().map(local_time).unwrap_or_default()
}
fn input_jst(text: &str) -> String {
    DateTime::parse_from_rfc3339(text)
        .map(|time| time.with_timezone(&jst()).to_rfc3339())
        .unwrap_or_else(|_| text.to_owned())
}
pub(super) fn japanese_datetimes(fields: &[Field], draft: &mut Value) {
    for field in fields
        .iter()
        .filter(|field| matches!(field.kind, InputKind::DateTime))
    {
        if let Some(value) = model::value_at(draft, field.key).as_str() {
            let local = input_jst(value);
            model::set_at(draft, field.key, json!(local));
        }
    }
}
fn money(value: &Value, key: &str) -> String {
    model::display_field(
        Resource::new("reservation-value", "", "", "/", ""),
        value,
        &format!("@money:{key}"),
    )
}
fn input_time(value: &Value) -> Option<DateTime<FixedOffset>> {
    let text = value.as_str()?;
    DateTime::parse_from_rfc3339(text).ok().or_else(|| {
        chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M")
            .ok()
            .and_then(|d| jst().from_local_datetime(&d).single())
    })
}
fn day_of(value: &Value) -> String {
    input_time(value)
        .map(|d| d.with_timezone(&jst()).format("%Y-%m-%d").to_string())
        .unwrap_or_else(today)
}
fn day_bounds(day: &str) -> Result<(String, String), String> {
    reservation_slots::utc_date_range(day, day, "Asia/Tokyo")
}
fn selected_day(data: Data, day: &str) -> Result<(), String> {
    let (from, to) = day_bounds(day)?;
    data.filters.update(|filters| {
        filters["from"] = json!(input_jst(&from));
        filters["to"] = json!(input_jst(&to));
    });
    data.load();
    Ok(())
}

#[component]
pub(super) fn reservation_toolbar(data: Data) -> Element {
    let day = signal(json!({"day":today()}));
    let error = signal(String::new());
    let jump = move |offset: i64| {
        let parsed = NaiveDate::parse_from_str(
            day.with_untracked(|v| v["day"].as_str().unwrap_or("").to_owned())
                .as_str(),
            "%Y-%m-%d",
        );
        match parsed
            .ok()
            .and_then(|d| d.checked_add_signed(Duration::days(offset)))
        {
            Some(next) => {
                let text = next.format("%Y-%m-%d").to_string();
                day.set(json!({"day":text}));
                if let Err(e) = selected_day(data, &text) {
                    error.set(e);
                } else {
                    error.set(String::new());
                }
            }
            None => error.set("予約日を年-月-日で入力してください".into()),
        }
    };
    render! {
        View(style: computed(move || group_style())) {
            SectionHeader(title: "予約カレンダー")
            InputFields(fields: vec![DAY_FIELD], draft: day, prefix: String::new(), token: data.connection.token, tenant: data.connection.tenant)
            View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Row, gap: px(8), padding: (px(8),px(16)))) {
                Text(value: "前日", style: computed(move || css!(flex_grow: 1.0, min_height: px(44), padding: px(10), color: hex(TINT), background_color: hex(FILL), border_radius: px(8), font_size: px(16), text_align: TextAlign::Center)), on_tap: move |_| jump(-1))
                Text(value: "この日を表示", style: computed(move || css!(flex_grow: 2.0, min_height: px(44), padding: px(10), color: hex(TINT), background_color: hex(FILL), border_radius: px(8), font_size: px(16), text_align: TextAlign::Center)), on_tap: move |_| jump(0))
                Text(value: "翌日", style: computed(move || css!(flex_grow: 1.0, min_height: px(44), padding: px(10), color: hex(TINT), background_color: hex(FILL), border_radius: px(8), font_size: px(16), text_align: TextAlign::Center)), on_tap: move |_| jump(1))
            }
            ActionButton(label: "全期間を表示", destructive: false, on_tap: move |_| { data.filters.update(|value| { value["from"] = json!(""); value["to"] = json!(""); }); error.set(String::new()); data.load(); })
            Show(when: move || !error.with(String::is_empty)) { ErrorNotice(message: computed(move || error.get())) }
            Show(when: move || data.resource.create.is_some_and(|op| data.can_mutate(op, &Value::Null))) {
                ActionButton(label: "空き枠から予約を作成", destructive: false, on_tap: move |_| {
                    if let Some(op) = data.resource.create {
                        let day = day.with_untracked(|v| v["day"].as_str().unwrap_or("").to_owned());
                        if let Ok((from, _)) = day_bounds(&day) { data.begin_prefilled(op, json!({"startsAt":local_time(&from)})); }
                        else { error.set("予約日を確認してください".into()); }
                    }
                })
            }
            ActionButton(label: "キャンセル待ち", destructive: false, on_tap: move |_| data.open_related("reservation-waitlist", Value::Null))
            ReservationSettingsLinks(data: data)
            Show(when: move || data.can_mutate(RELEASE_HOLDS, &Value::Null)) { ActionButton(label: "期限切れの仮押さえを解放", destructive: true, on_tap: move |_| data.begin(RELEASE_HOLDS, Value::Null)) }
            ReservationCsv(data: data)
        }
    }
}

#[component]
fn reservation_csv(data: Data) -> Element {
    let csv = signal(String::new());
    let filename = signal(String::new());
    let loading = signal(false);
    let message = signal(String::new());
    let owner = Owner::current();
    let load = move || {
        if loading.get_untracked() || !data.connection.allowed(data.resource) {
            return;
        }
        let Some(tenant) = data.connection.tenant.get_untracked() else {
            return;
        };
        let context = tenant.clone();
        loading.set(true);
        message.set(String::new());
        csv.set(String::new());
        spawn_in(owner, async move {
            let result = authed(data.connection.token, move |token| {
                api::export_reservations_csv(token, &tenant, None, None)
            })
            .await;
            if !owner.is_none_or(Owner::is_alive)
                || data.connection.tenant.get_untracked().as_ref() != Some(&context)
            {
                return;
            }
            loading.set(false);
            match result {
                Ok(value) => {
                    filename.set(value.filename);
                    csv.set(value.text);
                }
                Err(error) => message.set(error),
            }
        });
    };
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Show(when: move || field_glass::CSV_EXPORT_SUPPORTED && !loading.get()) { ActionButton(label: "予約CSV（全期間）を準備", destructive: false, on_tap: move |_| load()) }
            Show(when: move || loading.get()) { Notice(message: "予約CSVを準備中…") }
            Show(when: move || !csv.with(String::is_empty)) {
                Notice(message: "全期間の予約を出力します。一覧の検索・状態の絞り込みは適用されません。")
                field_glass::CsvExportButton(label: "予約CSVを保存・共有".to_owned(), csv: computed(move || csv.get()), name: computed(move || filename.get()), on_done: move |result: field_glass::FileExportDetail| {
                    if result.ok { message.set("予約CSVを書き出しました".into()); csv.set(String::new()); }
                    else if !result.error.is_empty() { message.set(result.error); }
                }, style: css!(height: px(48), margin: px(16)))
            }
            Show(when: move || !message.with(String::is_empty)) { Text(value: computed(move || message.get()), style: computed(move || css!(color: hex(SECONDARY_LABEL), padding: px(16), font_size: px(15)))) }
        }
    }
}

fn booking_operation(data: Data, op: Mutation) -> bool {
    (data.resource.key == "reservations"
        && (op.path == "/v1/erp/reservations" || (op.method == "PATCH" && !op.fields.is_empty())))
        || (data.resource.key == "reservation-waitlist"
            && (op.path.ends_with("/convert") || op.path == "/v1/erp/reservation-waitlist"))
}
#[component]
pub(super) fn reservation_form(
    data: Data,
    operation: Mutation,
    draft: RwSignal<Value>,
    original: RwSignal<Value>,
) -> Element {
    if booking_operation(data, operation) {
        let main_fields = operation
            .fields
            .iter()
            .filter(|f| {
                !matches!(
                    f.key,
                    "reservationTypeId"
                        | "resourceId"
                        | "serviceId"
                        | "customerId"
                        | "startsAt"
                        | "endsAt"
                        | "desiredStartsAt"
                        | "desiredEndsAt"
                )
            })
            .copied()
            .collect::<Vec<_>>();
        let has_customer = operation
            .fields
            .iter()
            .any(|field| field.key == "customerId");
        return render! {
            View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
                BookingSlotPicker(data: data, operation: operation, draft: draft, original: original)
                Show(when: move || has_customer) { BookingCustomerPicker(data: data, draft: draft) }
                InputFields(fields: main_fields, draft: draft, prefix: String::new(), token: data.connection.token, tenant: data.connection.tenant)
            }
        };
    }
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            InputFields(fields: operation.fields.to_vec(), draft: draft, prefix: String::new(), token: data.connection.token, tenant: data.connection.tenant)
            Show(when: move || data.resource.key == "reservations" && operation.path.ends_with("/cancel")) {
                CancellationPreview(original: original, draft: draft)
            }
            Show(when: move || data.resource.key == "reservations" && operation.path.ends_with("/notifications")) {
                NotificationPreview(original: original, draft: draft)
            }
        }
    }
}

#[component]
pub(super) fn reservation_settings_links(data: Data) -> Element {
    let open = signal(false);
    let choices = computed(move || {
        resources()
            .into_iter()
            .filter(|r| {
                r.key != data.resource.key
                    && data.connection.allowed(*r)
                    && matches!(
                        r.key,
                        "reservation-slots"
                            | "reservation-schedules"
                            | "reservation-schedule-overrides"
                            | "reservation-slot-generation"
                            | "reservation-types"
                            | "reservation-resources"
                            | "reservation-plans"
                            | "reservation-daily-budgets"
                            | "reservation-staff-notifications"
                            | "reservation-notification-templates"
                    )
            })
            .collect::<Vec<_>>()
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            ActionButton(label: "予約枠・プラン・通知の設定", destructive: false, on_tap: move |_| open.update(|v| *v = !*v))
            Show(when: move || open.get()) {
                ForEach(each: move || choices.get(), key: |r:&Resource| r.key, children: move |r:ReadSignal<Resource>| {
                    let r = r.get_untracked(); render! { ActionButton(label: r.label, destructive: false, on_tap: move |_| data.open_related(r.key,Value::Null)) }
                })
            }
        }
    }
}

fn options(rows: &[Value]) -> Vec<(String, String)> {
    rows.iter()
        .filter_map(|row| {
            Some((
                row["id"].as_str()?.to_owned(),
                row["name"].as_str()?.to_owned(),
            ))
        })
        .collect()
}
fn matching_resources(types: &[Value], resources: &[Value], type_id: &str) -> Vec<Value> {
    let Some(kind) = types.iter().find(|t| t["id"] == type_id) else {
        return vec![];
    };
    resources
        .iter()
        .filter(|r| {
            r["active"] != false
                && r["resourceModel"] == kind["resourceModel"]
                && kind["resourceModel"].is_string()
        })
        .cloned()
        .collect()
}
fn matching_plans(plans: &[Value], type_id: &str) -> Vec<Value> {
    plans
        .iter()
        .filter(|plan| {
            plan["enabled"] != false
                && plan["available"] != false
                && (plan["reservationTypeId"].as_str().is_none_or(str::is_empty)
                    || plan["reservationTypeId"] == type_id)
        })
        .cloned()
        .collect()
}
fn slot_candidates(slots: &[Value], minutes: Option<i64>) -> Vec<Value> {
    let mut sorted = slots.to_vec();
    sorted.sort_by_key(|row| {
        input_time(&row["startsAt"])
            .map(|time| time.timestamp())
            .unwrap_or(i64::MAX)
    });
    sorted
        .iter()
        .map(|slot| {
            let mut candidate = slot.clone();
            let start = input_time(&slot["startsAt"]);
            let end = input_time(&slot["endsAt"]);
            let wanted = start
                .zip(minutes.filter(|n| *n > 0))
                .and_then(|(start, minutes)| start.checked_add_signed(Duration::minutes(minutes)))
                .or(end);
            // One group at the start time is the server's inventory unit.
            // Service duration is deliberately not part of that lookup.
            let available = start.zip(wanted).is_some_and(|(start, end)| end > start)
                && slot["active"] == true
                && slot["availableQuantity"].as_i64().is_some_and(|n| n > 0);
            candidate["_bookable"] = json!(available);
            if let Some(end) = wanted {
                candidate["_bookingEndsAt"] = json!(end.to_rfc3339());
            }
            candidate
        })
        .collect()
}
fn select_slot(
    draft: &mut Value,
    slot: &Value,
    resource_id: &str,
    waitlist_create: bool,
) -> Result<(), String> {
    if slot["_bookable"] != true || slot["resourceId"] != resource_id {
        return Err("この枠は選択できません。空き状況を読み直してください".into());
    }
    let (start_key, end_key) = if waitlist_create {
        ("desiredStartsAt", "desiredEndsAt")
    } else {
        ("startsAt", "endsAt")
    };
    draft[start_key] = json!(input_jst(
        slot["startsAt"]
            .as_str()
            .ok_or("開始日時を確認できません")?
    ));
    draft[end_key] = json!(input_jst(
        slot["_bookingEndsAt"]
            .as_str()
            .ok_or("終了日時を確認できません")?
    ));
    draft["resourceId"] = json!(resource_id);
    Ok(())
}
fn apply_plan(
    draft: &mut Value,
    plan: &Value,
    price_editable: bool,
    kind: Option<&Value>,
) -> Result<(), String> {
    if price_editable {
        let defaults = crate::management::reservation_workflows::plan_form_defaults(plan, kind)?;
        for key in ["priceAmount", "prepaymentPolicy", "depositAmount"] {
            draft[key] = defaults[key].clone();
        }
    }
    draft["serviceId"] = plan["id"].clone();
    if let (Some(start), Some(minutes)) = (
        input_time(&draft["startsAt"]),
        plan["durationMinutes"].as_i64().filter(|n| *n > 0),
    ) {
        if let Some(end) = start.checked_add_signed(Duration::minutes(minutes)) {
            draft["endsAt"] = json!(end.with_timezone(&jst()).to_rfc3339());
        }
    }
    if let Some(eligible) = plan["eligibleResourceIds"].as_array() {
        if !eligible.contains(&draft["resourceId"]) {
            draft["resourceId"] = json!("");
        }
    }
    Ok(())
}

#[component]
fn booking_slot_picker(
    data: Data,
    operation: Mutation,
    draft: RwSignal<Value>,
    original: RwSignal<Value>,
) -> Element {
    let types = signal(Vec::<Value>::new());
    let locations = signal(Vec::<Value>::new());
    let plans = signal(Vec::<Value>::new());
    let type_options = signal(Vec::<(String, String)>::new());
    let resource_options = signal(Vec::<(String, String)>::new());
    let plan_options = signal(Vec::<(String, String)>::new());
    let loading = signal(false);
    let error = signal(String::new());
    let plan_warning = signal(String::new());
    let slots = signal(Vec::<Value>::new());
    let slots_loading = signal(false);
    let slots_ready = signal(false);
    let slots_error = signal(String::new());
    let generation = signal(0u64);
    let waitlist_create = operation.path == "/v1/erp/reservation-waitlist";
    let has_type = operation
        .fields
        .iter()
        .any(|field| field.key == "reservationTypeId");
    let has_plan = operation
        .fields
        .iter()
        .any(|field| field.key == "serviceId");
    let price_editable = operation
        .fields
        .iter()
        .any(|field| field.key == "priceAmount" && !matches!(field.kind, InputKind::ReadOnly));
    let start_key = if waitlist_create {
        "desiredStartsAt"
    } else {
        "startsAt"
    };
    let initial_day = day_of(&draft.with_untracked(|v| v[start_key].clone()));
    let day = signal(json!({"day":initial_day}));
    let times = operation
        .fields
        .iter()
        .filter(|field| {
            matches!(
                field.key,
                "startsAt" | "endsAt" | "desiredStartsAt" | "desiredEndsAt"
            )
        })
        .copied()
        .collect::<Vec<_>>();
    let owner = Owner::current();
    let type_id = computed(move || {
        let id = draft.with(|v| v["reservationTypeId"].as_str().unwrap_or("").to_owned());
        if id.is_empty() {
            original.with(|v| v["reservationTypeId"].as_str().unwrap_or("").to_owned())
        } else {
            id
        }
    });
    let resource_id =
        computed(move || draft.with(|v| v["resourceId"].as_str().unwrap_or("").to_owned()));
    let plan_id =
        computed(move || draft.with(|v| v["serviceId"].as_str().unwrap_or("").to_owned()));
    let load = move || {
        if loading.get_untracked() {
            return;
        }
        let Some(tenant) = data.connection.tenant.get_untracked() else {
            return;
        };
        if data.connection.token.with_untracked(Option::is_none) {
            return;
        }
        let context = tenant.clone();
        loading.set(true);
        error.set(String::new());
        spawn_in(owner, async move {
            let result = authed(data.connection.token, move |token| {
                let types = api::fetch_record(token, &tenant, "/v1/erp/reservation-types")?;
                let resources = api::fetch_record(token, &tenant, "/v1/erp/reservation-resources")?;
                let types = model::extract_rows(&types, "items")?;
                let resources = model::extract_rows(&resources, "items")?;
                let plans = if has_plan {
                    workforce::load_plans(token, &tenant, &json!({}))
                        .and_then(|v| model::extract_rows(&v, "items"))
                } else {
                    Ok(vec![])
                };
                Ok((types, resources, plans))
            })
            .await;
            if !owner.is_none_or(Owner::is_alive)
                || data.connection.tenant.get_untracked().as_ref() != Some(&context)
            {
                return;
            }
            loading.set(false);
            match result {
                Ok((loaded_types, loaded_locations, loaded_plans)) => {
                    let active_types = loaded_types
                        .into_iter()
                        .filter(|row| row["active"] != false)
                        .collect::<Vec<_>>();
                    type_options.set(options(&active_types));
                    types.set(active_types);
                    locations.set(loaded_locations);
                    match loaded_plans {
                        Ok(rows) => {
                            plans.set(rows);
                            plan_warning.set(String::new());
                        }
                        Err(e) => {
                            plans.set(vec![]);
                            plan_warning.set(format!("予約プランを読み込めません: {e}"));
                        }
                    }
                }
                Err(e) => error.set(e),
            }
        });
    };
    load();
    effect(move || {
        let mut eligible = matching_resources(&types.get(), &locations.get(), &type_id.get());
        if let Some(plan) =
            plans.with(|rows| rows.iter().find(|r| r["id"] == plan_id.get()).cloned())
        {
            if let Some(ids) = plan["eligibleResourceIds"].as_array() {
                eligible.retain(|r| ids.contains(&r["id"]));
            }
        }
        resource_options.set(options(&eligible));
        plan_options.set(
            matching_plans(&plans.get(), &type_id.get())
                .iter()
                .filter_map(|row| {
                    Some((
                        row["id"].as_str()?.into(),
                        format!(
                            "{} · ¥{} / 予約",
                            row["name"].as_str()?,
                            model::display(&row["priceAmount"])
                        ),
                    ))
                })
                .collect(),
        );
    });
    let scope = computed(move || {
        (
            type_id.get(),
            resource_id.get(),
            plan_id.get(),
            day.with(|v| v["day"].as_str().unwrap_or("").to_owned()),
        )
    });
    effect(move || {
        let _ = scope.get();
        generation.update(|g| *g += 1);
        slots.set(vec![]);
        slots_loading.set(false);
        slots_ready.set(false);
        slots_error.set(String::new());
    });
    let search = move || {
        let request_scope = scope.get_untracked();
        let (_, resource, _, day) = &request_scope;
        if resource.is_empty() {
            slots_error.set("予約種別と場所・設備を選んでください".into());
            return;
        }
        if !resource_options.with_untracked(|rows| rows.iter().any(|row| &row.0 == resource)) {
            slots_error.set("この予約種別に対応する場所・設備を選んでください".into());
            return;
        }
        if day_bounds(day).is_err() {
            slots_error.set("予約日を確認してください".into());
            return;
        }
        if slots_loading.get_untracked() {
            return;
        }
        let Some(tenant) = data.connection.tenant.get_untracked() else {
            return;
        };
        let context = tenant.clone();
        let resource = resource.clone();
        let day = day.clone();
        generation.update(|g| *g += 1);
        let version = generation.get_untracked();
        slots_loading.set(true);
        slots_ready.set(false);
        slots_error.set(String::new());
        slots.set(vec![]);
        let existing_duration = original.with_untracked(|v| {
            let start = input_time(&v["startsAt"]).or_else(|| input_time(&v["desiredStartsAt"]))?;
            let end = input_time(&v["endsAt"]).or_else(|| input_time(&v["desiredEndsAt"]))?;
            let duration = (end - start).num_minutes();
            (duration > 0).then_some(duration)
        });
        let plan_duration = plans.with_untracked(|rows| {
            rows.iter()
                .find(|r| r["id"] == plan_id.get_untracked())
                .and_then(|r| r["durationMinutes"].as_i64())
        });
        let unchanged_plan = original.with_untracked(|v| {
            !v.is_null() && v["serviceId"].as_str().unwrap_or("") == plan_id.get_untracked()
        });
        let duration = if unchanged_plan {
            existing_duration.or(plan_duration)
        } else {
            plan_duration.or(existing_duration)
        };
        spawn_in(owner, async move {
            let result = authed(data.connection.token, move |token| {
                reservation_slots::query_slots(
                    token,
                    &tenant,
                    &resource,
                    &day,
                    &day,
                    "Asia/Tokyo",
                    false,
                )
            })
            .await;
            if !owner.is_none_or(Owner::is_alive)
                || data.connection.tenant.get_untracked().as_ref() != Some(&context)
                || generation.get_untracked() != version
                || scope.get_untracked() != request_scope
            {
                return;
            }
            slots_loading.set(false);
            slots_ready.set(true);
            match result {
                Ok(value) => match model::extract_rows(&value, "items") {
                    Ok(rows) => slots.set(slot_candidates(&rows, duration)),
                    Err(e) => slots_error.set(e),
                },
                Err(e) => slots_error.set(e),
            }
        });
    };
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            SectionHeader(title: "日時・場所・プラン")
            Show(when: move || has_type) {
                Picker(label: "予約種別（必須）".to_owned(), options: type_options, selected: type_id, on_pick: move |id: String| {
                    draft.update(|value| { if value["reservationTypeId"] != id { value["reservationTypeId"] = json!(id); value["resourceId"] = json!(""); value["serviceId"] = json!(""); } });
                }, optional: false, loading: loading, error: error, on_retry: move |_| load())
            }
            Show(when: move || has_plan) {
                Picker(label: "予約プラン".to_owned(), options: plan_options, selected: plan_id, on_pick: move |id: String| {
                    if id.is_empty() { draft.update(|v| v["serviceId"] = json!("")); }
                    else if let Some(plan) = plans.with_untracked(|rows| rows.iter().find(|r| r["id"] == id).cloned()) {
                        let kind = types.with_untracked(|rows| rows.iter().find(|r| r["id"] == type_id.get_untracked()).cloned());
                        let mut value = draft.get_untracked();
                        match apply_plan(&mut value, &plan, price_editable, kind.as_ref()) {
                            Ok(()) => { draft.set(value); plan_warning.set(String::new()); }, Err(error) => plan_warning.set(error),
                        }
                    }
                }, optional: true, loading: loading, error: plan_warning, on_retry: move |_| load())
            }
            Picker(label: "場所・設備".to_owned(), options: resource_options, selected: resource_id, on_pick: move |id: String| draft.update(|v| v["resourceId"] = json!(id)), optional: true, loading: loading, error: error, on_retry: move |_| load())
            Text(value: computed(move || types.with(|rows| rows.iter().find(|row| row["id"] == type_id.get()).map(|row| {
                let label = row["quantityPresentation"]["countLabel"].as_str().unwrap_or("人数・利用数");
                row["quantityPolicy"]["maxCount"].as_i64().map(|n| format!("{label}の上限: {n}。空き組数は人数とは別に管理します。")).unwrap_or_else(|| "1予約で1組分の空き枠を使用します。人数とは別です。".into())
            }).unwrap_or_default())), style: computed(move || css!(font_size: px(14), color: hex(SECONDARY_LABEL), padding: px(16))))
            InputFields(fields: vec![DAY_FIELD], draft: day, prefix: String::new(), token: data.connection.token, tenant: data.connection.tenant)
            ActionButton(label: "この日の空き枠を調べる", destructive: false, on_tap: move |_| search())
            Show(when: move || slots_loading.get()) { Notice(message: "空き枠を確認中…") }
            Show(when: move || !slots_error.with(String::is_empty)) { ErrorNotice(message: computed(move || slots_error.get())) }
            Show(when: move || slots_ready.get() && slots.with(Vec::is_empty) && slots_error.with(String::is_empty)) { Notice(message: "この日は予約枠が開放されていません。営業時間・休業日と枠の生成を確認してください。") }
            ForEach(each: move || slots.get().into_iter().enumerate().collect::<Vec<_>>(), key: |row: &(usize, Value)| row.0, children: move |row: ReadSignal<(usize, Value)>| {
                render! { ListRow(title: computed(move || format!("{} 〜 {}", display_time(&row.get().1["startsAt"]), display_time(&row.get().1["_bookingEndsAt"]))), subtitle: computed(move || {
                    let row = row.get().1; format!("空き {}組 / 受付 {}組 · 仮押さえ {}組", model::display(&row["availableQuantity"]), model::display(&row["capacity"]), model::display(&row["heldQuantity"]))
                }), subtitle_color: SECONDARY_LABEL, trailing: computed(move || if row.get().1["_bookable"] == true { "選択".into() } else { "空きなし".into() }), avatar: false, separator: true, on_tap: move |_| {
                    let row = row.get_untracked().1; let resource = resource_id.get_untracked();
                    let mut value = draft.get_untracked(); match select_slot(&mut value, &row, &resource, waitlist_create) { Ok(()) => { draft.set(value); slots_error.set(String::new()); }, Err(e) => slots_error.set(e) }
                }) }
            })
            Notice(message: "日時は日本時間です。空き状況は保存時にも確認されます。日時を直接入力することもできます。")
            InputFields(fields: times, draft: draft, prefix: String::new(), token: data.connection.token, tenant: data.connection.tenant)
        }
    }
}

#[component]
fn booking_customer_picker(data: Data, draft: RwSignal<Value>) -> Element {
    let rows = signal(Vec::<Value>::new());
    let choices = signal(Vec::<(String, String)>::new());
    let loading = signal(false);
    let error = signal(String::new());
    let owner = Owner::current();
    let load = move || {
        if loading.get_untracked() {
            return;
        }
        let Some(tenant) = data.connection.tenant.get_untracked() else {
            return;
        };
        if data.connection.token.with_untracked(Option::is_none) {
            return;
        }
        let context = tenant.clone();
        loading.set(true);
        error.set(String::new());
        spawn_in(owner, async move {
            let result = authed(data.connection.token, move |token| {
                model::fetch_lookup(
                    token,
                    &tenant,
                    Lookup {
                        path: "/v1/erp/customers?limit=100",
                        list_key: "data",
                        value_key: "id",
                        label_keys: &["name", "email"],
                    },
                )
            })
            .await;
            if !owner.is_none_or(Owner::is_alive)
                || data.connection.tenant.get_untracked().as_ref() != Some(&context)
            {
                return;
            }
            loading.set(false);
            match result {
                Ok((value, truncated)) => match model::extract_rows(&value, "data") {
                    Ok(values) => {
                        choices.set(
                            values
                                .iter()
                                .filter_map(|row| {
                                    Some((
                                        row["id"].as_str()?.into(),
                                        format!(
                                            "{} {}",
                                            row["name"].as_str().unwrap_or("お名前なし"),
                                            row["email"].as_str().unwrap_or("")
                                        ),
                                    ))
                                })
                                .collect(),
                        );
                        rows.set(values);
                        if truncated {
                            error.set("表示上限に達しています。候補にないお客様は連絡先を直接入力できます".into());
                        }
                    }
                    Err(e) => error.set(e),
                },
                Err(e) => error.set(e),
            }
        });
    };
    load();
    render! { Picker(label: "登録済みのお客様".to_owned(), options: choices, selected: computed(move || draft.with(|v| v["customerId"].as_str().unwrap_or("").to_owned())), on_pick: move |id: String| {
        let customer = rows.with_untracked(|rows| rows.iter().find(|row| row["id"] == id).cloned());
        draft.update(|value| {
            value["customerId"] = json!(id);
            if let Some(customer) = &customer { for (field, key) in [("customerName","name"),("customerEmail","email"),("customerPhone","phone")] { value[field] = json!(customer[key].as_str().unwrap_or("")); } }
        });
    }, optional: true, loading: loading, error: error, on_retry: move |_| load()) }
}

#[component]
fn cancellation_preview(original: RwSignal<Value>, draft: RwSignal<Value>) -> Element {
    let preview = computed(move || {
        crate::management::reservation_workflows::cancellation_preview(
            &original.get(),
            &draft.get(),
            crate::management::reservation_workflows::now_jst(),
        )
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            SectionHeader(title: "取消・返金の確認")
            InfoGroup(rows: computed(move || match preview.get() {
                Ok(v) => vec![("支払済み".into(),money(&v,"paidAmount")),("キャンセル料".into(),money(&v,"cancellationFeeAmount")),("返金する金額".into(),money(&v,"refundAmount")),("取消規定の期限".into(),display_time(&v["cutoffAt"]))],
                Err(e) => vec![("入力内容の確認".into(),e)],
            }), inline: false)
            Notice(message: "返金依頼後も、完了するまでは返金待ちです。取消履歴と決済状態を確認してください。")
        }
    }
}

#[component]
fn notification_preview(original: RwSignal<Value>, draft: RwSignal<Value>) -> Element {
    let preview = computed(move || {
        crate::management::reservation_workflows::notification_preview(
            &original.get(),
            &draft.get(),
        )
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            SectionHeader(title: "送信するメールのプレビュー")
            InfoGroup(rows: computed(move || match preview.get() {
                Ok(value) => vec![("宛先".into(),model::display(&value["to"])),("件名".into(),model::display(&value["subject"])),("本文".into(),model::display(&value["body"])),("送信時の追加内容".into(),model::display(&value["notice"]))],
                Err(error) => vec![("入力内容の確認".into(),error)],
            }), inline: false)
        }
    }
}

#[component]
pub(super) fn reservation_detail_panel(data: Data, record: ReadSignal<Value>) -> Element {
    let link_message = signal(String::new());
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Show(when: move || !data.notice.with(String::is_empty)) { Text(value: computed(move || data.notice.get()), style: computed(move || css!(color: hex(GREEN), font_size: px(15), padding: px(16)))) }
            Show(when: move || record.with(|v| v["_reservationContextWarning"].as_str().is_some_and(|s| !s.is_empty()))) {
                ErrorNotice(message: computed(move || record.with(|v| v["_reservationContextWarning"].as_str().unwrap_or("").to_owned())))
                ActionButton(label: "予約の関連情報を再確認", destructive: false, on_tap: move |_| data.open(record.get_untracked()))
            }
            Show(when: move || reservation_waitlist::converted_record(&record.get()).is_some()) { ActionButton(label: "繰り上げた予約を開く", destructive: false, on_tap: move |_| { if let Some(row) = reservation_waitlist::converted_record(&record.get_untracked()) { data.open_related("reservations", row); } }) }
            Show(when: move || data.resource.key == "reservations") {
                Show(when: move || field_glass::PAYMENT_LINK_SUPPORTED && crate::management::reservation_workflows::checkout_url(&record.get()).is_some()) {
                    field_glass::PaymentLinkButton(label: "支払リンクを開く・コピー・共有".to_owned(), url: computed(move || record.with(|v| crate::management::reservation_workflows::checkout_url(v).unwrap_or("").to_owned())), on_done: move |result: field_glass::PaymentLinkDetail| {
                        if !result.error.is_empty() { link_message.set(result.error); }
                        else if result.ok { link_message.set(match result.action.as_str() { "copy" => "支払リンクをコピーしました", "share" => "支払リンクを共有しました", _ => "支払リンクを開きました。支払結果は予約を読み直して確認してください" }.into()); }
                    }, style: css!(height: px(48), margin: px(16)))
                    Show(when: move || !link_message.with(String::is_empty)) { Text(value: computed(move || link_message.get()), style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(15), padding: px(16)))) }
                }
                SectionHeader(title: "取消・返金履歴")
                HistoryRows(record: record, key: "_reservationCancellations", kind: "cancellation")
                SectionHeader(title: "未割当入金")
                HistoryRows(record: record, key: "_reservationUnmatchedPayments", kind: "payment")
            }
        }
    }
}

#[component]
fn history_rows(record: ReadSignal<Value>, key: &'static str, kind: &'static str) -> Element {
    let ready_key = if kind == "cancellation" {
        "cancellations"
    } else {
        "unmatchedPayments"
    };
    render! {
        View(style: computed(move || group_style())) {
            Show(when: move || record.with(|v| v["_reservationContextReady"][ready_key] != true)) { Notice(message: "まだ取得できていません。関連情報を読み直してください。") }
            Show(when: move || record.with(|v| v["_reservationContextReady"][ready_key] == true && v[key].as_array().is_some_and(Vec::is_empty))) { Notice(message: "履歴はありません") }
            ForEach(each: move || record.with(|v| v[key].as_array().cloned().unwrap_or_default()).into_iter().enumerate().collect::<Vec<_>>(), key: |row:&(usize,Value)| row.0, children: move |row:ReadSignal<(usize,Value)>| render! {
                InfoGroup(rows: computed(move || {
                    let v = row.get().1;
                    if kind == "cancellation" {
                        vec![("取消日時".into(),display_time(&v["createdAt"])),("理由".into(),model::display(&v["reason"])),("キャンセル料".into(),money(&v,"cancellationFeeAmount")),("返金依頼額".into(),money(&v,"refundAmount")),("取消前の状態".into(),crate::features::status_label(v["previousStatus"].as_str().unwrap_or(""))),("実行者の種類".into(),match v["actorType"].as_str() { Some("user") => "利用者", Some("service_account") => "サービスアカウント", Some("system") => "システム", _ => "不明" }.into())]
                    } else {
                        vec![("記録日時".into(),display_time(&v["recordedAt"])),("入金額".into(),money(&v,"amount")),("決済事業者".into(),model::display(&v["providerLabel"])),("確認日時".into(),display_time(&v["reviewedAt"])),("確認内容".into(),model::display(&v["reviewNote"]))]
                    }
                }), inline: false)
            })
        }
    }
}

#[component]
pub(super) fn handoff_panel(data: Data, record: ReadSignal<Value>) -> Element {
    let subject = handoff::subject_type(data.resource.key).unwrap_or("reservation");
    let notes = signal(Vec::<Value>::new());
    let loading = signal(false);
    let error = signal(String::new());
    let truncated = signal(false);
    let generation = signal(0u64);
    let owner = Owner::current();
    let load = move || {
        let Some(tenant) = data.connection.tenant.get_untracked() else {
            return;
        };
        if data.connection.token.with_untracked(Option::is_none) {
            return;
        }
        let id = record.with_untracked(|v| v["id"].as_str().unwrap_or("").to_owned());
        if id.is_empty() {
            return;
        }
        let context = tenant.clone();
        let expected_id = id.clone();
        generation.update(|g| *g += 1);
        let version = generation.get_untracked();
        loading.set(true);
        error.set(String::new());
        notes.set(vec![]);
        spawn_in(owner, async move {
            let result = authed(data.connection.token, move |token| {
                handoff::load(token, &tenant, subject, &id)
            })
            .await;
            if !owner.is_none_or(Owner::is_alive)
                || data.connection.tenant.get_untracked().as_ref() != Some(&context)
                || generation.get_untracked() != version
                || record.with_untracked(|v| v["id"] != expected_id)
            {
                return;
            }
            loading.set(false);
            match result {
                Ok(v) => {
                    truncated.set(v["truncated"] == true);
                    notes.set(v["items"].as_array().cloned().unwrap_or_default());
                }
                Err(e) => error.set(e),
            }
        });
    };
    effect(move || {
        let _ = data.detail_generation.get();
        if data.detail_ready.get() {
            load();
        }
    });
    let create = if subject == "reservation" {
        handoff::CREATE_RESERVATION
    } else {
        handoff::CREATE_WAITLIST
    };
    render! {
        View(style: computed(move || group_style())) {
            SectionHeader(title: "引き継ぎメモ")
            Show(when: move || loading.get()) { Notice(message: "引き継ぎを確認中…") }
            Show(when: move || !error.with(String::is_empty)) { ErrorNotice(message: computed(move || error.get())) }
            ActionButton(label: "引き継ぎを読み直す", destructive: false, on_tap: move |_| load())
            Show(when: move || data.can_mutate(create, &record.get())) { ActionButton(label: "引き継ぎメモを追加", destructive: false, on_tap: move |_| data.begin(create, record.get_untracked())) }
            Show(when: move || truncated.get()) { Notice(message: "表示上限の200件です。さらに過去の履歴がある可能性があります。") }
            ForEach(each: move || notes.get(), key: |note: &Value| note["id"].as_str().unwrap_or("").to_owned(), children: move |note: ReadSignal<Value>| render! {
                View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
                    InfoGroup(rows: computed(move || {
                        let v=note.get(); vec![("状態".into(),handoff::label(&v).into()),("分類".into(),model::display(&v["category"])),("内容".into(),model::display(&v["body"])),("記録日時".into(),display_time(&v["createdAt"])),("引き継ぎ日時".into(),display_time(&v["acknowledgedAt"])),("撤回理由".into(),model::display(&v["retractionReason"]))]
                    }), inline: false)
                    Show(when: move || data.can_mutate(handoff::ACKNOWLEDGE, &note.get())) { ActionButton(label: "このメモを引き継いだ", destructive: false, on_tap: move |_| data.begin(handoff::ACKNOWLEDGE, note.get_untracked())) }
                    Show(when: move || data.can_mutate(handoff::RETRACT, &note.get())) { ActionButton(label: "このメモを撤回", destructive: true, on_tap: move |_| data.begin(handoff::RETRACT, note.get_untracked())) }
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calendar_dates_use_japan_midnight_and_exclusive_end() {
        assert_eq!(
            day_bounds("2026-11-20").unwrap(),
            (
                "2026-11-19T15:00:00+00:00".into(),
                "2026-11-20T15:00:00+00:00".into()
            )
        );
        assert_eq!(local_time("2026-11-19T23:00:00Z"), "2026-11-20 08:00");
        assert!(day_bounds("2026-02-30").is_err());
    }
    #[test]
    fn slot_selection_uses_one_start_group_for_the_entire_service_duration() {
        let slots = json!([
            {"resourceId":"room","startsAt":"2026-11-20T00:00:00Z","endsAt":"2026-11-20T00:30:00Z","active":true,"availableQuantity":1},
            {"resourceId":"room","startsAt":"2026-11-20T00:30:00Z","endsAt":"2026-11-20T01:00:00Z","active":true,"availableQuantity":1},
            {"resourceId":"room","startsAt":"2026-11-20T01:00:00Z","endsAt":"2026-11-20T01:30:00Z","active":true,"availableQuantity":0}
        ]);
        let result = slot_candidates(slots.as_array().unwrap(), Some(60));
        assert_eq!(result[0]["_bookable"], true);
        assert_eq!(result[1]["_bookable"], true);
        assert_eq!(result[2]["_bookable"], false);
        let mut draft = json!({"quantity":"4"});
        select_slot(&mut draft, &result[0], "room", false).unwrap();
        assert_eq!(draft["startsAt"], "2026-11-20T09:00:00+09:00");
        assert_eq!(draft["endsAt"], "2026-11-20T10:00:00+09:00");
        assert_eq!(draft["quantity"], "4");
        assert!(select_slot(&mut draft, &result[0], "different", false).is_err());
    }
    #[test]
    fn scoped_resource_and_plan_selection_preserves_total_price() {
        let types = vec![json!({"id":"golf","resourceModel":"golf_course"})];
        let resources = vec![
            json!({"id":"ok","resourceModel":"golf_course","active":true}),
            json!({"id":"inactive","resourceModel":"golf_course","active":false}),
            json!({"id":"other","resourceModel":"room","active":true}),
        ];
        assert_eq!(matching_resources(&types, &resources, "golf").len(), 1);
        let mut draft = json!({"quantity":"4","priceAmount":"0"});
        apply_plan(
            &mut draft,
            &json!({"id":"plan","priceAmount":12000}),
            true,
            None,
        )
        .unwrap();
        assert_eq!(draft["priceAmount"], "12000");
        apply_plan(
            &mut draft,
            &json!({"id":"plan2","priceAmount":20000}),
            false,
            None,
        )
        .unwrap();
        assert_eq!(draft["priceAmount"], "12000");
        assert!(
            matching_plans(
                &[json!({"id":"stopped","enabled":true,"available":false})],
                "golf"
            )
            .is_empty()
        );
    }
    #[test]
    fn changing_plan_refreshes_duration_prepayment_and_eligible_resource() {
        let mut draft = json!({"serviceId":"old","startsAt":"2026-11-20 23:00","endsAt":"2026-11-20 23:30","resourceId":"old-room","quantity":"4"});
        apply_plan(&mut draft,&json!({"id":"new","priceAmount":12000,"durationMinutes":90,"prepaymentPolicy":"deposit_required","depositRatio":0.25,"eligibleResourceIds":["new-room"]}),true,None).unwrap();
        assert_eq!(draft["endsAt"], "2026-11-21T00:30:00+09:00");
        assert_eq!(draft["resourceId"], "");
        assert_eq!(draft["priceAmount"], "12000");
        assert_eq!(draft["depositAmount"], "3000");
        assert_eq!(draft["quantity"], "4");
    }
}
