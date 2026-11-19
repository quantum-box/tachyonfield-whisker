//! Read-only line items with the same labels and exact amounts as the editors.
//! Reference values are resolved to names; internal record identifiers stay hidden.

use crate::cash_book_screen::spawn_in;
use crate::management::model;
use crate::management::schema::{Field, InputKind, Lookup, Resource};
use crate::*;
use whisker::Owner;

const PRODUCT_LOOKUP: Lookup = Lookup {
    path: "/v1/order/products?limit=100",
    list_key: "items",
    value_key: "id",
    label_keys: &["name", "skuCode"],
};

const QUOTATION_LINES: &[Field] = &[
    Field::text("description", "品名・内容"),
    Field::new("productId", "商品", InputKind::Reference(PRODUCT_LOOKUP)),
    Field::new("quantity", "数量", InputKind::Decimal),
    Field::text("unit", "単位"),
    Field::new("unitPrice", "単価", InputKind::Money),
    Field::new("discountAmount", "値引額", InputKind::Money),
    Field::new("taxRate", "税率（%）", InputKind::Decimal),
];
const ORDER_LINES: &[Field] = &[
    Field::text("description", "品名・内容"),
    Field::text("sku", "SKU"),
    Field::new("quantity", "数量", InputKind::Decimal),
    Field::new("unitPrice", "単価", InputKind::Money),
    Field::new("amount", "金額", InputKind::Money),
];
const JOURNAL_LINES: &[Field] = &[
    Field::text("accountName", "勘定科目"),
    Field::text("accountCode", "科目コード"),
    Field::text("description", "摘要"),
    Field::new("debitAmount", "借方金額", InputKind::Decimal),
    Field::new("creditAmount", "貸方金額", InputKind::Decimal),
    Field::text("taxCategory", "税区分"),
    Field::new(
        "taxResolutionStatus",
        "税区分の確認",
        InputKind::Select(&[
            crate::management::schema::Choice::new("unresolved", "未確認"),
            crate::management::schema::Choice::new("resolved", "確認済み"),
        ]),
    ),
];
const ESTIMATE_LINES: &[Field] = &[
    Field::text("name", "材料・仕込み品"),
    Field::new("quantity", "使用量", InputKind::Decimal),
    Field::new("unitCost", "単位あたりの原価（円）", InputKind::Decimal),
    Field::new("cost", "原価（円）", InputKind::Decimal),
    Field::text("optionKey", "追加オプション"),
];
const ESTIMATE_MISSING: &[Field] = &[
    Field::text("name", "材料・仕込み品"),
    Field::text("message", "未計算の理由"),
];
const ESTIMATE_OPTIONS: &[Field] = &[
    Field::text("label", "オプション名"),
    Field::new("additionalCost", "追加原価（円）", InputKind::Decimal),
    Field::new(
        "additionalPriceExcludingTax",
        "追加の税抜売価（円）",
        InputKind::Decimal,
    ),
];

const CANDIDATE_LINES: &[Field] = &[
    Field::text("skuName", "品名"),
    Field::text("sku", "SKU"),
    Field::new("quantity", "数量", InputKind::Integer),
    Field::new("unitPrice", "単価", InputKind::Decimal),
    Field::new("amount", "金額", InputKind::Decimal),
    Field::text("taxCategory", "税区分"),
];
const RECEIVED_TAX_RATES: &[crate::management::schema::Choice] = &[
    crate::management::schema::Choice::new("standard_10", "10%"),
    crate::management::schema::Choice::new("reduced_8", "軽減8%"),
    crate::management::schema::Choice::new("exempt", "非課税・免税"),
];
const RECEIVED_LINES: &[Field] = &[
    Field::text("description", "品名・内容"),
    Field::new("quantity", "数量", InputKind::Decimal),
    Field::new("unitPrice", "単価", InputKind::Decimal),
    Field::new("amount", "金額", InputKind::Decimal),
    Field::new("taxRate", "税率", InputKind::Select(RECEIVED_TAX_RATES)),
];
const RECEIVED_TAX_TOTALS: &[Field] = &[
    Field::new("taxRate", "税率", InputKind::Select(RECEIVED_TAX_RATES)),
    Field::new("taxableAmount", "対象金額", InputKind::Decimal),
    Field::new("taxAmount", "税額", InputKind::Decimal),
];

