//! Field API と Cognito への呼び出し。
//!
//! どれも同期（ブロッキング）呼び出しなので、UI からは必ず
//! `run_blocking` 経由で別スレッドで呼ぶ。トークンはメモリにだけ持ち、
//! ディスクには書かない。

use std::time::Duration;

use serde_json::{Value, json};

/// Web クライアント（apps/client/src/platform/auth/auth-config.ts）と同じ公開クライアント。
const COGNITO_ENDPOINT: &str = "https://cognito-idp.ap-northeast-1.amazonaws.com/";
const COGNITO_CLIENT_ID: &str = "5n3h0du82ll3t02m99gnr8qj27";

const FIELD_API_BASE_URL: &str = "https://tachyon-field-api.txcloud.app";

/// apps/client/src/tenant/tenant-api.ts の OPERATOR_IDS / PLATFORM_IDS と同じ値。
const ENVIRONMENTS: [(&str, &str, &str); 2] = [
    (
        "production",
        "tn_01j91h09tpj5ehwbwfwfxpak2b",
        "tn_01hjjn348rn3t49zz6hvmfq67p",
    ),
    (
        "sandbox",
        "tn_01hy91qw3362djx6z9jerr34v4",
        "tn_01hjryxysgey07h5jz5wagqj0m",
    ),
];

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub environment: String,
    pub platform_id: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build()
}

/// サインインの結果。リフレッシュトークンは次回起動時の自動サインインに使う。
pub struct Session {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

/// アクセストークンが切れたときに `describe_error` が返す文言。
/// 画面側はこれを見て、リフレッシュトークンで取り直す。
pub const EXPIRED: &str = "サインインの有効期限が切れました。もう一度サインインしてください";

fn cognito(target: &str, body: Value) -> Result<Value, String> {
    let response = agent()
        .post(COGNITO_ENDPOINT)
        .set("Content-Type", "application/x-amz-json-1.1")
        .set(
            "X-Amz-Target",
            &format!("AWSCognitoIdentityProviderService.{target}"),
        )
        .send_string(&body.to_string());
    match response {
        Ok(response) => response.into_json().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(_, response)) => {
            let value: Value = response.into_json().unwrap_or(Value::Null);
            let kind = value["__type"].as_str().unwrap_or_default();
            Err(match kind {
                "NotAuthorizedException" => REJECTED.into(),
                "UserNotFoundException" => "このアカウントは見つかりません".into(),
                "UserNotConfirmedException" => "アカウントの確認が終わっていません".into(),
                _ => value["message"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "サインインに失敗しました".into()),
            })
        }
        Err(error) => Err(format!("通信できません: {error}")),
    }
}

fn session_from(value: Value) -> Result<Session, String> {
    if let Some(challenge) = value["ChallengeName"].as_str() {
        return Err(format!(
            "追加の確認（{challenge}）が必要なアカウントです。この試作アプリはまだ対応していません"
        ));
    }
    let result = &value["AuthenticationResult"];
    let access_token = result["AccessToken"]
        .as_str()
        .map(str::to_owned)
        .ok_or("サインイン結果にトークンがありません")?;
    Ok(Session {
        access_token,
        refresh_token: result["RefreshToken"].as_str().map(str::to_owned),
    })
}

/// ID とパスワードで Cognito にサインインする。
pub fn sign_in(username: &str, password: &str) -> Result<Session, String> {
    session_from(cognito(
        "InitiateAuth",
        json!({
            "AuthFlow": "USER_PASSWORD_AUTH",
            "ClientId": COGNITO_CLIENT_ID,
            "AuthParameters": { "USERNAME": username, "PASSWORD": password },
        }),
    )?)
}

/// 保存しておいたリフレッシュトークンで、新しいアクセストークンを取る。
pub fn refresh(refresh_token: &str) -> Result<Session, String> {
    session_from(
        cognito(
            "InitiateAuth",
            json!({
                "AuthFlow": "REFRESH_TOKEN_AUTH",
                "ClientId": COGNITO_CLIENT_ID,
                "AuthParameters": { "REFRESH_TOKEN": refresh_token },
            }),
        )
        .map_err(|error| {
            // 断られた（期限切れ・取り消し済み）ときだけ EXPIRED。通信の失敗はそのまま返す。
            if error == REJECTED {
                EXPIRED.to_owned()
            } else {
                error
            }
        })?,
    )
    .map_err(|_| EXPIRED.to_owned())
}

/// Cognito がトークンやパスワードを受け付けなかったときの文言（`cognito` が返す）。
const REJECTED: &str = "メールアドレスかパスワードが違います";

