//! Invoice delivery and cancellation use the same contracts as the web detail page.
//! Cancellation is reviewed separately so its reversal date and count are visible.
use super::{model, schema::*};
use crate::api::{self, Tenant};
use serde_json::{Value, json};

const INVOICES: Lookup = Lookup {
    path: "/v1/invoices?limit=100",
    list_key: "items",
    value_key: "id",
    label_keys: &["invoiceNumber", "clientName"],
};
const FILTERS: &[Field] =
    &[Field::new("invoiceId", "請求書", InputKind::Reference(INVOICES)).required()];
const ACK: Field = Field::new(
    "confirmed",
    "対象・金額・実行内容を確認した",
    InputKind::Bool,
)
.default("false");
const AMOUNT: Field =
    Field::new("_amount", "請求額", InputKind::ReadOnly).read("@money:totalAmount");
const NUMBER: Field = Field::new("_number", "請求番号", InputKind::ReadOnly).read("invoiceNumber");
const CLIENT: Field = Field::new("_client", "宛名", InputKind::ReadOnly).read("clientName");

pub fn decorate(mut resource: Resource) -> Resource {
    if resource.key != "invoices" {
        return resource;
    }
    resource.detail_fields = &[
        ("請求番号", "invoiceNumber"),
        ("宛名", "clientName"),
        ("メール", "clientEmail"),
        ("電話", "clientPhone"),
        ("合計", "@money:totalAmount"),
        ("税額", "@money:taxAmount"),
        ("支払期日", "dueDate"),
        ("明細", "lineItems"),
        ("備考", "notes"),
        ("支払リンク", "paymentLinkUrl"),
        ("支払リンク作成", "paymentLinkStatus"),
        ("支払リンクの失敗理由", "paymentLinkFailureCode"),
        ("メール送付", "emailDeliveryStatus"),
        ("メールの失敗理由", "emailDeliveryFailureCode"),
        ("SMS送付", "smsDeliveryStatus"),
        ("SMSの失敗理由", "smsDeliveryFailureCode"),
        ("送付日時", "sentAt"),
        ("入金日", "paidAt"),
    ];
    if let Some(edit) = resource.edit {
        resource.edit = Some(edit.when(not_void));
    }
    resource.actions = ACTIONS;
    resource
}

const ACTIONS: &[Mutation] = &[
        Mutation::new("状態・支払案内を更新", "PATCH", "/v1/invoices/{id}", &[
            NUMBER, CLIENT, AMOUNT,
            Field::new("status", "更新後の状態", InputKind::Select(&[
                Choice::new("Draft", "下書き"), Choice::new("Sent", "送付済み"),
                Choice::new("SendFailed", "送付失敗"), Choice::new("Overdue", "期限超過"),
            ])).required(),
            Field::new("createPaymentLink", "Stripeの支払リンクを作成", InputKind::Bool).default("false"),
            Field::new("sendEmail", "請求案内をメール送信", InputKind::Bool).default("false"),
            Field::new("_recipient", "メールの送付先", InputKind::ReadOnly).read("clientEmail"),
            ACK,
        ]).when(unpaid).prepare(prepare_status).execute(update_status)
          .explain("請求状態を更新します。選択した場合は支払リンクを作成し、表示した宛先にメールを送信します。入金済みへの変更は、入金確認・消込の処理で行います。"),
        Mutation::new("請求案内を再送", "POST", "/v1/invoices/{id}/payment-link/resend", &[
            NUMBER, CLIENT, AMOUNT,
            Field::new("_recipient", "メールの送付先", InputKind::ReadOnly).read("clientEmail"), ACK,
        ]).when(unpaid).prepare(prepare_resend).execute(resend)
          .explain("現在の請求案内をメールで再送します。支払リンクがない場合は作成します。"),
        Mutation::new("支払リンク・通知を再処理", "POST", "/v1/invoices/{id}/fulfill", &[
            NUMBER, CLIENT, AMOUNT,
            Field::new("_link", "支払リンクの状態", InputKind::ReadOnly).read("paymentLinkStatus"),
            Field::new("_email", "メールの状態", InputKind::ReadOnly).read("emailDeliveryStatus"),
            Field::new("_sms", "SMSの状態", InputKind::ReadOnly).read("smsDeliveryStatus"),
            Field::new("_recipient", "メールの送付先", InputKind::ReadOnly).read("clientEmail"),
            Field::new("_phone", "SMSの送付先", InputKind::ReadOnly).read("clientPhone"), ACK,
        ]).when(retryable).prepare(prepare_confirmed).execute(fulfill)
          .explain("保留・失敗中の支払リンク作成とメール・SMS送信を再処理します。"),
        Mutation::new("請求書PDFをメールで送付", "POST", "/v1/invoices/{id}/email-deliveries", &[
            NUMBER, CLIENT, AMOUNT,
            Field::new("recipient", "送付先", InputKind::Email).read("clientEmail").required(),
            Field::text("subject", "件名").required().default("請求書のご案内"),
            Field::new("body", "メール本文", InputKind::Multiline).required()
                .default("請求書をお送りします。添付のPDFをご確認ください。"),
            Field::text("contactName", "PDFに記載する担当者"), ACK,
        ]).when(not_void).prepare(prepare_email).execute(send_email)
          .idempotency_body("requestKey")
          .explain("接続済みのGmailから、請求書PDFを添付して送信します。この請求書の元になった販売契約のうち、下書きのものも有効になります。宛先・件名・本文と契約への影響を確認してください。送付履歴は「請求書のメール送付履歴」で確認できます。"),
    ];

