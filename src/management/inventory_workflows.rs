//! Inventory commands use the reviewed REST contracts, including stocktake versions.
//! Commerce product stock and ERP location stock are separate ledgers.

use super::{model, schema::*};
use crate::api::{self, Tenant};
use serde_json::{Value, json};
use std::collections::HashSet;

const GROUP: &str = "商品・在庫管理";
const STOCKTAKES: &str = "/v1/field/inventory/stocktakes";
const LOCATIONS_PATH: &str = "/v1/field/inventory/locations";
const WAREHOUSES_PATH: &str = "/v1/field/inventory/warehouses";
const TRANSFERS: &str = "/v1/field/inventory/stock-transfers";
const SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const PRODUCTS: Lookup = Lookup {
    path: "/v1/order/products",
    list_key: "items",
    value_key: "id",
    label_keys: &["name", "skuCode"],
};
const LOCATIONS: Lookup = Lookup {
    path: LOCATIONS_PATH,
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const WAREHOUSES: Lookup = Lookup {
    path: WAREHOUSES_PATH,
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const REASONS: &[Choice] = &[
    Choice::new("unrecorded_waste", "廃棄の記録漏れ"),
    Choice::new("spoilage", "傷み・期限切れ"),
    Choice::new("prep_loss", "仕込みのロス"),
    Choice::new("unrecorded_receipt", "入荷の記録漏れ"),
    Choice::new("unrecorded_usage", "使用の記録漏れ"),
    Choice::new("count_error", "数え間違い"),
    Choice::new("missing", "紛失・所在不明"),
    Choice::new("other", "その他"),
];
const CONFIRM: Field = Field::new("confirmed", "内容を確認しました", InputKind::Bool);
const STOCK_FIELDS: &[Field] = &[
    Field::new("name", "商品", InputKind::ReadOnly),
    Field::new("skuCode", "SKU", InputKind::ReadOnly),
    Field::new("stock.quantityOnHand", "現在の在庫数", InputKind::ReadOnly),
    Field::new(
        "stock.quantityAvailable",
        "現在の出庫可能数",
        InputKind::ReadOnly,
    ),
    Field::new("quantity", "数量", InputKind::Integer)
        .required()
        .help("入荷・出庫は1以上。増減調整は増やす数を正、減らす数を負で入力します。"),
    Field::new("note", "理由", InputKind::Multiline)
        .required()
        .help("2〜500文字で、記録を残す理由を入力してください。"),
    CONFIRM,
];
const TRANSFER_FIELDS: &[Field] = &[
    Field::text("sku", "SKU")
        .required()
        .help("拠点在庫で確認した品目のSKUを入力してください。"),
    Field::new("fromLocationId", "移動元", InputKind::Reference(LOCATIONS)).required(),
    Field::new("toLocationId", "移動先", InputKind::Reference(LOCATIONS)).required(),
    Field::new("quantity", "移動数量", InputKind::Integer).required(),
    Field::new("reason", "移動理由", InputKind::Multiline)
        .required()
        .help("2〜500文字で入力してください。"),
    CONFIRM,
];
const STOCKTAKE_CREATE: &[Field] = &[
    Field::new(
        "warehouseId",
        "棚卸する倉庫",
        InputKind::Reference(WAREHOUSES),
    )
    .required(),
    Field::new("countDate", "棚卸日", InputKind::Date)
        .required()
        .default("@today"),
    Field::new(
        "cadence",
        "周期",
        InputKind::Select(&[
            Choice::new("daily", "日次"),
            Choice::new("weekly", "週次"),
            Choice::new("monthly", "月次"),
            Choice::new("adhoc", "随時"),
        ]),
    )
    .required()
    .default("adhoc"),
    Field::new("note", "メモ", InputKind::Multiline).empty(Empty::Null),
];
const STOCKTAKE_LINES: &[Field] = &[
    Field::new("lineId", "", InputKind::Hidden).read("id"),
    Field::new("sku", "SKU", InputKind::ReadOnly),
    Field::new("theoreticalQuantity", "帳簿数量", InputKind::ReadOnly),
    Field::new("unitCostAmount", "評価単価（円）", InputKind::ReadOnly),
    Field::new("countedQuantity", "保存済みの実数合計", InputKind::ReadOnly),
    Field::new(
        "varianceQuantity",
        "保存済みの差異数量",
        InputKind::ReadOnly,
    ),
    Field::new(
        "counts",
        "保管場所ごとの実数",
        InputKind::Lines(&[
            Field::text("storageArea", "保管場所")
                .empty(Empty::String)
                .help("冷蔵庫・棚など。同じ場所を重複して登録しないでください。"),
            Field::new("quantity", "数えた数量", InputKind::Integer)
                .required()
                .help("実数0は0を入力します。行がない品目は未計数のままです。"),
        ]),
    ),
    Field::new("variance.reason", "差異理由", InputKind::Select(REASONS))
        .read("varianceReason")
        .empty(Empty::Null)
        .help("実数と帳簿数量が違う品目は、提出までに理由を選んでください。"),
    Field::new("variance.note", "差異のメモ", InputKind::Multiline)
        .read("varianceNote")
        .empty(Empty::Null)
        .help("「その他」のときは必須です。1024文字以内。"),
];
const STOCKTAKE_EDIT: &[Field] = &[
    Field::new("countDate", "棚卸日", InputKind::ReadOnly),
    Field::new("warehouseName", "倉庫", InputKind::ReadOnly),
    Field::new("lines", "棚卸明細", InputKind::Lines(STOCKTAKE_LINES)).fixed_rows(),
];
const STOCKTAKE_ACTION_FIELDS: &[Field] = &[
    Field::new("countDate", "棚卸日", InputKind::ReadOnly),
    Field::new("warehouseName", "倉庫", InputKind::ReadOnly),
    Field::new("totals.lineCount", "品目数", InputKind::ReadOnly),
    Field::new(
        "totals.countedLineCount",
        "数えた品目数",
        InputKind::ReadOnly,
    ),
    Field::new(
        "totals.varianceLineCount",
        "差異のある品目数",
        InputKind::ReadOnly,
    ),
    Field::new(
        "totals.varianceValue",
        "差異金額（円）",
        InputKind::ReadOnly,
    ),
    CONFIRM,
];

const fn command(
    label: &'static str,
    method: &'static str,
    path: &'static str,
    fields: &'static [Field],
) -> Mutation {
    Mutation::new(label, method, path, fields).capability("inventory.manage")
}

/// Extra screen for the commerce ledger; the existing inventory screen stays location-based.
pub const RESOURCES: &[Resource] = &[
    Resource::new("commerce-inventory", GROUP, "商品在庫の入荷・出庫・調整", "/v1/order/products/{productId}", "items")
        .route("inventory")
        .title(&["name"]).subtitle(&["skuCode", "stock.quantityOnHand"])
        .filters(&[Field::new("productId", "商品", InputKind::Reference(PRODUCTS)).required()])
        .load(load_product_inventory)
        .fields(&[
            ("商品", "name"), ("SKU", "skuCode"), ("在庫の状態", "stockState"),
            ("在庫数", "stock.quantityOnHand"), ("引当済み", "stock.quantityReserved"),
            ("出庫可能数", "stock.quantityAvailable"), ("在庫を管理する", "stock.trackInventory"),
            ("更新日時", "stock.updatedAt"),
        ])
        .read_lines(&[Field::new("movements", "在庫履歴（最新100件）", InputKind::Lines(&[
            Field::new("createdAt", "日時", InputKind::ReadOnly),
            Field::new("movementType", "操作", InputKind::ReadOnly),
            Field::new("quantity", "数量", InputKind::Integer),
            Field::new("note", "理由", InputKind::ReadOnly),
        ]))])
        .actions(&[
            command("入荷を記録", "POST", "/v1/erp/commerce/products/{id}/stock/receive", STOCK_FIELDS)
                .when(product_has_stock_operations).prepare(prepare_receive).execute(receive_stock)
                .explain("指定数量を商品在庫に加算します。拠点間の移動は「在庫移動」から記録してください。"),
            command("出庫を記録", "POST", "/v1/erp/commerce/products/{id}/stock/issue", STOCK_FIELDS)
                .when(product_has_stock_operations).prepare(prepare_issue).execute(issue_stock)
                .explain("指定数量を商品在庫から減らします。出庫可能数を確認して実行してください。"),
            command("在庫を増減調整", "POST", "/v1/erp/commerce/products/{id}/stock/adjust", STOCK_FIELDS)
                .when(product_has_stock_operations).prepare(prepare_adjust).execute(adjust_stock)
                .destructive()
                .explain("入力するのは調整後の在庫数ではなく増減数です。例: 10個から8個に直す場合は -2 を入力します。"),
        ]),
];

const STOCKTAKE_ACTIONS: &[Mutation] = &[
    command("棚卸を提出", "POST", "/v1/field/inventory/stocktakes/{id}/submit", STOCKTAKE_ACTION_FIELDS)
        .when(counting).prepare(prepare_submission).execute(submit_stocktake)
        .explain("保存済みの実数を提出します。全品目の計数と差異理由が必要です。提出だけでは在庫数は変わりません。"),
    command("実数の修正へ差し戻す", "POST", "/v1/field/inventory/stocktakes/{id}/reopen", STOCKTAKE_ACTION_FIELDS)
        .when(submitted).prepare(prepare_transition).execute(reopen_stocktake)
        .explain("提出済みの棚卸を差し戻して実数・理由を修正できる状態に戻します。"),
    command("棚卸を確定して在庫へ反映", "POST", "/v1/field/inventory/stocktakes/{id}/finalize", STOCKTAKE_ACTION_FIELDS)
        .when(submitted).prepare(prepare_transition).execute(finalize_stocktake).destructive()
        .explain("差異のある品目の帳簿在庫を実数に合わせて確定します。この棚卸を編集可能な状態には戻せません。"),
    command("帳簿数量を取り直す", "POST", "/v1/field/inventory/stocktakes/{id}/refresh", STOCKTAKE_ACTION_FIELDS)
        .when(counting).prepare(prepare_transition).execute(refresh_stocktake).destructive()
        .explain("開始後に在庫が動いた品目の帳簿数量を更新します。該当品目の保存済み実数と差異理由は消えるので、数え直してください。"),
    command("棚卸を中止", "POST", "/v1/field/inventory/stocktakes/{id}/cancel", STOCKTAKE_ACTION_FIELDS)
        .when(open_stocktake).prepare(prepare_transition).execute(cancel_stocktake).destructive()
        .explain("この棚卸を中止します。在庫数は変わらず、中止後は再開できません。"),
];
const PO_ACTIONS: &[Mutation] = &[command(
    "承認を申請",
    "POST",
    "/api/approvals",
    &[
        Field::new("purchaseOrderNumber", "発注番号", InputKind::ReadOnly),
        Field::new("vendorName", "仕入先", InputKind::ReadOnly),
        Field::new("totalAmount", "合計", InputKind::ReadOnly).read("@money:totalAmount"),
        Field::new("expectedDeliveryDate", "納品予定日", InputKind::ReadOnly),
        CONFIRM,
    ],
)
.when(po_submittable)
.prepare(prepare_po_submission)
.execute(submit_purchase_order)
.explain("現在の発注内容を承認へ回します。差戻し後は同じ承認ワークフローへ再提出します。")];

pub fn decorate(resource: Resource) -> Resource {
    match resource.key {
        "inventory-transfers" => resource.load(load_transfers)
            .subtitle(&["fromLocationName", "toLocationName", "quantity"])
            .fields(&[("SKU", "sku"), ("移動元", "fromLocationName"), ("移動先", "toLocationName"),
                ("数量", "quantity"), ("移動理由", "reason"), ("移動日時", "transferredAt")])
            .create(command("在庫移動を記録", "POST", "/v1/field/inventory/stock-transfers/idempotent", TRANSFER_FIELDS)
                .prepare(prepare_transfer).idempotency_body("idempotencyKey").execute(create_transfer)
                .explain("移動元の在庫を減らし、移動先へ同じ数量を入庫します。SKU・拠点・数量を確認してください。")),
        "inventory-stocktakes" => resource.load(load_stocktakes)
            .subtitle(&["warehouseName", "cadence", "totals.lineCount"])
            .fields(&[("棚卸日", "countDate"), ("倉庫", "warehouseName"), ("周期", "cadence"),
                ("メモ", "note"), ("品目数", "totals.lineCount"), ("数えた品目数", "totals.countedLineCount"),
                ("帳簿金額（円）", "totals.theoreticalValue"), ("実棚金額（円）", "totals.countedValue"),
                ("差異金額（円）", "totals.varianceValue"), ("提出日時", "submittedAt"), ("確定日時", "finalizedAt")])
            .create(command("棚卸を開始", "POST", STOCKTAKES, STOCKTAKE_CREATE)
                .prepare(prepare_start_stocktake).execute(start_stocktake)
                .explain("倉庫の帳簿数量と評価単価を取得し、棚卸を開始します。実数を保存して提出するまで在庫は変更されません。"))
            .edit(command("実数・差異理由を保存", "PUT", "/v1/field/inventory/stocktakes/{id}", STOCKTAKE_EDIT)
                .when(counting).prepare(prepare_counts).execute(save_counts)
                .explain("保管場所ごとに数えた数量を入力します。数量0と未計数は別です。保存後、全品目を数えて差異理由を記入したら提出してください。"))
            .actions(STOCKTAKE_ACTIONS),
        "purchase-orders" => resource.actions(PO_ACTIONS),
        _ => resource,
    }
}

fn text<'a>(value: &'a Value, key: &str, label: &str) -> Result<&'a str, String> {
    model::value_at(value, key)
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{label}を確認できません。読み直してください"))
}
fn rows<'a>(value: &'a Value, key: &str, label: &str) -> Result<&'a Vec<Value>, String> {
    model::value_at(value, key)
        .as_array()
        .ok_or_else(|| format!("{label}を読み込めませんでした"))
}
fn confirmed(body: &mut Value) -> Result<(), String> {
    if body["confirmed"].as_bool() != Some(true) {
        return Err("内容を確認してチェックを入れてください".into());
    }
    body.as_object_mut()
        .ok_or("操作内容を確認できません")?
        .remove("confirmed");
    Ok(())
}
fn trim_text(
    body: &mut Value,
    key: &str,
    label: &str,
    min: usize,
    max: usize,
) -> Result<(), String> {
    let value = body[key].as_str().unwrap_or("").trim();
    if !(min..=max).contains(&value.chars().count()) {
        return Err(format!("{label}は{min}〜{max}文字で入力してください"));
    }
    body[key] = json!(value);
    Ok(())
}
fn normalized_optional(value: &Value, max: usize, label: &str) -> Result<Value, String> {
    let text = value.as_str().unwrap_or("").trim();
    if text.chars().count() > max {
        return Err(format!("{label}は{max}文字以内で入力してください"));
    }
    Ok(if text.is_empty() {
        Value::Null
    } else {
        json!(text)
    })
}
fn positive_quantity(body: &Value, max: i64) -> Result<i64, String> {
    body["quantity"]
        .as_i64()
        .filter(|quantity| (1..=max).contains(quantity))
        .ok_or_else(|| format!("数量は1〜{max}の整数で入力してください"))
}
fn same_fields(original: &Value, fresh: &Value, keys: &[&str], label: &str) -> Result<(), String> {
    if keys
        .iter()
        .any(|key| model::value_at(original, key) != model::value_at(fresh, key))
    {
        return Err(format!(
            "{label}が更新されています。読み直して内容を確認してください"
        ));
    }
    Ok(())
}
fn fetch_items(token: &str, tenant: &Tenant, path: &str) -> Result<Vec<Value>, String> {
    let response = api::fetch_record(token, tenant, path)?;
    Ok(rows(&response, "items", "一覧")?.clone())
}
fn path_for(template: &str, original: &Value) -> Result<String, String> {
    model::record_path(
        Resource::new("inventory-command", GROUP, "", "", ""),
        template,
        original,
    )
}