/// サインアウト時に、リフレッシュトークンを Cognito 側でも使えなくする。
pub fn revoke(refresh_token: &str) {
    let _ = cognito(
        "RevokeToken",
        json!({ "ClientId": COGNITO_CLIENT_ID, "Token": refresh_token }),
    );
}

fn field_request(
    method: &str,
    path: &str,
    token: &str,
    operator_id: &str,
    platform_id: &str,
) -> ureq::Request {
    agent()
        .request(method, &format!("{FIELD_API_BASE_URL}{path}"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("x-operator-id", operator_id)
        .set("x-platform-id", platform_id)
        .set("Accept", "application/json")
}

fn describe_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(401, _) => EXPIRED.into(),
        ureq::Error::Status(403, _) => "この操作の権限がありません".into(),
        ureq::Error::Status(status, response) => {
            let body: Value = response.into_json().unwrap_or(Value::Null);
            let message = body["message"]
                .as_str()
                .or_else(|| body["error"].as_str())
                .unwrap_or("");
            format!("Field API がエラーを返しました（{status}）{message}")
        }
        ureq::Error::Transport(transport) => format!("通信できません: {transport}"),
    }
}

/// 本番・サンドボックスの両方から、使えるテナントを集める。
/// 片方だけ失敗した場合は、取れた方を返す。
pub fn list_tenants(token: &str) -> Result<Vec<Tenant>, String> {
    let mut tenants: Vec<Tenant> = Vec::new();
    let mut last_error = None;

    for (environment, operator_id, platform_id) in ENVIRONMENTS {
        let response = field_request(
            "POST",
            "/get_tenants?required_action=field%3AListTenants",
            token,
            operator_id,
            platform_id,
        )
        .call();
        let items: Vec<Value> = match response {
            Ok(response) => response.into_json().map_err(|e| e.to_string())?,
            Err(error) => {
                last_error = Some(describe_error(error));
                continue;
            }
        };
        for item in items {
            let (Some(id), Some(name)) = (item["id"].as_str(), item["name"].as_str()) else {
                continue;
            };
            if tenants.iter().any(|t| t.id == id) {
                continue;
            }
            tenants.push(Tenant {
                id: id.to_owned(),
                name: name.trim().to_owned(),
                environment: item["environment"]
                    .as_str()
                    .unwrap_or(environment)
                    .to_owned(),
                platform_id: item["platformId"]
                    .as_str()
                    .unwrap_or(platform_id)
                    .to_owned(),
            });
        }
    }

    if tenants.is_empty() {
        if let Some(error) = last_error {
            return Err(error);
        }
    }
    tenants.sort_by(|a, b| {
        (a.environment != "production", &a.name).cmp(&(b.environment != "production", &b.name))
    });
    Ok(tenants)
}

/// 機能の一覧を取る。応答の `list_key`（`items` など）に入った配列を返す。
/// 配列がそのまま返る API にも対応する。
///
/// 応答に `hasMore: true` があれば `offset` を進めて続きも読み、全部をつなげて返す
/// （画面の検索は手元の行で絞り込むので、読み残すと見つからなくなる）。
/// 読みすぎを防ぐため `MAX_PAGES` ページで止める。
pub fn fetch_list(
    token: &str,
    tenant: &Tenant,
    path: &str,
    list_key: &str,
) -> Result<Vec<Value>, String> {
    const MAX_PAGES: usize = 20;
    let mut list = Vec::new();
    for _ in 0..MAX_PAGES {
        let page_path = if list.is_empty() {
            path.to_owned()
        } else {
            let joiner = if path.contains('?') { '&' } else { '?' };
            format!("{path}{joiner}offset={}", list.len())
        };
        let body: Value = field_request("GET", &page_path, token, &tenant.id, &tenant.platform_id)
            .call()
            .map_err(describe_error)?
            .into_json()
            .map_err(|e| format!("応答を読めません: {e}"))?;
        let (items, has_more) = match body {
            Value::Array(items) => (items, false),
            mut body => {
                let has_more = body["hasMore"]
                    .as_bool()
                    .or_else(|| body["has_more"].as_bool());
                match body[list_key].take() {
                    Value::Array(items) => (items, has_more.unwrap_or(false)),
                    _ => return Err("応答の形が想定と違います".into()),
                }
            }
        };
        let got = items.len();
        list.extend(items);
        if !has_more || got == 0 {
            break;
        }
    }
    Ok(list)
}

