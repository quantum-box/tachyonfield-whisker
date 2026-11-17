//! 一度読んだ一覧と詳細を覚えておく。
//!
//! 画面を開いたら、まず覚えておいた中身をすぐ出し、裏で取り直して届いたら差し替える。
//! アプリを閉じても残るよう、端末のキャッシュ用フォルダ（iOS が容量に応じて消してよい場所）にも書く。
//! 一覧と詳細は会社（テナント）ごとに分けて覚える。選べる会社の一覧と、最後に開いていた画面も覚える。
//! サインアウトしたら全部消す。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::Tenant;

fn memory() -> &'static Mutex<HashMap<String, Value>> {
    static MEMORY: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    MEMORY.get_or_init(Default::default)
}

fn key(tenant: &Tenant, path: &str) -> String {
    format!("{}{path}", tenant.id)
}

/// 覚えておいた中身。無ければ None。
pub fn get(tenant: &Tenant, path: &str) -> Option<Value> {
    read(key(tenant, path))
}

/// 読んだ中身を覚える。
pub fn put(tenant: &Tenant, path: &str, value: &Value) {
    write(key(tenant, path), value);
}

const TENANTS: &str = "tenants";

/// 前に読んだ、選べる会社の一覧。
pub fn tenants() -> Option<Vec<Tenant>> {
    serde_json::from_value(read(TENANTS.into())?).ok()
}

pub fn put_tenants(tenants: &[Tenant]) {
    if let Ok(value) = serde_json::to_value(tenants) {
        write(TENANTS.into(), &value);
    }
}

const LAST_SCREEN: &str = "last-screen";

/// 最後に開いていた画面。次に起動したら、ここから始める。
/// 画面の移り変わりだけを覚え、入力途中の文字や下書き・開いていた詳細は覚えない。
/// 会社ごとではなく 1 つだけ覚え、覚えたときと会社が違えば使わない。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LastScreen {
    pub tenant: String,
    /// 裏のメニューの分野（名前）。
    pub area: String,
    /// 下のタブで開いていた機能のパス。None なら Agent。
    pub feature: Option<String>,
    /// 思想と行動で選んでいた事業部（空なら全社）と、閉じていた行。
    pub department: String,
    pub collapsed: Vec<String>,
}

/// 前回と同じ会社のときだけ、最後に開いていた画面を返す。
pub fn last_screen(tenant: &Tenant) -> Option<LastScreen> {
    serde_json::from_value::<LastScreen>(read(LAST_SCREEN.into())?)
        .ok()
        .filter(|last| last.tenant == tenant.id)
}

/// 最後に開いていた画面を書き換える。会社が変わっていたら、前の会社の分は捨てて覚え直す。
pub fn update_last_screen(tenant: &Tenant, change: impl FnOnce(&mut LastScreen)) {
    let mut last = last_screen(tenant).unwrap_or_else(|| LastScreen {
        tenant: tenant.id.clone(),
        ..LastScreen::default()
    });
    change(&mut last);
    if let Ok(value) = serde_json::to_value(&last) {
        write(LAST_SCREEN.into(), &value);
    }
}

fn read(key: String) -> Option<Value> {
    if let Some(value) = memory().lock().ok()?.get(&key) {
        return Some(value.clone());
    }
    let value: Value = serde_json::from_slice(&std::fs::read(file(&key)?).ok()?).ok()?;
    memory().lock().ok()?.insert(key, value.clone());
    Some(value)
}

fn write(key: String, value: &Value) {
    if let Some(file) = file(&key) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, value.to_string());
    }
    if let Ok(mut memory) = memory().lock() {
        memory.insert(key, value.clone());
    }
}

/// 全部忘れる（サインアウトのとき）。
pub fn clear() {
    if let Ok(mut memory) = memory().lock() {
        memory.clear();
    }
    if let Some(dir) = dir() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn dir() -> Option<PathBuf> {
    if !cfg!(target_vendor = "apple") {
        return None;
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Caches/app.txcloud.tachyonfield.whisker/data"))
}

fn file(key: &str) -> Option<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    Some(dir()?.join(format!("{:016x}.json", hasher.finish())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(id: &str) -> Tenant {
        Tenant {
            id: id.into(),
            name: String::new(),
            environment: String::new(),
            platform_id: String::new(),
        }
    }

    #[test]
    fn last_screen_is_kept_only_for_the_same_tenant() {
        let (a, b) = (tenant("tn_last_a"), tenant("tn_last_b"));
        update_last_screen(&a, |last| {
            last.area = "戦略".into();
            last.feature = Some("screen:strategy".into());
        });
        update_last_screen(&a, |last| {
            last.department = "div-1".into();
            last.collapsed = vec!["o1".into()];
        });
        let last = last_screen(&a).unwrap();
        // 画面と事業部・開閉は別々に書き換えても、両方残る。
        assert_eq!(last.feature.as_deref(), Some("screen:strategy"));
        assert_eq!(last.department, "div-1");
        assert_eq!(last.collapsed, vec!["o1".to_owned()]);
        // 会社が違えば使わない。
        assert!(last_screen(&b).is_none());
        // 別の会社で書き換えたら、前の会社の分は捨てる。
        update_last_screen(&b, |last| last.area = "現金".into());
        assert!(last_screen(&a).is_none());
        let last = last_screen(&b).unwrap();
        assert_eq!(last.area, "現金");
        assert!(last.department.is_empty() && last.collapsed.is_empty());
    }
}
