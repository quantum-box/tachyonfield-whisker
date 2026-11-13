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

#[derive(Clone, Debug, PartialEq)]
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
                "NotAuthorizedException" => "メールアドレスかパスワードが違います".into(),
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
    session_from(cognito(
        "InitiateAuth",
        json!({
            "AuthFlow": "REFRESH_TOKEN_AUTH",
            "ClientId": COGNITO_CLIENT_ID,
            "AuthParameters": { "REFRESH_TOKEN": refresh_token },
        }),
    )?)
    .map_err(|_| EXPIRED.to_owned())
}

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
    cancel: &std::sync::atomic::AtomicBool,
    mut emit: impl FnMut(AgentEvent),
) -> Result<(), String> {
    use std::io::BufRead;
    use std::sync::atomic::Ordering;

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
            "files": [],
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