fn load_product_inventory(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let product_id = text(filters, "productId", "商品")?;
    let product = api::fetch_record(
        token,
        tenant,
        &format!("/v1/order/products/{}", api::encode_query(product_id)),
    )?;
    let path = format!(
        "/v1/erp/commerce/products/{}/stock",
        api::encode_query(product_id)
    );
    let stock = if product["kind"] == "PLAN" {
        None
    } else {
        api::fetch_optional_record(token, tenant, &path)?
    };
    let mut record = product_inventory_record(product_id, &product, stock.as_ref())?;
    let mut movements = fetch_items(
        token,
        tenant,
        &format!("{path}/movements?limit=100&offset=0"),
    )?;
    if movements.len() > 100 {
        movements.truncate(100);
    }
    record["movements"] = Value::Array(movements);
    Ok(json!({"items":[record]}))
}
fn product_inventory_record(
    product_id: &str,
    product: &Value,
    stock: Option<&Value>,
) -> Result<Value, String> {
    if product["id"].as_str() != Some(product_id) {
        return Err("選択した商品と取得した商品が一致しません".into());
    }
    text(product, "name", "商品名")?;
    if let Some(stock) = stock {
        validate_product_stock(stock, product_id)?;
    }
    Ok(json!({
        "id":product_id, "name":product["name"], "kind":product["kind"], "skuCode":product["skuCode"],
        "stock":stock.cloned().unwrap_or(Value::Null),
        "stockState":if product["kind"] == "PLAN" {"プラン商品（在庫操作の対象外）"} else if stock.is_some() {"登録済み"} else {"在庫未登録"},
    }))
}
fn product_has_stock_operations(record: &Value) -> bool {
    record["id"].as_str().is_some_and(|id| !id.is_empty())
        && record["kind"].as_str().is_some_and(|kind| kind != "PLAN")
}
fn validate_product_stock(stock: &Value, product_id: &str) -> Result<(), String> {
    if stock["productId"].as_str() != Some(product_id)
        || ["quantityOnHand", "quantityAvailable", "quantityReserved"]
            .iter()
            .any(|key| stock[*key].as_i64().is_none())
        || stock["updatedAt"].as_str().is_none()
    {
        return Err("商品在庫の応答を確認できません。再読み込みしてください".into());
    }
    Ok(())
}
fn prepare_stock(body: &mut Value, original: &Value, operation: &str) -> Result<(), String> {
    if !product_has_stock_operations(original) {
        return Err("この商品では在庫操作を行えません".into());
    }
    confirmed(body)?;
    trim_text(body, "note", "理由", 2, 500)?;
    let quantity = body["quantity"]
        .as_i64()
        .ok_or("数量は整数で入力してください")?;
    if quantity == 0 || quantity.unsigned_abs() > i32::MAX as u64 {
        return Err("数量は0以外、絶対値2,147,483,647以下の整数で入力してください".into());
    }
    if operation != "adjust" {
        positive_quantity(body, i32::MAX.into())?;
    }
    if operation == "issue" {
        let available = original["stock"]["quantityAvailable"].as_i64().unwrap_or(0);
        if quantity > available {
            return Err(format!("出庫数量が出庫可能数（{available}）を超えています"));
        }
    }
    // Read-only evidence is never sent to a stock mutation.
    *body = json!({"quantity":quantity,"note":body["note"]});
    Ok(())
}
fn prepare_receive(body: &mut Value, original: &Value) -> Result<(), String> {
    prepare_stock(body, original, "receive")
}
fn prepare_issue(body: &mut Value, original: &Value) -> Result<(), String> {
    prepare_stock(body, original, "issue")
}
fn prepare_adjust(body: &mut Value, original: &Value) -> Result<(), String> {
    prepare_stock(body, original, "adjust")
}
fn stock_preflight(original: &Value, fresh: &Value) -> Result<(), String> {
    if !product_has_stock_operations(fresh) {
        return Err("商品が在庫操作の対象外に変わっています。読み直してください".into());
    }
    same_fields(
        original,
        fresh,
        &["id", "name", "kind", "skuCode", "stock"],
        "商品または在庫",
    )
}
fn mutate_stock(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
    operation: &str,
) -> Result<Value, String> {
    let id = text(original, "id", "商品")?;
    let product = api::fetch_record(
        token,
        tenant,
        &format!("/v1/order/products/{}", api::encode_query(id)),
    )?;
    let stock_path = format!("/v1/erp/commerce/products/{}/stock", api::encode_query(id));
    let stock = api::fetch_optional_record(token, tenant, &stock_path)?;
    let fresh = product_inventory_record(id, &product, stock.as_ref())?;
    stock_preflight(original, &fresh)?;
    let response = api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{stock_path}/{operation}"),
        body,
        None,
    )?;
    validate_product_stock(&response, id)?;
    Ok(response)
}
fn receive_stock(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    mutate_stock(token, tenant, body, original, "receive")
}
fn issue_stock(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    mutate_stock(token, tenant, body, original, "issue")
}
fn adjust_stock(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    mutate_stock(token, tenant, body, original, "adjust")
}

