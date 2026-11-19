//! Menu visibility metadata verified against the Field web client.
//! Source: quantum-box/tachyonfield @ 73e645adebe3b21dddb83b6d4556ce58b6bee08f,
//! apps/client/src/app/navigation.ts. The API enforces authorization separately.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuGate {
    /// The menu is visible when at least one of these actions is not denied.
    pub actions: &'static [&'static str],
    pub feature_flag: Option<&'static str>,
    /// Any delivered key enables the menu; an unresolved key is not delivered.
    pub delivery_keys: &'static [&'static str],
    pub only_tenant_id: Option<&'static str>,
    pub saas_availability: bool,
}

const GATES: &[(&str, MenuGate)] = &[
    (
        "",
        MenuGate {
            actions: &[],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "decisions",
        MenuGate {
            actions: &[
                "field:ViewStrategy",
                "field:ReadWorkCases",
                "field:ListProcurement",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "dashboard",
        MenuGate {
            actions: &["field:ViewSalesAnalytics"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "store-ops",
        MenuGate {
            actions: &[],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/store-opening-check",
        MenuGate {
            actions: &["field:ViewStrategy"],
            feature_flag: Some("feature.field.strategy"),
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "work-reports",
        MenuGate {
            actions: &["field:ListWorkReports"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "work-reports/overview",
        MenuGate {
            actions: &["field:ListWorkReports"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "work-reports/register-analysis",
        MenuGate {
            actions: &["field:ListWorkReports"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "agent/chat",
        MenuGate {
            actions: &[],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "strategy",
        MenuGate {
            actions: &["field:ViewStrategy"],
            feature_flag: Some("feature.field.strategy"),
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "strategy/plans",
        MenuGate {
            actions: &["field:ViewStrategy"],
            feature_flag: Some("feature.field.strategy"),
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "strategy/knowledge",
        MenuGate {
            actions: &["field:ViewStrategy"],
            feature_flag: Some("feature.field.strategy"),
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/organization-chart",
        MenuGate {
            actions: &["field:ListAccounting"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "settings/ai-execution",
        MenuGate {
            actions: &["field:ReadDelegations"],
            feature_flag: Some("feature.field.strategy"),
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "settings/ai-evaluation",
        MenuGate {
            actions: &["field:ViewStrategy"],
            feature_flag: Some("feature.field.strategy"),
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "deals",
        MenuGate {
            actions: &["deal:List"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "sales-meetings",
        MenuGate {
            actions: &[
                "deal:List",
                "field:ListOrders",
                "field:ListSalesContracts",
                "field:ListInvoices",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "sales-outreach",
        MenuGate {
            actions: &["field:ListSalesActivities"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "quotations",
        MenuGate {
            actions: &["field:ListInvoices"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "orders",
        MenuGate {
            actions: &["field:ListOrders"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "sales-contracts",
        MenuGate {
            actions: &["field:ListSalesContracts"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "tenant-sales-classifications",
        MenuGate {
            actions: &["field:ListSalesContracts"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: Some("tn_01ks18jhh1xvggktfzjx5jqsen"),
            saas_availability: false,
        },
    ),
    (
        "consumer-orders",
        MenuGate {
            actions: &["field:ListOrders"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "agent",
        MenuGate {
            actions: &[
                "deal:List",
                "field:ListOrders",
                "field:ListSalesContracts",
                "field:ListInvoices",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/return-requests",
        MenuGate {
            actions: &[
                "deal:List",
                "field:ListOrders",
                "field:ListSalesContracts",
                "field:ListInvoices",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "store/pickup",
        MenuGate {
            actions: &[
                "deal:List",
                "field:ListOrders",
                "field:ListSalesContracts",
                "field:ListInvoices",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "store/coupons",
        MenuGate {
            actions: &[
                "deal:List",
                "field:ListOrders",
                "field:ListSalesContracts",
                "field:ListInvoices",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "library/consumers",
        MenuGate {
            actions: &["order:ListAllClients"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "library/clients",
        MenuGate {
            actions: &["order:ListAllClients"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "library/business-cards",
        MenuGate {
            actions: &["order:GetClient"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "pre-receptions",
        MenuGate {
            actions: &["field:ListMembership"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "usage-receptions",
        MenuGate {
            actions: &["field:ListMembership"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "usage-receptions/new",
        MenuGate {
            actions: &["field:ListMembership"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "crm/form-inquiries",
        MenuGate {
            actions: &["field:ReadFormInquiries"],
            feature_flag: Some("feature.field.forms"),
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "crm/forms",
        MenuGate {
            actions: &["field:ReadForms"],
            feature_flag: Some("feature.field.forms"),
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "membership/members",
        MenuGate {
            actions: &["field:ListMembership"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "ocr",
        MenuGate {
            actions: &[
                "order:ListAllClients",
                "field:ListMembership",
                "field:ReadFormInquiries",
                "field:ReadForms",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "reservations",
        MenuGate {
            actions: &["field:ListReservations"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "reservations/waitlist",
        MenuGate {
            actions: &["field:ListReservations"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "reservations/types",
        MenuGate {
            actions: &["field:ListReservations"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "reservations/resources",
        MenuGate {
            actions: &["field:ListReservations"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "reservations/plans",
        MenuGate {
            actions: &["field:ListReservations"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "reservations/daily-budgets",
        MenuGate {
            actions: &["field:ListReservations"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "hrm/staff",
        MenuGate {
            actions: &["field:ListHrm"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "hrm/shifts",
        MenuGate {
            actions: &["field:ListHrm"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "hrm/attendance",
        MenuGate {
            actions: &["field:ListHrm"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "hrm/attendance/jobcan-import",
        MenuGate {
            actions: &["field:ListHrm"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "hrm/me",
        MenuGate {
            actions: &[],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "products",
        MenuGate {
            actions: &["order:ListAllProducts"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "inventory",
        MenuGate {
            actions: &["field:ListInventory"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "inventory/low-stock",
        MenuGate {
            actions: &["field:ListInventory"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "inventory/expiring-lots",
        MenuGate {
            actions: &["field:ListInventory"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "inventory/transfers",
        MenuGate {
            actions: &["field:ListInventory"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "inventory/stocktakes",
        MenuGate {
            actions: &["field:ListInventory"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "inventory/locations",
        MenuGate {
            actions: &["field:ListInventory"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "inventory/stores",
        MenuGate {
            actions: &["field:ListInventory"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/vendors",
        MenuGate {
            actions: &["field:ListPurchaseLedger"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/purchase-orders",
        MenuGate {
            actions: &["field:ListProcurement"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "procurement/deliveries",
        MenuGate {
            actions: &["field:ListProcurement"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/recipe-cost/ingredients",
        MenuGate {
            actions: &[
                "order:ListAllProducts",
                "field:ListInventory",
                "field:ListProcurement",
                "field:ListPurchaseLedger",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/recipe-cost/receipts",
        MenuGate {
            actions: &[
                "order:ListAllProducts",
                "field:ListInventory",
                "field:ListProcurement",
                "field:ListPurchaseLedger",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/recipe-cost/recipes",
        MenuGate {
            actions: &[
                "order:ListAllProducts",
                "field:ListInventory",
                "field:ListProcurement",
                "field:ListPurchaseLedger",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/recipe-cost/estimates",
        MenuGate {
            actions: &[
                "order:ListAllProducts",
                "field:ListInventory",
                "field:ListProcurement",
                "field:ListPurchaseLedger",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/food-loss",
        MenuGate {
            actions: &[
                "order:ListAllProducts",
                "field:ListInventory",
                "field:ListProcurement",
                "field:ListPurchaseLedger",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/food-loss/dashboard",
        MenuGate {
            actions: &[
                "order:ListAllProducts",
                "field:ListInventory",
                "field:ListProcurement",
                "field:ListPurchaseLedger",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "billing",
        MenuGate {
            actions: &["accounting:ListInvoices"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "invoices",
        MenuGate {
            actions: &["accounting:ListInvoices"],
            feature_flag: None,
            delivery_keys: &["field.invoice.general"],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "usage-billing",
        MenuGate {
            actions: &["field:ListUsage"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "cancellation-fees",
        MenuGate {
            actions: &["field:ListCancellationFees"],
            feature_flag: None,
            delivery_keys: &["field.invoice.cancellation_fee"],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/ar-ap",
        MenuGate {
            actions: &["field:ListAccounting"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "saas-subscriptions",
        MenuGate {
            actions: &["saas_subscription:List"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "contracts",
        MenuGate {
            actions: &["field:ListAccounting"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/dashboard",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "accounting/operations",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "accounting/work-cases",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/sales-ledger",
        MenuGate {
            actions: &["field:ListSalesLedger"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/purchase-ledger",
        MenuGate {
            actions: &["field:ListPurchaseLedger"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/expenses",
        MenuGate {
            actions: &["expense:List"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/cash-book",
        MenuGate {
            actions: &["field:ListReceipts"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "accounting/statement-imports",
        MenuGate {
            actions: &["field:ListAccounting"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/received-invoices",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/payments",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "accounting/ledger/journal",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "accounting/ledger/general",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "accounting/ledger/trial-balance",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "accounting/opening-balances",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "accounting/accounts",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/receipts",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/journal-classifications",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/evidence",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "accounting/monthly-closing",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "accounting/review-requests",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "accounting/inquiries",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "audit-logs",
        MenuGate {
            actions: &[
                "field:ListAccounting",
                "field:ListSalesLedger",
                "field:ListPurchaseLedger",
                "expense:List",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "imports",
        MenuGate {
            actions: &["field:ListBridgeDefinitions"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "exports",
        MenuGate {
            actions: &["field:ListBridgeDefinitions"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "reports",
        MenuGate {
            actions: &[
                "field:ViewSalesAnalytics",
                "field:ListAccounting",
                "field:ViewProfitDashboard",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "analysis",
        MenuGate {
            actions: &[
                "field:ViewSalesAnalytics",
                "field:ListAccounting",
                "field:ViewProfitDashboard",
            ],
            feature_flag: Some("feature.field.external-data-analysis"),
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "analysis/data",
        MenuGate {
            actions: &[
                "field:ViewSalesAnalytics",
                "field:ListAccounting",
                "field:ViewProfitDashboard",
            ],
            feature_flag: Some("feature.field.external-data-analysis"),
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "reports/sales",
        MenuGate {
            actions: &[
                "field:ViewSalesAnalytics",
                "field:ListAccounting",
                "field:ViewProfitDashboard",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/budget-forecast",
        MenuGate {
            actions: &[
                "field:ViewSalesAnalytics",
                "field:ListAccounting",
                "field:ViewProfitDashboard",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/profit-scenarios",
        MenuGate {
            actions: &[
                "field:ViewSalesAnalytics",
                "field:ListAccounting",
                "field:ViewProfitDashboard",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/cash-schedule",
        MenuGate {
            actions: &[
                "field:ViewSalesAnalytics",
                "field:ListAccounting",
                "field:ViewProfitDashboard",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/vendor-spend",
        MenuGate {
            actions: &[
                "field:ViewSalesAnalytics",
                "field:ListAccounting",
                "field:ViewProfitDashboard",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/finance-reports",
        MenuGate {
            actions: &[
                "field:ViewSalesAnalytics",
                "field:ListAccounting",
                "field:ViewProfitDashboard",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/credit-control",
        MenuGate {
            actions: &[
                "field:ViewSalesAnalytics",
                "field:ListAccounting",
                "field:ViewProfitDashboard",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "erp/finance-settings",
        MenuGate {
            actions: &[
                "field:ViewSalesAnalytics",
                "field:ListAccounting",
                "field:ViewProfitDashboard",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "reports/automated",
        MenuGate {
            actions: &[
                "field:ViewSalesAnalytics",
                "field:ListAccounting",
                "field:ViewProfitDashboard",
            ],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "reports/cancellation-fees",
        MenuGate {
            actions: &["field:ListReservations"],
            feature_flag: None,
            delivery_keys: &["field.invoice.cancellation_fee"],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "it-governance/accounts",
        MenuGate {
            actions: &["field:ReadSaasAccounts"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: true,
        },
    ),
    (
        "settings",
        MenuGate {
            actions: &[],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "extensions",
        MenuGate {
            actions: &["field:ListExtensions"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
    (
        "billing-accounts",
        MenuGate {
            actions: &["field:ListBillingAccounts"],
            feature_flag: None,
            delivery_keys: &[],
            only_tenant_id: None,
            saas_availability: false,
        },
    ),
];

/// Use the most specific known navigation route, without treating similarly
/// named paths as descendants. Unknown routes do not acquire a default grant.
pub fn for_route(route: &str) -> Option<MenuGate> {
    let route = route.trim_start_matches('/').trim_end_matches('/');
    GATES
        .iter()
        .filter(|(prefix, _)| {
            route == *prefix
                || (!prefix.is_empty()
                    && route
                        .strip_prefix(*prefix)
                        .is_some_and(|rest| rest.starts_with('/')))
        })
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, gate)| *gate)
}

pub fn feature_flags() -> Vec<&'static str> {
    let mut keys: Vec<_> = GATES
        .iter()
        .filter_map(|(_, gate)| gate.feature_flag)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

pub fn delivery_keys() -> Vec<&'static str> {
    let mut keys: Vec<_> = GATES
        .iter()
        .flat_map(|(_, gate)| gate.delivery_keys.iter().copied())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_routes_keep_the_most_specific_permission() {
        assert_eq!(
            for_route("settings/ai-execution/setup").unwrap().actions,
            &["field:ReadDelegations"]
        );
        assert_eq!(
            for_route("settings/profile").unwrap().actions,
            &[] as &[&str]
        );
        assert_eq!(
            for_route("inventory/stocktakes/new").unwrap().actions,
            &["field:ListInventory"]
        );
        assert!(for_route("inventory-secret").is_none());
        assert!(for_route("unregistered").is_none());
    }

    #[test]
    fn tenant_and_contract_restrictions_are_separate_from_menu_permissions() {
        assert!(
            for_route("tenant-sales-classifications")
                .unwrap()
                .only_tenant_id
                .is_some()
        );
        assert!(
            for_route("it-governance/accounts")
                .unwrap()
                .saas_availability
        );
        assert_eq!(
            for_route("strategy/knowledge").unwrap().feature_flag,
            Some("feature.field.strategy")
        );
        assert_eq!(
            for_route("cancellation-fees").unwrap().delivery_keys,
            &["field.invoice.cancellation_fee"]
        );
    }
}
