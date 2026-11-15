//! 証憑OCR の決まり（Web 版の receipt-labels.ts / receipt-schema.ts と同じ）。画面は `receipt_ocr_screen`。

use serde_json::Value;

use crate::cash_book_receipts::Receipt;

/// 一覧の 1 件。出納帳で使う `Receipt` に、証憑OCR の画面で要るものを足したもの。
#[derive(Clone, Debug, PartialEq)]
pub struct OcrReceipt {
    pub receipt: Receipt,
    pub file_url: Option<String>,
    pub expense_id: Option<String>,
    pub duplicate_of: Option<String>,
    pub customer_count: Option<f64>,
    /// 明細のどれかに 0 より大きい金額があるか（台帳へ反映できるかの決まり）。
    pub has_item_amount: bool,
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().replace(',', "").parse().ok(),
        _ => None,
    }
    .filter(|v: &f64| v.is_finite())
}

impl OcrReceipt {
    pub fn from_value(value: &Value) -> Option<Self> {
        let receipt = Receipt::from_value(value)?;
        let data = &value["extractedData"];
        let has_item_amount = data["items"].as_array().into_iter().flatten().any(|item| {
            let amount = number(&item["amount"])
                .or_else(|| number(&item["total"]))
                .or_else(|| Some(number(&item["unit_price"])? * number(&item["quantity"])?));
            amount.is_some_and(|a| a > 0.0)
        });
        Some(Self {
            receipt,
            file_url: text(&value["fileUrl"]),
            expense_id: text(&value["expenseId"]),
            duplicate_of: text(&value["duplicateOf"]),
            customer_count: number(&data["customer_count"]),
            has_item_amount,
        })
    }

    pub fn id(&self) -> &str {
        &self.receipt.id
    }

    /// 読み取り中（5 秒ごとに見に行く）。
    pub fn is_reading(&self) -> bool {
        matches!(self.receipt.status.as_str(), "pending" | "processing")
    }

    /// 一覧の右側：金額か、読み取りの状態。
    pub fn summary(&self) -> String {
        if self.receipt.status == "done" {
            match self.receipt.total {
                Some(total) => format!("￥{}", crate::cash_book::format_yen(total.round() as i64)),
                None => "金額未取得".into(),
            }
        } else {
            status_label(&self.receipt.status).to_owned()
        }
    }

    /// 台帳へ反映できない理由。反映できるなら `None`。
    pub fn ledger_rejection(&self) -> Option<&'static str> {
        if !self.receipt.total.is_some_and(|t| t > 0.0) {
            return Some("OCR合計金額が未取得または0以下のため、台帳へ反映できません。");
        }
        if !self.has_item_amount {
            return Some("有効な明細金額が取得できていないため、台帳へ反映できません。");
        }
        None
    }
}

/// 状態の絞り込み `(値, 名前)`。
pub const STATUS_FILTERS: &[(&str, &str)] = &[
    ("", "すべて"),
    ("pending", "処理待ち"),
    ("processing", "解析中"),
    ("done", "解析完了"),
    ("error", "エラー"),
    ("duplicate", "重複候補"),
];

/// 種別の絞り込み。
pub const TYPE_FILTERS: &[(&str, &str)] = &[
    ("", "すべての種別"),
    ("sales", "売上証憑"),
    ("purchase", "仕入証憑"),
    ("unsorted", "未仕分け"),
];

/// アップロードするときの種別。
pub const UPLOAD_TYPES: &[(&str, &str)] = &[
    ("purchase", "仕入"),
    ("sales", "売上"),
    ("unsorted", "あとで仕分け"),
];

/// 支払った人・手段。空は「未指定」。
pub const PAYERS: &[(&str, &str)] = &[
    ("", "支払い：未指定"),
    ("employee_advance", "自分で立替"),
    ("corporate_card", "法人カード"),
    ("store_cash", "拠点の現金"),
    ("company_paid", "会社が支払済み"),
];

pub fn status_label(value: &str) -> &str {
    STATUS_FILTERS
        .iter()
        .find(|(k, _)| *k == value && !k.is_empty())
        .map_or(value, |(_, l)| l)
}