fn named_location(locations: &[Value], id: &Value) -> String {
    locations
        .iter()
        .find(|location| location["id"] == *id)
        .and_then(|location| location["name"].as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("名称を確認できません")
        .into()
}
fn load_transfers(token: &str, tenant: &Tenant, _: &Value) -> Result<Value, String> {
    let mut transfers = fetch_items(token, tenant, TRANSFERS)?;
    let locations = fetch_items(token, tenant, LOCATIONS_PATH)?;
    for transfer in &mut transfers {
        transfer["fromLocationName"] =
            json!(named_location(&locations, &transfer["fromLocationId"]));
        transfer["toLocationName"] = json!(named_location(&locations, &transfer["toLocationId"]));
    }
    Ok(json!({"items":transfers}))
}
fn prepare_transfer(body: &mut Value, _: &Value) -> Result<(), String> {
    confirmed(body)?;
    trim_text(body, "sku", "SKU", 1, 255)?;
    trim_text(body, "reason", "移動理由", 2, 500)?;
    positive_quantity(body, SAFE_INTEGER)?;
    let from = text(body, "fromLocationId", "移動元")?;
    let to = text(body, "toLocationId", "移動先")?;
    if from == to {
        return Err("移動元と移動先は別の拠点を選択してください".into());
    }
    Ok(())
}
fn validate_transfer_locations(body: &Value, locations: &[Value]) -> Result<(), String> {
    for (key, label) in [("fromLocationId", "移動元"), ("toLocationId", "移動先")] {
        let chosen = locations
            .iter()
            .find(|location| location["id"] == body[key])
            .ok_or_else(|| format!("{label}の拠点が見つかりません。読み直してください"))?;
        if chosen["stockHolding"].as_bool() != Some(true) {
            return Err(format!("{label}は在庫を持つ拠点を選択してください"));
        }
    }
    Ok(())
}
fn create_transfer(token: &str, tenant: &Tenant, body: &Value, _: &Value) -> Result<Value, String> {
    let locations = fetch_items(token, tenant, LOCATIONS_PATH)?;
    validate_transfer_locations(body, &locations)?;
    text(body, "idempotencyKey", "再送識別情報")?;
    // The server checks stock atomically and replays the same key before applying
    // a second movement. Do not reject a retry using stock already reduced by it.
    let response = api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{TRANSFERS}/idempotent"),
        body,
        None,
    )?;
    same_fields(
        body,
        &response,
        &[
            "sku",
            "fromLocationId",
            "toLocationId",
            "quantity",
            "reason",
        ],
        "在庫移動の応答",
    )?;
    text(&response, "id", "移動履歴")?;
    if response["status"] != "completed" {
        return Err("移動の完了状態を確認できません。履歴を読み直してください".into());
    }
    Ok(response)
}

