//! The web's single-record document intake currently creates a quotation draft.
//! Extraction fills editable native fields and never submits a business record.
use super::schema::Resource;
use super::{files, model, schema::*};
use crate::cash_book_screen::spawn_in;
use crate::*;
use serde_json::json;
use whisker::Owner;

const LINES: &[Field] = &[
    Field::text("description", "品目名").required(),
    Field::new("quantity", "数量", InputKind::Integer).required(),
    Field::new("unitPrice", "単価（円）", InputKind::Money).required(),
    Field::new("discountAmount", "値引（円）", InputKind::Money).default("0"),
    Field::new("taxRate", "税率（%）", InputKind::Integer)
        .required()
        .default("10"),
    Field::text("unit", "単位").default("式"),
];
const FIELDS: &[Field] = &[
    Field::text("quotationNumber", "件名").required(),
    Field::new("currency", "通貨", InputKind::ReadOnly).default("JPY"),
    Field::new(
        "clientId",
        "取引先",
        InputKind::Reference(Lookup {
            path: "/v1/erp/clients",
            list_key: "items",
            value_key: "id",
            label_keys: &["name"],
        }),
    ),
    Field::new("items", "明細", InputKind::Lines(LINES)).required(),
    Field::new("notes", "備考", InputKind::Multiline).empty(Empty::String),
];
fn extraction_schema() -> Value {
    json!([
        {"key":"title","label":"件名","fieldType":"text","options":[],"itemFields":[]},
        {"key":"line_items","label":"明細","fieldType":"items","options":[],"itemFields":[
            {"key":"name","label":"品目名","fieldType":"text","options":[]},
            {"key":"quantity","label":"数量","fieldType":"number","options":[]},
            {"key":"unit_price","label":"単価","fieldType":"number","options":[]},
            {"key":"discount","label":"値引","fieldType":"number","options":[]}
        ]}
    ])
}
fn extracted(value: &Value) -> Result<Value, String> {
    let fields = &value["fields"];
    let rows = fields["line_items"]
        .as_array()
        .ok_or("見積明細を読み取れませんでした。手入力で補ってください")?;
    if rows.len() > 200 {
        return Err("明細が多すぎます。書類を分けて取り込んでください".into());
    }
    let rows:Vec<_>=rows.iter().map(|r|json!({
        "description":r["name"],"quantity":r["quantity"],"unitPrice":r["unit_price"],"discountAmount":r["discount"].as_str().filter(|s|!s.is_empty()).unwrap_or("0"),"taxRate":"10","unit":"式"
    })).collect();
    Ok(json!({"quotationNumber":fields["title"],"currency":"JPY","items":rows,"notes":""}))
}
fn prepare(body: &mut Value, _: &Value) -> Result<(), String> {
    let mut total_by_rate = std::collections::BTreeMap::<i64, i128>::new();
    for row in body["items"]
        .as_array_mut()
        .ok_or("明細を追加してください")?
    {
        let quantity = row["quantity"]
            .as_i64()
            .filter(|n| *n > 0)
            .ok_or("数量は1以上の整数で入力してください")?;
        let price = row["unitPrice"]
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or("単価は0以上の整数で入力してください")?;
        let discount = row["discountAmount"].as_i64().unwrap_or(0);
        let rate = row["taxRate"]
            .as_i64()
            .filter(|n| (0..=100).contains(n))
            .ok_or("税率は0〜100で入力してください")?;
        if discount < 0 {
            return Err("値引は0以上で入力してください".into());
        }
        let amount = quantity as i128 * price as i128 - discount as i128;
        if !(0..=i64::MAX as i128).contains(&amount) {
            return Err("明細の金額または値引を確認してください".into());
        }
        *total_by_rate.entry(rate).or_default() += amount;
        row["productId"] = Value::Null;
        row["discountAmount"] = json!(discount);
        if row["unit"].as_str().is_none_or(str::is_empty) {
            row["unit"] = Value::Null;
        }
    }
    let tax: i128 = total_by_rate
        .iter()
        .map(|(rate, total)| total * (*rate as i128) / 100)
        .sum();
    if tax > i64::MAX as i128 || total_by_rate.values().sum::<i128>() + tax > i64::MAX as i128 {
        return Err("見積の合計が大きすぎます".into());
    }
    body["taxAmount"] = json!(tax as i64);
    body["currency"] = json!("JPY");
    if body["clientId"].as_str().is_none_or(str::is_empty) {
        body["clientId"] = json!("fieldv2-unassigned-client");
    }
    body["clientName"] = Value::Null;
    body["clientEmail"] = Value::Null;
    // Preserve the same explicit template choice used by the web's intake form.
    let notes = body["notes"].as_str().unwrap_or("").to_owned();
    body["notes"] = json!(format!(
        "__field_quotation_template:builtin-simple\n{notes}"
    ));
    Ok(())
}
fn created(value: &Value) -> Result<(), String> {
    if value["id"].as_str().is_some_and(|v| !v.is_empty()) {
        Ok(())
    } else {
        Err("見積の作成結果を確認できませんでした。一覧を確認してください".into())
    }
}
pub const RESOURCES:&[Resource]=&[
    Resource::new("document-intake","顧客管理","書類取込（見積書）","/v1/erp/quotations","items")
        .pagination(Pagination::Offset{limit:100}).title(&["quotationNumber"]).subtitle(&["clientName"]).status("status")
        .detail("/v1/erp/quotations/{id}","").fields(&[("件名","quotationNumber"),("取引先","clientName"),("状態","status"),("通貨","currency"),("合計","@money:totalAmount")])
        .read_lines(&[Field::new("items","見積明細",InputKind::Lines(LINES))])
        .create(Mutation::new("内容を確認して見積を作成","POST","/v1/erp/quotations",FIELDS).prepare(prepare).validate_response(created)
            .document(DocumentPrefill{entity:"quotation",schema:extraction_schema,convert:extracted})
            .explain("読み取り結果は下書きです。原本と件名・品目・数量・金額・税率を確認してください。登録後の発行は見積の画面から行います。"))
        .route("ocr"),
];

