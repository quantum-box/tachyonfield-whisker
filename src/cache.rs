//! 一度読んだ一覧と詳細を覚えておく。
//!
//! 画面を開いたら、まず覚えておいた中身をすぐ出し、裏で取り直して届いたら差し替える。
//! アプリを閉じても残るよう、端末のキャッシュ用フォルダ（iOS が容量に応じて消してよい場所）にも書く。
//! 一覧と詳細は会社（テナント）ごとに分けて覚える。選べる会社の一覧も覚える。サインアウトしたら全部消す。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

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