#[derive(Clone, Debug, PartialEq)]
struct LineDefinition {
    key: &'static str,
    label: &'static str,
    fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq)]
struct DisplaySection {
    key: &'static str,
    label: &'static str,
    rows: Vec<DisplayLine>,
}

#[derive(Clone, Debug, PartialEq)]
struct DisplayLine {
    index: usize,
    fields: Vec<(usize, String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
enum NameResult {
    Loading,
    Ready(Vec<(String, String)>),
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
struct LookupState {
    lookup: Lookup,
    result: NameResult,
}

fn line_definitions(resource: Resource) -> Vec<LineDefinition> {
    let mut sections = Vec::<LineDefinition>::new();
    for operation in [resource.edit, resource.create].into_iter().flatten() {
        for field in operation.fields {
            let InputKind::Lines(children) = field.kind else {
                continue;
            };
            if sections.iter().any(|section| section.key == field.read_key) {
                continue;
            }
            let mut fields = children.to_vec();
            if matches!(resource.key, "purchase-orders" | "invoices")
                && !fields.iter().any(|field| field.key == "amount")
            {
                fields.push(Field::new("amount", "金額", InputKind::Money));
            }
            sections.push(LineDefinition {
                key: field.read_key,
                label: field.label,
                fields,
            });
        }
    }
    let fallback: &[Field] = match resource.key {
        "quotations" => &[Field::new(
            "items",
            "見積明細",
            InputKind::Lines(QUOTATION_LINES),
        )],
        "orders" => &[Field::new(
            "items",
            "受注明細",
            InputKind::Lines(ORDER_LINES),
        )],
        "journal-entries" => &[Field::new(
            "lines",
            "仕訳明細",
            InputKind::Lines(JOURNAL_LINES),
        )],
        "journal-candidates" => &[Field::new(
            "lines",
            "仕訳候補の明細",
            InputKind::Lines(CANDIDATE_LINES),
        )],
        "received-invoices" => &[
            Field::new(
                "invoice.content.lines",
                "請求明細",
                InputKind::Lines(RECEIVED_LINES),
            ),
            Field::new(
                "invoice.content.taxSummaries",
                "税率ごとの金額",
                InputKind::Lines(RECEIVED_TAX_TOTALS),
            ),
        ],
        "recipe-estimates" => &[
            Field::new("lines", "材料ごとの原価", InputKind::Lines(ESTIMATE_LINES)),
            Field::new(
                "missing",
                "未計算の材料",
                InputKind::Lines(ESTIMATE_MISSING),
            ),
            Field::new(
                "optionCosts",
                "追加オプションの原価",
                InputKind::Lines(ESTIMATE_OPTIONS),
            ),
        ],
        _ => &[],
    };
    for field in resource.read_lines.iter().chain(fallback) {
        if sections.iter().any(|section| section.key == field.read_key) {
            continue;
        }
        if let InputKind::Lines(children) = field.kind {
            sections.push(LineDefinition {
                key: field.read_key,
                label: field.label,
                fields: children.to_vec(),
            });
        }
    }
    sections
}

/// Used by the enclosing detail screen to avoid repeating array summaries.
pub(crate) fn expanded_line_keys(resource: Resource) -> Vec<&'static str> {
    line_definitions(resource)
        .into_iter()
        .map(|section| section.key)
        .collect()
}

fn required_lookups(definitions: &[LineDefinition], record: &Value) -> Vec<Lookup> {
    let mut result = Vec::new();
    for section in definitions {
        let Some(rows) = model::value_at(record, section.key).as_array() else {
            continue;
        };
        for field in &section.fields {
            let lookup = match field.kind {
                InputKind::Reference(lookup) | InputKind::DependentReference { lookup, .. } => {
                    lookup
                }
                _ => continue,
            };
            if rows
                .iter()
                .any(|row| !model::input_text(model::value_at(row, field.read_key)).is_empty())
                && !result.contains(&lookup)
            {
                result.push(lookup);
            }
        }
    }
    result
}

fn line_value(
    resource: Resource,
    field: Field,
    row: &Value,
    record: &Value,
    lookups: &[LookupState],
) -> String {
    if matches!(field.kind, InputKind::Hidden | InputKind::Lines(_)) {
        return String::new();
    }
    let value = model::value_at(row, field.read_key);
    if value.is_null() {
        return String::new();
    }
    match field.kind {
        InputKind::Reference(lookup) | InputKind::DependentReference { lookup, .. } => {
            let id = model::input_text(value);
            if id.is_empty() {
                return String::new();
            }
            match lookups
                .iter()
                .find(|state| state.lookup == lookup)
                .map(|state| &state.result)
            {
                Some(NameResult::Ready(options)) => options
                    .iter()
                    .find(|(key, _)| *key == id)
                    .map(|(_, name)| name.clone())
                    .unwrap_or_else(|| "名称を確認できません".into()),
                Some(NameResult::Failed) => "名称を確認できません".into(),
                _ => "名称を読み込み中…".into(),
            }
        }
        InputKind::Money => {
            let mut scoped = row.clone();
            let nested_currency = field.read_key.rsplit_once('.').and_then(|(parent, _)| {
                let parent = model::value_at(row, parent);
                parent
                    .get("currency")
                    .or_else(|| parent.get("currencyCode"))
                    .filter(|v| !v.is_null())
                    .cloned()
            });
            if let Some(currency) = nested_currency {
                scoped["currency"] = currency;
            } else if scoped["currency"].is_null() {
                scoped["currency"] = record
                    .get("currency")
                    .or_else(|| record.get("currencyCode"))
                    .cloned()
                    .unwrap_or_else(|| Value::String("JPY".into()));
            }
            model::display_field(resource, &scoped, &format!("@money:{}", field.read_key))
        }
        InputKind::Select(choices) => {
            let selected = model::input_text(value);
            choices
                .iter()
                .find(|choice| choice.value == selected)
                .map(|choice| choice.label.to_owned())
                .unwrap_or_else(|| "確認が必要な設定".into())
        }
        _ => {
            // Optional exact strings take precedence over older numeric projections.
            if field.read_key == "quantity" {
                if let Some(exact) = row["quantityDecimal"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                {
                    return exact.into();
                }
            }
            if matches!(field.read_key, "debitAmount" | "creditAmount") {
                if let Some(exact) = row[format!("{}Str", field.read_key)]
                    .as_str()
                    .filter(|text| !text.is_empty())
                {
                    return exact.into();
                }
            }
            model::display(value)
        }
    }
}

fn displayed_sections(
    resource: Resource,
    definitions: &[LineDefinition],
    record: &Value,
    lookups: &[LookupState],
) -> Vec<DisplaySection> {
    definitions
        .iter()
        .filter_map(|section| {
            let rows = model::value_at(record, section.key).as_array()?;
            let rows: Vec<_> = rows
                .iter()
                .enumerate()
                .filter(|(_, row)| row.is_object())
                .map(|(index, row)| {
                    let fields = section
                        .fields
                        .iter()
                        .enumerate()
                        .filter_map(|(i, field)| {
                            let text = line_value(resource, *field, row, record, lookups);
                            (!text.is_empty()).then(|| (i, field.label.to_owned(), text))
                        })
                        .collect();
                    DisplayLine { index, fields }
                })
                .collect();
            (!rows.is_empty()).then(|| DisplaySection {
                key: section.key,
                label: section.label,
                rows,
            })
        })
        .collect()
}

#[component]
pub(crate) fn record_lines(
    resource: Resource,
    record: ReadSignal<Value>,
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
) -> Element {
    let definitions = signal(crate::management::detail::line_definitions(resource));
    let names = signal(Vec::<crate::management::detail::LookupState>::new());
    let lookup_scope = signal(None::<Tenant>);
    let scope_generation = signal(0_u64);
    let retry = signal(0_u64);
    let owner = Owner::current();
    effect(move || {
        let _retry = retry.get();
        let record = record.get();
        let lookups =
            crate::management::detail::required_lookups(&definitions.get_untracked(), &record);
        let current = tenant.get();
        let signed_in = token.with(Option::is_some);
        if !signed_in || current.is_none() {
            if lookup_scope.with_untracked(Option::is_some) {
                scope_generation.update(|value| *value += 1);
            }
            names.set(Vec::new());
            lookup_scope.set(None);
            return;
        }
        if lookup_scope.get_untracked() != current {
            scope_generation.update(|value| *value += 1);
            names.set(Vec::new());
            lookup_scope.set(current.clone());
        }
        let current = current.unwrap();
        for lookup in lookups {
            if names.with_untracked(|states| {
                states.iter().any(|state| {
                    state.lookup == lookup
                        && !matches!(state.result, crate::management::detail::NameResult::Failed)
                })
            }) {
                continue;
            }
            names.update(|states| {
                states.retain(|state| state.lookup != lookup);
                states.push(crate::management::detail::LookupState {
                    lookup,
                    result: crate::management::detail::NameResult::Loading,
                });
            });
            let context = current.clone();
            let current = current.clone();
            let request_scope = scope_generation.get_untracked();
            spawn_in(owner, async move {
                let result = authed(token, move |access| {
                    model::fetch_lookup(access, &current, lookup)
                })
                .await;
                if !owner.is_none_or(Owner::is_alive)
                    || tenant.get_untracked().as_ref() != Some(&context)
                    || token.with_untracked(Option::is_none)
                    || scope_generation.get_untracked() != request_scope
                {
                    return;
                }
                let result =
                    result.and_then(|(data, _)| model::lookup_options(lookup, &data, None));
                names.update(|states| {
                    if let Some(state) = states.iter_mut().find(|state| state.lookup == lookup) {
                        state.result = match result {
                            Ok(options) => crate::management::detail::NameResult::Ready(options),
                            Err(_) => crate::management::detail::NameResult::Failed,
                        };
                    }
                });
            });
        }
    });
    let sections = computed(move || {
        crate::management::detail::displayed_sections(
            resource,
            &definitions.get(),
            &record.get(),
            &names.get(),
        )
    });
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
            Show(when: move || names.with(|states| states.iter().any(|state| matches!(state.result, crate::management::detail::NameResult::Failed)))) {
                Text(value: "名称を再読み込み", style: computed(move || css!(color: hex(TINT), font_size: px(15), padding: px(16))), on_tap: move |_| retry.update(|value| *value += 1))
            }
            ForEach(
                each: move || sections.get(),
                key: |section: &crate::management::detail::DisplaySection| section.key,
                children: move |section: ReadSignal<crate::management::detail::DisplaySection>| render! {
                    View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column)) {
                        SectionHeader(title: computed(move || section.get().label.to_owned()))
                        ForEach(
                            each: move || section.get().rows,
                            key: |row: &crate::management::detail::DisplayLine| row.index,
                            children: move |row: ReadSignal<crate::management::detail::DisplayLine>| render! {
                                View(style: computed(move || group_style().margin_bottom(px(10)))) {
                                    Text(value: computed(move || format!("明細 {}", row.get().index + 1)), style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(13), padding: (px(10), px(16)))))
                                    ForEach(
                                        each: move || row.get().fields,
                                        key: |field: &(usize, String, String)| field.0,
                                        children: move |field: ReadSignal<(usize, String, String)>| render! {
                                            View(style: computed(move || css!(display: Display::Flex, flex_direction: FlexDirection::Column, gap: px(3), padding: (px(10), px(16))).border_bottom(hairline()))) {
                                                Text(value: computed(move || field.get().1), style: computed(move || css!(color: hex(SECONDARY_LABEL), font_size: px(13))))
                                                Text(value: computed(move || field.get().2), style: computed(move || css!(color: hex(LABEL), font_size: px(17))))
                                            }
                                        },
                                    )
                                }
                            },
                        )
                    }
                },
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn line_details_hide_ids_and_keep_currency_and_fractional_quantity_exact() {
        let resource = crate::management::resources()
            .into_iter()
            .find(|resource| resource.key == "invoices")
            .unwrap();
        let record = json!({"currency":"USD", "lineItems":[{"description":"作業", "quantity":1, "quantityDecimal":"1.250000000000000001", "unitPrice":12345, "amount":15431, "internalSecret":"private"}]});
        let sections = displayed_sections(resource, &line_definitions(resource), &record, &[]);
        let fields = &sections[0].rows[0].fields;
        assert!(
            fields
                .iter()
                .any(|(_, label, value)| label == "数量" && value == "1.250000000000000001")
        );
        assert!(
            fields
                .iter()
                .any(|(_, label, value)| label == "単価" && value == "123.45 USD")
        );
        assert!(
            fields
                .iter()
                .any(|(_, label, value)| label == "金額" && value == "154.31 USD")
        );
        assert!(!format!("{sections:?}").contains("private"));
    }