/// 案件のパイプラインと段階は ID（`pl_…` / `ps_…`）で届くので、名前に置き換える。
/// パイプラインの定義が読めないときや、見つからない ID は画面に出さない（ID は見せない）。
pub fn resolve_pipelines(token: &str, tenant: &Tenant, deals: &mut [Value]) {
    let mut names = std::collections::HashMap::new();
    if let Ok(body) = field_request(
        "GET",
        "/v1/erp/pipelines",
        token,
        &tenant.id,
        &tenant.platform_id,
    )
    .call()
    .map_err(describe_error)
    .and_then(|r| r.into_json::<Value>().map_err(|e| e.to_string()))
    {
        for pipeline in body["items"].as_array().into_iter().flatten() {
            if let (Some(id), Some(label)) = (pipeline["id"].as_str(), pipeline["label"].as_str()) {
                names.insert(id.to_owned(), label.to_owned());
            }
            for stage in pipeline["stages"].as_array().into_iter().flatten() {
                if let (Some(id), Some(label)) = (stage["id"].as_str(), stage["label"].as_str()) {
                    names.insert(id.to_owned(), label.to_owned());
                }
            }
        }
    }
    for deal in deals.iter_mut() {
        for key in ["pipeline", "stage", "pipelineId", "stageId"] {
            let Some(id) = deal[key].as_str().map(str::to_owned) else {
                continue;
            };
            if !(id.starts_with("pl_") || id.starts_with("ps_")) {
                continue;
            }
            let target = match key {
                "pipelineId" => "pipeline",
                "stageId" => "stage",
                other => other,
            };
            deal[target] = names
                .get(&id)
                .map_or(Value::Null, |name| Value::String(name.clone()));
        }
    }
}

/// 1 件を取り直す（一覧の応答には入っていない項目を詳細で出すため）。
pub fn fetch_record(token: &str, tenant: &Tenant, path: &str) -> Result<Value, String> {
    field_request("GET", path, token, &tenant.id, &tenant.platform_id)
        .call()
        .map_err(describe_error)?
        .into_json()
        .map_err(|e| format!("応答を読めません: {e}"))
}

/// 現金出納帳の帳簿設定。まだ設定の API が無い環境（404）では `None`。
pub fn cash_book_settings(token: &str, tenant: &Tenant) -> Result<Option<Value>, String> {
    match field_request(
        "GET",
        "/v1/field/cash-book/settings",
        token,
        &tenant.id,
        &tenant.platform_id,
    )
    .call()
    {
        Ok(response) => response
            .into_json()
            .map(Some)
            .map_err(|e| format!("応答を読めません: {e}")),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(error) => Err(describe_error(error)),
    }
}