fn load_stocktakes(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let resource = Resource::new("stocktake-list", GROUP, "棚卸", STOCKTAKES, "items")
        .pagination(Pagination::Page { size: 20 });
    let mut collection = model::fetch_collection(token, tenant, resource, filters)?;
    let warehouses = fetch_items(token, tenant, WAREHOUSES_PATH)?;
    for record in &mut collection.rows {
        record["warehouseName"] = json!(named_location(&warehouses, &record["warehouseId"]));
    }
    Ok(json!({"items":collection.rows,"truncated":collection.truncated}))
}
fn counting(record: &Value) -> bool {
    record["status"] == "counting"
}
fn submitted(record: &Value) -> bool {
    record["status"] == "submitted"
}
fn open_stocktake(record: &Value) -> bool {
    counting(record) || submitted(record)
}
fn stocktake_version(original: &Value) -> Result<u64, String> {
    original["version"]
        .as_u64()
        .filter(|version| *version <= u32::MAX.into())
        .ok_or_else(|| "棚卸の版を確認できません。読み直してください".into())
}
fn prepare_start_stocktake(body: &mut Value, _: &Value) -> Result<(), String> {
    text(body, "warehouseId", "倉庫")?;
    let date = text(body, "countDate", "棚卸日")?;
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| "棚卸日を正しく入力してください")?;
    if !matches!(
        body["cadence"].as_str(),
        Some("daily" | "weekly" | "monthly" | "adhoc")
    ) {
        return Err("棚卸の周期を選択してください".into());
    }
    body["note"] = normalized_optional(&body["note"], 1024, "メモ")?;
    Ok(())
}
fn start_stocktake(token: &str, tenant: &Tenant, body: &Value, _: &Value) -> Result<Value, String> {
    let warehouses = fetch_items(token, tenant, WAREHOUSES_PATH)?;
    if !warehouses
        .iter()
        .any(|warehouse| warehouse["id"] == body["warehouseId"])
    {
        return Err("倉庫が見つかりません。読み直してください".into());
    }
    let response = api::mutate_record(token, tenant, "POST", STOCKTAKES, body, None)?;
    same_fields(
        body,
        &response,
        &["warehouseId", "countDate", "cadence"],
        "棚卸の応答",
    )?;
    check_stocktake_response(&response, "counting")?;
    Ok(response)
}
fn normalize_counts(value: &Value) -> Result<(Value, Option<i64>), String> {
    let counts = if value.is_null() {
        &[][..]
    } else {
        value
            .as_array()
            .ok_or("実数の形式を確認できません")?
            .as_slice()
    };
    if counts.len() > 32 {
        return Err("保管場所は1品目につき32か所までです".into());
    }
    let mut seen = HashSet::new();
    let mut total = 0_i64;
    let mut normalized = Vec::with_capacity(counts.len());
    for count in counts {
        let area = count["storageArea"]
            .as_str()
            .ok_or("保管場所を確認できません")?
            .trim();
        if area.chars().count() > 255 {
            return Err("保管場所は255文字以内で入力してください".into());
        }
        if !seen.insert(area.to_owned()) {
            return Err("同じ保管場所が重複しています".into());
        }
        let quantity = count["quantity"]
            .as_i64()
            .filter(|quantity| (0..=SAFE_INTEGER).contains(quantity))
            .ok_or("実数は0以上の安全に扱える整数で入力してください")?;
        total = total
            .checked_add(quantity)
            .filter(|total| *total <= SAFE_INTEGER)
            .ok_or("実数の合計が大きすぎます")?;
        normalized.push(json!({"storageArea":area,"quantity":quantity}));
    }
    Ok((
        Value::Array(normalized),
        (!counts.is_empty()).then_some(total),
    ))
}
fn normalized_variance(
    line: &Value,
    total: Option<i64>,
    theoretical: i64,
) -> Result<Value, String> {
    let note = normalized_optional(&line["variance"]["note"], 1024, "差異のメモ")?;
    if total.is_none_or(|total| total == theoretical) {
        return Ok(json!({"reason":null,"note":null}));
    }
    let reason = line["variance"]["reason"].as_str().unwrap_or("");
    if reason.is_empty() {
        return Ok(json!({"reason":null,"note":null}));
    }
    if !REASONS.iter().any(|choice| choice.value == reason) {
        return Err("差異理由を選び直してください".into());
    }
    if reason == "other" && note.is_null() {
        return Err("差異理由が「その他」のときはメモを入力してください".into());
    }
    Ok(json!({"reason":reason,"note":note}))
}
fn prepare_counts(body: &mut Value, original: &Value) -> Result<(), String> {
    if !counting(original) {
        return Err("実数を編集できるのは計数中の棚卸です".into());
    }
    let version = stocktake_version(original)?;
    let original_lines = rows(original, "lines", "棚卸明細")?;
    let input = rows(body, "lines", "入力した明細")?;
    if input.len() != original_lines.len() {
        return Err("棚卸の品目が変わっています。読み直してください".into());
    }
    let mut edits = Vec::new();
    let mut seen = HashSet::new();
    for line in input {
        let id = text(line, "lineId", "棚卸の品目")?;
        if !seen.insert(id) {
            return Err("同じ棚卸明細が重複しています".into());
        }
        let prior = original_lines
            .iter()
            .find(|prior| prior["id"].as_str() == Some(id))
            .ok_or("元の棚卸明細を確認できません")?;
        let theoretical = prior["theoreticalQuantity"]
            .as_i64()
            .ok_or("帳簿数量を確認できません")?;
        let (counts, total) = normalize_counts(&line["counts"])?;
        let (old_counts, _) = normalize_counts(&prior["counts"])?;
        let variance = normalized_variance(line, total, theoretical)?;
        let old_variance = json!({"reason":prior["varianceReason"],"note":prior["varianceNote"]});
        let mut edit = json!({"lineId":id});
        if counts != old_counts {
            edit["counts"] = counts;
        }
        if variance != old_variance {
            edit["variance"] = variance;
        }
        if edit.as_object().is_some_and(|edit| edit.len() > 1) {
            edits.push(edit);
        }
    }
    if edits.is_empty() {
        return Err("実数・差異理由に変更がありません".into());
    }
    if edits.len() > 2000 {
        return Err("一度に保存できる品目は2000件までです".into());
    }
    *body = json!({"version":version,"lines":edits});
    Ok(())
}
fn prepare_transition(body: &mut Value, original: &Value) -> Result<(), String> {
    confirmed(body)?;
    *body = json!({"version":stocktake_version(original)?});
    Ok(())
}
fn ready_to_submit(original: &Value) -> Result<(), String> {
    let lines = rows(original, "lines", "棚卸明細")?;
    if lines.is_empty() {
        return Err("棚卸明細を確認できません。読み直してください".into());
    }
    for line in lines {
        let sku = text(line, "sku", "SKU")?;
        let (_, total) = normalize_counts(&line["counts"])?;
        let total = total.ok_or_else(|| {
            format!("{sku} の実数が未入力です。0個の場合も実数0を保存してください")
        })?;
        let theoretical = line["theoreticalQuantity"]
            .as_i64()
            .ok_or("帳簿数量を確認できません")?;
        if total != theoretical {
            let reason = line["varianceReason"].as_str().unwrap_or("");
            if !REASONS.iter().any(|choice| choice.value == reason) {
                return Err(format!("{sku} の差異理由を保存してから提出してください"));
            }
            if reason == "other"
                && normalized_optional(&line["varianceNote"], 1024, "差異のメモ")?.is_null()
            {
                return Err(format!("{sku} の差異メモを入力してください"));
            }
        }
    }
    Ok(())
}
fn prepare_submission(body: &mut Value, original: &Value) -> Result<(), String> {
    ready_to_submit(original)?;
    prepare_transition(body, original)
}
fn stocktake_preflight(body: &Value, original: &Value, fresh: &Value) -> Result<(), String> {
    stocktake_version(fresh)?;
    text(original, "id", "棚卸")?;
    same_fields(
        original,
        fresh,
        &["id", "version", "status", "warehouseId", "countDate"],
        "棚卸",
    )?;
    if body["version"] != original["version"] {
        return Err("棚卸の版が一致しません。読み直してください".into());
    }
    Ok(())
}
fn check_stocktake_response(response: &Value, status: &str) -> Result<(), String> {
    text(response, "id", "棚卸")?;
    stocktake_version(response)?;
    rows(response, "lines", "棚卸明細")?;
    if response["status"].as_str() != Some(status) {
        return Err("棚卸の更新結果を確認できません。一覧を読み直してください".into());
    }
    Ok(())
}
fn stocktake_error(error: String) -> String {
    if error.contains("409") || error.to_ascii_lowercase().contains("conflict") {
        "棚卸または帳簿在庫が更新されています。再読み込みしてください。在庫が動いていた場合は、差戻し後に帳簿数量を取り直して数え直してください。".into()
    } else {
        error
    }
}
fn execute_stocktake(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
    action: Option<&str>,
) -> Result<Value, String> {
    let detail = path_for("/v1/field/inventory/stocktakes/{id}", original)?;
    let fresh = api::fetch_record(token, tenant, &detail)?;
    stocktake_preflight(body, original, &fresh)?;
    let expected = match action {
        None if counting(&fresh) => "counting",
        Some("submit") if counting(&fresh) => {
            ready_to_submit(&fresh)?;
            "submitted"
        }
        Some("refresh") if counting(&fresh) => "counting",
        Some("reopen") if submitted(&fresh) => "counting",
        Some("finalize") if submitted(&fresh) => {
            ready_to_submit(&fresh)?;
            "finalized"
        }
        Some("cancel") if open_stocktake(&fresh) => "cancelled",
        _ => return Err("棚卸の状態が変わっています。読み直してください".into()),
    };
    let path = action.map_or_else(|| detail.clone(), |action| format!("{detail}/{action}"));
    let response = api::mutate_record(
        token,
        tenant,
        if action.is_some() { "POST" } else { "PUT" },
        &path,
        body,
        None,
    )
    .map_err(stocktake_error)?;
    check_stocktake_response(&response, expected)?;
    if response["id"] != original["id"] {
        return Err("棚卸の応答が一致しません。読み直してください".into());
    }
    Ok(response)
}
fn save_counts(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    execute_stocktake(token, tenant, body, original, None)
}
fn submit_stocktake(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    execute_stocktake(token, tenant, body, original, Some("submit"))
}
fn reopen_stocktake(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    execute_stocktake(token, tenant, body, original, Some("reopen"))
}
fn finalize_stocktake(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    execute_stocktake(token, tenant, body, original, Some("finalize"))
}
fn refresh_stocktake(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    execute_stocktake(token, tenant, body, original, Some("refresh"))
}
fn cancel_stocktake(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    execute_stocktake(token, tenant, body, original, Some("cancel"))
}