pub fn type_label(value: &str) -> &str {
    match value {
        "purchase" => "仕入",
        "sales" => "売上",
        "unsorted" => "未仕分け",
        other => other,
    }
}

pub fn payment_label(value: Option<&str>) -> &'static str {
    match value {
        None => "—",
        Some("cash") => "現金",
        Some("credit_card") => "クレジットカード",
        Some("ic_card") => "交通系IC・電子マネー",
        Some("qr_code") => "QRコード決済",
        Some("other") => "その他",
        Some(_) => "判別できません",
    }
}

/// 読み取りに失敗した理由を、何をすればよいかの言葉にする。
pub fn failure_message(value: Option<&str>) -> String {
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        return "解析に失敗しました。原本を確認して再解析してください。".into();
    };
    let lower = value.to_lowercase();
    if value == "receipt OCR timeout" {
        return "解析が時間内に完了しませんでした。保存済みの原本から再解析できます。".into();
    }
    if let Some((pages, max)) = too_many_pages(value) {
        return format!(
            "原本は{pages}ページあります。{max}ページ以下に分割してアップロードしてください。"
        );
    }
    if value.contains("document could not be read; it may be encrypted or damaged") {
        return "原本を読み取れません。PDFの暗号化を解除し、正常に開ける原本をアップロードしてください。".into();
    }
    if value.contains("upstream rejected the document (status 413)")
        || value.contains("upstream returned status 413")
    {
        return "原本のファイルが大きすぎて読取サービスに送れません。画質を下げるか、ページを分割してアップロードしてください。".into();
    }
    if lower.contains("upstream returned status 400")
        || lower.contains("upstream rejected the document")
        || lower.contains("upstream rejected the request")
        || lower.contains("vision_not_supported")
        || lower.contains("model_configuration_rejected")
    {
        return "読取サービスが解析リクエストを拒否しました。入力内容の誤りとは断定できません。同じ原本の再解析で直らない場合は、管理者にモデル・画像対応と診断ログの確認を依頼してください。原本は保存されています。".into();
    }
    if ["402", "billing", "balance_exhausted", "payment_method"]
        .iter()
        .any(|k| lower.contains(k))
    {
        return "読取サービスの利用設定を確認できません。管理者に利用残高・契約設定の確認を依頼してください。".into();
    }
    if lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("rate-limit")
        || lower.contains("rate_limit")
    {
        return "読取サービスが混雑しています。時間をおいて再解析してください。".into();
    }
    if ["timeout", "unreachable", "serviceunavailable", "upstream"]
        .iter()
        .any(|k| lower.contains(k))
    {
        return "読取サービスで障害が発生しています。時間をおいて再解析してください。原本は保存されています。".into();
    }
    if (lower.contains("unsupported") && (lower.contains("format") || lower.contains("type")))
        || (lower.contains("invalid") && (lower.contains("image") || lower.contains("pdf")))
        || (lower.contains("empty") && (lower.contains("file") || lower.contains("document")))
    {
        return "ファイルを読み取れません。対応形式と原本の内容を確認してください。".into();
    }
    "解析に失敗しました。原本と読取サービスの状態を管理者に確認してから再解析してください。".into()
}

/// "document has N pages, ... send at most M pages" から (N, M)。
fn too_many_pages(value: &str) -> Option<(String, String)> {
    let rest = value.split("document has ").nth(1)?;
    let pages: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let tail = rest.split("send at most ").nth(1)?;
    let max: String = tail.chars().take_while(char::is_ascii_digit).collect();
    (!pages.is_empty()
        && !max.is_empty()
        && value.contains("more than one extraction request can carry"))
    .then_some((pages, max))
}

/// 経費にできる金額（1 円以上の整数）。
pub fn bookable_amount(value: &str) -> Option<i64> {
    let cleaned: String = value.trim().chars().filter(|c| *c != ',').collect();
    let amount: i64 = cleaned.parse().ok()?;
    (amount > 0).then_some(amount)
}