/// JSON を送って JSON を受け取る（PUT / POST）。
pub fn send_json(
    token: &str,
    tenant: &Tenant,
    method: &str,
    path: &str,
    body: &Value,
    timeout: Duration,
) -> Result<Value, String> {
    ureq::AgentBuilder::new()
        .timeout(timeout)
        .build()
        .request(method, &format!("{FIELD_API_BASE_URL}{path}"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("x-operator-id", &tenant.id)
        .set("x-platform-id", &tenant.platform_id)
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .send_json(body.clone())
        .map_err(describe_error)?
        .into_json()
        .map_err(|e| format!("応答を読めません: {e}"))
}

/// 帳簿設定を保存し、保存後の設定（`{ data, version }`）を返す。
pub fn save_cash_book_settings(
    token: &str,
    tenant: &Tenant,
    data: &Value,
    expected_version: i64,
) -> Result<Value, String> {
    send_json(
        token,
        tenant,
        "PUT",
        "/v1/field/cash-book/settings",
        &json!({ "data": data, "expectedVersion": expected_version }),
        Duration::from_secs(15),
    )
    .map_err(|message| {
        if message.contains("（409）") {
            "ほかの端末で設定が保存されました。読み直してから入れ直してください。".into()
        } else {
            message
        }
    })
}

/// 店舗マスタ（帳簿を足すときに選ぶ）。
pub fn inventory_stores(token: &str, tenant: &Tenant) -> Result<Vec<Value>, String> {
    fetch_list(token, tenant, "/v1/field/inventory/stores", "items")
}

/// レジ管理表の日を読む（92 日まで）。返ってきた帳簿が頼んだものと違えば断る。
pub fn register_count_days(
    token: &str,
    tenant: &Tenant,
    from: &str,
    to: &str,
    book_id: &str,
) -> Result<Value, String> {
    let path = format!(
        "/v1/field/register-counts/days?from={from}&to={to}&book_id={}",
        encode_query(book_id)
    );
    let value = fetch_record(token, tenant, &path)?;
    if value["bookId"].as_str() != Some(book_id) {
        return Err(
            "店舗・レジ別の記録を読み込めませんでした。しばらくしてから再読み込みしてください。".into(),
        );
    }
    Ok(value)
}

/// レジ管理表の 1 日を保存し、新しい版を返す。
pub fn save_register_count_day(
    token: &str,
    tenant: &Tenant,
    date: &str,
    data: &Value,
    expected_version: Option<i64>,
    book_id: &str,
) -> Result<i64, SaveError> {
    let body = json!({ "bookId": book_id, "data": data, "expectedVersion": expected_version });
    let response = field_request(
        "PUT",
        &format!("/v1/field/register-counts/days/{date}"),
        token,
        &tenant.id,
        &tenant.platform_id,
    )
    .set("Content-Type", "application/json")
    .send_json(body);
    match response {
        Ok(response) => {
            let value: Value = response
                .into_json()
                .map_err(|e| SaveError::Other(format!("応答を読めません: {e}")))?;
            value["version"]
                .as_i64()
                .ok_or_else(|| SaveError::Other("応答の形が想定と違います".into()))
        }
        Err(ureq::Error::Status(409, _)) => Err(SaveError::Conflict),
        Err(error) => Err(SaveError::Other(describe_error(error))),
    }
}

/// 売上台帳（確定済み、1 日分）。`receipt_id` を付けるとその伝票にひもづくものだけ。
pub fn sales_ledger(token: &str, tenant: &Tenant, date: &str, receipt_id: &str) -> Result<Value, String> {
    fetch_record(
        token,
        tenant,
        &format!(
            "/v1/sales-ledger?group_by=day&date_from={date}&date_to={date}&status=confirmed&receipt_id={}",
            encode_query(receipt_id)
        ),
    )
}

/// 気象庁の予報（府県予報区ごとの JSON）。
pub fn jma_forecast(office_code: &str) -> Result<Value, String> {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
        .build()
        .get(&format!(
            "https://www.jma.go.jp/bosai/forecast/data/forecast/{}.json",
            encode_query(office_code)
        ))
        .call()
        .map_err(|_| "天気を取得できませんでした。".to_owned())?
        .into_json()
        .map_err(|_| "天気を取得できませんでした。".to_owned())
}

/// レシートを種類ごとに読む（アップロードした日の範囲。1 回 200 件まで）。`(一覧, 打ち切られたか)`。
pub fn list_receipts(
    token: &str,
    tenant: &Tenant,
    receipt_type: &str,
    from: &str,
    to: &str,
) -> Result<(Vec<Value>, bool), String> {
    let path = format!(
        "/v1/field/receipts?receipt_type={receipt_type}&date_from={}&date_to={}",
        encode_query(&format!("{from}T00:00:00+09:00")),
        encode_query(&format!("{to}T23:59:59+09:00")),
    );
    let mut body = fetch_record(token, tenant, &path)?;
    match body["items"].take() {
        Value::Array(items) => {
            let truncated = items.len() >= 200;
            Ok((items, truncated))
        }
        _ => Err("応答の形が想定と違います".into()),
    }
}

/// 1 枚のレシート（読み取りが終わったか見るため）。
pub fn get_receipt(token: &str, tenant: &Tenant, id: &str) -> Result<Value, String> {
    fetch_record(token, tenant, &format!("/v1/field/receipts/{}", encode_query(id)))
}

/// どこかの出納帳（またはレジ管理表）にもう入っているレシート。
pub fn receipt_uses(
    token: &str,
    tenant: &Tenant,
    ids: &[String],
    register_count: bool,
) -> Result<Vec<String>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut body = json!({ "receiptIds": ids });
    if register_count {
        body["registerCount"] = json!(true);
    }
    let value = send_json(
        token,
        tenant,
        "POST",
        "/v1/field/cash-book/receipt-uses",
        &body,
        Duration::from_secs(15),
    )?;
    Ok(value["receiptIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect())
}

/// レシートの写真・PDF を上げる（読み取りはサーバーで始まる）。同じファイルなら前のものが返る。
pub fn upload_receipt(
    token: &str,
    tenant: &Tenant,
    name: &str,
    content_type: &str,
    bytes: &[u8],
    receipt_type: &str,
    payer_kind: Option<&str>,
) -> Result<Value, String> {
    let boundary = format!("----whisker{:x}", bytes.len() ^ 0x5eed_cafe);
    let safe_name: String = name.chars().filter(|c| *c != '"' && *c != '\r' && *c != '\n').collect();
    let mut body: Vec<u8> = Vec::with_capacity(bytes.len() + 512);
    if let Some(payer) = payer_kind {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"payer_kind\"\r\n\r\n{payer}\r\n").as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"receipt_type\"\r\n\r\n{receipt_type}\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{safe_name}\"\r\n\
             Content-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    field_request("POST", "/v1/field/receipts/upload", token, &tenant.id, &tenant.platform_id)
        .timeout(Duration::from_secs(60))
        .set("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
        .send_bytes(&body)
        .map_err(|error| match error {
            ureq::Error::Status(413, _) => "ファイルが大きすぎます。写真を小さくしてください".into(),
            other => describe_error(other),
        })?
        .into_json()
        .map_err(|e| format!("応答を読めません: {e}"))
}

/// 証憑OCRの一覧（新しい順に 200 件まで）。`receipt_type` / `status` は空なら絞らない。
pub fn search_receipts(
    token: &str,
    tenant: &Tenant,
    receipt_type: &str,
    status: &str,
    unprocessed: bool,
) -> Result<(Vec<Value>, bool), String> {
    let mut query = Vec::new();
    if !receipt_type.is_empty() {
        query.push(format!("receipt_type={}", encode_query(receipt_type)));
    }
    if !status.is_empty() {
        query.push(format!("status={}", encode_query(status)));
    }
    if unprocessed {
        query.push("unprocessed=true".to_owned());
    }
    let path = if query.is_empty() {
        "/v1/field/receipts".to_owned()
    } else {
        format!("/v1/field/receipts?{}", query.join("&"))
    };
    let mut body = fetch_record(token, tenant, &path)?;
    match body["items"].take() {
        Value::Array(items) => {
            let truncated = items.len() >= 200;
            Ok((items, truncated))
        }
        _ => Err("応答の形が想定と違います".into()),
    }
}

/// 証憑を読み直す。`receipt_type` を渡すと、その種別に変えて読み直す。
pub fn reprocess_receipt(
    token: &str,
    tenant: &Tenant,
    id: &str,
    receipt_type: Option<&str>,
) -> Result<Value, String> {
    let body = match receipt_type {
        Some(t) => json!({ "receiptType": t }),
        None => json!({}),
    };
    send_json(
        token,
        tenant,
        "POST",
        &format!("/v1/field/receipts/{}/reprocess", encode_query(id)),
        &body,
        Duration::from_secs(120),
    )
}

/// 重複候補の証憑を、別の取引として登録する。
pub fn confirm_distinct_receipt(token: &str, tenant: &Tenant, id: &str) -> Result<Value, String> {
    send_json(
        token,
        tenant,
        "POST",
        &format!("/v1/field/receipts/{}/confirm-distinct", encode_query(id)),
        &json!({}),
        Duration::from_secs(30),
    )
}

/// 使える経費科目 `(id, 名前)`。
pub fn expense_categories(token: &str, tenant: &Tenant) -> Result<Vec<(String, String)>, String> {
    let body = fetch_record(token, tenant, "/v1/erp/expense-categories")?;
    Ok(body["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["isActive"].as_bool().unwrap_or(true))
        .filter_map(|c| {
            Some((
                c["id"].as_str()?.to_owned(),
                c["name"].as_str().unwrap_or("").to_owned(),
            ))
        })
        .collect())
}

/// 証憑から経費を起票する（何度呼んでも 1 件）。`{ created, expense, receipt }` が返る。
pub fn book_receipt_expense(
    token: &str,
    tenant: &Tenant,
    id: &str,
    input: &Value,
) -> Result<Value, String> {
    send_json(
        token,
        tenant,
        "POST",
        &format!("/v1/field/receipts/{}/expense", encode_query(id)),
        input,
        Duration::from_secs(30),
    )
}

/// 証憑を仕入台帳（`sales` なら売上台帳）へ反映する。反映した件数を返す。
pub fn ledger_from_receipt(
    token: &str,
    tenant: &Tenant,
    id: &str,
    sales: bool,
) -> Result<usize, String> {
    let base = if sales {
        "/v1/sales-ledger"
    } else {
        "/v1/purchase-ledger"
    };
    let value = send_json(
        token,
        tenant,
        "POST",
        &format!("{base}/from-receipt/{}", encode_query(id)),
        &json!({}),
        Duration::from_secs(30),
    )?;
    Ok(if sales {
        value["items"].as_array().map_or(0, Vec::len)
    } else {
        value["createdCount"].as_u64().unwrap_or(0) as usize
    })
}

/// レシートの読み取りを直す（金額・支払先・日付など）。直したレシートが返る。
pub fn correct_receipt(token: &str, tenant: &Tenant, id: &str, reading: &Value) -> Result<Value, String> {
    send_json(
        token,
        tenant,
        "PATCH",
        &format!("/v1/field/receipts/{}/reading", encode_query(id)),
        reading,
        Duration::from_secs(30),
    )
}

/// 商品 1 つ（締めレシートを売上項目に振り分けるのに使う）。
pub fn get_product(token: &str, tenant: &Tenant, id: &str) -> Result<Value, String> {
    fetch_record(token, tenant, &format!("/v1/order/products/{}", encode_query(id)))
}

/// 店舗マスタに店舗を 1 つ登録する（帳簿を足すには店舗が要る）。
pub fn create_store(token: &str, tenant: &Tenant, name: &str) -> Result<Value, String> {
    send_json(
        token,
        tenant,
        "POST",
        "/v1/field/inventory/stores",
        &json!({ "name": name, "address": null, "businessHours": null, "posLocationId": null }),
        Duration::from_secs(15),
    )
}

/// 商品マスタを探す（売上項目との連携に使う）。`(商品, 続きがあるか)`。
pub fn search_products(
    token: &str,
    tenant: &Tenant,
    query: &str,
    offset: usize,
) -> Result<(Vec<Value>, bool), String> {
    let mut path = format!("/v1/order/products?limit=20&offset={offset}");
    if !query.trim().is_empty() {
        path.push_str("&q=");
        path.push_str(&encode_query(query.trim()));
    }
    let mut body = fetch_record(token, tenant, &path)?;
    let more = body["hasNextPage"].as_bool().unwrap_or(false);
    match body["items"].take() {
        Value::Array(items) => Ok((items, more)),
        _ => Err("応答の形が想定と違います".into()),
    }
}

/// 印刷形式の案を AI に作ってもらう。`document` は参考帳票（base64 と PDF のページ）。
pub fn suggest_cash_book_print(
    token: &str,
    tenant: &Tenant,
    instructions: &str,
    current: &Value,
    document: Option<(String, Option<u32>)>,
) -> Result<Value, String> {
    let mut body = json!({ "instructions": instructions, "current": current });
    let timeout = if let Some((base64, page)) = document {
        body["document"] = json!({ "base64": base64 });
        if let Some(page) = page {
            body["document"]["page"] = json!(page);
        }
        Duration::from_secs(90)
    } else {
        Duration::from_secs(30)
    };
    send_json(
        token,
        tenant,
        "POST",
        "/v1/field/cash-book/print-suggestions",
        &body,
        timeout,
    )
}

/// 現金出納帳の日を `from`〜`to`（両端を含む、92 日まで）読む。
/// `book_id` が無ければ従来の帳簿（店舗・レジを分ける前）。
pub fn cash_book_days(
    token: &str,
    tenant: &Tenant,
    from: &str,
    to: &str,
    book_id: Option<&str>,
) -> Result<Value, String> {
    let mut path = format!("/v1/field/cash-book/days?from={from}&to={to}");
    if let Some(id) = book_id {
        path.push_str("&book_id=");
        path.push_str(&encode_query(id));
    }
    fetch_record(token, tenant, &path)
}

/// 出納帳の保存が断られた理由。
#[derive(Clone, Debug, PartialEq)]
pub enum SaveError {
    /// ほかの端末が先にこの日を保存した（版が合わない）。
    Conflict,
    /// 行に入れたレシートが、すでにほかの日（`date`）の出納帳に入っている。
    ReceiptUsed { receipt_id: String, date: String },
    Other(String),
}

/// 現金出納帳の 1 日を保存し、新しい版を返す。`expected_version` は編集を始めたときの版
/// （一度も保存していない日は `None`）。合わなければ `SaveError::Conflict`。
pub fn save_cash_book_day(
    token: &str,
    tenant: &Tenant,
    date: &str,
    data: &Value,
    expected_version: Option<i64>,
    book_id: Option<&str>,
) -> Result<i64, SaveError> {
    let mut body = json!({ "data": data, "expectedVersion": expected_version });
    if let Some(id) = book_id {
        body["bookId"] = json!(id);
    }
    let response = field_request(
        "PUT",
        &format!("/v1/field/cash-book/days/{date}"),
        token,
        &tenant.id,
        &tenant.platform_id,
    )
    .set("Content-Type", "application/json")
    .send_json(body);
    match response {
        Ok(response) => {
            let value: Value = response
                .into_json()
                .map_err(|e| SaveError::Other(format!("応答を読めません: {e}")))?;
            value["version"]
                .as_i64()
                .ok_or_else(|| SaveError::Other("応答の形が想定と違います".into()))
        }
        Err(ureq::Error::Status(409, response)) => {
            let body: Value = response.into_json().unwrap_or(Value::Null);
            Err(receipt_used(body["message"].as_str().unwrap_or(""))
                .map_or(SaveError::Conflict, |(receipt_id, date)| {
                    SaveError::ReceiptUsed { receipt_id, date }
                }))
        }
        Err(error) => Err(SaveError::Other(describe_error(error))),
    }
}

/// サーバーの文言（cash_book_api.rs の receipt_used_message）からレシートと日を取り出す。
fn receipt_used(message: &str) -> Option<(String, String)> {
    let rest = message.split("receipt ").nth(1)?;
    let (receipt_id, rest) = rest.split_once(" is already on the cash book for ")?;
    let date: String = rest.chars().take(10).collect();
    let valid = date.len() == 10 && date.chars().filter(|c| *c == '-').count() == 2;
    valid.then(|| (receipt_id.to_owned(), date))
}

fn encode_query(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Field Agent の execute だけを流す Worker。本番 API は応答をため込む（Lambda）ので、
/// Web 版（apps/client/src/api/field-stream-url.ts）と同じくこちらへ送る。
const AGENT_STREAM_BASE_URL: &str = "https://field-agent-stream.txcloud.app";

/// Field Agent から届いた 1 つの知らせ。
pub enum AgentEvent {
    /// 返事の文の続き。`kind` が変わったら別の段落として扱う。
    Text { kind: String, text: String },
    /// 操作を始めた。
    ToolStart { id: String, label: String },
    /// 操作が終わった。
    ToolDone { id: String, label: String, ok: bool },
    /// 最後のまとめ（文として流れてこなかったときだけ出す）。
    Completion(String),
    /// 途中で失敗した。
    Failed(String),
    /// アクセストークンを取り直した。画面側はこれを覚えておく。
    Renewed(String),
    /// 添付ファイルを Field へ上げ終えた。次の回からもこの会話で使えるよう覚えておく。
    Attached(Vec<AgentFile>),
}

/// Field Agent の会話を 1 つ作り、その ID（`as_…`）を返す。
pub fn agent_session(token: &str, tenant: &Tenant) -> Result<String, String> {
    let value: Value = field_request(
        "POST",
        "/v1/field/agent/sessions",
        token,
        &tenant.id,
        &tenant.platform_id,
    )
    .send_json(json!({}))
    .map_err(describe_error)?
    .into_json()
    .map_err(|e| format!("応答を読めません: {e}"))?;
    value["session_id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "会話を始められませんでした".into())
}

/// Field Agent に 1 つ頼み、返事を少しずつ `emit` に渡す（SSE）。
/// 道具（Field の操作）はサーバー側で実行され、その経過も届く。
/// `cancel` が立ったら、次の知らせを受けたところで読むのをやめる。
pub fn agent_stream(
    token: &str,
    tenant: &Tenant,
    session: &str,
    task: &str,
    files: &[AgentFile],
    inline: &[Value],
    cancel: &std::sync::atomic::AtomicBool,
    emit: impl Fn(AgentEvent),
) -> Result<(), String> {
    use std::io::BufRead;
    use std::sync::atomic::Ordering;

    // 止められた頼みは、サーバーへ送らない（送ると操作が実行されてしまう）。
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }

    let response = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        // 操作の実行中は何も届かないことがあるので、1 回の待ちは長めにとる。
        .timeout_read(Duration::from_secs(150))
        .build()
        .post(&format!(
            "{AGENT_STREAM_BASE_URL}/v1/field/agent/sessions/{session}/execute"
        ))
        .set("Authorization", &format!("Bearer {token}"))
        .set("x-operator-id", &tenant.id)
        .set("x-platform-id", &tenant.platform_id)
        .set("Accept", "text/event-stream")
        .send_json(json!({
            "task": task,
            "page": "/mobile",
            "server_tools": true,
            "attachments": inline,
            "files": files,
            "allowed_jobs": [],
        }))
        .map_err(describe_error)?;

    let mut reader = std::io::BufReader::new(response.into_reader());
    let mut line = String::new();
    let mut data = String::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| format!("通信が切れました: {e}"))?;
        if read == 0 {
            return Ok(());
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = trimmed.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            continue;
        }
        if !trimmed.is_empty() || data.is_empty() {
            continue;
        }
        let event: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
        data.clear();
        let kind = event["type"].as_str().unwrap_or_default();
        match kind {
            "say" | "assistant" | "ask" => {
                if let Some(text) = event["text"].as_str() {
                    emit(AgentEvent::Text {
                        kind: kind.to_owned(),
                        text: text.to_owned(),
                    });
                }
            }
            "tool_call_pending" => emit(AgentEvent::ToolStart {
                id: event["tool_id"].as_str().unwrap_or_default().to_owned(),
                label: tool_label(&event),
            }),
            "field_tool" => emit(AgentEvent::ToolDone {
                id: event["tool_id"].as_str().unwrap_or_default().to_owned(),
                label: tool_label(&event),
                ok: event["state"].as_str() != Some("error"),
            }),
            "attempt_completion" => {
                if let Some(result) = event["result"].as_str() {
                    emit(AgentEvent::Completion(result.to_owned()));
                }
            }
            "error" => {
                let reason = event["message"]
                    .as_str()
                    .or_else(|| event["error"].as_str())
                    .unwrap_or_default();
                emit(AgentEvent::Failed(if reason.is_empty() {
                    "Field Agent が途中で止まりました".into()
                } else {
                    format!("Field Agent が途中で止まりました（{reason}）")
                }));
                return Ok(());
            }
            "done" => return Ok(()),
            _ => {}
        }
    }
}

/// 操作の名前を、画面に出す言葉にする（Web 版の toolLabel と同じ考え方。ID は出さない）。
fn tool_label(event: &Value) -> String {
    let args = match &event["args"] {
        Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        other => other.clone(),
    };
    let arg = |key: &str| args[key].as_str().unwrap_or_default().to_owned();
    match event["tool_name"].as_str().unwrap_or_default() {
        "field_search_operations" => format!("操作を探す：{}", arg("query")),
        "field_describe_operation" => "使い方を確認".into(),
        "field_read_attachment" => "添付ファイルを読む".into(),
        "field_upload_attachment" => "添付ファイルを送る".into(),
        "field_execute_operation" => {
            let operation = &event["operation"];
            let summary = operation["summary"].as_str().filter(|s| !s.is_empty());
            match summary {
                Some(summary) => summary.to_owned(),
                None => match operation["method"].as_str().unwrap_or("GET") {
                    "GET" => "データを読む".into(),
                    _ => "データを書き込む".into(),
                },
            }
        }
        "" => "操作".into(),
        other => other.replace('_', " "),
    }
}

/// Agent に渡すファイル。先に Field へ上げておき、その置き場所（id）で渡す。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct AgentFile {
    pub id: String,
    pub name: String,
    pub content_type: String,
    pub size: u64,
}

/// 1 回で送れる大きさ。これより大きいファイルは、預け先の URL を受け取って直接上げる。
const RELAY_UPLOAD_BYTES: usize = 4_500_000;

/// Agent に渡すファイルを Field に上げ、置き場所の id を返す。
pub fn upload_attachment(
    token: &str,
    tenant: &Tenant,
    name: &str,
    content_type: &str,
    bytes: &[u8],
) -> Result<String, String> {
    let request = |method: &str, path: &str| {
        field_request(method, path, token, &tenant.id, &tenant.platform_id)
            .timeout(Duration::from_secs(120))
    };
    if bytes.len() <= RELAY_UPLOAD_BYTES {
        let response: Value = request("POST", "/v1/field/agent/attachments/upload")
            .query("name", name)
            .query("content_type", content_type)
            .set("Content-Type", "application/octet-stream")
            .send_bytes(bytes)
            .map_err(describe_error)?
            .into_json()
            .map_err(|e| e.to_string())?;
        return attachment_id(&response);
    }
    let created: Value = request("POST", "/v1/field/agent/attachments")
        .send_json(json!({ "name": name, "content_type": content_type, "size": bytes.len() }))
        .map_err(describe_error)?
        .into_json()
        .map_err(|e| e.to_string())?;
    let id = attachment_id(&created)?;
    let url = created["upload_url"]
        .as_str()
        .ok_or("ファイルの預け先が返ってきませんでした")?;
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(300))
        .build()
        .put(url)
        .set("content-type", content_type)
        .send_bytes(bytes)
        .map_err(|e| format!("ファイルを上げられませんでした: {e}"))?;
    request("POST", "/v1/field/agent/attachments/confirm")
        .send_json(json!({ "attachment_id": id }))
        .map_err(describe_error)?;
    Ok(id)
}

fn attachment_id(response: &Value) -> Result<String, String> {
    response["attachment_id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "ファイルの置き場所が返ってきませんでした".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_receipt_used_message() {
        let message = "receipt rc_01abc is already on the cash book for 2026-11-03; remove it from that day first";
        assert_eq!(receipt_used(message), Some(("rc_01abc".into(), "2026-11-03".into())));
        assert_eq!(receipt_used("expectedVersion does not match"), None);
        assert_eq!(encode_query("a b/é"), "a%20b%2F%C3%A9");
    }
}
