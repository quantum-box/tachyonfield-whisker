//! Sales, customers, booking and people management contracts.
//! These are native screen definitions; writes are explicit allow-lists, not record echoes.

use super::schema::*;
use serde_json::{Value, json};

const CLIENTS: Lookup = Lookup {
    path: "/v1/erp/clients",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const CUSTOMERS: Lookup = Lookup {
    path: "/v1/erp/customers?limit=100",
    list_key: "data",
    value_key: "id",
    label_keys: &["name", "email"],
};
const PIPELINES: Lookup = Lookup {
    path: "/v1/erp/pipelines",
    list_key: "items",
    value_key: "id",
    label_keys: &["label"],
};
const RESERVATION_TYPES: Lookup = Lookup {
    path: "/v1/erp/reservation-types",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};
const RESERVATION_RESOURCES: Lookup = Lookup {
    path: "/v1/erp/reservation-resources",
    list_key: "items",
    value_key: "id",
    label_keys: &["name"],
};

const EMPLOYMENT_TYPES: &[Choice] = &[
    Choice::new("full_time", "常勤"),
    Choice::new("part_time", "パート"),
    Choice::new("contract", "外部委託"),
];
const EMPLOYMENT_STATUSES: &[Choice] = &[
    Choice::new("active", "在籍"),
    Choice::new("on_leave", "休職"),
    Choice::new("retired", "退職"),
];
const ACTIVITY_KINDS: &[Choice] = &[
    Choice::new("call", "架電"),
    Choice::new("visit", "訪問"),
    Choice::new("email", "メール"),
    Choice::new("meeting", "商談"),
    Choice::new("note", "メモ"),
];
const RESOURCE_TYPES: &[Choice] = &[
    Choice::new("staff", "スタッフ"),
    Choice::new("room", "部屋・施設"),
    Choice::new("equipment", "設備"),
    Choice::new("vehicle", "車両"),
    Choice::new("other", "その他"),
];
const WAITLIST_STATUSES: &[Choice] = &[
    Choice::new("waiting", "待機中"),
    Choice::new("converted", "予約済み"),
    Choice::new("withdrawn", "辞退"),
    Choice::new("expired", "期限終了"),
];
const INQUIRY_STATES: &[Choice] = &[
    Choice::new("new", "未対応"),
    Choice::new("in_progress", "対応中"),
    Choice::new("awaiting_customer", "相手の返信待ち"),
    Choice::new("resolved", "完了"),
];
const INQUIRY_LANES: &[Choice] = &[
    Choice::new("unclassified", "未分類"),
    Choice::new("sales", "営業"),
    Choice::new("support", "サポート"),
    Choice::new("other", "その他"),
];

const CLIENT_FIELDS: &[Field] = &[
    Field::text("name", "会社名").required(),
    Field::new("email", "メールアドレス", InputKind::Email).empty(Empty::Null),
    Field::text("phoneNumber", "電話番号").empty(Empty::Null),
    Field::text("faxNumber", "FAX").empty(Empty::Null),
    Field::text("industry", "業種").empty(Empty::Null),
    Field::text("representative", "代表者").empty(Empty::Null),
    Field::text("website", "Webサイト").empty(Empty::Null),
    Field::text("corporateNumber", "法人番号")
        .read("corporationNumber")
        .empty(Empty::Null),
    Field::new("capital", "資本金", InputKind::Integer).empty(Empty::Null),
    Field::text("founded", "設立日").empty(Empty::Null),
    Field::new(
        "listed",
        "上場区分",
        InputKind::Select(&[
            Choice::new("unknown", "未設定"),
            Choice::new("true", "上場"),
            Choice::new("false", "未上場"),
        ]),
    )
    .default("unknown"),
    Field::text("address.postalCode", "郵便番号")
        .read("headOfficeAddress.postalCode")
        .empty(Empty::String),
    Field::text("address.state", "都道府県")
        .read("headOfficeAddress.state")
        .empty(Empty::String),
    Field::text("address.city", "市区町村")
        .read("headOfficeAddress.city")
        .empty(Empty::String),
    Field::text("address.address1", "町名・番地")
        .read("headOfficeAddress.address1")
        .empty(Empty::String),
    Field::text("address.address2", "建物名")
        .read("headOfficeAddress.address2")
        .empty(Empty::Null),
    Field::new("tags", "タグ（1行に1つ）", InputKind::StringLines),
    Field::new("description", "備考", InputKind::Multiline).empty(Empty::Null),
];

const CUSTOMER_FIELDS: &[Field] = &[
    Field::text("name", "氏名").required(),
    Field::text("nameKana", "フリガナ").empty(Empty::Null),
    Field::new("email", "メールアドレス", InputKind::Email).empty(Empty::Null),
    Field::text("phone", "電話番号").empty(Empty::Null),
];

const CUSTOMER_CREATE_FIELDS: &[Field] = &[
    Field::text("name", "氏名").required(),
    Field::text("nameKana", "フリガナ").empty(Empty::Null),
    Field::new("email", "メールアドレス", InputKind::Email).empty(Empty::Null),
    Field::text("phone", "電話番号").empty(Empty::Null),
    Field::text("address.postalCode", "郵便番号").empty(Empty::String),
    Field::text("address.state", "都道府県").empty(Empty::String),
    Field::text("address.city", "市区町村").empty(Empty::String),
    Field::text("address.address1", "町名・番地").empty(Empty::String),
    Field::text("address.address2", "建物名").empty(Empty::Null),
];

const DEAL_FIELDS: &[Field] = &[
    Field::text("name", "案件名").required(),
    Field::new("clientId", "取引先", InputKind::Reference(CLIENTS)).required(),
    Field::new("amount", "金額", InputKind::IntegerString)
        .required()
        .default("0"),
    Field::new("pipeline", "パイプライン", InputKind::Reference(PIPELINES)).required(),
    Field::new(
        "stage",
        "段階",
        InputKind::DependentReference {
            lookup: PIPELINES,
            parent_key: "pipeline",
            children_key: "stages",
        },
    )
    .required(),
    Field::new("expectedCloseOn", "成約予定日", InputKind::Date).empty(Empty::Null),
    Field::text("ownerName", "担当者名").empty(Empty::String),
    Field::new("memo", "メモ", InputKind::Multiline).empty(Empty::String),
];

const ACTIVITY_CREATE_FIELDS: &[Field] = &[
    Field::new("clientId", "取引先", InputKind::Reference(CLIENTS)).required(),
    Field::new("kind", "種類", InputKind::Select(ACTIVITY_KINDS))
        .required()
        .default("note"),
    Field::new("content", "活動内容", InputKind::Multiline).required(),
    Field::new("occurredAt", "日時", InputKind::DateTime)
        .empty(Empty::Null)
        .help("空欄の場合は登録日時を使います。"),
];
const ACTIVITY_EDIT_FIELDS: &[Field] = &[
    Field::new("kind", "種類", InputKind::Select(ACTIVITY_KINDS)).required(),
    Field::new("content", "活動内容", InputKind::Multiline).required(),
];

const STAFF_FIELDS: &[Field] = &[
    Field::text("name", "氏名").required(),
    Field::new(
        "employmentType",
        "雇用形態",
        InputKind::Select(EMPLOYMENT_TYPES),
    )
    .required()
    .default("part_time"),
    Field::new(
        "employmentStatus",
        "在籍状況",
        InputKind::Select(EMPLOYMENT_STATUSES),
    )
    .required()
    .default("active"),
    Field::text("jobTitle", "役職").empty(Empty::Null),
    Field::new("email", "メールアドレス", InputKind::Email).empty(Empty::Null),
    Field::text("phone", "電話番号").empty(Empty::Null),
    Field::new("hiredAt", "入社日", InputKind::Date).empty(Empty::Null),
    Field::new("contractEndDate", "契約終了日", InputKind::Date).empty(Empty::Null),
];

const RESERVATION_TYPE_FIELDS: &[Field] = &[
    Field::text("name", "名称").required(),
    Field::text("code", "コード")
        .required()
        .help("英小文字・数字・アンダースコアで入力します。"),
    Field::text("resourceModel", "種別モデル")
        .required()
        .default("generic_capacity")
        .help("汎用は generic_capacity。既存の種別と同じモデルを指定します。"),
    Field::text("description", "説明").empty(Empty::Omit),
    Field::new("active", "利用する", InputKind::Bool).default("true"),
    Field::new("quantityPolicy.maxCount", "数量上限", InputKind::Integer)
        .empty(Empty::Null)
        .help("1〜12。空欄の場合は上限の指定を解除します。"),
    Field::text("quantityPresentation.countLabel", "数量の表示名").empty(Empty::Null),
    Field::text("quantityPresentation.singularLabel", "単数の表示名").empty(Empty::Null),
    Field::new(
        "phoneBookingEnabled",
        "電話予約を受け付ける",
        InputKind::Bool,
    )
    .default("false"),
    Field::text("phoneBookingNumber", "電話予約の番号").empty(Empty::Omit),
];

const RESERVATION_RESOURCE_FIELDS: &[Field] = &[
    Field::text("name", "名称").required(),
    Field::new("resourceType", "種類", InputKind::Select(RESOURCE_TYPES))
        .required()
        .default("other"),
    Field::text("resourceModel", "種別モデル")
        .required()
        .default("generic_capacity")
        .help("汎用は generic_capacity。予約種別と一致させます。"),
    Field::new("active", "利用する", InputKind::Bool).default("true"),
];

const RESERVATION_CREATE_FIELDS: &[Field] = &[
    Field::new(
        "reservationTypeId",
        "予約種別",
        InputKind::Reference(RESERVATION_TYPES),
    )
    .required(),
    Field::new(
        "resourceId",
        "場所・設備",
        InputKind::Reference(RESERVATION_RESOURCES),
    ),
    Field::text("customerName", "お客様名").required(),
    Field::new("customerEmail", "メールアドレス", InputKind::Email).empty(Empty::String),
    Field::text("customerPhone", "電話番号").empty(Empty::String),
    Field::new("startsAt", "開始（日本時間）", InputKind::DateTime).required(),
    Field::new("endsAt", "終了（日本時間）", InputKind::DateTime).required(),
    Field::new("quantity", "人数・数量", InputKind::Integer)
        .required()
        .default("1"),
    Field::new("priceAmount", "料金（円）", InputKind::Integer)
        .required()
        .default("0"),
    Field::new("notes", "備考", InputKind::Multiline).empty(Empty::String),
];
const RESERVATION_EDIT_FIELDS: &[Field] = &[
    Field::text("customerName", "お客様名").required(),
    Field::new("customerEmail", "メールアドレス", InputKind::Email).empty(Empty::String),
    Field::text("customerPhone", "電話番号").empty(Empty::String),
    Field::new("startsAt", "開始（日本時間）", InputKind::DateTime).required(),
    Field::new("endsAt", "終了（日本時間）", InputKind::DateTime).required(),
    Field::new("notes", "備考", InputKind::Multiline).empty(Empty::String),
];

const WAITLIST_CREATE_FIELDS: &[Field] = &[
    Field::new(
        "reservationTypeId",
        "予約種別",
        InputKind::Reference(RESERVATION_TYPES),
    )
    .required(),
    Field::new(
        "resourceId",
        "場所・設備",
        InputKind::Reference(RESERVATION_RESOURCES),
    ),
    Field::text("customerName", "お客様名").required(),
    Field::new("customerEmail", "メールアドレス", InputKind::Email),
    Field::text("customerPhone", "電話番号"),
    Field::new(
        "desiredStartsAt",
        "希望開始（日本時間）",
        InputKind::DateTime,
    )
    .required(),
    Field::new("desiredEndsAt", "希望終了（日本時間）", InputKind::DateTime).required(),
    Field::new("quantity", "人数・数量", InputKind::Integer)
        .required()
        .default("1"),
    Field::new("notes", "備考", InputKind::Multiline),
];

const OUTREACH_LIST_FIELDS: &[Field] = &[
    Field::text("name", "営業リスト名").required(),
    Field::new("purpose", "目的", InputKind::Multiline).empty(Empty::Null),
    Field::text("senderName", "差出人名").empty(Empty::Null),
    Field::new("senderSignature", "署名", InputKind::Multiline).empty(Empty::Null),
    Field::text("templateSubject", "メールのひな形・件名").empty(Empty::Null),
    Field::new("templateBody", "メールのひな形・本文", InputKind::Multiline).empty(Empty::Null),
];

const CONTRACT_FIELDS: &[Field] = &[
    Field::text("contractNumber", "契約番号").required(),
    Field::text("title", "件名").required(),
    Field::new("clientId", "取引先", InputKind::Reference(CLIENTS)).empty(Empty::Null),
    Field::text("clientName", "契約先の名称").required(),
    Field::new("monthlyAmountYen", "月額（円）", InputKind::Integer)
        .required()
        .default("0"),
    Field::new(
        "lineItems",
        "明細",
        InputKind::Lines(&[
            Field::text("description", "品名・内容").required(),
            Field::new("quantity", "数量", InputKind::Integer)
                .required()
                .default("1"),
            Field::new("unitPrice", "単価（円）", InputKind::Integer)
                .required()
                .default("0"),
        ]),
    )
    .help("明細を入力した場合は、その合計を月額にします。"),
    Field::new(
        "billingCycle",
        "請求周期",
        InputKind::Select(&[
            Choice::new("monthly", "毎月"),
            Choice::new("quarterly", "四半期"),
            Choice::new("annual", "毎年"),
        ]),
    )
    .required()
    .default("monthly"),
    Field::new("startDate", "開始日", InputKind::Date).required(),
    Field::new("endDate", "終了日", InputKind::Date).empty(Empty::Null),
    Field::new("terms", "条項", InputKind::Multiline).empty(Empty::Null),
    Field::new("notes", "備考", InputKind::Multiline).empty(Empty::Null),
];

const CONTACT_DETAIL: &[(&str, &str)] = &[
    ("氏名", "name"),
    ("フリガナ", "nameKana"),
    ("メール", "email"),
    ("電話", "phone"),
    ("会員", "member"),
    ("注文回数", "orderCount"),
    ("最終注文", "lastOrderedAt"),
];

pub const RESOURCES: &[Resource] = &[
    Resource::new("clients", "顧客管理", "取引先", "/v1/erp/clients", "items")
        .subtitle(&["industry", "phoneNumber"])
        .detail("/v1/erp/clients/{id}", "")
        .fields(&[("会社名", "name"), ("業種", "industry"), ("メール", "email"), ("電話", "phoneNumber"),
            ("FAX", "faxNumber"), ("代表者", "representative"), ("Webサイト", "website"),
            ("法人番号", "corporationNumber"), ("資本金", "capital"), ("設立", "founded"),
            ("郵便番号", "headOfficeAddress.postalCode"), ("都道府県", "headOfficeAddress.state"),
            ("市区町村", "headOfficeAddress.city"), ("町名・番地", "headOfficeAddress.address1"),
            ("建物名", "headOfficeAddress.address2"), ("備考", "description")])
        .create(Mutation::new("取引先を追加", "POST", "/v1/erp/clients", CLIENT_FIELDS).prepare(prepare_client))
        .edit(Mutation::new("取引先を編集", "PUT", "/v1/erp/clients/{id}", CLIENT_FIELDS).prepare(prepare_client))
        .actions(&[Mutation::new("アーカイブ", "POST", "/v1/erp/clients/{id}/archive", &[]).destructive()
            .explain("取引先をアーカイブします。関連する履歴の扱いはサーバーの規定に従います。")])
        .route("library/clients"),
    Resource::new("customers", "顧客管理", "個人顧客", "/v1/erp/customers?limit=100", "data")
        .subtitle(&["email", "phone"]).detail("/v1/erp/customers/{id}", "").fields(CONTACT_DETAIL)
        .filters(&[Field::text("name", "氏名"), Field::new("email", "メール（完全一致）", InputKind::Email),
            Field::text("phone", "電話番号"), Field::new("segment", "区分", InputKind::Select(&[
                Choice::new("all", "すべて"), Choice::new("member", "会員"), Choice::new("non_member", "非会員"),
                Choice::new("never_ordered", "未取引"),
            ]))])
        .create(Mutation::new("個人顧客を追加", "POST", "/v1/erp/customers", CUSTOMER_CREATE_FIELDS).prepare(prepare_customer_create))
        .edit(Mutation::new("個人顧客を編集", "PUT", "/v1/erp/customers/{id}", CUSTOMER_FIELDS)
            .when(customer_editable).prepare(prepare_customer_update))
        .actions(&[Mutation::new("個人顧客を削除", "DELETE", "/v1/storekit/customers/{id}", &[])
            .when(customer_editable).destructive().explain("この個人顧客を削除します。元に戻せません。")])
        .route("library/consumers"),
    Resource::new("deals", "販売管理", "案件", "/v1/erp/deals", "items")
        .pagination(Pagination::Offset { limit: 100 }).subtitle(&["clientName", "amount"]).status("stage")
        .detail("/v1/erp/deals/{id}", "")
        .fields(&[("案件名", "name"), ("取引先", "clientName"), ("金額", "amount"), ("担当者", "ownerName"),
            ("成約予定日", "expectedCloseOn"), ("メモ", "memo"), ("更新日時", "updatedAt")])
        .filters(&[Field::text("q", "検索"), Field::new("client_id", "取引先", InputKind::Reference(CLIENTS))])
        .create(Mutation::new("案件を追加", "POST", "/v1/erp/deals", DEAL_FIELDS).prepare(prepare_deal_create))
        .edit(Mutation::new("案件を編集", "PATCH", "/v1/erp/deals/{id}", DEAL_FIELDS).prepare(prepare_deal_update))
        .actions(&[Mutation::new("案件を削除", "DELETE", "/v1/erp/deals/{id}", &[]).destructive()
            .explain("この案件を削除します。元に戻せません。")]).route("deals"),
    Resource::new("sales-activities", "販売管理", "営業活動", "/v1/erp/sales-activities", "items")
        .title(&["content"]).subtitle(&["occurredAt", "kind"])
        .fields(&[("種類", "kind"), ("活動内容", "content"), ("日時", "occurredAt")])
        .filters(&[Field::new("client_id", "取引先", InputKind::Reference(CLIENTS))])
        .create(Mutation::new("活動を記録", "POST", "/v1/erp/sales-activities", ACTIVITY_CREATE_FIELDS).prepare(prepare_activity))
        .edit(Mutation::new("活動を編集", "PATCH", "/v1/erp/sales-activities/{id}", ACTIVITY_EDIT_FIELDS))
        .actions(&[Mutation::new("活動を削除", "DELETE", "/v1/erp/sales-activities/{id}", &[]).destructive()
            .explain("この活動履歴を削除します。")]).route("library/clients"),
    Resource::new("sales-meetings", "販売管理", "商談", "/v1/erp/sales-meetings", "items")
        .title(&["title"]).subtitle(&["startsAt", "clientName", "prospectName"]).status("status")
        .detail("/v1/erp/sales-meetings/{id}", "")
        .fields(&[("件名", "title"), ("取引先", "clientName"), ("見込み先", "prospectName"), ("開始", "startsAt"),
            ("終了", "endsAt"), ("場所", "location"), ("説明", "description"), ("商談URL", "meetingUrl")])
        .filters(&[Field::new("from", "開始日から（日本時間）", InputKind::DateTime),
            Field::new("to", "開始日まで（日本時間）", InputKind::DateTime),
            Field::new("filter", "表示", InputKind::Select(&[Choice::new("sales", "商談"), Choice::new("unlinked", "未関連付け"), Choice::new("all", "すべて")]))])
        .route("sales-meetings"),
    Resource::new("sales-outreach-lists", "販売管理", "営業リスト", "/v1/erp/sales-outreach/lists", "items")
        .subtitle(&["purpose", "updatedAt"]).detail("/v1/erp/sales-outreach/lists/{id}", "")
        .fields(&[("名称", "name"), ("目的", "purpose"), ("差出人", "senderName"), ("署名", "senderSignature"),
            ("ひな形・件名", "templateSubject"), ("ひな形・本文", "templateBody"), ("更新日時", "updatedAt")])
        .create(Mutation::new("営業リストを追加", "POST", "/v1/erp/sales-outreach/lists", OUTREACH_LIST_FIELDS))
        .edit(Mutation::new("営業リストを編集", "PATCH", "/v1/erp/sales-outreach/lists/{id}", OUTREACH_LIST_FIELDS))
        .route("sales-outreach"),
    Resource::new("quotations", "販売管理", "見積", "/v1/erp/quotations", "items")
        .title(&["quotationNumber", "clientName"])
        .subtitle(&["clientName", "@money:totalAmount"]).status("status").detail("/v1/erp/quotations/{id}", "")
        .fields(&[("見積番号・件名", "quotationNumber"), ("取引先", "clientName"), ("メール", "clientEmail"),
            ("小計", "@money:subtotalAmount"), ("税額", "@money:taxAmount"), ("合計", "@money:totalAmount"), ("通貨", "currency"),
            ("有効期限", "validUntil")])
        .actions(&[
            Mutation::new("取引先を設定", "PATCH", "/v1/erp/quotations/{id}", &[
                Field::new("clientId", "取引先", InputKind::Reference(CLIENTS)).required(),
            ]).when(quote_is_draft),
            Mutation::new("見積を発行", "PATCH", "/v1/erp/quotations/{id}", &[
                Field::new("clientId", "取引先", InputKind::Reference(CLIENTS)).required(),
            ]).when(quote_is_draft).prepare(issue_quote).explain("見積を発行済みにします。メールは送信しません。"),
            Mutation::new("受注を作成", "POST", "/v1/erp/quotations/{id}/accept-order", &[])
                .when(quote_can_accept).explain("この見積から受注を作成します。"),
            Mutation::new("見積を取り消す", "PATCH", "/v1/erp/quotations/{id}", &[])
                .when(quote_can_cancel).prepare(cancel_quote).destructive().explain("この見積を取消済みにします。"),
        ]).route("quotations"),
    Resource::new("orders", "販売管理", "受注", "/v1/erp/orders", "items")
        .pagination(Pagination::Offset { limit: 100 }).title(&["orderNumber", "clientName"])
        .subtitle(&["clientName", "@money:totalAmount"]).status("status").detail("/v1/erp/orders/{id}", "")
        .fields(&[("受注番号", "orderNumber"), ("取引先", "clientName"), ("メール", "clientEmail"),
            ("小計", "@money:subtotalAmount"), ("税額", "@money:taxAmount"), ("合計", "@money:totalAmount"), ("通貨", "currency"),
            ("備考", "notes"), ("更新日時", "updatedAt")])
        .edit(Mutation::new("受注の備考を編集", "PATCH", "/v1/erp/orders/{id}", &[
            Field::new("notes", "備考", InputKind::Multiline).empty(Empty::Null),
        ]))
        .actions(&[
            Mutation::new("請求書を作成", "POST", "/v1/erp/orders/{id}/convert-to-invoice", &[])
                .when(order_can_invoice).explain("受注の内容から請求書を作成します。"),
            Mutation::new("受注を取り消す", "PATCH", "/v1/erp/orders/{id}", &[])
                .when(order_can_cancel).prepare(cancel_order).destructive().explain("この受注を取消済みにします。"),
        ]).route("orders"),
    Resource::new("sales-contracts", "販売管理", "販売契約", "/v1/erp/sales-contracts", "items")
        .pagination(Pagination::Offset { limit: 100 }).title(&["title", "contractNumber"])
        .subtitle(&["clientName", "monthlyAmountYen"]).status("status").detail("/v1/erp/sales-contracts/{id}", "")
        .fields(&[("契約番号", "contractNumber"), ("件名", "title"), ("取引先", "clientName"),
            ("月額（円）", "monthlyAmountYen"), ("請求周期", "billingCycle"), ("開始日", "startDate"),
            ("終了日", "endDate"), ("条項", "terms"), ("備考", "notes")])
        .create(Mutation::new("契約の下書きを追加", "POST", "/v1/erp/sales-contracts", CONTRACT_FIELDS)
            .prepare(prepare_contract_create).explain("下書きとして保存します。契約の有効化や請求は行いません。"))
        .edit(Mutation::new("契約の下書きを編集", "PUT", "/v1/erp/sales-contracts/{id}", CONTRACT_FIELDS)
            .when(manual_contract_draft).prepare(prepare_contract_update))
        .route("sales-contracts"),
    Resource::new("contract-templates", "販売管理", "契約ひな形", "/v1/erp/sales-contract-templates", "items")
        .fields(&[("名称", "name"), ("本文", "body"), ("標準", "isDefault")])
        .create(Mutation::new("ひな形を追加", "POST", "/v1/erp/sales-contract-templates", &[
            Field::text("name", "名称").required(), Field::new("body", "本文", InputKind::Multiline).required(),
            Field::new("isDefault", "標準にする", InputKind::Bool).default("false"),
        ]))
        .edit(Mutation::new("ひな形を編集", "PUT", "/v1/erp/sales-contract-templates/{id}", &[
            Field::text("name", "名称").required(), Field::new("body", "本文", InputKind::Multiline).required(),
            Field::new("isDefault", "標準にする", InputKind::Bool),
        ]))
        .actions(&[Mutation::new("ひな形を削除", "DELETE", "/v1/erp/sales-contract-templates/{id}", &[]).destructive()
            .explain("この契約ひな形を削除します。")]).route("sales-contracts"),
    Resource::new("consumer-orders", "販売管理", "B2C受注", "/v1/storekit/orders", "items")
        .pagination(Pagination::Offset { limit: 100 }).title(&["order_number", "customer_name", "shipping_name"])
        .subtitle(&["created_at", "customer_email"]).status("status").detail("/v1/storekit/orders/{id}", "")
        .fields(&[("受注番号", "order_number"), ("お客様", "customer_name"), ("メール", "customer_email"),
            ("決済状況", "payment_status"), ("受取方法", "fulfillment_method"), ("お届け先", "shipping_name"),
            ("お届け先電話", "shipping_phone"), ("作成日時", "created_at"), ("更新日時", "updated_at")])
        .filters(&[Field::new("customer_id", "お客様", InputKind::Reference(CUSTOMERS))]).route("consumer-orders"),
    Resource::new("return-requests", "販売管理", "返品申請", "/v1/erp/return-requests", "items")
        .title(&["returnNumber", "orderNumber"]).subtitle(&["requestedAt", "reason"]).status("status")
        .detail("/v1/erp/return-requests/{id}", "")
        .fields(&[("返品番号", "returnNumber"), ("受注番号", "orderNumber"), ("理由", "reason"),
            ("お客様のメモ", "customerNote"), ("申請日", "requestedAt"), ("返品期限", "returnDueAt"),
            ("検品開始", "inspectionStartedAt"), ("検品完了", "inspectedAt"), ("完了", "completedAt")])
        .route("erp/return-requests"),
    Resource::new("business-cards", "顧客管理", "名刺", "/v1/erp/business-cards", "items")
        .pagination(Pagination::Offset { limit: 100 }).title(&["contactPersonName", "cardCompanyName"])
        .subtitle(&["clientName", "contactPersonTitle"]).fields(&[("氏名", "contactPersonName"),
            ("会社", "clientName"), ("役職", "contactPersonTitle"), ("メール", "contactPersonEmail"),
            ("電話", "contactPersonPhone"), ("名刺の住所", "cardAddress"), ("交換日", "exchangedOn"),
            ("交換した人", "exchangedBy"), ("会った場所", "metAt"), ("メモ", "memo")])
        .filters(&[Field::text("q", "検索"), Field::new("clientId", "取引先", InputKind::Reference(CLIENTS))])
        .edit(Mutation::new("名刺の記録を編集", "PUT", "/v1/erp/business-cards/{id}", &[
            Field::text("cardCompanyName", "名刺の会社名").empty(Empty::Null),
            Field::text("cardAddress", "名刺の住所").empty(Empty::Null),
            Field::new("exchangedOn", "交換日", InputKind::Date).empty(Empty::Null),
            Field::text("exchangedBy", "交換した人").empty(Empty::Null),
            Field::text("metAt", "会った場所").empty(Empty::Null),
            Field::new("memo", "メモ", InputKind::Multiline).empty(Empty::Null),
        ]).prepare(preserve_card_contact))
        .actions(&[Mutation::new("名刺を削除", "DELETE", "/v1/erp/business-cards/{id}", &[]).destructive()
            .explain("この名刺の記録を削除します。")]).route("library/business-cards"),
    Resource::new("forms", "顧客管理", "フォーム", "/v1/erp/field-forms/forms", "items")
        .subtitle(&["updatedAt"]).status("status").detail("/v1/erp/field-forms/forms/{id}", "")
        .fields(&[("名称", "name"), ("状態", "status"), ("未公開の変更", "hasUnpublishedChanges"),
            ("公開日時", "published.publishedAt"), ("更新日時", "updatedAt")])
        .create(Mutation::new("フォームの下書きを追加", "POST", "/v1/erp/field-forms/forms", &[
            Field::text("name", "フォーム名").required(),
            Field::new("template", "ひな形", InputKind::Select(&[Choice::new("contact", "お問い合わせ"), Choice::new("blank", "空のフォーム")])).required().default("contact"),
        ]))
        .actions(&[
            Mutation::new("受付を停止", "POST", "/v1/erp/field-forms/forms/{id}/pause", &[])
                .when(form_published).destructive().explain("このフォームからの新しい受付を停止します。"),
            Mutation::new("受付を再開", "POST", "/v1/erp/field-forms/forms/{id}/resume", &[])
                .when(form_paused).explain("公開済みのフォームで受付を再開します。"),
        ]).route("crm/forms"),
    Resource::new("form-inquiries", "顧客管理", "お問い合わせ", "/v1/erp/field-forms/inquiries", "items")
        .pagination(Pagination::Offset { limit: 100 }).title(&["name", "company", "email"])
        .subtitle(&["category", "receivedAt"]).status("state").detail("/v1/erp/field-forms/inquiries/{id}", "")
        .fields(&[("受付番号", "reference"), ("氏名", "name"), ("会社", "company"), ("メール", "email"),
            ("分類", "category"), ("区分", "lane"), ("次の対応期限", "dueAt"), ("受付日時", "receivedAt"),
            ("フォーム名", "formName"), ("内容", "values"), ("社内メモ", "notes")])
        .filters(&[Field::new("state", "状態", InputKind::Select(INQUIRY_STATES)), Field::text("q", "検索")])
        .actions(&[
            Mutation::new("対応状況を変更", "POST", "/v1/erp/field-forms/inquiries/{id}/transition", &[
                Field::new("state", "対応状況", InputKind::Select(INQUIRY_STATES)).required(),
        ]).when(inquiry_editable).prepare(inquiry_version),
            Mutation::new("区分を変更", "POST", "/v1/erp/field-forms/inquiries/{id}/lane", &[
                Field::new("lane", "区分", InputKind::Select(INQUIRY_LANES)).required(),
            ]).when(inquiry_editable).prepare(inquiry_version),
            Mutation::new("対応期限を変更", "POST", "/v1/erp/field-forms/inquiries/{id}/schedule", &[
                Field::new("dueAt", "次の対応期限（日本時間）", InputKind::DateTime).empty(Empty::Null),
            ]).when(inquiry_editable).prepare(inquiry_version),
            Mutation::new("社内メモを追加", "POST", "/v1/erp/field-forms/inquiries/{id}/notes", &[
                Field::new("body", "メモ", InputKind::Multiline).required(),
            ]).when(inquiry_editable).prepare(inquiry_version),
        ]).route("crm/form-inquiries"),
    Resource::new("usage-receptions", "顧客管理", "受付台帳", "/v1/erp/usage-receptions", "items")
        .pagination(Pagination::Offset { limit: 100 }).title(&["customerName"]).subtitle(&["receivedAt"])
        .detail("/v1/erp/usage-receptions/{id}", "")
        .fields(&[("お客様", "customerName"), ("受付日時", "receivedAt"), ("メモ", "note"),
            ("取消日時", "cancelledAt"), ("取消理由", "cancellationReason")])
        .actions(&[Mutation::new("受付を取り消す", "POST", "/v1/erp/usage-receptions/{id}/cancel", &[
            Field::new("reason", "取消理由", InputKind::Multiline).required(),
        ]).when(reception_open).destructive().explain("この受付を取り消します。履歴は残ります。")])
        .route("usage-receptions"),
    Resource::new("pre-receptions", "顧客管理", "事前受付", "/v1/erp/pre-receptions", "")
        .title(&["definition.title", "definitionKey", "summary.definitionKey"])
        .subtitle(&["createdAt", "source"]).status("status").detail("/v1/erp/pre-receptions/{id}", "")
        .fields(&[("受付名", "definition.title"), ("状態", "summary.status"), ("受付日時", "summary.createdAt"),
            ("有効期限", "summary.expiresAt"), ("確認日時", "summary.reviewedAt"), ("確定日時", "summary.confirmedAt")])
        .route("pre-receptions"),
    Resource::new("reservations", "予約管理", "予約管理", "/v1/erp/reservations", "items")
        .pagination(Pagination::Offset { limit: 100 }).title(&["customerName", "reservationNumber"])
        .subtitle(&["startsAt", "@money:priceAmount"]).status("status").detail("/v1/erp/reservations/{id}", "")
        .fields(&[("予約番号", "reservationNumber"), ("お客様", "customerName"), ("メール", "customerEmail"),
            ("電話", "customerPhone"), ("開始", "startsAt"), ("終了", "endsAt"), ("人数・数量", "quantity"),
            ("料金", "@money:priceAmount"), ("通貨", "currency"), ("支払済み", "@money:paidAmount"), ("決済状況", "paymentStatus"),
            ("備考", "notes"), ("取消理由", "cancelReason")])
        .filters(&[Field::text("search", "検索"),
            Field::new("from", "開始日から（日本時間）", InputKind::DateTime),
            Field::new("to", "開始日まで（日本時間）", InputKind::DateTime),
            Field::new("reservationTypeId", "予約種別", InputKind::Reference(RESERVATION_TYPES)),
            Field::new("resourceId", "場所・設備", InputKind::Reference(RESERVATION_RESOURCES))])
        .create(Mutation::new("予約を追加", "POST", "/v1/erp/reservations", RESERVATION_CREATE_FIELDS)
            .prepare(prepare_reservation_create).idempotency_body("idempotencyKey"))
        .edit(Mutation::new("予約内容を編集", "PATCH", "/v1/erp/reservations/{id}", RESERVATION_EDIT_FIELDS)
            .when(reservation_editable).prepare(prepare_reservation_edit))
        .actions(&[Mutation::new("予約を取り消す", "POST", "/v1/erp/reservations/{id}/cancel", &[
            Field::new("reason", "取消理由", InputKind::Multiline).required(),
            Field::new("cancellationFeeAmount", "キャンセル料（円）", InputKind::Integer)
                .help("空欄の場合は予約の規定に従います。"),
            Field::new("refundAmount", "返金額（円）", InputKind::Integer)
                .help("空欄の場合は予約の規定に従います。"),
        ]).when(reservation_can_cancel).prepare(prepare_reservation_cancel).idempotency_header().destructive()
            .explain("予約を取り消します。決済済みの場合は規定に従って返金が発生します。金額を確認して実行してください。")])
        .route("reservations"),
    Resource::new("reservation-types", "予約管理", "予約種別", "/v1/erp/reservation-types/manage?includeInactive=true", "items")
        .subtitle(&["code", "description"])
        .fields(&[("名称", "name"), ("コード", "code"), ("説明", "description"), ("利用中", "active"),
            ("種別モデル", "resourceModel"), ("数量上限", "quantityPolicy.maxCount"),
            ("数量の表示名", "quantityPresentation.countLabel"), ("電話予約", "phoneBookingEnabled"),
            ("電話予約の番号", "phoneBookingNumber")])
        .create(Mutation::new("予約種別を追加", "POST", "/v1/erp/reservation-types", RESERVATION_TYPE_FIELDS).prepare(prepare_reservation_type))
        .edit(Mutation::new("予約種別を編集", "PATCH", "/v1/erp/reservation-types/{id}", RESERVATION_TYPE_FIELDS).prepare(prepare_reservation_type))
        .actions(&[Mutation::new("予約種別を削除", "DELETE", "/v1/erp/reservation-types/{id}", &[]).destructive()
            .explain("使用中の予約種別は削除できません。その場合は編集で利用を停止してください。")])
        .route("reservations/types"),
    Resource::new("reservation-resources", "予約管理", "場所・設備", "/v1/erp/reservation-resources?includeInactive=true", "items")
        .subtitle(&["resourceType", "resourceModel"])
        .fields(&[("名称", "name"), ("種類", "resourceType"), ("種別モデル", "resourceModel"), ("利用中", "active")])
        .create(Mutation::new("場所・設備を追加", "POST", "/v1/erp/reservation-resources", RESERVATION_RESOURCE_FIELDS).prepare(prepare_resource))
        .edit(Mutation::new("場所・設備を編集", "PATCH", "/v1/erp/reservation-resources/{id}", RESERVATION_RESOURCE_FIELDS).prepare(prepare_resource))
        .actions(&[Mutation::new("場所・設備を削除", "DELETE", "/v1/erp/reservation-resources/{id}", &[]).destructive()
            .explain("予約などで使用中の場所・設備は削除できません。その場合は編集で利用を停止してください。")])
        .route("reservations/resources"),
    Resource::new("reservation-waitlist", "予約管理", "キャンセル待ち", "/v1/erp/reservation-waitlist?limit=100", "items")
        .title(&["customerName", "customerEmail"]).subtitle(&["desiredStartsAt", "quantity"]).status("status")
        .detail("/v1/erp/reservation-waitlist/{id}", "")
        .fields(&[("お客様", "customerName"), ("メール", "customerEmail"), ("電話", "customerPhone"),
            ("希望開始", "desiredStartsAt"), ("希望終了", "desiredEndsAt"), ("人数・数量", "quantity"),
            ("順番", "queuePosition"), ("空いている枠数", "availableSlotGroups"), ("備考", "notes"),
            ("終了理由", "closedReason")])
        .filters(&[Field::new("status", "状態", InputKind::Select(WAITLIST_STATUSES)),
            Field::new("reservationTypeId", "予約種別", InputKind::Reference(RESERVATION_TYPES)),
            Field::new("from", "希望開始から（日本時間）", InputKind::DateTime),
            Field::new("to", "希望開始まで（日本時間）", InputKind::DateTime)])
        .create(Mutation::new("キャンセル待ちを追加", "POST", "/v1/erp/reservation-waitlist", WAITLIST_CREATE_FIELDS)
            .prepare(prepare_waitlist))
        .actions(&[
            Mutation::new("空き枠で予約する", "POST", "/v1/erp/reservation-waitlist/{id}/convert", &[])
                .when(waitlist_seatable).explain("希望日時の空き枠で予約を作成します。実行時に満席になっている場合は予約されません。"),
            Mutation::new("キャンセル待ちを終了", "POST", "/v1/erp/reservation-waitlist/{id}/close", &[
                Field::new("status", "終了理由", InputKind::Select(&[Choice::new("withdrawn", "辞退"), Choice::new("expired", "期限終了")])).required(),
                Field::new("reason", "メモ", InputKind::Multiline),
            ]).when(waitlist_waiting).destructive().explain("このキャンセル待ちを終了します。"),
        ]).route("reservations/waitlist"),
    Resource::new("staff", "人事労務", "スタッフ管理", "/v1/erp/hrm/staff", "items")
        .subtitle(&["jobTitle", "employmentType"]).status("employmentStatus")
        .fields(&[("氏名", "name"), ("雇用形態", "employmentType"), ("在籍状況", "employmentStatus"),
            ("役職", "jobTitle"), ("メール", "email"), ("電話", "phone"), ("入社日", "hiredAt"),
            ("契約終了日", "contractEndDate"), ("更新日時", "updatedAt")])
        .create(Mutation::new("スタッフを追加", "POST", "/v1/erp/hrm/staff", STAFF_FIELDS).prepare(prepare_staff).capability("hrm.manage"))
        .edit(Mutation::new("スタッフを編集", "PATCH", "/v1/erp/hrm/staff/{id}", STAFF_FIELDS).prepare(prepare_staff_update).capability("hrm.manage"))
        .actions(&[Mutation::new("スタッフを削除", "DELETE", "/v1/erp/hrm/staff/{id}", &[]).destructive().capability("hrm.manage")
            .explain("スタッフを在籍一覧から外します。過去の勤怠・給与の履歴は残ります。")]).route("hrm/staff"),
    Resource::new("my-leave-requests", "人事労務", "自分の休暇申請", "/v1/erp/hrm/me/leave-requests", "items")
        .title(&["date"]).subtitle(&["requestType", "reason"]).status("status")
        .fields(&[("日付", "date"), ("種類", "requestType"), ("理由", "reason"), ("状態", "status"),
            ("判断日時", "decidedAt"), ("判断メモ", "decisionNote")])
        .create(Mutation::new("休暇を申請", "POST", "/v1/erp/hrm/me/leave-requests", &[
            Field::new("date", "日付", InputKind::Date).required(),
            Field::new("requestType", "種類", InputKind::Select(&[
                Choice::new("day_off", "休み希望"), Choice::new("paid_leave", "有休"), Choice::new("unavailable", "不可日"),
            ])).required().default("day_off"),
            Field::new("reason", "理由", InputKind::Multiline).empty(Empty::Null),
        ]))
        .actions(&[Mutation::new("申請を取り消す", "POST", "/v1/erp/hrm/me/leave-requests/{id}/cancel", &[])
            .when(leave_can_cancel).prepare(leave_version).destructive().explain("この休暇申請を取り消します。")])
        .route("hrm/me"),
    Resource::new("membership-roster", "顧客管理", "会員台帳", "/v1/erp/membership/customers", "items")
        .pagination(Pagination::Offset { limit: 100 }).id("customerId|planId|startedOn")
        .subtitle(&["planName", "phone"]).status("assignmentStatus")
        .fields(&[("氏名", "name"), ("フリガナ", "nameKana"), ("メール", "email"), ("電話", "phone"),
            ("プラン", "planName"), ("開始日", "startedOn"), ("終了日", "endedOn"), ("対象数", "subjectCount")])
        .filters(&[Field::text("q", "検索"), Field::new("state", "状態", InputKind::Select(&[
            Choice::new("current", "有効"), Choice::new("expiring", "失効間近"), Choice::new("lapsed", "失効・終了"), Choice::new("all", "すべて"),
        ]))]).route("membership/members"),
    Resource::new("pipelines", "販売管理", "パイプライン", "/v1/erp/pipelines", "items")
        .title(&["label"]).fields(&[("名称", "label"), ("段階", "stages")]).route("deals"),
    Resource::new("coupons", "販売管理", "クーポン", "/v1/erp/commerce/coupons", "data")
        .pagination(Pagination::Offset { limit: 100 }).title(&["code"]).subtitle(&["discountType", "expiresAt"])
        .fields(&[("コード", "code"), ("割引の種類", "discountType"), ("割引値", "discountValue"),
            ("利用中", "isActive"), ("利用回数", "usedCount"), ("利用上限", "usageLimit"),
            ("1人1回", "usePerUser"), ("有効期限", "expiresAt"), ("併用制限グループ", "exclusionGroup")])
        .create(Mutation::new("クーポンを作成", "POST", "/v1/erp/commerce/coupons", &[
            Field::text("code", "クーポンコード").required(),
            Field::new("discountType", "割引の種類", InputKind::Select(&[Choice::new("PERCENTAGE", "割合（%）"), Choice::new("FIXED", "定額（円）")])).required().default("PERCENTAGE"),
            Field::new("discountValue", "割引値", InputKind::Integer).required().default("1"),
            Field::new("minimumOrderAmountJpy", "最低注文金額（円）", InputKind::Integer).default("0"),
            Field::new("usageLimit", "利用上限", InputKind::Integer).default("0").help("0の場合は上限を設定しません。"),
            Field::new("usePerUser", "1人1回にする", InputKind::Bool).default("false"),
            Field::new("expiresAt", "有効期限（日本時間）", InputKind::DateTime).empty(Empty::Null),
            Field::text("exclusionGroup", "併用制限グループ").empty(Empty::Null),
        ]).prepare(prepare_coupon))
        .actions(&[Mutation::new("クーポンを停止", "POST", "/v1/erp/commerce/coupons/{id}/disable", &[])
            .when(coupon_active).destructive().explain("このクーポンの利用を停止します。")]).route("store/coupons"),
];

fn required_text(body: &Value, key: &str, label: &str, max: usize) -> Result<(), String> {
    let value = body[key].as_str().unwrap_or_default();
    if value.trim().is_empty() {
        return Err(format!("{label}を入力してください。"));
    }
    if value.chars().count() > max {
        return Err(format!("{label}は{max}文字以内で入力してください。"));
    }
    Ok(())
}

fn ensure_nullable(body: &mut Value, keys: &[&str]) {
    if let Some(object) = body.as_object_mut() {
        for key in keys {
            object.entry((*key).to_owned()).or_insert(Value::Null);
        }
    }
}

fn nonnegative_integer(body: &Value, key: &str, required: bool) -> Result<(), String> {
    match body.get(key) {
        None | Some(Value::Null) if !required => Ok(()),
        Some(Value::Number(number)) if number.as_u64().is_some() => Ok(()),
        Some(Value::String(text))
            if !text.is_empty()
                && text.bytes().all(|b| b.is_ascii_digit())
                && text.parse::<u64>().is_ok() =>
        {
            Ok(())
        }
        _ => Err("金額・数量は0以上の整数で入力してください。".into()),
    }
}

fn prepare_client(body: &mut Value, original: &Value) -> Result<(), String> {
    required_text(body, "name", "会社名", 255)?;
    nonnegative_integer(body, "capital", false)?;
    let corporation = body["corporateNumber"].as_str().unwrap_or_default();
    if !corporation.is_empty()
        && (corporation.len() != 13 || !corporation.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("法人番号は13桁の数字で入力してください。".into());
    }
    body["listed"] = match body["listed"].as_str() {
        Some("true") => Value::Bool(true),
        Some("false") => Value::Bool(false),
        _ => Value::Null,
    };
    let has_address = body
        .get("address")
        .and_then(Value::as_object)
        .is_some_and(|address| {
            address
                .values()
                .any(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
        });
    if !has_address {
        body["address"] = Value::Null;
    }
    // A caller that is only editing contact fields must never erase unrelated tags.
    if body.get("tags").is_none() {
        body["tags"] = original
            .get("tags")
            .filter(|v| v.is_array())
            .cloned()
            .unwrap_or_else(|| json!([]));
    }
    ensure_nullable(
        body,
        &[
            "capital",
            "corporateNumber",
            "description",
            "email",
            "faxNumber",
            "founded",
            "industry",
            "phoneNumber",
            "representative",
            "website",
        ],
    );
    Ok(())
}

fn prepare_customer_create(body: &mut Value, _: &Value) -> Result<(), String> {
    required_text(body, "name", "氏名", 255)?;
    ensure_nullable(body, &["email", "nameKana", "phone"]);
    let has_address = body
        .get("address")
        .and_then(Value::as_object)
        .is_some_and(|address| {
            address
                .values()
                .any(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
        });
    if !has_address {
        body["address"] = Value::Null;
    }
    Ok(())
}

fn customer_editable(original: &Value) -> bool {
    original["editable"].as_bool() == Some(true)
}

fn prepare_customer_update(body: &mut Value, original: &Value) -> Result<(), String> {
    if !customer_editable(original) {
        return Err("この個人顧客は編集できません。".into());
    }
    required_text(body, "name", "氏名", 255)?;
    ensure_nullable(body, &["email", "nameKana", "phone"]);
    Ok(())
}

fn validate_deal(body: &Value) -> Result<(), String> {
    required_text(body, "name", "案件名", 255)?;
    required_text(body, "clientId", "取引先", 255)?;
    required_text(body, "pipeline", "パイプライン", 255)?;
    required_text(body, "stage", "段階", 255)?;
    nonnegative_integer(body, "amount", true)?;
    if body["memo"].as_str().unwrap_or_default().chars().count() > 4000 {
        return Err("メモは4,000文字以内で入力してください。".into());
    }
    Ok(())
}

fn prepare_deal_create(body: &mut Value, _: &Value) -> Result<(), String> {
    validate_deal(body)?;
    ensure_nullable(body, &["expectedCloseOn", "memo", "ownerName"]);
    Ok(())
}

fn prepare_deal_update(body: &mut Value, _: &Value) -> Result<(), String> {
    validate_deal(body)?;
    body["clearExpectedCloseOn"] = Value::Bool(
        body["expectedCloseOn"]
            .as_str()
            .is_none_or(|s| s.is_empty()),
    );
    for key in ["memo", "ownerName"] {
        if body[key].is_null() {
            body[key] = Value::String(String::new());
        }
    }
    // ownerStaffId is deliberately absent; users without HRM access must keep the existing relation.
    Ok(())
}

fn prepare_activity(body: &mut Value, _: &Value) -> Result<(), String> {
    required_text(body, "clientId", "取引先", 255)?;
    required_text(body, "content", "活動内容", 4000)?;
    ensure_nullable(body, &["occurredAt"]);
    body["createdBy"] = Value::Null;
    body["dealId"] = Value::Null;
    Ok(())
}

fn prepare_staff(body: &mut Value, _: &Value) -> Result<(), String> {
    required_text(body, "name", "氏名", 255)?;
    ensure_nullable(
        body,
        &["contractEndDate", "email", "hiredAt", "jobTitle", "phone"],
    );
    Ok(())
}

fn prepare_staff_update(body: &mut Value, original: &Value) -> Result<(), String> {
    prepare_staff(body, original)?;
    let stamp = original["updatedAt"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("更新日時を確認できません。スタッフ一覧を再読み込みしてください。")?;
    body["expectedUpdatedAt"] = Value::String(stamp.to_owned());
    Ok(())
}

fn valid_slug(body: &Value, key: &str, label: &str) -> Result<(), String> {
    required_text(body, key, label, 128)?;
    if !body[key]
        .as_str()
        .unwrap_or_default()
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(format!(
            "{label}は英小文字・数字・アンダースコアで入力してください。"
        ));
    }
    Ok(())
}

fn prepare_resource(body: &mut Value, _: &Value) -> Result<(), String> {
    required_text(body, "name", "名称", 255)?;
    if body["name"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .any(char::is_control)
    {
        return Err("名称に改行や制御文字は使えません。".into());
    }
    valid_slug(body, "resourceModel", "種別モデル")
}

fn prepare_reservation_type(body: &mut Value, original: &Value) -> Result<(), String> {
    prepare_resource(body, original)?;
    valid_slug(body, "code", "コード")?;
    let description = body["description"].as_str().unwrap_or_default();
    if description.chars().count() > 2000 || description.chars().any(char::is_control) {
        return Err("説明は改行を含めず2,000文字以内で入力してください。".into());
    }
    if body["phoneBookingEnabled"].as_bool() == Some(true) {
        let number = body["phoneBookingNumber"]
            .as_str()
            .unwrap_or_default()
            .trim();
        let digits = number.bytes().filter(u8::is_ascii_digit).count();
        if number.len() > 32
            || !(7..=20).contains(&digits)
            || !number
                .bytes()
                .all(|b| b.is_ascii_digit() || b" +-()".contains(&b))
        {
            return Err("電話番号は7〜20桁の数字とスペース・+ - ( )で入力してください。".into());
        }
    } else if let Some(map) = body.as_object_mut() {
        // API intentionally retains the stored number when phone booking is disabled.
        map.remove("phoneBookingNumber");
    }
    if let Some(policy) = body.get("quantityPolicy") {
        if policy["maxCount"].is_null() {
            if original["quantityPolicy"].is_null() {
                if let Some(map) = body.as_object_mut() {
                    map.remove("quantityPolicy");
                }
            } else {
                body["quantityPolicy"] = Value::Null;
            }
        } else if !policy["maxCount"]
            .as_u64()
            .is_some_and(|n| (1..=12).contains(&n))
        {
            return Err("数量上限は1〜12の整数で入力してください。".into());
        }
    }
    if let Some(presentation) = body.get("quantityPresentation") {
        let count = presentation["countLabel"].as_str().unwrap_or_default();
        let singular = presentation["singularLabel"].as_str().unwrap_or_default();
        if count.is_empty() && singular.is_empty() {
            if original["quantityPresentation"].is_null() {
                if let Some(map) = body.as_object_mut() {
                    map.remove("quantityPresentation");
                }
            } else {
                body["quantityPresentation"] = Value::Null;
            }
        } else if count.is_empty()
            || singular.is_empty()
            || count.chars().count() > 255
            || singular.chars().count() > 255
            || count.chars().chain(singular.chars()).any(char::is_control)
        {
            return Err(
                "数量の表示名と単数の表示名を、両方とも255文字以内で入力してください。".into(),
            );
        }
    }
    Ok(())
}

fn time_range(body: &Value, start: &str, end: &str) -> Result<(), String> {
    let from = chrono::DateTime::parse_from_rfc3339(body[start].as_str().unwrap_or_default())
        .map_err(|_| "開始日時を確認してください。")?;
    let to = chrono::DateTime::parse_from_rfc3339(body[end].as_str().unwrap_or_default())
        .map_err(|_| "終了日時を確認してください。")?;
    if to <= from {
        return Err("終了日時は開始日時より後にしてください。".into());
    }
    Ok(())
}

fn prepare_reservation_create(body: &mut Value, _: &Value) -> Result<(), String> {
    time_range(body, "startsAt", "endsAt")?;
    nonnegative_integer(body, "priceAmount", true)?;
    nonnegative_integer(body, "quantity", true)?;
    if body["quantity"].as_u64().unwrap_or_default() == 0 {
        return Err("人数・数量は1以上で入力してください。".into());
    }
    body["timezone"] = Value::String("Asia/Tokyo".into());
    Ok(())
}

fn prepare_reservation_edit(body: &mut Value, original: &Value) -> Result<(), String> {
    if !reservation_editable(original) {
        return Err("終了・取消済みの予約は編集できません。".into());
    }
    time_range(body, "startsAt", "endsAt")
}

fn reservation_editable(record: &Value) -> bool {
    matches!(
        record["status"].as_str(),
        Some(
            "requested"
                | "payment_pending"
                | "confirmed"
                | "change_requested"
                | "cancel_requested"
                | "waiting"
                | "admin_review"
                | "suspended"
        )
    )
}

fn reservation_can_cancel(record: &Value) -> bool {
    reservation_editable(record) && record["currency"].as_str().is_some_and(|c| c == "JPY")
}

fn prepare_reservation_cancel(body: &mut Value, original: &Value) -> Result<(), String> {
    if !reservation_can_cancel(original) {
        return Err("この予約はアプリから取り消せません。最新の状態を確認してください。".into());
    }
    required_text(body, "reason", "取消理由", 2000)?;
    nonnegative_integer(body, "cancellationFeeAmount", false)?;
    nonnegative_integer(body, "refundAmount", false)
}

fn prepare_waitlist(body: &mut Value, _: &Value) -> Result<(), String> {
    time_range(body, "desiredStartsAt", "desiredEndsAt")?;
    nonnegative_integer(body, "quantity", true)?;
    if body["quantity"].as_u64().unwrap_or_default() == 0 {
        return Err("人数・数量は1以上で入力してください。".into());
    }
    Ok(())
}

fn waitlist_waiting(record: &Value) -> bool {
    record["status"].as_str() == Some("waiting")
}
fn waitlist_seatable(record: &Value) -> bool {
    waitlist_waiting(record)
        && record["availableSlotGroups"]
            .as_u64()
            .is_some_and(|n| n > 0)
}
fn quote_is_draft(record: &Value) -> bool {
    status_is(record, "draft")
}
fn quote_can_accept(record: &Value) -> bool {
    status_is(record, "sent")
}
fn quote_can_cancel(record: &Value) -> bool {
    status_is(record, "draft") || status_is(record, "sent")
}
fn status_is(record: &Value, value: &str) -> bool {
    record["status"]
        .as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case(value))
}
fn issue_quote(body: &mut Value, _: &Value) -> Result<(), String> {
    body["status"] = json!("Sent");
    Ok(())
}
fn cancel_quote(body: &mut Value, _: &Value) -> Result<(), String> {
    body["status"] = json!("Cancelled");
    Ok(())
}
fn cancel_order(body: &mut Value, _: &Value) -> Result<(), String> {
    body["status"] = json!("cancelled");
    Ok(())
}
fn order_can_cancel(record: &Value) -> bool {
    record["id"]
        .as_str()
        .is_some_and(|id| id.starts_with("ord_"))
        && (status_is(record, "pending")
            || status_is(record, "confirmed")
            || status_is(record, "approval_needed"))
}
fn order_can_invoice(record: &Value) -> bool {
    (status_is(record, "confirmed")
        || status_is(record, "shipped")
        || status_is(record, "completed"))
        && record["convertedInvoiceId"].is_null()
}
fn form_published(record: &Value) -> bool {
    record["status"].as_str() == Some("published")
}
fn form_paused(record: &Value) -> bool {
    record["status"].as_str() == Some("paused")
}
fn reception_open(record: &Value) -> bool {
    record.get("cancelledAt").is_some_and(Value::is_null)
}
fn leave_can_cancel(record: &Value) -> bool {
    matches!(record["status"].as_str(), Some("requested" | "approved"))
}

fn preserve_card_contact(body: &mut Value, original: &Value) -> Result<(), String> {
    let id = original["contactPersonId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("名刺の担当者を確認できません。一覧を再読み込みしてください。")?;
    body["contactPersonId"] = json!(id);
    Ok(())
}

fn manual_contract_draft(original: &Value) -> bool {
    original["status"].as_str() == Some("draft")
        && original["sourceType"].as_str() == Some("manual")
}

fn prepare_contract(body: &mut Value) -> Result<(), String> {
    required_text(body, "contractNumber", "契約番号", 255)?;
    required_text(body, "title", "件名", 255)?;
    required_text(body, "clientName", "契約先の名称", 255)?;
    nonnegative_integer(body, "monthlyAmountYen", true)?;
    let start = chrono::NaiveDate::parse_from_str(
        body["startDate"].as_str().unwrap_or_default(),
        "%Y-%m-%d",
    )
    .map_err(|_| "開始日を確認してください。")?;
    if let Some(end) = body["endDate"].as_str().filter(|s| !s.is_empty()) {
        let end = chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d")
            .map_err(|_| "終了日を確認してください。")?;
        if end < start {
            return Err("終了日は開始日以降にしてください。".into());
        }
    }
    if let Some(lines) = body["lineItems"]
        .as_array()
        .filter(|lines| !lines.is_empty())
    {
        let mut total = 0_u64;
        for line in lines {
            required_text(line, "description", "品名・内容", 1000)?;
            let quantity = line["quantity"]
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or("明細の数量は1以上の整数で入力してください。")?;
            let price = line["unitPrice"]
                .as_u64()
                .ok_or("明細の単価は0以上の整数で入力してください。")?;
            let amount = quantity
                .checked_mul(price)
                .ok_or("明細の金額が大きすぎます。")?;
            total = total
                .checked_add(amount)
                .filter(|n| *n <= i64::MAX as u64)
                .ok_or("月額の合計が大きすぎます。")?;
        }
        body["monthlyAmountYen"] = json!(total);
    } else {
        body["lineItems"] = Value::Null;
    }
    ensure_nullable(body, &["clientId", "endDate", "terms", "notes"]);
    Ok(())
}

fn prepare_contract_create(body: &mut Value, _: &Value) -> Result<(), String> {
    prepare_contract(body)?;
    body["status"] = json!("draft");
    body["sourceType"] = json!("manual");
    body["sourceId"] = Value::Null;
    body["templateId"] = Value::Null;
    Ok(())
}

fn prepare_contract_update(body: &mut Value, original: &Value) -> Result<(), String> {
    if !manual_contract_draft(original) {
        return Err("手動で作成した下書き契約だけを編集できます。".into());
    }
    prepare_contract(body)?;
    body["status"] = json!("draft");
    body["sourceType"] = json!("manual");
    body["sourceId"] = original.get("sourceId").cloned().unwrap_or(Value::Null);
    body["templateId"] = original.get("templateId").cloned().unwrap_or(Value::Null);
    Ok(())
}

fn inquiry_version(body: &mut Value, original: &Value) -> Result<(), String> {
    let version = original["version"]
        .as_u64()
        .ok_or("お問い合わせの版を確認できません。再読み込みしてください。")?;
    body["expectedVersion"] = json!(version);
    Ok(())
}

fn inquiry_editable(original: &Value) -> bool {
    original["erased"].as_bool() == Some(false) && original["quarantined"].as_bool() == Some(false)
}

fn coupon_active(original: &Value) -> bool {
    original["isActive"].as_bool() == Some(true)
}

fn prepare_coupon(body: &mut Value, _: &Value) -> Result<(), String> {
    required_text(body, "code", "クーポンコード", 100)?;
    let code = body["code"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if !code
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(
            "クーポンコードは半角英数字・ハイフン・アンダースコアで入力してください。".into(),
        );
    }
    let discount = body["discountValue"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or("割引値は1以上の整数で入力してください。")?;
    if body["discountType"] == "PERCENTAGE" && discount > 100 {
        return Err("割引率は100%以下にしてください。".into());
    }
    nonnegative_integer(body, "minimumOrderAmountJpy", false)?;
    nonnegative_integer(body, "usageLimit", false)?;
    let minimum = body["minimumOrderAmountJpy"].as_u64().unwrap_or(0);
    let exact = minimum
        .checked_mul(1_000_000)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or("最低注文金額が大きすぎます。")?;
    body["minimumOrderAmount"] = if exact == 0 {
        Value::Null
    } else {
        json!(exact)
    };
    if let Some(map) = body.as_object_mut() {
        map.remove("minimumOrderAmountJpy");
    }
    if body["usageLimit"].as_u64().unwrap_or(0) == 0 {
        body["usageLimit"] = Value::Null;
    }
    body["code"] = json!(code);
    body["currency"] = json!("JPY");
    ensure_nullable(body, &["expiresAt", "exclusionGroup"]);
    Ok(())
}

fn leave_version(body: &mut Value, original: &Value) -> Result<(), String> {
    body["expectedVersion"] = original.get("version").cloned().unwrap_or(Value::Null);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::model;
    use super::*;

    #[test]
    fn client_round_trip_preserves_tags_and_does_not_echo_private_fields() {
        let original = json!({"tags":["常連"],"providers":[{"providerPrimaryId":"private"}]});
        let mut body = json!({"name":"株式会社例","listed":"unknown","corporateNumber":null,"address":{"state":"","city":"","address1":"","address2":null,"postalCode":""}});
        prepare_client(&mut body, &original).unwrap();
        assert_eq!(body["tags"], json!(["常連"]));
        assert!(body["address"].is_null());
        assert!(body["listed"].is_null());
        assert!(body.get("providers").is_none());
    }

    #[test]
    fn imported_customer_cannot_be_edited() {
        let mut body = json!({"name":"顧客"});
        assert!(prepare_customer_update(&mut body, &json!({"editable":false})).is_err());
        assert!(prepare_customer_update(&mut body, &json!({})).is_err());
        assert!(prepare_customer_update(&mut body, &json!({"editable":true})).is_ok());
    }

    #[test]
    fn deal_clears_optional_values_without_losing_staff_relation() {
        let mut body = json!({"name":"導入支援","clientId":"cl_1","amount":"9007199254740993","pipeline":"p_1","stage":"s_1","expectedCloseOn":null,"memo":null,"ownerName":null});
        prepare_deal_update(&mut body, &json!({"ownerStaffId":"staff_1"})).unwrap();
        assert_eq!(body["amount"], "9007199254740993");
        assert_eq!(body["clearExpectedCloseOn"], true);
        assert_eq!(body["memo"], "");
        assert_eq!(body["ownerName"], "");
        assert!(body.get("ownerStaffId").is_none());
        body["amount"] = json!("-1");
        assert!(prepare_deal_update(&mut body, &Value::Null).is_err());
    }

    #[test]
    fn staff_edit_requires_optimistic_concurrency_token() {
        let mut body = json!({"name":"職員"});
        assert!(prepare_staff_update(&mut body, &json!({})).is_err());
        prepare_staff_update(&mut body, &json!({"updatedAt":"2026-11-19T10:00:00Z"})).unwrap();
        assert_eq!(body["expectedUpdatedAt"], "2026-11-19T10:00:00Z");
    }

    #[test]
    fn every_staff_write_requires_an_explicit_hrm_management_grant() {
        let staff = RESOURCES.iter().find(|r| r.key == "staff").unwrap();
        for operation in staff
            .create
            .iter()
            .chain(staff.edit.iter())
            .chain(staff.actions)
        {
            assert_eq!(operation.capability, Some("hrm.manage"));
            assert!(!model::capability_allowed(
                *operation,
                &Value::Null,
                &Value::Null
            ));
            assert!(!model::capability_allowed(
                *operation,
                &json!({"hrm":{"manage":false}}),
                &Value::Null
            ));
            assert!(model::capability_allowed(
                *operation,
                &json!({"hrm":{"manage":true}}),
                &Value::Null
            ));
        }
    }

    #[test]
    fn phone_booking_disable_keeps_server_number_and_quantity_rules() {
        let mut body = json!({"name":"予約","code":"golf","resourceModel":"golf_course_holes","phoneBookingEnabled":false,"phoneBookingNumber":""});
        prepare_reservation_type(
            &mut body,
            &json!({"quantityPolicy":{"maxCount":4},"phoneBookingNumber":"011-000-0000"}),
        )
        .unwrap();
        assert!(body.get("phoneBookingNumber").is_none());
        assert!(body.get("quantityPolicy").is_none());
    }

    #[test]
    fn quantity_settings_can_be_cleared_without_creating_empty_rules() {
        let mut body = json!({"name":"予約","code":"golf","resourceModel":"golf_course_holes","phoneBookingEnabled":false,
            "quantityPolicy":{"maxCount":null},"quantityPresentation":{"countLabel":null,"singularLabel":null}});
        prepare_reservation_type(&mut body, &json!({"quantityPolicy":{"maxCount":4},"quantityPresentation":{"countLabel":"人数","singularLabel":"人"}})).unwrap();
        assert!(body["quantityPolicy"].is_null());
        assert!(body["quantityPresentation"].is_null());
        body["quantityPolicy"] = json!({"maxCount":13});
        assert!(prepare_reservation_type(&mut body, &Value::Null).is_err());
    }

    #[test]
    fn bookings_validate_chronological_order_not_timezone_text_order() {
        let mut body = json!({"startsAt":"2026-11-19T10:00:00+09:00","endsAt":"2026-11-19T02:00:00Z","quantity":1,"priceAmount":1000});
        prepare_reservation_create(&mut body, &Value::Null).unwrap();
        assert_eq!(body["timezone"], "Asia/Tokyo");
        body["endsAt"] = json!("2026-11-19T00:59:59Z");
        assert!(prepare_reservation_create(&mut body, &Value::Null).is_err());
    }

    #[test]
    fn waitlist_conversion_requires_a_real_available_slot() {
        assert!(!waitlist_seatable(&json!({"status":"waiting"})));
        assert!(!waitlist_seatable(
            &json!({"status":"waiting","availableSlotGroups":0})
        ));
        assert!(!waitlist_seatable(
            &json!({"status":"converted","availableSlotGroups":1})
        ));
        assert!(waitlist_seatable(
            &json!({"status":"waiting","availableSlotGroups":1})
        ));
    }

    #[test]
    fn inquiry_actions_use_loaded_version_not_a_user_supplied_number() {
        let mut body = json!({"state":"resolved","expectedVersion":1});
        inquiry_version(&mut body, &json!({"version":7})).unwrap();
        assert_eq!(body["expectedVersion"], 7);
        assert!(inquiry_version(&mut body, &json!({})).is_err());
    }

    #[test]
    fn cancellation_contract_has_stable_idempotency_and_jpy_guard() {
        let reservations = RESOURCES.iter().find(|r| r.key == "reservations").unwrap();
        assert_eq!(
            reservations.create.unwrap().idempotency,
            Idempotency::Body("idempotencyKey")
        );
        assert_eq!(reservations.actions[0].idempotency, Idempotency::Header);
        assert!(reservation_can_cancel(
            &json!({"status":"confirmed","currency":"JPY"})
        ));
        assert!(!reservation_can_cancel(
            &json!({"status":"confirmed","currency":"USD"})
        ));
        assert!(!reservation_can_cancel(
            &json!({"status":"cancelled","currency":"JPY"})
        ));
    }

    #[test]
    fn coupon_minimum_uses_exact_checked_minor_units() {
        let mut body = json!({"code":"welcome-10","discountType":"PERCENTAGE","discountValue":10,"minimumOrderAmountJpy":123456789,"usageLimit":0});
        prepare_coupon(&mut body, &Value::Null).unwrap();
        assert_eq!(body["minimumOrderAmount"], 123456789000000u64);
        assert_eq!(body["code"], "WELCOME-10");
        assert!(body["usageLimit"].is_null());
        assert!(body.get("minimumOrderAmountJpy").is_none());
        body["minimumOrderAmountJpy"] = json!(u64::MAX);
        assert!(prepare_coupon(&mut body, &Value::Null).is_err());
    }

    #[test]
    fn contract_draft_total_is_exact_and_cannot_activate_or_reassign_origin() {
        let mut body = json!({"contractNumber":"C-1","title":"利用契約","clientName":"例社","monthlyAmountYen":0,
            "startDate":"2026-11-19","endDate":null,"lineItems":[{"description":"月額","quantity":3,"unitPrice":9007199254740993u64}],
            "status":"active","sourceType":"provider_managed"});
        prepare_contract_create(&mut body, &Value::Null).unwrap();
        assert_eq!(body["monthlyAmountYen"], 27021597764222979u64);
        assert_eq!(body["status"], "draft");
        assert_eq!(body["sourceType"], "manual");
        assert!(
            prepare_contract_update(&mut body, &json!({"status":"active","sourceType":"manual"}))
                .is_err()
        );
        assert!(
            prepare_contract_update(&mut body, &json!({"status":"draft","sourceType":"quote"}))
                .is_err()
        );
        prepare_contract_update(
            &mut body,
            &json!({"status":"draft","sourceType":"manual","templateId":"tpl_1"}),
        )
        .unwrap();
        assert_eq!(body["templateId"], "tpl_1");
    }

    #[test]
    fn native_client_form_reads_aliases_and_preserves_untouched_fields() {
        let operation = RESOURCES
            .iter()
            .find(|r| r.key == "clients")
            .unwrap()
            .edit
            .unwrap();
        let original = json!({"name":"例社","corporationNumber":"1234567890123","listed":false,
            "headOfficeAddress":{"postalCode":"060-0001","state":"北海道","city":"札幌市","address1":"北一条","address2":null},
            "tags":["会員","法人"],"phoneNumber":"011-111-1111","capital":9007199254740993u64,
            "providers":[{"providerPrimaryId":"server-only"}]});
        let mut values = model::draft(operation.fields, &original);
        model::set_at(&mut values, "phoneNumber", json!(""));
        model::set_at(&mut values, "address.city", json!("小樽市"));
        let body = model::request_body(operation, &values, &original, "unchanged").unwrap();
        assert_eq!(body["corporateNumber"], "1234567890123");
        assert_eq!(body["listed"], false);
        assert_eq!(body["capital"], 9007199254740993u64);
        assert_eq!(body["tags"], json!(["会員", "法人"]));
        assert_eq!(body["address"]["city"], "小樽市");
        assert_eq!(body["address"]["address1"], "北一条");
        assert!(body["phoneNumber"].is_null());
        assert!(body.get("headOfficeAddress").is_none());
        assert!(body.get("providers").is_none());
    }

    #[test]
    fn native_deal_and_staff_edits_keep_exact_values_and_concurrency() {
        let operation = RESOURCES
            .iter()
            .find(|r| r.key == "deals")
            .unwrap()
            .edit
            .unwrap();
        let original = json!({"name":"導入支援","clientId":"cl_1","amount":"9007199254740993","pipeline":"p_1","stage":"s_1",
            "expectedCloseOn":"2026-12-01","ownerName":"担当者","ownerStaffId":"protected-staff","memo":"前のメモ"});
        let mut values = model::draft(operation.fields, &original);
        model::set_at(&mut values, "memo", json!(""));
        model::set_at(&mut values, "expectedCloseOn", json!(""));
        let body = model::request_body(operation, &values, &original, "key").unwrap();
        assert_eq!(body["amount"], "9007199254740993");
        assert_eq!(body["clearExpectedCloseOn"], true);
        assert_eq!(body["memo"], "");
        assert_eq!(body["ownerName"], "担当者");
        assert!(body.get("ownerStaffId").is_none());

        let operation = RESOURCES
            .iter()
            .find(|r| r.key == "staff")
            .unwrap()
            .edit
            .unwrap();
        let original = json!({"name":"職員","employmentStatus":"active","employmentType":"part_time",
            "updatedAt":"2026-11-19T10:00:00Z","jobTitle":"主任","attributesJson":{"protected":true}});
        let mut values = model::draft(operation.fields, &original);
        model::set_at(&mut values, "jobTitle", json!(""));
        let body = model::request_body(operation, &values, &original, "key").unwrap();
        assert_eq!(body["expectedUpdatedAt"], "2026-11-19T10:00:00Z");
        assert!(body["jobTitle"].is_null());
        assert!(body.get("attributesJson").is_none());
    }

    #[test]
    fn native_booking_creation_retries_with_the_same_identity() {
        let operation = RESOURCES
            .iter()
            .find(|r| r.key == "reservations")
            .unwrap()
            .create
            .unwrap();
        let mut values = model::draft(operation.fields, &Value::Null);
        for (key, value) in [
            ("reservationTypeId", "type_1"),
            ("customerName", "予約者"),
            ("startsAt", "2026-11-19 10:00"),
            ("endsAt", "2026-11-19 11:00"),
        ] {
            model::set_at(&mut values, key, json!(value));
        }
        let first = model::request_body(operation, &values, &Value::Null, "same-request").unwrap();
        let retry = model::request_body(operation, &values, &Value::Null, "same-request").unwrap();
        assert_eq!(first, retry);
        assert_eq!(first["idempotencyKey"], "same-request");
        assert_eq!(first["startsAt"], "2026-11-19T10:00:00+09:00");
        assert_eq!(first["timezone"], "Asia/Tokyo");
        assert_eq!(first["quantity"], 1);
        assert_eq!(first["priceAmount"], 0);
        assert!(first.get("status").is_none());
    }

    #[test]
    fn native_contract_lines_keep_exact_total_and_loaded_origin() {
        let operation = RESOURCES
            .iter()
            .find(|r| r.key == "sales-contracts")
            .unwrap()
            .edit
            .unwrap();
        let original = json!({"contractNumber":"C-1","title":"保守契約","clientName":"例社","monthlyAmountYen":0,
            "startDate":"2026-11-19","billingCycle":"monthly","status":"draft","sourceType":"manual",
            "sourceId":null,"templateId":"template_1","lineItems":[{"description":"保守","quantity":1,"unitPrice":9007199254740993u64}]});
        let mut values = model::draft(operation.fields, &original);
        model::set_at(&mut values, "lineItems.0.quantity", json!("3"));
        let body = model::request_body(operation, &values, &original, "key").unwrap();
        assert_eq!(body["monthlyAmountYen"], 27021597764222979u64);
        assert_eq!(body["templateId"], "template_1");
        assert_eq!(body["status"], "draft");
        assert_eq!(body["lineItems"][0]["quantity"], 3);
    }
}