pub const RESOURCES: &[Resource] = &[
    Resource::new("invoice-void-review", "請求・債権管理", "請求書の取消確認",
        "/v1/invoices/{invoiceId}/void-eligibility", "")
        .route("invoices").filters(FILTERS).load(load_void_review)
        .title(&["invoiceNumber", "clientName"]).subtitle(&["clientName", "@money:totalAmount"])
        .status("status").fields(&[
            ("請求番号", "invoiceNumber"), ("宛名", "clientName"), ("請求額", "@money:totalAmount"),
            ("支払期日", "dueDate"), ("明細", "lineItems"), ("取消できる", "_voidPlan.eligible"),
            ("取消できない理由", "_voidPlan.reasonCode"), ("逆仕訳予定日", "_voidPlan.plannedReversalOn"),
            ("逆仕訳する仕訳数", "_voidPlan.reversedJournalCount"), ("会計期間", "_voidPlan.period.yearMonth"),
            ("会計期間の状態", "_voidPlan.period.status"),
        ]).actions(&[
            Mutation::new("請求書を取り消す", "POST", "/v1/invoices/{id}/void", &[
                NUMBER, CLIENT, AMOUNT,
                Field::new("_date", "逆仕訳予定日", InputKind::ReadOnly).read("_voidPlan.plannedReversalOn"),
                Field::new("_journals", "逆仕訳する仕訳数", InputKind::ReadOnly).read("_voidPlan.reversedJournalCount"),
                Field::new("reason", "取消理由", InputKind::Multiline).required(), ACK,
            ]).when(can_void).prepare(prepare_void).execute(void_invoice).idempotency_body("_requestKey")
              .destructive().explain("請求書と支払リンクを取り消し、必要な逆仕訳を作成します。予定日・件数・請求額を確認してください。入金済みの請求書はここから取り消せません。"),
        ]),
    Resource::new("invoice-email-history", "請求・債権管理", "請求書のメール送付履歴（最新100件）",
        "/v1/invoices/{invoiceId}/email-deliveries", "items")
        .route("invoices").filters(FILTERS).title(&["subject"]).subtitle(&["recipient", "createdAt"])
        .status("status").fields(&[
            ("宛先", "recipient"), ("件名", "subject"), ("添付PDF", "attachmentName"),
            ("送付日時", "sentAt"), ("失敗理由", "failureMessage"), ("作成日時", "createdAt"),
        ]),
];

