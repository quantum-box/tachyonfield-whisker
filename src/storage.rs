//! サインイン状態を端末のキーチェーン（iOS / macOS の安全な保存場所）に残す。
//!
//! 残すのは 2 つだけ：
//! - 長く使える更新用のキー（リフレッシュトークン）。次回起動時に
//!   これでアクセストークンを取り直し、パスワードなしで入れる。
//! - 選んだテナント（ID・名前・環境）。
//!
//! アクセストークン（1 時間で切れる）とパスワードは保存しない。
//! 保存に失敗しても、今回のサインインはそのまま使えるようにする。

use serde_json::{Value, json};

use crate::api::Tenant;

const SERVICE: &str = "app.txcloud.tachyonfield.whisker";
const REFRESH_TOKEN: &str = "cognito-refresh-token";
const TENANT: &str = "selected-tenant";

pub fn refresh_token() -> Option<String> {
    read(REFRESH_TOKEN)
}

pub fn save_refresh_token(token: &str) {
    write(REFRESH_TOKEN, token);
}

pub fn tenant() -> Option<Tenant> {
    let value: Value = serde_json::from_str(&read(TENANT)?).ok()?;
    let text = |k: &str| value[k].as_str().map(str::to_owned);
    Some(Tenant {
        id: text("id")?,
        name: text("name").unwrap_or_default(),
        environment: text("environment").unwrap_or_default(),
        platform_id: text("platformId")?,
    })
}

pub fn save_tenant(tenant: &Tenant) {
    let value = json!({
        "id": tenant.id,
        "name": tenant.name,
        "environment": tenant.environment,
        "platformId": tenant.platform_id,
    });
    write(TENANT, &value.to_string());
}

/// 選んだテナントだけ忘れる（iOS 以外の「会社を切り替え」で使う。iOS はその場で切り替える）。
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub fn forget_tenant() {
    remove(TENANT);
}

/// サインアウト：保存したものを全部消す。
pub fn forget_all() {
    remove(REFRESH_TOKEN);
    remove(TENANT);
}

#[cfg(target_vendor = "apple")]
fn read(account: &str) -> Option<String> {
    let bytes = security_framework::passwords::get_generic_password(SERVICE, account).ok()?;
    String::from_utf8(bytes).ok().filter(|s| !s.is_empty())
}

#[cfg(target_vendor = "apple")]
fn write(account: &str, value: &str) {
    if let Err(error) =
        security_framework::passwords::set_generic_password(SERVICE, account, value.as_bytes())
    {
        eprintln!("[field-mobile] キーチェーンに保存できません（{account}）: {error}");
    }
}

#[cfg(target_vendor = "apple")]
fn remove(account: &str) {
    let _ = security_framework::passwords::delete_generic_password(SERVICE, account);
}

// Apple 以外（Android など）はまだ保存しない。毎回サインインになる。
#[cfg(not(target_vendor = "apple"))]
fn read(_account: &str) -> Option<String> {
    None
}

#[cfg(not(target_vendor = "apple"))]
fn write(_account: &str, _value: &str) {}

#[cfg(not(target_vendor = "apple"))]
fn remove(_account: &str) {}