/// 経費を起票できなかったときの言葉。
pub fn expense_error_message(error: &str) -> String {
    if error.contains("（422）") {
        if error.to_lowercase().contains("no total could be read") {
            return "このレシートは合計金額を読み取れていないため、経費にできません。金額を入力してから起票してください。".into();
        }
        if error.to_lowercase().contains("not finished being read") {
            return "このレシートはまだ読み取り中です。読み取りが終わってから起票してください。"
                .into();
        }
    }
    format!("経費の起票に失敗しました。{error}")
}

/// 種別を変えられなかったときの言葉（409 は処理中か反映済み）。
pub fn type_change_error(error: &str) -> String {
    if error.contains("（409）") {
        "処理中、または経費・台帳に反映済みのため変更できません。反映先の記録を確認してください。"
            .into()
    } else {
        error.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn receipt(extra: Value) -> OcrReceipt {
        let mut value = json!({ "id": "r1", "status": "done", "receiptType": "purchase", "createdAt": "2026-11-15T10:00:00+09:00" });
        for (k, v) in extra.as_object().unwrap() {
            value[k] = v.clone();
        }
        OcrReceipt::from_value(&value).unwrap()
    }

    #[test]
    fn ledger_needs_a_total_and_a_priced_line() {
        let none = receipt(json!({ "extractedData": { "items": [] } }));
        assert_eq!(
            none.ledger_rejection(),
            Some("OCR合計金額が未取得または0以下のため、台帳へ反映できません。")
        );
        let no_items = receipt(
            json!({ "extractedData": { "total": "1,200", "items": [{ "name": "a", "amount": 0 }] } }),
        );
        assert_eq!(
            no_items.ledger_rejection(),
            Some("有効な明細金額が取得できていないため、台帳へ反映できません。")
        );
        let unit = receipt(
            json!({ "extractedData": { "total": 1200, "items": [{ "name": "a", "unit_price": "600", "quantity": 2 }] } }),
        );
        assert_eq!(unit.ledger_rejection(), None);
        assert_eq!(unit.summary(), "￥1,200");
    }

    #[test]
    fn reads_the_fields_the_screen_needs() {
        let r = receipt(
            json!({ "fileUrl": "https://x/y.jpg", "expenseId": "e1", "duplicateOf": "r0", "extractedData": { "customer_count": 12 } }),
        );
        assert_eq!(r.file_url.as_deref(), Some("https://x/y.jpg"));
        assert_eq!(r.expense_id.as_deref(), Some("e1"));
        assert_eq!(r.duplicate_of.as_deref(), Some("r0"));
        assert_eq!(r.customer_count, Some(12.0));
        assert_eq!(r.summary(), "金額未取得");
    }

    #[test]
    fn failure_messages_match_the_web() {
        assert_eq!(
            failure_message(None),
            "解析に失敗しました。原本を確認して再解析してください。"
        );
        assert!(failure_message(Some("receipt OCR timeout")).starts_with("解析が時間内に"));
        assert_eq!(
            failure_message(Some(
                "document has 12 pages, which is more than one extraction request can carry; send at most 5 pages or images"
            )),
            "原本は12ページあります。5ページ以下に分割してアップロードしてください。"
        );
        assert!(
            failure_message(Some("upstream returned status 429"))
                .starts_with("読取サービスが解析リクエスト")
                == false
        );
        assert!(failure_message(Some("rate limit hit")).starts_with("読取サービスが混雑"));
        assert!(failure_message(Some("upstream unreachable")).starts_with("読取サービスで障害"));
        assert!(failure_message(Some("something odd")).starts_with("解析に失敗しました。原本と"));
    }

    #[test]
    fn labels_and_amounts() {
        assert_eq!(status_label("processing"), "解析中");
        assert_eq!(payment_label(Some("qr_code")), "QRコード決済");
        assert_eq!(payment_label(Some("zzz")), "判別できません");
        assert_eq!(bookable_amount("1,200"), Some(1200));
        assert_eq!(bookable_amount("0"), None);
        assert_eq!(bookable_amount("12.5"), None);
        assert!(
            expense_error_message("Field API がエラーを返しました（422）no total could be read")
                .contains("合計金額を読み取れていない")
        );
        assert!(type_change_error("Field API がエラーを返しました（409）").starts_with("処理中"));
    }
}