#[component]
pub(crate) fn document_input(
    operation: Mutation,
    draft: RwSignal<Value>,
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
) -> Element {
    let Some(prefill) = operation.document else {
        return render! {View()};
    };
    let selected = signal(None::<field_glass::PickedFile>);
    let busy = signal(false);
    let error = signal(String::new());
    let warning = signal(String::new());
    let owner = Owner::current();
    let load = move || {
        if busy.get_untracked() {
            return;
        }
        let (Some(file), Some(current)) = (selected.get_untracked(), tenant.get_untracked()) else {
            return;
        };
        let ty = match files::content_type(&file) {
            Ok(ty) => ty,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let context = current.clone();
        busy.set(true);
        error.set(String::new());
        warning.set(String::new());
        // A new original replaces the old form only after the explicit extraction action.
        let mut empty = model::draft(operation.fields, &Value::Null);
        empty["_nativeUploadsInFlight"] = json!(1);
        draft.set(empty);
        spawn_in(owner, async move {
            let result = authed(token, move |access| {
                let bytes = files::read(&file, 6 * 1024 * 1024)?;
                api::extract_management_document(
                    access,
                    &current,
                    prefill.entity,
                    &(prefill.schema)(),
                    &file.name,
                    ty,
                    &bytes,
                )
            })
            .await;
            if !owner.is_none_or(Owner::is_alive)
                || tenant.get_untracked().as_ref() != Some(&context)
                || token.with_untracked(Option::is_none)
            {
                return;
            }
            busy.set(false);
            draft.update(|v| v["_nativeUploadsInFlight"] = json!(0));
            match result {
                Ok(response) => match (prefill.convert)(&response) {
                    Ok(value) => {
                        draft.set(model::draft(operation.fields, &value));
                        let warnings = response["warnings"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n");
                        warning.set(format!(
                            "読み取りました。原本と照らし合わせて内容を確認してください。{}",
                            if warnings.is_empty() {
                                String::new()
                            } else {
                                format!("\n{warnings}")
                            }
                        ));
                    }
                    Err(message) => error.set(message),
                },
                Err(message) => error.set(message),
            }
        });
    };
    render! {
        View(style:computed(move||group_style().padding(px(16)))){
            Text(value:"見積書の写真・PDFから入力",style:computed(move||css!(color:hex(LABEL),font_size:px(17))))
            Show(when:move||!busy.get()){
                field_glass::AttachButton(on_pick:move|file:field_glass::PickedFile|{selected.set(Some(file));error.set(String::new());warning.set("新しい書類はまだ入力へ反映されていません。読み取り、または手入力を選んでください。".into());draft.update(|v|v["_nativeDocumentPending"]=json!(true));},style:css!(width:px(44),height:px(44)))
            }
            Show(when:move||selected.with(Option::is_some)){
                Text(value:computed(move||selected.with(|f|f.as_ref().map(|f|f.name.clone()).unwrap_or_default())),style:computed(move||css!(color:hex(LABEL),font_size:px(14))))
                field_glass::DocumentPreview(url:computed(move||selected.with(|f|f.as_ref().map(|f|format!("file://{}",f.path)).unwrap_or_default())),style:css!(width:percent(100),height:px(280)))
                Show(when:move||!busy.get()){
                    Text(value:"この書類を読み取り、入力を置き換える",style:computed(move||css!(color:hex(TINT),font_size:px(16),padding:px(12))),on_tap:move|_|load())
                    Show(when:move||draft.with(|v|v["_nativeDocumentPending"].as_bool()==Some(true))){
                        Text(value:"入力をクリアして、この書類を手入力する",style:computed(move||css!(color:hex(TINT),font_size:px(15),padding:px(12))),on_tap:move|_|{draft.set(model::draft(operation.fields,&Value::Null));warning.set("原本を見ながら以下の項目を入力してください。".into());})
                    }
                }
            }
            Show(when:move||busy.get()){Notice(message:"書類を読み取り中…")}
            Show(when:move||!error.with(String::is_empty)){ErrorNotice(message:computed(move||error.get()))}
            Show(when:move||!warning.with(String::is_empty)){Text(value:computed(move||warning.get()),style:computed(move||css!(color:hex(SECONDARY_LABEL),font_size:px(14),padding:px(8))))}
            Text(value:if cfg!(target_os="ios"){ "6MBまで。長いPDFは必要なページだけに分けて選択してください。" }else{"書類の選択はiOSで利用できます。以下の項目は手入力できます。"},style:computed(move||css!(color:hex(SECONDARY_LABEL),font_size:px(12))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_suggestions_are_never_saved_without_review_and_validation() {
        let raw = json!({"fields":{"title":"見積","line_items":[{"name":"商品","quantity":"2","unit_price":"100","discount":"10"}]},"manualApprovalRequired":true});
        let op = RESOURCES[0].create.unwrap();
        let value = extracted(&raw).unwrap();
        let draft = model::draft(op.fields, &value);
        let body = model::request_body(op, &draft, &Value::Null, "").unwrap();
        assert_eq!(body["taxAmount"], 19);
        assert_eq!(body["items"][0]["quantity"], 2);
        assert_eq!(body["clientId"], "fieldv2-unassigned-client");
        assert!(body.get("manualApprovalRequired").is_none());
        let mut draft = draft;
        draft["items"][0]["quantity"] = json!("0");
        assert!(model::request_body(op, &draft, &Value::Null, "").is_err());
    }
}