fn not_void(record: &Value) -> bool {
    record["status"]
        .as_str()
        .is_some_and(|status| status != "Void")
}
fn unpaid(record: &Value) -> bool {
    matches!(
        record["status"].as_str(),
        Some("Draft" | "Sent" | "SendFailed" | "Overdue")
    )
}
fn retryable(record: &Value) -> bool {
    unpaid(record)
        && [
            "paymentLinkStatus",
            "emailDeliveryStatus",
            "smsDeliveryStatus",
        ]
        .iter()
        .any(|key| matches!(record[*key].as_str(), Some("Pending" | "Failed")))
}
fn can_void(record: &Value) -> bool {
    unpaid(record) && record["_voidPlan"]["eligible"] == true
}
fn require_confirmation(body: &mut Value) -> Result<(), String> {
    if body["confirmed"] != true {
        return Err("対象・金額・実行内容を確認し、確認欄にチェックしてください。".into());
    }
    body.as_object_mut()
        .ok_or("入力を確認してください。")?
        .remove("confirmed");
    Ok(())
}
fn prepare_confirmed(body: &mut Value, _: &Value) -> Result<(), String> {
    require_confirmation(body)
}
fn prepare_status(body: &mut Value, original: &Value) -> Result<(), String> {
    require_confirmation(body)?;
    if !unpaid(original)
        || !matches!(
            body["status"].as_str(),
            Some("Draft" | "Sent" | "SendFailed" | "Overdue")
        )
    {
        return Err("現在の請求書では、この状態変更はできません。入金済みへの変更は入金確認から行ってください。".into());
    }
    if body["sendEmail"] == true {
        require_recipient(original)?;
    }
    body["paymentLinkProvider"] = if body["createPaymentLink"] == true {
        json!("stripe")
    } else {
        Value::Null
    };
    Ok(())
}
fn prepare_resend(body: &mut Value, original: &Value) -> Result<(), String> {
    require_confirmation(body)?;
    require_recipient(original)?;
    body["sendEmail"] = json!(true);
    Ok(())
}
fn prepare_email(body: &mut Value, _: &Value) -> Result<(), String> {
    require_confirmation(body)?;
    for (field, max, label) in [
        ("subject", 998, "件名"),
        ("body", 50_000, "本文"),
        ("recipient", 255, "宛先"),
    ] {
        let value = body[field].as_str().unwrap_or_default();
        if value.trim().is_empty() || value.chars().count() > max {
            return Err(format!("{label}の長さを確認してください。"));
        }
    }
    Ok(())
}
fn prepare_void(body: &mut Value, original: &Value) -> Result<(), String> {
    require_confirmation(body)?;
    if !can_void(original) {
        return Err("取消できる状態を確認できません。読み直してください。".into());
    }
    let reason = body["reason"].as_str().unwrap_or_default().trim();
    if reason.is_empty() || reason.chars().count() > 1024 {
        return Err("取消理由を1〜1,024文字で入力してください。".into());
    }
    body["reason"] = json!(reason);
    Ok(())
}
fn require_recipient(record: &Value) -> Result<(), String> {
    let email = record["clientEmail"].as_str().unwrap_or_default();
    if !email.contains('@') || email.chars().any(char::is_whitespace) {
        return Err("連絡先の編集から、有効なメールアドレスを設定してください。".into());
    }
    Ok(())
}
fn path(record: &Value) -> Result<String, String> {
    let id = record["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("請求書を選び直してください。")?;
    model::request_path(
        Resource::new("invoice", "", "", "/v1/invoices", ""),
        "/v1/invoices/{id}",
        &json!({"id":id}),
        &Value::Null,
    )
}
fn check_current(original: &Value, current: &Value) -> Result<(), String> {
    if original["updatedAt"].as_str().is_none_or(str::is_empty) {
        return Err("請求書の更新情報を確認できません。読み直してください。".into());
    }
    for key in [
        "id",
        "updatedAt",
        "status",
        "clientId",
        "clientName",
        "clientEmail",
        "clientPhone",
        "currency",
        "totalAmount",
        "taxAmount",
        "lineItems",
        "dueDate",
        "paymentLinkUrl",
        "paymentLinkStatus",
        "emailDeliveryStatus",
        "smsDeliveryStatus",
    ] {
        if original[key] != current[key] {
            return Err(
                "請求内容・送付先・状態が変更されています。読み直して確認してください。".into(),
            );
        }
    }
    Ok(())
}
fn current_invoice(token: &str, tenant: &Tenant, original: &Value) -> Result<Value, String> {
    let current = api::fetch_record(token, tenant, &path(original)?)?;
    check_current(original, &current)?;
    Ok(current)
}
fn check_delivery(value: &Value, email: bool, link: bool, sms: bool) -> Result<(), String> {
    if !value.is_object() || value["id"].as_str().is_none() {
        return Err("処理結果を確認できません。請求書を読み直してください。".into());
    }
    let failures = [
        (email, "emailDeliveryStatus", "メール送信"),
        (link, "paymentLinkStatus", "支払リンク作成"),
        (sms, "smsDeliveryStatus", "SMS送信"),
    ]
    .into_iter()
    .filter_map(|(selected, field, label)| {
        if !selected {
            return None;
        }
        match value[field].as_str() {
            Some("Sent" | "Ready") => None,
            Some("Failed") => Some(format!("{label}に失敗")),
            _ => Some(format!("{label}は完了未確認")),
        }
    })
    .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "請求書の更新は保存されていますが、{}です。最新の状態と失敗理由を確認してください。",
            failures.join("・")
        ))
    }
}
fn update_status(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    current_invoice(token, tenant, original)?;
    let value = api::mutate_record(token, tenant, "PATCH", &path(original)?, body, None)?;
    check_delivery(
        &value,
        body["sendEmail"] == true,
        body["createPaymentLink"] == true,
        false,
    )?;
    Ok(value)
}
fn resend(token: &str, tenant: &Tenant, body: &Value, original: &Value) -> Result<Value, String> {
    let current = current_invoice(token, tenant, original)?;
    let value = api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{}/payment-link/resend", path(original)?),
        body,
        None,
    )?;
    check_delivery(
        &value,
        true,
        current["paymentLinkUrl"].as_str().is_none_or(str::is_empty),
        false,
    )?;
    Ok(value)
}
fn fulfill(token: &str, tenant: &Tenant, body: &Value, original: &Value) -> Result<Value, String> {
    let current = current_invoice(token, tenant, original)?;
    let pending = |key: &str| matches!(current[key].as_str(), Some("Pending" | "Failed"));
    let value = api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{}/fulfill", path(original)?),
        body,
        None,
    )?;
    check_delivery(
        &value,
        pending("emailDeliveryStatus"),
        pending("paymentLinkStatus"),
        pending("smsDeliveryStatus"),
    )?;
    Ok(value)
}
fn check_email_delivery(value: &Value) -> Result<(), String> {
    match value["status"].as_str() {
        Some("sent") => Ok(()),
        Some("failed") => Err(format!(
            "メール送付に失敗しました。送付履歴を確認し、再送する場合はこの画面を閉じて送付をやり直してください。{}",
            value["failureMessage"].as_str().unwrap_or_default()
        )),
        _ => Err("メールの送付完了を確認できません。送付履歴を確認してください。".into()),
    }
}
fn send_email(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let endpoint = format!("{}/email-deliveries", path(original)?);
    // The server deduplicates requestKey; recover an acknowledged delivery before
    // comparing the invoice that successful delivery may itself have changed.
    let prior = api::fetch_record(token, tenant, &endpoint)?;
    let items = prior["items"]
        .as_array()
        .ok_or("送付履歴を確認できません。")?;
    if let Some(existing) = items
        .iter()
        .find(|item| item["requestKey"] == body["requestKey"])
    {
        check_email_delivery(existing)?;
        return Ok(existing.clone());
    }
    current_invoice(token, tenant, original)?;
    let value = api::mutate_record(token, tenant, "POST", &endpoint, body, None)?;
    check_email_delivery(&value)?;
    Ok(value)
}
fn load_void_review(token: &str, tenant: &Tenant, filters: &Value) -> Result<Value, String> {
    let id = filters["invoiceId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("取消を確認する請求書を選択してください。")?;
    let endpoint = path(&json!({"id":id}))?;
    let mut invoice = api::fetch_record(token, tenant, &endpoint)?;
    let plan = api::fetch_record(token, tenant, &format!("{endpoint}/void-eligibility"))?;
    if invoice["id"].as_str() != Some(id)
        || !plan["eligible"].is_boolean()
        || !plan["reversedJournalCount"].is_u64()
    {
        return Err("請求書の取消確認を読み取れませんでした。".into());
    }
    invoice["_voidPlan"] = plan;
    Ok(invoice)
}
fn void_invoice(
    token: &str,
    tenant: &Tenant,
    body: &Value,
    original: &Value,
) -> Result<Value, String> {
    let endpoint = path(original)?;
    let current = api::fetch_record(token, tenant, &endpoint)?;
    // A previously committed void can still need its reversal retried with the
    // exact same key. The server verifies the key against the original fact.
    if current["status"] != "Void" {
        check_current(original, &current)?;
        let plan = api::fetch_record(token, tenant, &format!("{endpoint}/void-eligibility"))?;
        if plan != original["_voidPlan"] || plan["eligible"] != true {
            return Err(
                "取消の可否・逆仕訳予定が変わっています。取消確認を読み直してください。".into(),
            );
        }
    }
    let key = body["_requestKey"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("取消の再送キーを確認できません。")?;
    let value = api::mutate_record(
        token,
        tenant,
        "POST",
        &format!("{endpoint}/void"),
        &json!({"reason":body["reason"]}),
        Some(key),
    )?;
    if value["invoice"]["status"] != "Void" {
        return Err(
            "取消結果を確認できません。同じ内容のまま再試行するか、請求書を読み直してください。"
                .into(),
        );
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn invoice() -> Value {
        json!({"id":"invoice-a", "status":"Draft", "updatedAt":"2026-11-19T10:00:00Z", "clientEmail":"billing@example.com", "currency":"USD", "totalAmount":12345,
            "_voidPlan":{"eligible":true, "plannedReversalOn":"2026-11-19", "reversedJournalCount":2}})
    }
    #[test]
    fn manual_paid_state_and_unconfirmed_delivery_are_rejected() {
        let original = invoice();
        let mut body = json!({"status":"Paid", "confirmed":true});
        assert!(prepare_status(&mut body, &original).is_err());
        let mut body =
            json!({"status":"Sent", "sendEmail":true, "createPaymentLink":true, "confirmed":false});
        assert!(prepare_status(&mut body, &original).is_err());
        body["confirmed"] = json!(true);
        prepare_status(&mut body, &original).unwrap();
        assert_eq!(body["paymentLinkProvider"], "stripe");
        assert!(body.get("confirmed").is_none());
    }
    #[test]
    fn delivery_success_requires_each_requested_channel_to_finish() {
        assert!(
            check_delivery(
                &json!({"id":"a", "emailDeliveryStatus":"Failed"}),
                true,
                false,
                false
            )
            .is_err()
        );
        assert!(
            check_delivery(
                &json!({"id":"a", "emailDeliveryStatus":"Pending"}),
                true,
                false,
                false
            )
            .is_err()
        );
        assert!(
            check_delivery(
                &json!({"id":"a", "emailDeliveryStatus":"Sent", "smsDeliveryStatus":"Failed"}),
                true,
                false,
                false
            )
            .is_ok()
        );
        assert!(check_email_delivery(&json!({"status":"failed"})).is_err());
        assert!(check_email_delivery(&json!({"status":"sent"})).is_ok());
    }
    #[test]
    fn changed_recipient_amount_or_version_blocks_the_confirmed_request() {
        let original = invoice();
        for key in ["clientEmail", "updatedAt", "totalAmount"] {
            let mut changed = original.clone();
            changed[key] = json!("changed");
            assert!(check_current(&original, &changed).is_err());
        }
        assert!(check_current(&original, &original).is_ok());
        assert!(check_current(&json!({"id":"a"}), &json!({"id":"a"})).is_err());
    }
    #[test]
    fn cancellation_needs_review_and_preserves_the_retry_key() {
        let original = invoice();
        let op = RESOURCES[0].actions[0];
        let mut draft = model::draft(op.fields, &original);
        assert_eq!(draft["_amount"], "123.45 USD");
        draft["confirmed"] = json!(true);
        draft["reason"] = json!("請求の重複");
        let body = model::request_body(op, &draft, &original, "same-key").unwrap();
        assert_eq!(
            body,
            json!({"reason":"請求の重複", "_requestKey":"same-key"})
        );
        let mut changed = original.clone();
        changed["_voidPlan"]["eligible"] = json!(false);
        assert!(model::request_body(op, &draft, &changed, "same-key").is_err());
        let mut paid = original.clone();
        paid["status"] = json!("Paid");
        assert!(!can_void(&paid));
        assert!(!unpaid(&paid));
    }
    #[test]
    fn void_and_history_selectors_escape_ids_without_changing_the_endpoint() {
        let filters = json!({"invoiceId":"invoice/a?x=1"});
        assert_eq!(
            model::query_path(RESOURCES[1].path, FILTERS, &filters).unwrap(),
            "/v1/invoices/invoice%2Fa%3Fx%3D1/email-deliveries"
        );
        assert!(path(&json!({"id":".."})).is_err());
    }
}
