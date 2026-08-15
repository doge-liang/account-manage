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
