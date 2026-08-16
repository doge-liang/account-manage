//! 用量缓存 — 对齐 _usage_load_cache/_usage_save_cache（原子写）。

use crate::store;
use serde_json::{Map, Value};
use std::fs;

pub fn load_cache() -> Value {
    let f = store::usage_cache_file();
    if !f.exists() {
        return Value::Object(Map::new());
    }
    match fs::read_to_string(&f) {
        Ok(s) => serde_json::from_str(&s).unwrap_or(Value::Object(Map::new())),
        Err(_) => Value::Object(Map::new()),
    }
}

pub fn save_cache(cache: &Value) {
    let dir = store::data_dir();
    let _ = fs::create_dir_all(&dir);
    let tmp = dir.join("usage_cache.json.tmp");
    if fs::write(&tmp, serde_json::to_string(cache).unwrap_or_default()).is_ok() {
        let _ = fs::rename(&tmp, store::usage_cache_file());
    }
}

/// 记录一次失败的抓取：保留旧数据字段（继续展示最后成功值），
/// 叠加 last_error + error_at 供前端显示「抓取失败」状态。
pub fn record_error(cfg_id: &str, err: &str) {
    let mut cache = load_cache();
    let entry = cache
        .as_object_mut()
        .unwrap()
        .entry(cfg_id.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(o) = entry.as_object_mut() {
        o.insert("last_error".into(), Value::String(err.chars().take(200).collect()));
        o.insert("error_at".into(), Value::String(store::now_iso()));
        // 有 last_error 时清掉 fetched_at 的语义由前端判断（保留最后成功时间）
    }
    save_cache(&cache);
}