fn po_submittable(original: &Value) -> bool {
    matches!(original["status"].as_str(), Some("Draft" | "Remanded"))
}
fn prepare_po_submission(body: &mut Value, original: &Value) -> Result<(), String> {
    if !po_submittable(original) {
        return Err("下書き・差戻しの発注書だけ申請できます".into());
    }
    confirmed(body)?;
    *body = json!({});
    Ok(())
}
fn po_submission_preflight(original: &Value, fresh: &Value) -> Result<(), String> {
    if !po_submittable(fresh) {
        return Err("発注書はすでに申請済み、または申請できない状態です".into());
    }
    text(original, "id", "発注書")?;
    text(original, "updatedAt", "発注書の更新日時")?;
    if rows(original, "items", "発注明細")?.is_empty() {
        return Err("発注明細を確認してください".into());
    }
    same_fields(
        original,
        fresh,
        &[
            "id",
            "status",
            "updatedAt",
            "vendorId",
            "vendorName",
            "currency",
            "purchaseOrderNumber",
            "items",
            "subtotalAmount",
            "taxAmount",
            "totalAmount",
            "taxCategory",
            "expectedDeliveryDate",
            "notes",
        ],
        "発注内容",
    )
}
fn po_approval_body(order: &Value, workflows: Option<&Value>) -> Result<Value, String> {
    let id = text(order, "id", "発注書")?;
    if order["status"] == "Remanded" {
        let workflows = workflows.ok_or("差戻しの承認情報を取得してください")?;
        let workflow = rows(workflows, "items", "承認情報")?
            .first()
            .ok_or("差戻しの承認情報が見つかりません。読み直してください")?;
        if workflow["resourceId"].as_str() != Some(id)
            || workflow["resourceType"] != "purchase_order"
            || workflow["status"] != "Remanded"
        {
            return Err("差戻しの承認情報が一致しません。読み直してください".into());
        }
        Ok(json!({"action":"submit","id":text(workflow,"id","承認情報")?}))
    } else if order["status"] == "Draft" {
        Ok(json!({"action":"submit","resourceId":id,"resourceType":"purchase_order"}))
    } else {
        Err("この発注書は承認を申請できません".into())
    }
}
fn submit_purchase_order(
    token: &str,
    tenant: &Tenant,
    _: &Value,
    original: &Value,
) -> Result<Value, String> {
    let path = path_for("/v1/erp/purchase-orders/{id}", original)?;
    let fresh = api::fetch_record(token, tenant, &path)?;
    po_submission_preflight(original, &fresh)?;
    let id = text(&fresh, "id", "発注書")?;
    let workflows = if fresh["status"] == "Remanded" {
        Some(api::fetch_record(
            token,
            tenant,
            &format!(
                "/api/approvals?limit=1&resourceId={}&resourceType=purchase_order&status=Remanded",
                api::encode_query(id)
            ),
        )?)
    } else {
        None
    };
    let payload = po_approval_body(&fresh, workflows.as_ref())?;
    let response = api::mutate_record(token, tenant, "POST", "/api/approvals", &payload, None)?;
    if response["resourceId"].as_str() != Some(id)
        || response["resourceType"] != "purchase_order"
        || response["status"] != "Requested"
    {
        return Err("承認申請の結果を確認できません。発注書と判断待ちを読み直してください".into());
    }
    text(&response, "id", "承認申請")?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stocktake() -> Value {
        json!({"id":"take-a","warehouseId":"warehouse-a","warehouseName":"本店倉庫","countDate":"2026-11-19","version":7,"status":"counting","lines":[
            {"id":"line-a","sku":"豆","theoreticalQuantity":10,"unitCostAmount":"123.4567","counts":null,"countedQuantity":null,"varianceReason":null,"varianceNote":null},
            {"id":"line-b","sku":"牛乳","theoreticalQuantity":2,"unitCostAmount":"99.875","counts":[{"storageArea":"冷蔵庫","quantity":2}],"countedQuantity":2,"varianceReason":null,"varianceNote":null}
        ]})
    }
    fn edit() -> Mutation {
        decorate(Resource::new(
            "inventory-stocktakes",
            GROUP,
            "",
            STOCKTAKES,
            "items",
        ))
        .edit
        .unwrap()
    }
    #[test]
    fn count_edit_distinguishes_zero_from_uncounted_and_preserves_line_identity() {
        let original = stocktake();
        let operation = edit();
        let mut draft = model::draft(operation.fields, &original);
        assert_eq!(draft["lines"][0]["counts"], json!([]));
        draft["lines"][0]["counts"] = json!([{"storageArea":"棚","quantity":"0"}]);
        draft["lines"][0]["variance"] = json!({"reason":"missing","note":"確認済み"});
        let payload = model::request_body(operation, &draft, &original, "unused").unwrap();
        assert_eq!(
            payload,
            json!({"version":7,"lines":[{"lineId":"line-a","counts":[{"storageArea":"棚","quantity":0}],"variance":{"reason":"missing","note":"確認済み"}}]})
        );
        assert_eq!(original["lines"][0]["unitCostAmount"], "123.4567");
        draft["lines"].as_array_mut().unwrap().reverse();
        assert_eq!(
            model::request_body(operation, &draft, &original, "unused").unwrap(),
            payload
        );
        draft["lines"][0]["lineId"] = json!("line-a");
        assert!(model::request_body(operation, &draft, &original, "unused").is_err());
    }
    #[test]
    fn deleting_all_counts_resets_uncounted_and_clears_old_variance() {
        let mut original = stocktake();
        original["lines"][0]["counts"] = json!([{"storageArea":"棚","quantity":8}]);
        original["lines"][0]["varianceReason"] = json!("missing");
        original["lines"][0]["varianceNote"] = json!("前回のメモ");
        let operation = edit();
        let mut draft = model::draft(operation.fields, &original);
        draft["lines"][0]["counts"] = json!([]);
        let body = model::request_body(operation, &draft, &original, "unused").unwrap();
        assert_eq!(
            body["lines"][0],
            json!({"lineId":"line-a","counts":[],"variance":{"reason":null,"note":null}})
        );
        draft["lines"][0]["counts"] = json!([{"storageArea":"棚","quantity":"10"}]);
        assert_eq!(
            model::request_body(operation, &draft, &original, "unused").unwrap()["lines"][0]["variance"],
            json!({"reason":null,"note":null})
        );
    }
    #[test]
    fn count_edit_rejects_missing_rows_duplicate_areas_negative_and_oversized_totals() {
        let original = stocktake();
        let operation = edit();
        let mut draft = model::draft(operation.fields, &original);
        draft["lines"].as_array_mut().unwrap().pop();
        assert!(model::request_body(operation, &draft, &original, "unused").is_err());
        for counts in [
            json!([{"storageArea":"棚","quantity":1},{"storageArea":" 棚 ","quantity":2}]),
            json!([{"storageArea":"棚","quantity":-1}]),
            json!([{"storageArea":"棚","quantity":SAFE_INTEGER},{"storageArea":"冷蔵庫","quantity":1}]),
        ] {
            assert!(normalize_counts(&counts).is_err());
        }
    }
    #[test]
    fn submission_requires_all_counts_and_explained_variance() {
        let mut original = stocktake();
        assert!(ready_to_submit(&original).is_err());
        original["lines"][0]["counts"] = json!([{"storageArea":"棚","quantity":0}]);
        assert!(ready_to_submit(&original).is_err());
        original["lines"][0]["varianceReason"] = json!("other");
        assert!(ready_to_submit(&original).is_err());
        original["lines"][0]["varianceNote"] = json!("記録を照合して所在を確認した");
        assert!(ready_to_submit(&original).is_ok());
        let mut payload = json!({"confirmed":true});
        prepare_submission(&mut payload, &original).unwrap();
        assert_eq!(payload, json!({"version":7}));
    }
    #[test]
    fn stocktake_transition_checks_version_and_available_status() {
        let original = stocktake();
        let mut fresh = original.clone();
        fresh["version"] = json!(8);
        assert!(stocktake_preflight(&json!({"version":7}), &original, &fresh).is_err());
        fresh = original.clone();
        fresh["status"] = json!("submitted");
        assert!(stocktake_preflight(&json!({"version":7}), &original, &fresh).is_err());
        let resource = decorate(Resource::new(
            "inventory-stocktakes",
            GROUP,
            "",
            STOCKTAKES,
            "items",
        ));
        assert!(
            !resource
                .actions
                .iter()
                .find(|op| op.path.ends_with("/finalize"))
                .unwrap()
                .is_available(&original)
        );
        assert!(
            resource
                .actions
                .iter()
                .find(|op| op.path.ends_with("/finalize"))
                .unwrap()
                .is_available(&fresh)
        );
        fresh["status"] = json!("finalized");
        assert!(
            resource
                .actions
                .iter()
                .all(|operation| !operation.is_available(&fresh))
        );
    }
    #[test]
    fn transfer_uses_location_identity_and_retains_idempotency_on_equal_retry() {
        let operation = decorate(Resource::new(
            "inventory-transfers",
            GROUP,
            "",
            TRANSFERS,
            "items",
        ))
        .create
        .unwrap();
        let draft = json!({"sku":" BEAN ","fromLocationId":"location-a","toLocationId":"location-b","quantity":"3","reason":" 店舗補充 ","confirmed":true});
        let body = model::request_body(operation, &draft, &Value::Null, "same-key").unwrap();
        assert_eq!(
            body,
            json!({"sku":"BEAN","fromLocationId":"location-a","toLocationId":"location-b","quantity":3,"reason":"店舗補充","idempotencyKey":"same-key"})
        );
        assert_eq!(
            model::request_body(operation, &draft, &Value::Null, "same-key").unwrap(),
            body
        );
        let locations = vec![
            json!({"id":"location-a","referenceId":"warehouse-a","stockHolding":true}),
            json!({"id":"location-b","referenceId":"store-b","stockHolding":true}),
        ];
        validate_transfer_locations(&body, &locations).unwrap();
        let mut wrong = body.clone();
        wrong["fromLocationId"] = json!("warehouse-a");
        assert!(validate_transfer_locations(&wrong, &locations).is_err());
        let mut same = draft.clone();
        same["toLocationId"] = same["fromLocationId"].clone();
        assert!(model::request_body(operation, &same, &Value::Null, "same-key").is_err());
    }
    #[test]
    fn commerce_adjust_is_signed_delta_and_issue_respects_available_stock() {
        let original = json!({"id":"product-a","name":"商品","kind":"PHYSICAL","skuCode":"A","stock":{"quantityOnHand":10,"quantityAvailable":7,"quantityReserved":3}});
        let mut body = json!({"quantity":-2,"note":"棚卸調整","confirmed":true});
        prepare_adjust(&mut body, &original).unwrap();
        assert_eq!(body, json!({"quantity":-2,"note":"棚卸調整"}));
        for quantity in [0, -1, 8, i64::from(i32::MAX) + 1] {
            assert!(
                prepare_issue(
                    &mut json!({"quantity":quantity,"note":"出庫記録","confirmed":true}),
                    &original
                )
                .is_err()
            );
        }
        assert!(
            prepare_issue(
                &mut json!({"quantity":7,"note":"出庫記録","confirmed":true}),
                &original
            )
            .is_ok()
        );
        assert!(
            prepare_adjust(
                &mut json!({"quantity":1,"note":"棚卸調整","confirmed":false}),
                &original
            )
            .is_err()
        );
        // An ERP stock-level row is never a product-stock identity.
        assert!(!product_has_stock_operations(
            &json!({"id":"level-a","stockItemId":"item-a","sku":"A","quantityOnHand":10})
        ));
    }
    #[test]
    fn product_stock_preflight_blocks_stale_counts_and_supports_unregistered_stock() {
        let product = json!({"id":"product-a","kind":"PHYSICAL","name":"商品","skuCode":"A"});
        let original = product_inventory_record("product-a", &product, None).unwrap();
        stock_preflight(&original, &original).unwrap();
        let stock = json!({"productId":"product-a","quantityOnHand":2,"quantityReserved":0,"quantityAvailable":2,"updatedAt":"new"});
        let fresh = product_inventory_record("product-a", &product, Some(&stock)).unwrap();
        assert!(stock_preflight(&original, &fresh).is_err());
        let mut changed = fresh.clone();
        changed["stock"]["quantityReserved"] = json!(1);
        assert!(stock_preflight(&fresh, &changed).is_err());
        assert!(product_inventory_record("other-product", &product, None).is_err());
    }
    #[test]
    fn po_resubmission_reuses_matching_remanded_workflow_and_checks_fresh_contents() {
        let original = json!({"id":"po-a","status":"Draft","updatedAt":"old","items":[{"unitCost":199,"quantity":2}],"currency":"USD","totalAmount":398});
        assert_eq!(
            po_approval_body(&original, None).unwrap(),
            json!({"action":"submit","resourceId":"po-a","resourceType":"purchase_order"})
        );
        let mut remanded = original.clone();
        remanded["status"] = json!("Remanded");
        let response = json!({"items":[{"id":"approval-a","resourceId":"po-a","resourceType":"purchase_order","status":"Remanded"}]});
        assert_eq!(
            po_approval_body(&remanded, Some(&response)).unwrap(),
            json!({"action":"submit","id":"approval-a"})
        );
        assert!(po_approval_body(&remanded, Some(&json!({"items":[]}))).is_err());
        let mut wrong = response.clone();
        wrong["items"][0]["resourceId"] = json!("other-po");
        assert!(po_approval_body(&remanded, Some(&wrong)).is_err());
        let mut fresh = original.clone();
        fresh["items"][0]["unitCost"] = json!(200);
        assert!(po_submission_preflight(&original, &fresh).is_err());
        po_submission_preflight(&original, &original).unwrap();
    }
}