    #[test]
    fn material_names_resolve_without_exposing_reference_ids() {
        let resource = crate::management::resources()
            .into_iter()
            .find(|resource| resource.key == "recipes")
            .unwrap();
        let record = json!({"lines":[{"id":"internal-row", "ingredientId":"internal-ingredient", "amount":"0.0000000001", "unit":"g", "weightBasis":"as_purchased"}]});
        let definitions = line_definitions(resource);
        let lookups = required_lookups(&definitions, &record);
        assert_eq!(lookups.len(), 1);
        let pending = displayed_sections(resource, &definitions, &record, &[]);
        assert!(!format!("{pending:?}").contains("internal-ingredient"));
        let loaded = displayed_sections(
            resource,
            &definitions,
            &record,
            &[LookupState {
                lookup: lookups[0],
                result: NameResult::Ready(vec![("internal-ingredient".into(), "小麦粉".into())]),
            }],
        );
        assert!(
            loaded[0].rows[0]
                .fields
                .iter()
                .any(|(_, label, value)| label == "材料" && value == "小麦粉")
        );
        assert!(
            loaded[0].rows[0]
                .fields
                .iter()
                .any(|(_, label, value)| label == "使用量" && value == "0.0000000001")
        );
        assert!(!format!("{loaded:?}").contains("internal-row"));
    }

