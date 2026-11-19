//! Inventory, procurement and accounting management contracts.
//!
//! These definitions describe the public Field REST interface. Money that the
//! service represents as decimal strings stays a string throughout editing.

use super::schema::*;
use serde_json::{Value, json};

const fn inventory_mutation(
    label: &'static str,
    method: &'static str,
    path: &'static str,
    fields: &'static [Field],
) -> Mutation {
    Mutation::new(label, method, path, fields).capability("inventory.manage")
}

const INVENTORY: &str = "商品・在庫管理";
const FINANCE: &str = "請求・債権管理";
const ACCOUNTING: &str = "会計・監査";
const UNITS: &[Choice] = &[
    Choice::new("g", "g"),
    Choice::new("ml", "ml"),
    Choice::new("piece", "個"),
];
const INGREDIENTS: Lookup = Lookup {
    path: "/v1/field/inventory/recipe-cost/ingredients",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const RECIPES: Lookup = Lookup {
    path: "/v1/field/inventory/recipe-cost/recipes",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const PRODUCTS: Lookup = Lookup {
    path: "/v1/order/products?limit=100",
    list_key: "items",
    value_key: "id",
    label_keys: &["name", "skuCode"],
};
const VENDORS: Lookup = Lookup {
    path: "/v1/erp/vendors?limit=100",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const CLIENTS: Lookup = Lookup {
    path: "/v1/erp/clients",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const ACCOUNTS: Lookup = Lookup {
    path: "/v1/erp/accounting/accounts",
    list_key: "items",
    value_key: "code",
    label_keys: &["name", "code"],
};
const EXPENSE_CATEGORIES: Lookup = Lookup {
    path: "/v1/erp/expense-categories",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};

const STORE_FIELDS: &[Field] = &[
    Field::text("name", "店舗名").required(),
    Field::new("address", "住所", InputKind::Multiline).empty(Empty::Null),
    Field::text("businessHours", "営業時間").empty(Empty::Null),
];
const VENDOR_FIELDS: &[Field] = &[
    Field::text("name", "仕入先名").required(),
    Field::new(
        "status",
        "状態",
        InputKind::Select(&[
            Choice::new("Active", "有効"),
            Choice::new("Inactive", "無効"),
        ]),
    )
    .required()
    .default("Active"),
    Field::text("contactName", "担当者名").empty(Empty::Null),
    Field::new("contactEmail", "メールアドレス", InputKind::Email).empty(Empty::Null),
    Field::text("contactPhone", "電話番号").empty(Empty::Null),
    Field::new("address", "住所", InputKind::Multiline).empty(Empty::Null),
    Field::text("paymentTerms", "支払条件").empty(Empty::Null),
    Field::text("qualifiedInvoiceIssuerNumber", "適格請求書発行事業者番号").empty(Empty::Null),
    Field::text("taxRegistrationName", "登録名義").empty(Empty::Null),
    Field::new("notes", "メモ", InputKind::Multiline).empty(Empty::Null),
];
const PRODUCT_FIELDS: &[Field] = &[
    Field::text("name", "商品名").required(),
    Field::new(
        "kind",
        "商品種別",
        InputKind::Select(&[
            Choice::new("PRODUCT", "商品"),
            Choice::new("PLAN", "プラン"),
            Choice::new("OPTION", "オプション"),
            Choice::new("API_SERVICE", "APIサービス"),
            Choice::new("SOFTWARE", "ソフトウェア"),
        ]),
    )
    .required()
    .default("PRODUCT"),
    Field::new("listPrice", "定価（円）", InputKind::Integer)
        .required()
        .default("0"),
    Field::text("skuCode", "SKU").empty(Empty::Null),
    Field::text("janCode", "JANコード").empty(Empty::Null),
    Field::text("upcCode", "UPCコード").empty(Empty::Null),
    Field::text("category", "カテゴリ").empty(Empty::Null),
    Field::text("subcategory", "サブカテゴリ").empty(Empty::Null),
    Field::new("description", "説明", InputKind::Multiline).empty(Empty::Null),
    Field::new(
        "billingCycle",
        "課金周期",
        InputKind::Select(&[
            Choice::new("ONCE", "単発"),
            Choice::new("DAILY", "毎日"),
            Choice::new("WEEKLY", "毎週"),
            Choice::new("MONTHLY", "毎月"),
            Choice::new("YEARLY", "毎年"),
            Choice::new("USAGE_BASED", "従量"),
        ]),
    )
    .required()
    .default("ONCE"),
    Field::new(
        "status",
        "状態",
        InputKind::Select(&[
            Choice::new("DRAFT", "下書き"),
            Choice::new("ACTIVE", "有効"),
            Choice::new("ARCHIVED", "アーカイブ"),
        ]),
    )
    .required()
    .default("DRAFT"),
    Field::new(
        "publicationStatus",
        "公開設定",
        InputKind::Select(&[
            Choice::new("PRIVATE", "非公開"),
            Choice::new("PUBLIC_USE_DEFAULT", "基本情報で公開"),
            Choice::new("PUBLIC", "公開用の情報で公開"),
        ]),
    )
    .required()
    .default("PRIVATE"),
    Field::text("publicationName", "公開用の商品名").empty(Empty::Null),
    Field::new(
        "publicationDescription",
        "公開用の説明",
        InputKind::Multiline,
    )
    .empty(Empty::Null),
];
const INGREDIENT_FIELDS: &[Field] = &[
    Field::text("name", "材料名").required(),
    Field::new("baseUnit", "基準単位", InputKind::Select(UNITS))
        .required()
        .default("g"),
    Field::new("yieldRate", "歩留まり", InputKind::Decimal)
        .empty(Empty::Null)
        .help("0より大きく1以下。90%なら0.9"),
    Field::new("gramsPerMl", "1mlあたりの重さ（g）", InputKind::Decimal).empty(Empty::Null),
    Field::new("note", "メモ", InputKind::Multiline).empty(Empty::Null),
];
const PACKAGE_FIELDS: &[Field] = &[
    Field::new("ingredientId", "材料", InputKind::Reference(INGREDIENTS)).required(),
    Field::text("productName", "商品名").required(),
    Field::new("contentAmount", "内容量", InputKind::Decimal).required(),
    Field::new("contentUnit", "単位", InputKind::Select(UNITS))
        .required()
        .default("g"),
    Field::new("piecesPerPack", "1パックの個数", InputKind::Integer)
        .required()
        .default("1"),
    Field::text("janCode", "JANコード").empty(Empty::Null),
];
const PRICE_FIELDS: &[Field] = &[
    Field::new("ingredientId", "材料", InputKind::Reference(INGREDIENTS)).required(),
    Field::new("purchasedOn", "購入日", InputKind::Date).required(),
    Field::new("amount", "購入金額（円）", InputKind::Decimal).required(),
    Field::new("quantity", "購入量（材料の基準単位）", InputKind::Decimal).required(),
    Field::new("amountIncludesTax", "税込金額", InputKind::Bool).default("true"),
    Field::new(
        "taxRateBp",
        "税率",
        InputKind::Select(&[Choice::new("800", "8%"), Choice::new("1000", "10%")]),
    )
    .required()
    .default("800"),
    Field::text("supplierName", "仕入先名").empty(Empty::Null),
];
const RECIPE_LINES: &[Field] = &[
    Field::new("ingredientId", "材料", InputKind::Reference(INGREDIENTS))
        .empty(Empty::Null)
        .help("材料または仕込み品のどちらかを選択"),
    Field::new("preparationId", "仕込み品", InputKind::Reference(RECIPES)).empty(Empty::Null),
    Field::new("amount", "使用量", InputKind::Decimal).required(),
    Field::new("unit", "単位", InputKind::Select(UNITS))
        .required()
        .default("g"),
    Field::new(
        "weightBasis",
        "量の基準",
        InputKind::Select(&[
            Choice::new("as_purchased", "買ったままの量"),
            Choice::new("edible_portion", "可食部の量"),
        ]),
    )
    .required()
    .default("as_purchased"),
    Field::text("optionKey", "追加オプションのキー")
        .empty(Empty::Null)
        .help("基本の材料なら空欄"),
    Field::text("note", "メモ").empty(Empty::Null),
];
const RECIPE_FIELDS: &[Field] = &[
    Field::text("name", "レシピ名").required(),
    Field::new(
        "kind",
        "種類",
        InputKind::Select(&[
            Choice::new("menu", "メニュー"),
            Choice::new("preparation", "仕込み品"),
        ]),
    )
    .required()
    .default("menu"),
    Field::new(
        "productId",
        "商品との紐付け",
        InputKind::Reference(PRODUCTS),
    )
    .empty(Empty::Null),
    Field::new(
        "sellingPriceExcludingTax",
        "税抜売価（円）",
        InputKind::Decimal,
    )
    .empty(Empty::Null),
    Field::new("yieldAmount", "仕込みの出来高", InputKind::Decimal).empty(Empty::Null),
    Field::new("yieldUnit", "出来高の単位", InputKind::Select(UNITS)).empty(Empty::Null),
    Field::new("lines", "材料", InputKind::Lines(RECIPE_LINES)),
    Field::new(
        "options",
        "追加オプション",
        InputKind::Lines(&[
            Field::text("key", "キー").required(),
            Field::text("label", "表示名").required(),
            Field::new(
                "additionalPriceExcludingTax",
                "追加の税抜売価（円）",
                InputKind::Decimal,
            )
            .empty(Empty::Null),
        ]),
    ),
    Field::new(
        "procedureSteps",
        "調理手順",
        InputKind::Lines(&[
            Field::new("text", "手順", InputKind::Multiline).required(),
            Field::text("imageUrl", "写真のURL").empty(Empty::Null),
        ]),
    ),
    Field::new("note", "メモ", InputKind::Multiline).empty(Empty::Null),
];
const PO_FIELDS: &[Field] = &[
    Field::new("vendorId", "仕入先", InputKind::Reference(VENDORS)).required(),
    Field::text("currency", "通貨").required().default("JPY"),
    Field::new("expectedDeliveryDate", "納品予定日", InputKind::Date).empty(Empty::Null),
    Field::new(
        "items",
        "発注明細",
        InputKind::Lines(&[
            Field::text("description", "品名").required(),
            Field::new("quantity", "数量", InputKind::Integer)
                .required()
                .default("1"),
            Field::new("unitCost", "単価", InputKind::Money).required(),
            Field::new("taxAmount", "明細の税額", InputKind::Money)
                .required()
                .default("0"),
        ]),
    )
    .required(),
    Field::new("notes", "備考", InputKind::Multiline).empty(Empty::Null),
];
const EXPENSE_FIELDS: &[Field] = &[
    Field::new("expenseDate", "日付", InputKind::Date).required(),
    Field::text("payee", "支払先").required(),
    Field::text("currency", "通貨").required().default("JPY"),
    Field::new("memo", "メモ", InputKind::Multiline).empty(Empty::String),
    Field::new(
        "lines",
        "経費明細",
        InputKind::Lines(&[
            Field::new("id", "", InputKind::Hidden),
            Field::new(
                "categoryId",
                "経費カテゴリ",
                InputKind::Reference(EXPENSE_CATEGORIES),
            )
            .required(),
            Field::text("description", "内容").empty(Empty::Null),
            Field::new("amount", "金額", InputKind::Money).required(),
            Field::new(
                "taxCategory",
                "税区分",
                InputKind::Select(&[
                    Choice::new("taxable_10", "課税10%"),
                    Choice::new("reduced_8", "軽減8%"),
                    Choice::new("non_taxable", "非課税"),
                    Choice::new("export_exempt", "免税"),
                    Choice::new("out_of_scope", "対象外"),
                ]),
            )
            .required()
            .default("taxable_10"),
            Field::new("taxAmount", "税額", InputKind::Money)
                .required()
                .default("0"),
            Field::new(
                "burdenSiteId",
                "負担する拠点",
                InputKind::Reference(Lookup {
                    path: "/v1/erp/master-control/organization",
                    list_key: "sites",
                    value_key: "id",
                    label_keys: &["name"],
                }),
            )
            .empty(Empty::Null),
        ]),
    )
    .required(),
];
const INVOICE_FIELDS: &[Field] = &[
    Field::new("clientId", "取引先", InputKind::Reference(CLIENTS)).required(),
    Field::text("clientName", "宛名").empty(Empty::Null),
    Field::new("clientEmail", "メールアドレス", InputKind::Email).empty(Empty::Null),
    Field::text("clientPhone", "電話番号").empty(Empty::Null),
    Field::text("currency", "通貨").required().default("JPY"),
    Field::new("dueDate", "支払期日", InputKind::Date).required(),
    Field::new(
        "lineItems",
        "請求明細",
        InputKind::Lines(&[
            Field::text("description", "品名・内容").required(),
            Field::new("quantity", "数量", InputKind::Integer)
                .required()
                .default("1"),
            Field::new("unitPrice", "単価", InputKind::Money).required(),
        ]),
    )
    .required(),
    Field::new("taxAmount", "合計税額", InputKind::Money)
        .required()
        .default("0"),
    Field::new("notes", "備考", InputKind::Multiline).empty(Empty::Null),
];
const CATEGORY_FIELDS: &[Field] = &[
    Field::text("name", "カテゴリ名").required(),
    Field::text("code", "コード").empty(Empty::Null),
    Field::new("description", "説明", InputKind::Multiline).empty(Empty::Null),
    Field::new("isActive", "有効", InputKind::Bool).default("true"),
];
const ACCOUNT_FIELDS: &[Field] = &[
    Field::text("code", "科目コード").required(),
    Field::text("name", "科目名").required(),
    Field::new(
        "accountType",
        "勘定区分",
        InputKind::Select(&[
            Choice::new("asset", "資産"),
            Choice::new("liability", "負債"),
            Choice::new("equity", "純資産"),
            Choice::new("revenue", "収益"),
            Choice::new("expense", "費用"),
        ]),
    )
    .required(),
    Field::new("active", "有効", InputKind::Bool).default("true"),
    Field::new("isControlAccount", "統制勘定", InputKind::Bool).default("false"),
    Field::text("taxCategory", "税区分").help("会社で使用している税区分を指定"),
];
const DATE_FILTERS: &[Field] = &[
    Field::new("date_from", "開始日", InputKind::Date).default("@monthStart"),
    Field::new("date_to", "終了日", InputKind::Date).default("@today"),
];

pub const RESOURCES: &[Resource] = &[
    Resource::new(
        "products",
        INVENTORY,
        "商品マスタ",
        "/v1/order/products",
        "items",
    )
    .route("products")
    .pagination(Pagination::Offset { limit: 50 })
    .title(&["name"])
    .subtitle(&["skuCode", "listPrice"])
    .status("status")
    .detail("/v1/order/products/{id}", "")
    .fields(&[
        ("商品名", "name"),
        ("種別", "kind"),
        ("定価（円）", "listPrice"),
        ("SKU", "skuCode"),
        ("カテゴリ", "category"),
        ("説明", "description"),
        ("課金周期", "billingCycle"),
        ("公開", "publicationStatus"),
        ("公開名", "publicationName"),
        ("公開の説明", "publicationDescription"),
        ("タグ", "tags"),
    ])
    .create(
        inventory_mutation("商品を登録", "POST", "/v1/order/products", PRODUCT_FIELDS)
            .prepare(prepare_product_create)
            .idempotency_body("idempotencyKey"),
    )
    .edit(
        inventory_mutation(
            "商品を編集",
            "PATCH",
            "/v1/order/products/{id}",
            PRODUCT_FIELDS,
        )
        .prepare(prepare_product_edit),
    )
    .actions(&[
        inventory_mutation("商品を削除", "DELETE", "/v1/order/products/{id}", &[])
            .destructive()
            .explain("この商品を削除します。利用状況によって削除できない場合があります。"),
    ]),
    Resource::new(
        "inventory",
        INVENTORY,
        "在庫管理",
        "/v1/field/inventory/stock-levels?include_stores=true",
        "items",
    )
    .route("inventory")
    .title(&["sku"])
    .subtitle(&["warehouseName", "quantityOnHand"])
    .fields(&[
        ("SKU", "sku"),
        ("在庫拠点", "warehouseName"),
        ("在庫数", "quantityOnHand"),
        ("更新日時", "lastUpdatedAt"),
    ]),
    Resource::new(
        "inventory-low-stock",
        INVENTORY,
        "低在庫",
        "/v1/field/inventory/low-stock-alerts",
        "items",
    )
    .route("inventory/low-stock")
    .id("stockItemId")
    .title(&["skuCode"])
    .subtitle(&["warehouseName", "quantityOnHand"])
    .status("decisionStatus")
    .fields(&[
        ("SKU", "skuCode"),
        ("在庫拠点", "warehouseName"),
        ("在庫数", "quantityOnHand"),
        ("安全在庫", "safetyStockQuantity"),
        ("発注点", "reorderPointQuantity"),
        ("不足数", "shortageQuantity"),
        ("推奨発注数", "recommendedOrderQuantity"),
        ("判断メモ", "decisionNote"),
    ])
    .actions(&[inventory_mutation(
        "補充の判断を記録",
        "PATCH",
        "/v1/field/inventory/low-stock-alerts/{id}/decision",
        &[
            Field::new(
                "status",
                "判断",
                InputKind::Select(&[
                    Choice::new("pending", "未判断"),
                    Choice::new("on_hold", "保留"),
                    Choice::new("purchase_candidate", "発注候補"),
                ]),
            )
            .read("decisionStatus")
            .required(),
            Field::new("note", "メモ", InputKind::Multiline)
                .read("decisionNote")
                .empty(Empty::Null),
        ],
    )]),
    Resource::new(
        "inventory-expiring-lots",
        INVENTORY,
        "期限が近いロット",
        "/v1/field/inventory/expiring-lots",
        "items",
    )
    .route("inventory/expiring-lots")
    .id("stockLotId")
    .title(&["sku", "lotNo"])
    .subtitle(&["warehouseName", "expiresAt"])
    .status("status")
    .filters(&[
        Field::new("as_of", "基準日時", InputKind::DateTime)
            .required()
            .default("@now"),
        Field::new("within_days", "何日先まで", InputKind::Integer)
            .required()
            .default("3"),
    ])
    .fields(&[
        ("SKU", "sku"),
        ("ロット番号", "lotNo"),
        ("在庫拠点", "warehouseName"),
        ("在庫数", "quantityOnHand"),
        ("期限", "expiresAt"),
        ("期限までの日数", "daysUntilExpiry"),
        ("単価", "unitCost"),
        ("見込金額", "estimatedAmount"),
    ]),
    Resource::new(
        "inventory-transfers",
        INVENTORY,
        "在庫移動",
        "/v1/field/inventory/stock-transfers",
        "items",
    )
    .route("inventory/transfers")
    .title(&["sku"])
    .subtitle(&["quantity", "transferredAt"])
    .status("status")
    .fields(&[
        ("SKU", "sku"),
        ("数量", "quantity"),
        ("移動理由", "reason"),
        ("移動日時", "transferredAt"),
    ]),
    Resource::new(
        "inventory-stocktakes",
        INVENTORY,
        "棚卸",
        "/v1/field/inventory/stocktakes",
        "items",
    )
    .route("inventory/stocktakes")
    .pagination(Pagination::Page { size: 20 })
    .title(&["countDate"])
    .subtitle(&["cadence", "totals.lineCount"])
    .status("status")
    .detail("/v1/field/inventory/stocktakes/{id}", "")
    .fields(&[
        ("棚卸日", "countDate"),
        ("周期", "cadence"),
        ("メモ", "note"),
        ("明細数", "totals.lineCount"),
        ("数えた明細数", "totals.countedLineCount"),
        ("帳簿金額", "totals.theoreticalValue"),
        ("実棚金額", "totals.countedValue"),
        ("差異金額", "totals.varianceValue"),
        ("確定日時", "finalizedAt"),
    ]),
    Resource::new(
        "inventory-locations",
        INVENTORY,
        "在庫拠点",
        "/v1/field/inventory/locations",
        "items",
    )
    .route("inventory/locations")
    .id("referenceId")
    .title(&["name"])
    .subtitle(&["kind"])
    .fields(&[
        ("拠点名", "name"),
        ("種別", "kind"),
        ("在庫を持つ", "stockHolding"),
    ])
    .create(inventory_mutation(
        "倉庫を登録",
        "POST",
        "/v1/field/inventory/warehouses",
        &[Field::text("name", "倉庫名").required()],
    ))
    .edit(
        inventory_mutation(
            "倉庫名を編集",
            "PUT",
            "/v1/field/inventory/warehouses/{id}",
            &[Field::text("name", "倉庫名").required()],
        )
        .when(is_warehouse),
    )
    .actions(&[inventory_mutation(
        "倉庫を削除",
        "DELETE",
        "/v1/field/inventory/warehouses/{id}",
        &[],
    )
    .when(is_warehouse)
    .destructive()]),
    Resource::new(
        "inventory-stores",
        INVENTORY,
        "店舗マスタ",
        "/v1/field/inventory/stores",
        "items",
    )
    .route("inventory/stores")
    .title(&["name"])
    .subtitle(&["address"])
    .detail("/v1/field/inventory/stores/{id}", "")
    .fields(&[
        ("店舗名", "name"),
        ("住所", "address"),
        ("営業時間", "businessHours"),
    ])
    .create(
        inventory_mutation(
            "店舗を登録",
            "POST",
            "/v1/field/inventory/stores",
            STORE_FIELDS,
        )
        .prepare(prepare_store),
    )
    .edit(
        inventory_mutation(
            "店舗を編集",
            "PUT",
            "/v1/field/inventory/stores/{id}",
            STORE_FIELDS,
        )
        .prepare(prepare_store),
    )
    .actions(&[inventory_mutation(
        "店舗を削除",
        "DELETE",
        "/v1/field/inventory/stores/{id}",
        &[],
    )
    .destructive()]),
    Resource::new("vendors", INVENTORY, "仕入先", "/v1/erp/vendors", "items")
        .route("erp/vendors")
        .pagination(Pagination::Offset { limit: 50 })
        .title(&["name"])
        .subtitle(&["contactName", "contactEmail"])
        .status("status")
        .detail("/v1/erp/vendors/{id}", "")
        .fields(&[
            ("仕入先名", "name"),
            ("担当者", "contactName"),
            ("メール", "contactEmail"),
            ("電話", "contactPhone"),
            ("住所", "address"),
            ("支払条件", "paymentTerms"),
            ("登録番号", "qualifiedInvoiceIssuerNumber"),
            ("メモ", "notes"),
        ])
        .create(
            inventory_mutation("仕入先を登録", "POST", "/v1/erp/vendors", VENDOR_FIELDS)
                .prepare(prepare_vendor),
        )
        .edit(
            inventory_mutation(
                "仕入先を編集",
                "PATCH",
                "/v1/erp/vendors/{id}",
                VENDOR_FIELDS,
            )
            .prepare(prepare_vendor),
        ),
    Resource::new(
        "purchase-orders",
        INVENTORY,
        "発注書",
        "/v1/erp/purchase-orders",
        "items",
    )
    .route("erp/purchase-orders")
    .pagination(Pagination::Offset { limit: 50 })
    .title(&["vendorName", "purchaseOrderNumber"])
    .subtitle(&["purchaseOrderNumber", "@money:totalAmount"])
    .status("status")
    .detail("/v1/erp/purchase-orders/{id}", "")
    .fields(&[
        ("発注番号", "purchaseOrderNumber"),
        ("仕入先", "vendorName"),
        ("通貨", "currency"),
        ("合計", "@money:totalAmount"),
        ("税額", "@money:taxAmount"),
        ("納品予定日", "expectedDeliveryDate"),
        ("明細", "items"),
        ("備考", "notes"),
    ])
    .create(
        inventory_mutation(
            "発注書の下書きを作成",
            "POST",
            "/v1/erp/purchase-orders",
            PO_FIELDS,
        )
        .prepare(prepare_po_create),
    )
    .edit(
        inventory_mutation(
            "下書きを編集",
            "PATCH",
            "/v1/erp/purchase-orders/{id}",
            PO_FIELDS,
        )
        .prepare(prepare_po_edit)
        .when(po_editable),
    ),
    Resource::new(
        "deliveries",
        INVENTORY,
        "納品・検収",
        "/v1/field/procurement/deliveries",
        "items",
    )
    .route("procurement/deliveries")
    .title(&["documentName"])
    .subtitle(&["supplierName", "uploadedAt"])
    .status("status")
    .detail("/v1/field/procurement/deliveries/{id}", "")
    .fields(&[
        ("書類名", "documentName"),
        ("仕入先", "supplierName"),
        ("倉庫", "warehouseName"),
        ("明細数", "lineCount"),
        ("合計数量", "totalQuantity"),
        ("確認が必要", "reviewRequired"),
        ("確認理由", "reviewReasons"),
        ("受領日時", "receivedAt"),
        ("メモ", "note"),
    ]),
    Resource::new(
        "ingredients",
        INVENTORY,
        "材料単価",
        "/v1/field/inventory/recipe-cost/ingredients",
        "items",
    )
    .route("erp/recipe-cost/ingredients")
    .title(&["name"])
    .subtitle(&["baseUnit", "latestPrice.unitCost"])
    .fields(&[
        ("材料名", "name"),
        ("基準単位", "baseUnit"),
        ("最新単価", "latestPrice.unitCost"),
        ("購入日", "latestPrice.purchasedOn"),
        ("仕入先", "latestPrice.supplierName"),
        ("歩留まり", "yieldRate"),
        ("1mlあたりの重さ", "gramsPerMl"),
        ("メモ", "note"),
    ])
    .create(
        inventory_mutation(
            "材料を登録",
            "POST",
            "/v1/field/inventory/recipe-cost/ingredients",
            INGREDIENT_FIELDS,
        )
        .prepare(prepare_ingredient),
    )
    .edit(
        inventory_mutation(
            "材料を編集",
            "PUT",
            "/v1/field/inventory/recipe-cost/ingredients/{id}",
            INGREDIENT_FIELDS,
        )
        .prepare(prepare_ingredient),
    )
    .actions(&[inventory_mutation(
        "材料をアーカイブ",
        "POST",
        "/v1/field/inventory/recipe-cost/ingredients/{id}/archive",
        &[],
    )
    .prepare(prepare_archive)
    .destructive()]),
    Resource::new(
        "ingredient-packages",
        INVENTORY,
        "材料の包装規格",
        "/v1/field/inventory/recipe-cost/package-specs",
        "items",
    )
    .route("erp/recipe-cost/ingredients")
    .title(&["productName"])
    .subtitle(&["contentAmount", "contentUnit"])
    .fields(&[
        ("商品名", "productName"),
        ("内容量", "contentAmount"),
        ("単位", "contentUnit"),
        ("1パックの個数", "piecesPerPack"),
        ("JANコード", "janCode"),
        ("確定日時", "confirmedAt"),
        ("廃止日時", "retiredAt"),
    ])
    .create(
        inventory_mutation(
            "包装規格を登録",
            "POST",
            "/v1/field/inventory/recipe-cost/package-specs",
            PACKAGE_FIELDS,
        )
        .prepare(prepare_package),
    )
    .actions(&[inventory_mutation(
        "包装規格を廃止",
        "POST",
        "/v1/field/inventory/recipe-cost/package-specs/{id}/retire",
        &[],
    )
    .when(package_active)
    .destructive()]),
    Resource::new(
        "ingredient-prices",
        INVENTORY,
        "仕入単価の記録",
        "/v1/field/inventory/recipe-cost/prices",
        "items",
    )
    .route("erp/recipe-cost/ingredients")
    .title(&["supplierName", "purchasedOn"])
    .subtitle(&["purchasedOn", "unitCost"])
    .fields(&[
        ("購入日", "purchasedOn"),
        ("仕入先", "supplierName"),
        ("購入金額", "lineAmount"),
        ("税抜金額", "amountExcludingTax"),
        ("基準単位の量", "quantityInBaseUnit"),
        ("単価", "unitCost"),
        ("置き換え日時", "supersededAt"),
    ])
    .create(
        inventory_mutation(
            "仕入単価を記録",
            "POST",
            "/v1/field/inventory/recipe-cost/prices",
            PRICE_FIELDS,
        )
        .prepare(prepare_price),
    )
    .actions(&[inventory_mutation(
        "この単価を無効にする",
        "POST",
        "/v1/field/inventory/recipe-cost/prices/{id}/supersede",
        &[],
    )
    .when(price_active)
    .destructive()]),
    Resource::new(
        "recipes",
        INVENTORY,
        "レシピ",
        "/v1/field/inventory/recipe-cost/recipes",
        "items",
    )
    .route("erp/recipe-cost/recipes")
    .title(&["name"])
    .subtitle(&["kind", "sellingPriceExcludingTax"])
    .detail("/v1/field/inventory/recipe-cost/recipes/{id}", "")
    .fields(&[
        ("レシピ名", "name"),
        ("種類", "kind"),
        ("版", "revision"),
        ("税抜売価", "sellingPriceExcludingTax"),
        ("出来高", "yieldAmount"),
        ("出来高の単位", "yieldUnit"),
        ("材料", "lines"),
        ("オプション", "options"),
        ("調理手順", "procedureSteps"),
        ("メモ", "note"),
    ])
    .create(
        inventory_mutation(
            "レシピを登録",
            "POST",
            "/v1/field/inventory/recipe-cost/recipes",
            RECIPE_FIELDS,
        )
        .prepare(prepare_recipe),
    )
    .edit(
        inventory_mutation(
            "レシピを編集",
            "PUT",
            "/v1/field/inventory/recipe-cost/recipes/{id}",
            RECIPE_FIELDS,
        )
        .prepare(prepare_recipe),
    )
    .actions(&[inventory_mutation(
        "レシピをアーカイブ",
        "POST",
        "/v1/field/inventory/recipe-cost/recipes/{id}/archive",
        &[],
    )
    .prepare(prepare_archive)
    .destructive()]),
    Resource::new(
        "recipe-estimates",
        INVENTORY,
        "メニュー原価",
        "/v1/field/inventory/recipe-cost/estimates",
        "items",
    )
    .route("erp/recipe-cost/estimates")
    .id("recipeId")
    .title(&["recipeName"])
    .subtitle(&["totalCost", "costRate"])
    .status("status")
    .detail("/v1/field/inventory/recipe-cost/recipes/{id}/estimate", "")
    .fields(&[
        ("レシピ名", "recipeName"),
        ("税抜売価", "sellingPriceExcludingTax"),
        ("理論原価", "totalCost"),
        ("原価率", "costRate"),
        ("売価と材料原価の差", "grossProfit"),
        ("材料ごとの原価", "lines"),
        ("未計算の材料", "missing"),
        ("追加オプション原価", "optionCosts"),
    ]),
    Resource::new(
        "food-loss",
        INVENTORY,
        "食品ロス",
        "/v1/field/inventory/food-loss/records",
        "items",
    )
    .route("erp/food-loss")
    .title(&["itemName"])
    .subtitle(&["occurredAt", "quantity", "unit"])
    .status("costStatus")
    .filters(&[
        Field::new("from", "開始日", InputKind::Date),
        Field::new("to", "終了日", InputKind::Date),
        Field::new("includeVoided", "取消済みも表示", InputKind::Bool),
    ])
    .fields(&[
        ("品名", "itemName"),
        ("発生日時", "occurredAt"),
        ("数量", "quantity"),
        ("単位", "unit"),
        ("理由", "reason"),
        ("段階", "stage"),
        ("原価", "costAmount"),
        ("原価の補足", "costMessage"),
        ("在庫への反映", "inventoryStatus"),
        ("担当者", "staffName"),
        ("メモ", "note"),
        ("取消日時", "voidedAt"),
    ])
    .create(
        inventory_mutation(
            "食品ロスを記録",
            "POST",
            "/v1/field/inventory/food-loss/records",
            &[
                Field::new(
                    "itemKind",
                    "種類",
                    InputKind::Select(&[
                        Choice::new("ingredient", "食材"),
                        Choice::new("preparation", "仕込み品"),
                        Choice::new("finished_product", "完成品"),
                    ]),
                )
                .required()
                .default("ingredient"),
                Field::new("ingredientId", "食材", InputKind::Reference(INGREDIENTS))
                    .empty(Empty::Null),
                Field::new("recipeId", "レシピ", InputKind::Reference(RECIPES)).empty(Empty::Null),
                Field::text("itemName", "品名").empty(Empty::Null),
                Field::new("occurredAt", "発生日時", InputKind::DateTime).required(),
                Field::new("quantity", "数量", InputKind::Decimal).required(),
                Field::new("unit", "単位", InputKind::Select(UNITS))
                    .required()
                    .default("g"),
                Field::new(
                    "reason",
                    "理由",
                    InputKind::Select(&[
                        Choice::new("overproduction", "仕込み余り"),
                        Choice::new("unsold", "売れ残り"),
                        Choice::new("expired", "期限切れ"),
                        Choice::new("quality_defect", "品質不良"),
                        Choice::new("remake", "作り直し"),
                        Choice::new("plate_waste", "食べ残し"),
                        Choice::new("other", "その他"),
                    ]),
                )
                .required(),
                Field::new(
                    "stage",
                    "段階",
                    InputKind::Select(&[
                        Choice::new("storage", "保管"),
                        Choice::new("preparation", "仕込み"),
                        Choice::new("cooking", "調理"),
                        Choice::new("display", "提供前"),
                        Choice::new("after_serving", "提供後"),
                    ]),
                )
                .empty(Empty::Null),
                Field::text("staffName", "担当者").empty(Empty::Null),
                Field::new("note", "メモ", InputKind::Multiline).empty(Empty::Null),
            ],
        )
        .prepare(prepare_food_loss)
        .idempotency_body("clientRequestId"),
    )
    .actions(&[inventory_mutation(
        "記録を取り消す",
        "POST",
        "/v1/field/inventory/food-loss/records/{id}/void",
        &[Field::new("reason", "取消理由", InputKind::Multiline).required()],
    )
    .when(loss_active)
    .destructive()]),
    Resource::new("invoices", FINANCE, "請求書", "/v1/invoices", "items")
        .route("invoices")
        .pagination(Pagination::Offset { limit: 50 })
        .title(&["clientName", "invoiceNumber"])
        .subtitle(&["invoiceNumber", "@money:totalAmount", "dueDate"])
        .status("status")
        .detail("/v1/invoices/{id}", "")
        .fields(&[
            ("請求番号", "invoiceNumber"),
            ("宛名", "clientName"),
            ("メール", "clientEmail"),
            ("電話", "clientPhone"),
            ("通貨", "currency"),
            ("合計", "@money:totalAmount"),
            ("税額", "@money:taxAmount"),
            ("支払期日", "dueDate"),
            ("明細", "lineItems"),
            ("備考", "notes"),
            ("送付日時", "sentAt"),
            ("入金日", "paidAt"),
        ])
        .create(
            Mutation::new(
                "請求書の下書きを作成",
                "POST",
                "/v1/invoices",
                INVOICE_FIELDS,
            )
            .prepare(prepare_invoice)
            .idempotency_body("idempotencyKey")
            .explain("下書きを保存します。メールやSMSは送信しません。"),
        )
        .edit(Mutation::new(
            "連絡先と備考を編集",
            "PATCH",
            "/v1/invoices/{id}",
            &[
                Field::new("clientEmail", "メールアドレス", InputKind::Email).empty(Empty::String),
                Field::new("notes", "備考", InputKind::Multiline).empty(Empty::String),
            ],
        )),
    Resource::new("expenses", ACCOUNTING, "経費", "/v1/erp/expenses", "items")
        .route("erp/expenses")
        .pagination(Pagination::Offset { limit: 50 })
        .title(&["payee", "memo"])
        .subtitle(&["expenseDate", "@money:amount"])
        .status("status")
        .detail("/v1/erp/expenses/{id}", "")
        .fields(&[
            ("日付", "expenseDate"),
            ("支払先", "payee"),
            ("金額", "@money:amount"),
            ("通貨", "currency"),
            ("明細", "lines"),
            ("メモ", "memo"),
            ("差戻し理由", "rejectionReason"),
        ])
        .create(
            Mutation::new(
                "経費の下書きを作成",
                "POST",
                "/v1/erp/expenses",
                EXPENSE_FIELDS,
            )
            .prepare(prepare_expense)
            .idempotency_body("clientRequestId"),
        )
        .edit(
            Mutation::new(
                "経費を編集",
                "PATCH",
                "/v1/erp/expenses/{id}",
                EXPENSE_FIELDS,
            )
            .prepare(prepare_expense)
            .when(expense_editable),
        )
        .actions(&[
            Mutation::new("承認を申請", "POST", "/v1/erp/expenses/{id}/submit", &[])
                .when(expense_editable)
                .explain("入力内容を確認して、承認を申請します。"),
            Mutation::new(
                "申請を取り下げる",
                "POST",
                "/v1/erp/expenses/{id}/withdraw",
                &[Field::new("reason", "理由", InputKind::Multiline)],
            )
            .when(expense_submitted)
            .actor_match("requestedBy")
            .destructive(),
        ]),
    Resource::new(
        "expense-categories",
        ACCOUNTING,
        "経費カテゴリ",
        "/v1/erp/expense-categories",
        "items",
    )
    .route("erp/expenses")
    .title(&["name"])
    .subtitle(&["code"])
    .fields(&[
        ("カテゴリ名", "name"),
        ("コード", "code"),
        ("説明", "description"),
        ("有効", "isActive"),
    ])
    .create(Mutation::new(
        "カテゴリを登録",
        "POST",
        "/v1/erp/expense-categories",
        CATEGORY_FIELDS,
    ))
    .edit(Mutation::new(
        "カテゴリを編集",
        "PATCH",
        "/v1/erp/expense-categories/{id}",
        CATEGORY_FIELDS,
    )),
    Resource::new(
        "journal-entries",
        ACCOUNTING,
        "仕訳帳",
        "/v1/erp/accounting/journal-entries",
        "items",
    )
    .route("accounting/ledger/journal")
    .pagination(Pagination::Offset { limit: 50 })
    .title(&["description"])
    .subtitle(&["entryDate", "debitTotal"])
    .status("status")
    .filters(DATE_FILTERS)
    .detail("/v1/erp/accounting/journal-entries/{id}", "")
    .fields(&[
        ("日付", "entryDate"),
        ("摘要", "description"),
        ("区分", "entryKind"),
        ("借方合計", "debitTotal"),
        ("貸方合計", "creditTotal"),
        ("明細", "lines"),
        ("訂正理由", "correctionReason"),
    ]),
    Resource::new(
        "account-master",
        ACCOUNTING,
        "勘定科目",
        "/v1/erp/accounting/accounts",
        "items",
    )
    .route("accounting/accounts")
    .id("code")
    .title(&["name"])
    .subtitle(&["code", "accountType"])
    .fields(&[
        ("科目コード", "code"),
        ("科目名", "name"),
        ("勘定区分", "accountType"),
        ("税区分", "taxCategory"),
        ("有効", "active"),
        ("統制勘定", "isControlAccount"),
        ("表示区分", "statementClassification.groupCode"),
    ])
    .create(Mutation::new(
        "勘定科目を登録",
        "POST",
        "/v1/erp/accounting/accounts",
        ACCOUNT_FIELDS,
    ))
    .edit(
        Mutation::new(
            "勘定科目を編集",
            "PUT",
            "/v1/erp/accounting/accounts/{id}",
            ACCOUNT_FIELDS,
        )
        .prepare(prepare_account)
        .when(account_editable),
    ),
    Resource::new(
        "sales-ledger",
        ACCOUNTING,
        "売上台帳",
        "/v1/sales-ledger?group_by=day",
        "items",
    )
    .route("erp/sales-ledger")
    .title(&["productName"])
    .subtitle(&["date", "totalAmount"])
    .status("status")
    .filters(DATE_FILTERS)
    .detail("/v1/sales-ledger/{id}", "")
    .fields(&[
        ("日付", "date"),
        ("商品名", "productName"),
        ("数量", "quantity"),
        ("単価", "unitPrice"),
        ("売上金額", "totalAmount"),
        ("決済方法", "paymentMethod"),
        ("メモ", "notes"),
    ]),
    Resource::new(
        "purchase-ledger",
        ACCOUNTING,
        "仕入台帳",
        "/v1/purchase-ledger",
        "items",
    )
    .route("erp/purchase-ledger")
    .title(&["itemName"])
    .subtitle(&["date", "supplierName", "totalCost"])
    .status("status")
    .filters(DATE_FILTERS)
    .detail("/v1/purchase-ledger/{id}", "")
    .fields(&[
        ("日付", "date"),
        ("品名", "itemName"),
        ("仕入先", "supplierName"),
        ("費目", "category"),
        ("数量", "quantity"),
        ("単価", "unitCost"),
        ("仕入金額", "totalCost"),
        ("メモ", "notes"),
    ]),
    Resource::new(
        "saas-subscriptions",
        FINANCE,
        "SaaS契約管理",
        "/v1/erp/saas-subscriptions",
        "items",
    )
    .route("saas-subscriptions")
    .title(&["serviceName"])
    .subtitle(&["currentPlan", "monthlyAmountYen"])
    .status("status")
    .detail("/v1/erp/saas-subscriptions/{id}", "")
    .fields(&[
        ("サービス", "serviceName"),
        ("プラン", "currentPlan"),
        ("月額（円）", "monthlyAmountYen"),
        ("席数", "seats"),
        ("担当チーム", "ownerTeam"),
        ("更新日", "renewalDate"),
        ("利用理由", "reason"),
        ("承認状態", "approvalState"),
    ])
    .edit(
        Mutation::new(
            "契約情報を編集",
            "PATCH",
            "/v1/erp/saas-subscriptions/{id}",
            &[
                Field::text("serviceName", "サービス名").required(),
                Field::new("monthlyAmountYen", "月額（円）", InputKind::Integer).required(),
                Field::text("ownerTeam", "担当チーム").required(),
                Field::new("renewalDate", "更新日", InputKind::Date).required(),
                Field::new(
                    "billingCycle",
                    "課金周期",
                    InputKind::Select(&[
                        Choice::new("monthly", "月払い"),
                        Choice::new("annual", "年払い"),
                    ]),
                )
                .required(),
                Field::new("reason", "利用理由", InputKind::Multiline).required(),
            ],
        )
        .prepare(prepare_saas),
    ),
    Resource::new(
        "contracts",
        FINANCE,
        "契約台帳",
        "/v1/erp/contracts",
        "items",
    )
    .route("contracts")
    .title(&["title"])
    .subtitle(&["counterparty.entityName", "amount"])
    .status("status")
    .detail("/v1/erp/contracts/{id}", "")
    .fields(&[
        ("件名", "title"),
        ("相手", "counterparty.entityName"),
        ("種類", "kind"),
        ("収支方向", "direction"),
        ("1回あたりの金額", "amount"),
        ("課金周期", "billingCycle"),
        ("支払日", "paymentDay"),
        ("開始日", "startDate"),
        ("終了日", "endDate"),
        ("メモ", "notes"),
    ]),
    Resource::new(
        "received-invoices",
        ACCOUNTING,
        "受領請求書",
        "/v1/field/procurement/purchase-invoices",
        "items",
    )
    .route("erp/received-invoices")
    .id("invoice.id")
    .title(&[
        "invoice.content.vendor.name",
        "invoice.content.invoiceNumber",
    ])
    .subtitle(&["invoice.content.totalAmount", "invoice.content.dueDate"])
    .status("invoice.status")
    .detail("/v1/field/procurement/purchase-invoices/{id}", "")
    .fields(&[
        ("仕入先", "invoice.content.vendor.name"),
        ("請求番号", "invoice.content.invoiceNumber"),
        ("発行日", "invoice.content.issueDate"),
        ("支払期日", "invoice.content.dueDate"),
        ("通貨", "invoice.content.currency"),
        ("金額", "invoice.content.totalAmount"),
        ("確認事項", "issues"),
    ]),
    Resource::new(
        "general-ledger",
        ACCOUNTING,
        "総勘定元帳",
        "/v1/erp/accounting/general-ledger",
        "items",
    )
    .route("accounting/ledger/general")
    .pagination(Pagination::Offset { limit: 50 })
    .id("lineId")
    .title(&["journalDescription", "accountName"])
    .subtitle(&["entryDate", "balance"])
    .filters(&[
        Field::new("account_code", "勘定科目", InputKind::Reference(ACCOUNTS)).required(),
        Field::new("date_from", "開始日", InputKind::Date).default("@monthStart"),
        Field::new("date_to", "終了日", InputKind::Date).default("@today"),
    ])
    .fields(&[
        ("日付", "entryDate"),
        ("科目", "accountName"),
        ("摘要", "journalDescription"),
        ("借方", "debitAmount"),
        ("貸方", "creditAmount"),
        ("残高", "balance"),
        ("仕訳区分", "entryKind"),
    ]),
    Resource::new(
        "trial-balance",
        ACCOUNTING,
        "試算表",
        "/v1/erp/accounting/trial-balance",
        "items",
    )
    .route("accounting/ledger/trial-balance")
    .id("accountCode")
    .title(&["accountName"])
    .subtitle(&["accountCode", "balance"])
    .filters(DATE_FILTERS)
    .fields(&[
        ("科目コード", "accountCode"),
        ("科目", "accountName"),
        ("勘定区分", "accountType"),
        ("借方合計", "debitTotal"),
        ("貸方合計", "creditTotal"),
        ("残高", "balance"),
    ]),
    Resource::new(
        "fiscal-years",
        ACCOUNTING,
        "会計年度",
        "/v1/erp/accounting/fiscal-years",
        "items",
    )
    .route("accounting/opening-balances")
    .title(&["compatibilityFiscalYear"])
    .subtitle(&["startsOn", "endsOn"])
    .fields(&[
        ("年度", "compatibilityFiscalYear"),
        ("期", "fiscalOrdinal"),
        ("期首", "startsOn"),
        ("期末", "endsOn"),
        ("正式帳簿の開始年度", "isFormalLedgerStart"),
        ("登録状態", "registrationStatus"),
    ]),
    Resource::new(
        "payment-schedule",
        ACCOUNTING,
        "支払予定",
        "/v1/erp/payments/schedule",
        "entries",
    )
    .route("erp/payments")
    .id("arApItemId")
    .title(&["payeeName"])
    .subtitle(&["dueDate", "outstandingAmount"])
    .status("status")
    .fields(&[
        ("支払先", "payeeName"),
        ("支払期日", "dueDate"),
        ("通貨", "currency"),
        ("請求金額", "totalAmount"),
        ("支払済み", "settledAmount"),
        ("未払金額", "outstandingAmount"),
        ("支払予約額", "reservedAmount"),
        ("支払可能額", "schedulableAmount"),
        ("支払方法", "paymentMethod"),
        ("口座", "account.masked"),
        ("保留事項", "holds"),
    ]),
    Resource::new(
        "ar-ap",
        FINANCE,
        "売掛・買掛",
        "/v1/erp/ar-ap/items",
        "items",
    )
    .route("erp/ar-ap")
    .title(&["counterpartyName"])
    .subtitle(&["sourceNumber", "outstandingAmount", "dueDate"])
    .status("effectiveStatus")
    .filters(&[
        Field::new("as_of", "基準日", InputKind::Date)
            .required()
            .default("@today"),
        Field::new(
            "kind",
            "種類",
            InputKind::Select(&[
                Choice::new("receivable", "売掛"),
                Choice::new("payable", "買掛"),
            ]),
        ),
        Field::text("counterparty", "相手先"),
        Field::new("overdue_only", "期日超過のみ", InputKind::Bool),
    ])
    .fields(&[
        ("相手先", "counterpartyName"),
        ("種類", "kind"),
        ("番号", "sourceNumber"),
        ("発生日", "issueDate"),
        ("期日", "dueDate"),
        ("通貨", "currency"),
        ("金額", "totalAmount"),
        ("消込済み", "settledAmount"),
        ("未消込", "outstandingAmount"),
        ("超過日数", "daysOverdue"),
        ("メモ", "notes"),
    ]),
    Resource::new(
        "usage-billing-periods",
        FINANCE,
        "使用量課金の対象月",
        "/v1/erp/usage/rating/periods",
        "periods",
    )
    .route("usage-billing")
    .id("periodKey")
    .title(&["periodKey"])
    .subtitle(&["recordCount", "lineCount"])
    .status("status")
    .fields(&[
        ("対象月", "periodKey"),
        ("使用記録数", "recordCount"),
        ("課金明細数", "lineCount"),
        ("計算日時", "ratedAt"),
        ("確定日時", "finalizedAt"),
    ]),
    Resource::new(
        "statement-imports",
        ACCOUNTING,
        "明細インポート履歴",
        "/v1/erp/reconciliation/statements",
        "items",
    )
    .route("accounting/statement-imports")
    .pagination(Pagination::Offset { limit: 50 })
    .title(&["sourceName"])
    .subtitle(&["createdAt", "lineCount"])
    .fields(&[
        ("取込名", "sourceName"),
        ("種類", "sourceType"),
        ("取込日時", "createdAt"),
        ("明細数", "lineCount"),
        ("通貨", "currency"),
        ("残高検証", "balanceValidation.status"),
        ("検証した行数", "balanceValidation.checkedRows"),
        ("問題数", "balanceValidation.issueCount"),
    ]),
    Resource::new(
        "journal-candidates",
        ACCOUNTING,
        "仕訳候補",
        "/v1/field/accounting/purchase-journal-candidates",
        "items",
    )
    .route("erp/journal-classifications")
    .title(&["supplierName"])
    .subtitle(&["occurredAt", "totalAmount"])
    .status("status")
    .fields(&[
        ("仕入先", "supplierName"),
        ("発生日時", "occurredAt"),
        ("合計", "totalAmount"),
        ("税区分", "taxCategory"),
        ("明細", "lines"),
    ]),
    Resource::new(
        "evidence",
        ACCOUNTING,
        "電子証憑",
        "/v1/erp/evidence/search?sort_by=transaction_date&sort_direction=desc",
        "items",
    )
    .route("erp/evidence")
    .pagination(Pagination::Offset { limit: 20 })
    .id("evidence.id")
    .title(&["evidence.originalFileName", "evidence.counterparty"])
    .subtitle(&["evidence.transactionDate", "evidence.amount"])
    .status("evidence.status")
    .detail("/v1/erp/evidence/{id}", "")
    .filters(&[
        Field::new("transaction_date_from", "取引開始日", InputKind::Date),
        Field::new("transaction_date_to", "取引終了日", InputKind::Date),
        Field::text("counterparty", "取引相手"),
    ])
    .fields(&[
        ("ファイル名", "evidence.originalFileName"),
        ("取引相手", "evidence.counterparty"),
        ("書類番号", "evidence.documentNumber"),
        ("取引日", "evidence.transactionDate"),
        ("通貨", "evidence.currency"),
        ("金額", "evidence.amount"),
        ("税額", "evidence.taxAmount"),
        ("書類種類", "evidence.voucherType"),
        ("検証状態", "evidence.verificationStatus"),
        ("保存期限", "evidence.retentionUntil"),
    ]),
    Resource::new(
        "accounting-inquiries",
        ACCOUNTING,
        "確認依頼",
        "/v1/erp/inquiries?limit=100",
        "items",
    )
    .route("accounting/inquiries")
    .title(&["title"])
    .subtitle(&["kind", "dueAt"])
    .status("status")
    .detail("/v1/erp/inquiries/{id}", "inquiry")
    .fields(&[
        ("件名", "title"),
        ("質問", "publicQuestion"),
        ("種類", "kind"),
        ("期限", "dueAt"),
        ("期限超過", "overdue"),
        ("内部メモ", "internalNote"),
        ("最終回答", "lastAnsweredAt"),
        ("終了理由", "closeReason"),
    ]),
    Resource::new(
        "audit-logs",
        ACCOUNTING,
        "監査ログ",
        "/v1/audit-logs",
        "items",
    )
    .route("audit-logs")
    .pagination(Pagination::Cursor { limit: 50 })
    .title(&["action"])
    .subtitle(&["resourceType", "resource_type", "createdAt", "created_at"])
    .filters(&[
        Field::new("from", "開始日時", InputKind::DateTime),
        Field::new("to", "終了日時", InputKind::DateTime),
        Field::text("action", "操作"),
        Field::text("resource_type", "対象の種類"),
    ])
    .fields(&[
        ("操作", "action"),
        ("対象種類", "resourceType"),
        ("対象種類", "resource_type"),
        ("実行者の種類", "actorType"),
        ("実行者の種類", "actor_type"),
        ("日時", "createdAt"),
        ("日時", "created_at"),
    ]),
];

fn is_warehouse(record: &Value) -> bool {
    record["kind"].as_str() == Some("warehouse")
}
fn package_active(record: &Value) -> bool {
    record["retiredAt"].is_null()
}
fn price_active(record: &Value) -> bool {
    record["supersededAt"].is_null()
}
fn loss_active(record: &Value) -> bool {
    record["voidedAt"].is_null()
}
fn po_editable(record: &Value) -> bool {
    matches!(record["status"].as_str(), Some("Draft" | "Remanded"))
}
fn expense_editable(record: &Value) -> bool {
    record["status"]
        .as_str()
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "draft" | "rejected"))
}
fn expense_submitted(record: &Value) -> bool {
    record["status"]
        .as_str()
        .is_some_and(|v| v.eq_ignore_ascii_case("submitted"))
}
fn account_editable(record: &Value) -> bool {
    record["seedManaged"] != true && record["accountTypeLocked"] != true
}

fn nonnegative_integer(value: &Value, label: &str) -> Result<i64, String> {
    value
        .as_i64()
        .filter(|v| (0..=9_007_199_254_740_991).contains(v))
        .ok_or_else(|| format!("{label}は0以上の整数で入力してください。"))
}
fn positive_decimal(value: &Value, label: &str) -> Result<(), String> {
    let s = value.as_str().unwrap_or("");
    let (whole, fraction) = s.split_once('.').unwrap_or((s, ""));
    if whole.is_empty()
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || !fraction.bytes().all(|c| c.is_ascii_digit())
        || !s.bytes().any(|c| (b'1'..=b'9').contains(&c))
    {
        return Err(format!("{label}は0より大きい数で入力してください。"));
    }
    Ok(())
}
fn copy_required(body: &mut Value, source: &Value, from: &str, to: &str) -> Result<(), String> {
    let v = &source[from];
    if v.is_null() {
        return Err("最新の情報を読み直してから編集してください。".into());
    }
    body[to] = v.clone();
    Ok(())
}
fn prepare_store(body: &mut Value, original: &Value) -> Result<(), String> {
    body["posLocationId"] = original
        .get("posLocationId")
        .cloned()
        .unwrap_or(Value::Null);
    Ok(())
}
fn prepare_vendor(body: &mut Value, original: &Value) -> Result<(), String> {
    body["defaultTaxCategory"] = original
        .get("defaultTaxCategory")
        .cloned()
        .unwrap_or(Value::Null);
    Ok(())
}
fn prepare_product_create(body: &mut Value, _: &Value) -> Result<(), String> {
    validate_product(body)?;
    body["tags"] = json!([]);
    Ok(())
}
fn prepare_product_edit(body: &mut Value, original: &Value) -> Result<(), String> {
    validate_product(body)?;
    copy_required(body, original, "updatedAt", "expectedUpdatedAt")
}
fn validate_product(body: &Value) -> Result<(), String> {
    let price = nonnegative_integer(&body["listPrice"], "定価")?;
    if price > 2_147_483_647 {
        return Err("定価は2,147,483,647円以下で入力してください。".into());
    }
    Ok(())
}
fn prepare_ingredient(body: &mut Value, _: &Value) -> Result<(), String> {
    if !body["yieldRate"].is_null() {
        positive_decimal(&body["yieldRate"], "歩留まり")?;
        let rate = body["yieldRate"].as_str().unwrap_or_default();
        let (whole, frac) = rate.split_once('.').unwrap_or((rate, ""));
        let whole = whole.trim_start_matches('0');
        if !(whole.is_empty() || (whole == "1" && frac.bytes().all(|c| c == b'0'))) {
            return Err("歩留まりは1以下で入力してください。".into());
        }
    }
    if !body["gramsPerMl"].is_null() {
        positive_decimal(&body["gramsPerMl"], "1mlあたりの重さ")?;
    }
    Ok(())
}
fn prepare_package(body: &mut Value, _: &Value) -> Result<(), String> {
    positive_decimal(&body["contentAmount"], "内容量")?;
    if nonnegative_integer(&body["piecesPerPack"], "個数")? == 0 {
        return Err("個数は1以上で入力してください。".into());
    }
    Ok(())
}
fn prepare_price(body: &mut Value, _: &Value) -> Result<(), String> {
    positive_decimal(&body["amount"], "購入金額")?;
    positive_decimal(&body["quantity"], "購入量")?;
    let tax = match body["taxRateBp"].as_str() {
        Some("800") => 800,
        Some("1000") => 1000,
        _ => return Err("税率を選択してください。".into()),
    };
    body["taxRateBp"] = json!(tax);
    body["packageSpecId"] = Value::Null;
    body["packCount"] = Value::Null;
    Ok(())
}
fn prepare_archive(body: &mut Value, _: &Value) -> Result<(), String> {
    body["archived"] = json!(true);
    Ok(())
}
fn prepare_recipe(body: &mut Value, original: &Value) -> Result<(), String> {
    if original.get("id").is_some()
        && original.get("kind").is_some()
        && body["kind"] != original["kind"]
    {
        return Err("登録済みのレシピは種類を変更できません。".into());
    }
    let lines = body["lines"]
        .as_array()
        .ok_or("材料の入力を確認してください。")?;
    for line in lines {
        positive_decimal(&line["amount"], "使用量")?;
        let ingredient = line["ingredientId"].as_str().is_some_and(|s| !s.is_empty());
        let preparation = line["preparationId"]
            .as_str()
            .is_some_and(|s| !s.is_empty());
        if ingredient == preparation {
            return Err("各行で材料または仕込み品のどちらか1つを選択してください。".into());
        }
        if preparation && line["preparationId"] == original["id"] {
            return Err("レシピ自身を仕込み品に指定できません。".into());
        }
        if preparation && line["weightBasis"].as_str() == Some("edible_portion") {
            return Err("仕込み品の使用量には可食部の基準を指定できません。".into());
        }
    }
    for key in ["options", "procedureSteps"] {
        if !body[key].is_array() {
            body[key] = original.get(key).cloned().unwrap_or_else(|| json!([]));
        }
    }
    match body["kind"].as_str() {
        Some("preparation") => {
            if !body["sellingPriceExcludingTax"].is_null() {
                return Err("仕込み品の売価は空欄にしてください。".into());
            }
            match (body["yieldAmount"].is_null(), body["yieldUnit"].is_null()) {
                (true, true) => {}
                (false, false) => positive_decimal(&body["yieldAmount"], "出来高")?,
                _ => return Err("出来高と単位は両方入力するか、両方を空欄にしてください。".into()),
            }
            if body["options"]
                .as_array()
                .is_some_and(|options| !options.is_empty())
            {
                return Err("追加オプションはメニューだけに設定できます。".into());
            }
        }
        Some("menu") => {
            if !body["yieldAmount"].is_null() || !body["yieldUnit"].is_null() {
                return Err("メニューの出来高と単位は空欄にしてください。".into());
            }
        }
        _ => return Err("レシピの種類を選択してください。".into()),
    }
    let mut option_keys = std::collections::HashSet::new();
    for option in body["options"]
        .as_array()
        .ok_or("オプションを確認してください。")?
    {
        let key = option["key"]
            .as_str()
            .filter(|k| !k.is_empty())
            .ok_or("オプションのキーを入力してください。")?;
        if !option_keys.insert(key) {
            return Err("オプションのキーが重複しています。".into());
        }
    }
    for line in body["lines"].as_array().unwrap() {
        if let Some(key) = line["optionKey"].as_str().filter(|k| !k.is_empty()) {
            if !option_keys.contains(key) {
                return Err("材料のオプションキーに対応するオプションを追加してください。".into());
            }
        }
    }
    if original.get("id").is_some() {
        copy_required(body, original, "revision", "expectedRevision")?;
    }
    Ok(())
}
fn validate_po(body: &Value) -> Result<(), String> {
    let lines = body["items"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or("発注明細を追加してください。")?;
    let mut total = 0_i64;
    for line in lines {
        let qty = nonnegative_integer(&line["quantity"], "数量")?;
        if qty == 0 {
            return Err("数量は1以上で入力してください。".into());
        }
        let cost = nonnegative_integer(&line["unitCost"], "単価")?;
        let tax = nonnegative_integer(&line["taxAmount"], "税額")?;
        total = total
            .checked_add(
                qty.checked_mul(cost)
                    .and_then(|v| v.checked_add(tax))
                    .ok_or("金額が大きすぎます。")?,
            )
            .ok_or("金額が大きすぎます。")?;
        if total > 9_007_199_254_740_991 {
            return Err("合計金額が大きすぎます。".into());
        }
    }
    Ok(())
}
fn prepare_po_create(body: &mut Value, _: &Value) -> Result<(), String> {
    validate_po(body)?;
    body["status"] = json!("Draft");
    Ok(())
}
fn prepare_po_edit(body: &mut Value, _: &Value) -> Result<(), String> {
    validate_po(body)
}
fn prepare_expense(body: &mut Value, _: &Value) -> Result<(), String> {
    let lines = body["lines"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or("経費明細を追加してください。")?;
    let mut total = 0_i64;
    for line in lines {
        let amount = nonnegative_integer(&line["amount"], "金額")?;
        if amount == 0 {
            return Err("経費明細の金額は0より大きくしてください。".into());
        }
        let tax = nonnegative_integer(&line["taxAmount"], "税額")?;
        if tax > amount {
            return Err("税額は明細の金額以下で入力してください。".into());
        }
        total = total
            .checked_add(amount)
            .ok_or("合計金額が大きすぎます。")?;
        if total > 9_007_199_254_740_991 {
            return Err("合計金額が大きすぎます。".into());
        }
    }
    Ok(())
}
fn prepare_invoice(body: &mut Value, _: &Value) -> Result<(), String> {
    let lines = body["lineItems"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or("請求明細を追加してください。")?;
    let mut total = nonnegative_integer(&body["taxAmount"], "税額")?;
    for line in lines {
        let qty = nonnegative_integer(&line["quantity"], "数量")?;
        if qty == 0 {
            return Err("数量は1以上で入力してください。".into());
        }
        let price = nonnegative_integer(&line["unitPrice"], "単価")?;
        total = qty
            .checked_mul(price)
            .and_then(|n| total.checked_add(n))
            .ok_or("合計金額が大きすぎます。")?;
        if total > 9_007_199_254_740_991 {
            return Err("合計金額が大きすぎます。".into());
        }
    }
    body["sendEmail"] = json!(false);
    body["sendSms"] = json!(false);
    body["createPaymentLink"] = json!(false);
    body["paymentLinkProvider"] = Value::Null;
    Ok(())
}
fn prepare_food_loss(body: &mut Value, _: &Value) -> Result<(), String> {
    positive_decimal(&body["quantity"], "数量")?;
    match body["itemKind"].as_str() {
        Some("ingredient") => {
            if body["ingredientId"].as_str().is_none_or(str::is_empty) {
                return Err("食材を選択してください。".into());
            }
            if !body["recipeId"].is_null() {
                return Err("食材の場合はレシピを空欄にしてください。".into());
            }
        }
        Some("preparation" | "finished_product") => {
            if body["recipeId"].as_str().is_none_or(str::is_empty) {
                return Err("レシピを選択してください。".into());
            }
            if !body["ingredientId"].is_null() {
                return Err("レシピの場合は食材を空欄にしてください。".into());
            }
        }
        _ => return Err("種類を選択してください。".into()),
    }
    Ok(())
}
fn prepare_account(body: &mut Value, original: &Value) -> Result<(), String> {
    if body["code"] != original["code"] {
        return Err("登録済みの科目コードは変更できません。".into());
    }
    if original["seedManaged"] == true {
        return Err("既定の勘定科目はこの画面では変更できません。".into());
    }
    if original["accountTypeLocked"] == true && body["accountType"] != original["accountType"] {
        return Err("仕訳がある勘定科目の区分は変更できません。".into());
    }
    if let Some(classification) = original
        .get("statementClassification")
        .filter(|v| v.is_object())
    {
        for (from, to) in [
            ("displayOrder", "statementDisplayOrder"),
            ("groupCode", "statementGroupCode"),
            ("normalBalance", "statementNormalBalance"),
            ("statementKind", "statementKind"),
        ] {
            if let Some(v) = classification.get(from) {
                body[to] = v.clone();
            }
        }
    }
    Ok(())
}
fn prepare_saas(body: &mut Value, _: &Value) -> Result<(), String> {
    nonnegative_integer(&body["monthlyAmountYen"], "月額")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::model;
    use super::*;

    #[test]
    fn recipe_form_round_trip_keeps_options_steps_and_exact_quantities() {
        let original = json!({
            "id":"recipe-a", "revision":7, "name":"スープ", "kind":"menu",
            "sellingPriceExcludingTax":"123.456789012345678901", "productId":"product-a",
            "lines":[{"ingredientId":"ingredient-a", "preparationId":null, "amount":"0.2204000000000000001", "unit":"g", "weightBasis":"edible_portion", "optionKey":"cheese", "note":"加熱後"}],
            "options":[{"key":"cheese", "label":"チーズ", "additionalPriceExcludingTax":"10.000000000001"}],
            "procedureSteps":[{"text":"煮込む", "imageUrl":"https://example.test/step.png"}]
        });
        let operation = RESOURCES
            .iter()
            .find(|r| r.key == "recipes")
            .unwrap()
            .edit
            .unwrap();
        let payload = model::request_body(
            operation,
            &model::draft(operation.fields, &original),
            &original,
            "unused",
        )
        .unwrap();
        for key in [
            "lines",
            "options",
            "procedureSteps",
            "sellingPriceExcludingTax",
            "productId",
        ] {
            assert_eq!(payload[key], original[key], "{key}");
        }
        assert_eq!(payload["expectedRevision"], 7);
        assert!(payload.get("id").is_none());
    }

    #[test]
    fn usd_purchase_and_expense_forms_round_trip_minor_units_and_site() {
        let cases = [
            (
                "purchase-orders",
                json!({"id":"po-a", "status":"Draft", "vendorId":"vendor-a", "currency":"USD", "items":[{"description":"食材", "quantity":2, "unitCost":12345, "taxAmount":987}]}),
                "items",
                "unitCost",
                12345,
            ),
            (
                "expenses",
                json!({"id":"expense-a", "status":"draft", "expenseDate":"2026-11-19", "payee":"仕入先", "currency":"USD", "lines":[{"id":"expl-a", "categoryId":"category-a", "amount":1099, "taxCategory":"taxable_10", "taxAmount":100, "burdenSiteId":"site-a"}]}),
                "lines",
                "amount",
                1099,
            ),
        ];
        for (key, original, rows_key, amount_key, amount) in cases {
            let operation = RESOURCES
                .iter()
                .find(|r| r.key == key)
                .unwrap()
                .edit
                .unwrap();
            let draft = model::draft(operation.fields, &original);
            assert_eq!(
                draft[rows_key][0][amount_key],
                if key == "expenses" { "10.99" } else { "123.45" }
            );
            let payload = model::request_body(operation, &draft, &original, "unused").unwrap();
            assert_eq!(payload[rows_key][0][amount_key], amount);
            assert_eq!(
                payload[rows_key][0]["taxAmount"],
                original[rows_key][0]["taxAmount"]
            );
            if key == "expenses" {
                assert_eq!(payload["lines"][0]["burdenSiteId"], "site-a");
                assert_eq!(payload["lines"][0]["id"], "expl-a");
            }
        }
    }

    #[test]
    fn invoice_form_encodes_usd_input_exactly_and_stays_a_draft() {
        let operation = RESOURCES
            .iter()
            .find(|r| r.key == "invoices")
            .unwrap()
            .create
            .unwrap();
        let draft = json!({"clientId":"client-a", "currency":"USD", "dueDate":"2026-11-30", "lineItems":[{"description":"月額", "quantity":"1", "unitPrice":"123.45"}], "taxAmount":"9.87"});
        let payload = model::request_body(operation, &draft, &Value::Null, "request-a").unwrap();
        assert_eq!(payload["lineItems"][0]["unitPrice"], 12345);
        assert_eq!(payload["taxAmount"], 987);
        assert_eq!(payload["idempotencyKey"], "request-a");
        assert_eq!(payload["sendEmail"], false);
        assert_eq!(payload["sendSms"], false);
    }

    #[test]
    fn expense_line_identity_survives_deletion_without_being_reassigned() {
        let operation = RESOURCES
            .iter()
            .find(|r| r.key == "expenses")
            .unwrap()
            .edit
            .unwrap();
        let original = json!({"id":"expense-a", "status":"draft", "expenseDate":"2026-11-19", "payee":"仕入先", "currency":"JPY", "lines":[
            {"id":"expl-a", "categoryId":"category-a", "amount":100, "taxCategory":"taxable_10", "taxAmount":0},
            {"id":"expl-b", "categoryId":"category-b", "amount":200, "taxCategory":"taxable_10", "taxAmount":0}
        ]});
        let mut draft = model::draft(operation.fields, &original);
        draft["lines"].as_array_mut().unwrap().remove(0);
        let mut added = draft["lines"][0].clone();
        added["id"] = json!("");
        draft["lines"].as_array_mut().unwrap().push(added);
        let payload = model::request_body(operation, &draft, &original, "unused").unwrap();
        assert_eq!(payload["lines"][0]["id"], "expl-b");
        assert!(payload["lines"][1].get("id").is_none());
        draft["lines"][1]["id"] = json!("expl-unrelated");
        assert!(model::request_body(operation, &draft, &original, "unused").is_err());
        draft["lines"][1]["id"] = json!("expl-b");
        assert!(model::request_body(operation, &draft, &original, "unused").is_err());
    }

    #[test]
    fn recipe_yield_follows_kind_and_allows_unmeasured_preparations() {
        let mut body =
            json!({"kind":"preparation", "lines":[], "yieldAmount":null, "yieldUnit":null});
        assert!(prepare_recipe(&mut body, &Value::Null).is_ok());
        body["yieldAmount"] = json!("10");
        assert!(prepare_recipe(&mut body, &Value::Null).is_err());
        body["yieldUnit"] = json!("g");
        assert!(prepare_recipe(&mut body, &Value::Null).is_ok());
        body["kind"] = json!("menu");
        assert!(prepare_recipe(&mut body, &Value::Null).is_err());
        body["yieldAmount"] = Value::Null;
        body["yieldUnit"] = Value::Null;
        assert!(
            prepare_recipe(
                &mut body,
                &json!({"id":"recipe-a", "kind":"preparation", "revision":1})
            )
            .is_err()
        );
    }
    #[test]
    fn recipe_uses_opened_revision_and_preserves_unedited_collections() {
        let original = json!({"id":"recipe-a","revision":7,"options":[{"key":"cheese","label":"チーズ"}],"procedureSteps":[{"text":"煮込む"}]});
        let mut body = json!({"kind":"menu","lines":[{"ingredientId":"i-a","amount":"0.2204000000000000001"}]});
        prepare_recipe(&mut body, &original).unwrap();
        assert_eq!(body["expectedRevision"], 7);
        assert_eq!(body["options"], original["options"]);
        assert_eq!(body["procedureSteps"], original["procedureSteps"]);
        assert_eq!(body["lines"][0]["amount"], "0.2204000000000000001");
    }
    #[test]
    fn recipe_rejects_ambiguous_material_and_self_reference() {
        for line in [
            json!({"ingredientId":"i","preparationId":"r","amount":"1"}),
            json!({"preparationId":"self","amount":"1"}),
        ] {
            let mut body = json!({"kind":"menu","lines":[line]});
            assert!(prepare_recipe(&mut body, &json!({"id":"self","revision":2})).is_err());
        }
    }
    #[test]
    fn ingredient_limits_are_exact_and_never_rounded() {
        for s in ["1.0000000000000000001", "0", "-0.1", "NaN", "1e-3"] {
            assert!(prepare_ingredient(&mut json!({"yieldRate":s}), &Value::Null).is_err());
        }
        for s in ["1", "0.0000000000000000001", "0001.000"] {
            assert!(prepare_ingredient(&mut json!({"yieldRate":s}), &Value::Null).is_ok());
        }
    }
    #[test]
    fn product_edit_requires_fetched_version_and_preserves_other_columns() {
        let mut body = json!({"name":"商品","listPrice":123});
        assert!(prepare_product_edit(&mut body, &json!({"id":"p"})).is_err());
        prepare_product_edit(
            &mut body,
            &json!({"updatedAt":"2026-11-19T00:00:00Z","tags":["a"]}),
        )
        .unwrap();
        assert_eq!(body["expectedUpdatedAt"], "2026-11-19T00:00:00Z");
        assert!(body.get("tags").is_none());
        assert!(body.get("variations").is_none());
    }
    #[test]
    fn store_update_keeps_existing_pos_link() {
        let mut body = json!({"name":"店舗"});
        prepare_store(&mut body, &json!({"posLocationId":"pos-1"})).unwrap();
        assert_eq!(body["posLocationId"], "pos-1");
    }
    #[test]
    fn invoice_draft_cannot_send_or_overflow_amounts() {
        let mut body = json!({"lineItems":[{"quantity":2,"unitPrice":500}],"taxAmount":100});
        prepare_invoice(&mut body, &Value::Null).unwrap();
        assert_eq!(body["sendEmail"], false);
        assert_eq!(body["sendSms"], false);
        assert_eq!(body["createPaymentLink"], false);
        let mut bad = json!({"lineItems":[{"quantity":9_007_199_254_740_991_i64,"unitPrice":2}],"taxAmount":0});
        assert!(prepare_invoice(&mut bad, &Value::Null).is_err());
    }
    #[test]
    fn purchase_order_create_starts_in_draft_and_rejects_zero_quantity() {
        let mut body = json!({"items":[{"quantity":1,"unitCost":50,"taxAmount":5}]});
        prepare_po_create(&mut body, &Value::Null).unwrap();
        assert_eq!(body["status"], "Draft");
        body["items"][0]["quantity"] = json!(0);
        assert!(prepare_po_create(&mut body, &Value::Null).is_err());
    }
}