    #[test]
    fn read_only_journal_lines_use_exact_amount_strings() {
        let resource = crate::management::resources()
            .into_iter()
            .find(|resource| resource.key == "journal-entries")
            .unwrap();
        let record = json!({"lines":[{"accountId":"private-account", "accountName":"現金", "accountCode":"100", "debitAmount":1, "debitAmountStr":"9007199254740993", "creditAmount":0, "taxResolutionStatus":"unresolved"}]});
        let sections = displayed_sections(resource, &line_definitions(resource), &record, &[]);
        assert!(
            sections[0].rows[0]
                .fields
                .iter()
                .any(|(_, label, value)| label == "借方金額" && value == "9007199254740993")
        );
        assert!(!format!("{sections:?}").contains("private-account"));
    }

    #[test]
    fn received_invoice_decimals_are_not_rescaled_as_minor_units() {
        let resource = crate::management::resources()
            .into_iter()
            .find(|resource| resource.key == "received-invoices")
            .unwrap();
        let record = json!({"invoice":{"content":{"currency":"USD", "lines":[{"description":"食材", "quantity":"1.000000000001", "unitPrice":"123.4567890123456789", "amount":"123.4567890123456789", "taxRate":"reduced_8"}], "payee":{"registeredAccountId":"private-bank"}}}});
        let sections = displayed_sections(resource, &line_definitions(resource), &record, &[]);
        assert!(
            sections[0].rows[0]
                .fields
                .iter()
                .any(|(_, label, value)| label == "金額" && value == "123.4567890123456789")
        );
        assert!(
            sections[0].rows[0]
                .fields
                .iter()
                .any(|(_, label, value)| label == "税率" && value == "軽減8%")
        );
        assert!(!format!("{sections:?}").contains("private-bank"));
    }
}
