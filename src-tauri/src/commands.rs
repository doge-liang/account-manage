//! Tauri commands — 逐条对应 server.py 的 25 个 HTTP 路由。
//! 数据全程用 serde_json::Value（与 Python 版整棵 JSON 读写的行为一致）。

use crate::store;
use crate::vault;
use serde_json::{json, Value};
use std::sync::Mutex;

/// 全局数据锁（对齐 Python _lock）
pub struct State(pub Mutex<()>);

fn err(msg: impl Into<String>) -> Value {
    json!({ "error": msg.into() })
}

fn ok<T: serde::Serialize>(v: T) -> Value {
    serde_json::to_value(v).unwrap()
}

// ---- 数据 ----

#[tauri::command]
pub fn get_data() -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    store::load_data_raw()
}

#[tauri::command]
pub fn save_settings(payload: Value) -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    let mut data = store::load_data_raw();
    let settings = data
        .as_object_mut()
        .unwrap()
        .entry("settings")
        .or_insert_with(|| json!({}));
    let s = settings.as_object_mut().unwrap();
    if let Some(vp) = payload.get("vault_path").and_then(|v| v.as_str()) {
        let v = vp.trim();
        s.insert(
            "vault_path".into(),
            if v.is_empty() {
                json!(store::default_vault().to_string_lossy())
            } else {
                json!(v)
            },
        );
    }
    if let Some(n) = payload.get("name").and_then(|v| v.as_str()) {
        let v = n.trim();
        s.insert(
            "name".into(),
            if v.is_empty() { json!("账号管家") } else { json!(v) },
        );
    }
    let _ = store::save_data_raw(&data);
    ok(json!({ "ok": true, "settings": data["settings"] }))
}

// ---- 账号 ----

const KNOWN_CATEGORIES: &[&str] = &[
    "ai_member", "api", "email", "phone", "wechat", "public_account", "qq", "zlibrary", "apple",
    "other",
];

fn validate_account(p: &Value, data: &Value) -> Result<Value, String> {
    let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
    if name.is_empty() {
        return Err("显示名称不能为空".into());
    }
    let category = p.get("category").and_then(|v| v.as_str()).unwrap_or("other").trim();
    if !KNOWN_CATEGORIES.contains(&category) {
        return Err(format!("未知类别: {category}"));
    }
    let fields = p.get("fields").cloned().unwrap_or(json!({}));
    if !fields.is_object() {
        return Err("fields 必须是对象".into());
    }
    Ok(json!({
        "category": category,
        "name": name,
        "vendor": p.get("vendor").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "username": p.get("username").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "url": p.get("url").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "status": p.get("status").and_then(|v| v.as_str()).unwrap_or("active").trim(),
        "notes": p.get("notes").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "fields": fields,
    }))
}

#[tauri::command]
pub fn upsert_account(id: Option<String>, payload: Value) -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    let mut data = store::load_data_raw();
    let acc = match validate_account(&payload, &data) {
        Ok(a) => a,
        Err(e) => return err(e),
    };
    let now = store::now_iso();
    let accounts = data["accounts"].as_array_mut().unwrap();
    match id {
        Some(acc_id) => {
            let idx = accounts.iter().position(|a| a["id"] == json!(acc_id));
            match idx {
                Some(i) => {
                    let created = accounts[i].get("created_at").cloned().unwrap_or(json!(now));
                    let mut updated = acc;
                    updated["id"] = json!(acc_id);
                    updated["created_at"] = created;
                    updated["updated_at"] = json!(now);
                    accounts[i] = updated.clone();
                    let _ = store::save_data_raw(&data);
                    ok(json!({ "ok": true, "account": updated }))
                }
                None => err(format!("账号 {acc_id} 不存在")),
            }
        }
        None => {
            let new_id = store::new_id("acc", accounts, "id");
            let mut created = acc;
            created["id"] = json!(new_id);
            created["created_at"] = json!(now);
            created["updated_at"] = json!(now);
            accounts.push(created.clone());
            let _ = store::save_data_raw(&data);
            ok(json!({ "ok": true, "account": created }))
        }
    }
}

#[tauri::command]
pub fn delete_account(id: String) -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    let mut data = store::load_data_raw();
    let accounts = data["accounts"].as_array().unwrap().clone();
    if !accounts.iter().any(|a| a["id"] == json!(id)) {
        return err(format!("账号 {id} 不存在"));
    }
    data["accounts"] = Value::Array(
        accounts.into_iter().filter(|a| a["id"] != json!(id)).collect(),
    );
    let rels_before = data["relations"].as_array().unwrap().len();
    data["relations"] = Value::Array(
        data["relations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["from"] != json!(id) && r["to"] != json!(id))
            .cloned()
            .collect(),
    );
    let removed = rels_before - data["relations"].as_array().unwrap().len();
    let _ = store::save_data_raw(&data);
    ok(json!({ "ok": true, "removed_relations": removed }))
}

// ---- 关联 ----

fn validate_relation(p: &Value, data: &Value) -> Result<Value, String> {
    let frm = p.get("from").and_then(|v| v.as_str()).unwrap_or("").trim();
    let to = p.get("to").and_then(|v| v.as_str()).unwrap_or("").trim();
    if frm.is_empty() || to.is_empty() {
        return Err("关联的两端账号不能为空".into());
    }
    let ids: Vec<&str> = data["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["id"].as_str())
        .collect();
    if !ids.contains(&frm) || !ids.contains(&to) {
        return Err("关联的账号不存在".into());
    }
    if frm == to {
        return Err("不能关联到自身".into());
    }
    let rtype = p.get("type").and_then(|v| v.as_str()).unwrap_or("其他").trim();
    Ok(json!({
        "from": frm, "to": to,
        "type": if rtype.is_empty() { "其他" } else { rtype },
        "note": p.get("note").and_then(|v| v.as_str()).unwrap_or("").trim(),
    }))
}

#[tauri::command]
pub fn upsert_relation(id: Option<String>, payload: Value) -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    let mut data = store::load_data_raw();
    let rel = match validate_relation(&payload, &data) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let now = store::now_iso();
    let relations = data["relations"].as_array_mut().unwrap();
    match id {
        Some(rel_id) => {
            let idx = relations.iter().position(|r| r["id"] == json!(rel_id));
            match idx {
                Some(i) => {
                    let mut updated = rel;
                    updated["id"] = json!(rel_id);
                    relations[i] = updated.clone();
                    let _ = store::save_data_raw(&data);
                    ok(json!({ "ok": true, "relation": updated }))
                }
                None => err(format!("关联 {rel_id} 不存在")),
            }
        }
        None => {
            let new_id = store::new_id("rel", relations, "id");
            let mut created = rel;
            created["id"] = json!(new_id);
            created["created_at"] = json!(now);
            relations.push(created.clone());
            let _ = store::save_data_raw(&data);
            ok(json!({ "ok": true, "relation": created }))
        }
    }
}

#[tauri::command]
pub fn delete_relation(id: String) -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    let mut data = store::load_data_raw();
    let rels = data["relations"].as_array().unwrap().clone();
    if !rels.iter().any(|r| r["id"] == json!(id)) {
        return err(format!("关联 {id} 不存在"));
    }
    data["relations"] = Value::Array(rels.into_iter().filter(|r| r["id"] != json!(id)).collect());
    let _ = store::save_data_raw(&data);
    ok(json!({ "ok": true }))
}

// ---- 查询链接 ----

fn validate_query_link(p: &Value) -> Result<Value, String> {
    let label = p.get("label").and_then(|v| v.as_str()).unwrap_or("").trim();
    let url = p.get("url").and_then(|v| v.as_str()).unwrap_or("").trim();
    if label.is_empty() || url.is_empty() {
        return Err("标签和链接不能为空".into());
    }
    Ok(json!({
        "category": p.get("category").and_then(|v| v.as_str()).unwrap_or("other").trim(),
        "vendor": p.get("vendor").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "label": label,
        "url": url,
    }))
}

#[tauri::command]
pub fn upsert_query_link(id: Option<String>, payload: Value) -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    let mut data = store::load_data_raw();
    let q = match validate_query_link(&payload) {
        Ok(q) => q,
        Err(e) => return err(e),
    };
    let links = data["query_links"].as_array_mut().unwrap();
    match id {
        Some(qid) => {
            let idx = links.iter().position(|x| x["id"] == json!(qid));
            match idx {
                Some(i) => {
                    let mut updated = q;
                    updated["id"] = json!(qid);
                    links[i] = updated.clone();
                    let _ = store::save_data_raw(&data);
                    ok(json!({ "ok": true, "query_link": updated }))
                }
                None => err(format!("查询链接 {qid} 不存在")),
            }
        }
        None => {
            let new_id = store::new_id("q", links, "id");
            let mut created = q;
            created["id"] = json!(new_id);
            links.push(created.clone());
            let _ = store::save_data_raw(&data);
            ok(json!({ "ok": true, "query_link": created }))
        }
    }
}

#[tauri::command]
pub fn delete_query_link(id: String) -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    let mut data = store::load_data_raw();
    let links = data["query_links"].as_array().unwrap().clone();
    if !links.iter().any(|q| q["id"] == json!(id)) {
        return err(format!("查询链接 {id} 不存在"));
    }
    data["query_links"] = Value::Array(links.into_iter().filter(|q| q["id"] != json!(id)).collect());
    let _ = store::save_data_raw(&data);
    ok(json!({ "ok": true }))
}

// ---- 用量配置 ----

fn validate_usage_config(p: &Value, data: &Value) -> Result<Value, String> {
    let account_id = p.get("account_id").and_then(|v| v.as_str()).unwrap_or("").trim();
    let ids: Vec<&str> = data["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["id"].as_str())
        .collect();
    if !ids.contains(&account_id) {
        return Err("account_id 不存在".into());
    }
    let provider = p.get("provider").and_then(|v| v.as_str()).unwrap_or("").trim();
    let api_key = p.get("api_key").and_then(|v| v.as_str()).unwrap_or("").trim();
    let mut url = p.get("url").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let mut method = p.get("method").and_then(|v| v.as_str()).unwrap_or("GET").trim().to_uppercase();
    let mut jp_used = p.get("jsonpath_used").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let mut jp_total = p.get("jsonpath_total").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let headers = p.get("headers").cloned().unwrap_or(json!({}));
    let body = p.get("body").and_then(|v| v.as_str()).unwrap_or("");

    let meta = crate::providers::metadata(provider);
    if !provider.is_empty() && meta.is_some() {
        let m = meta.unwrap();
        if m.get("requires_api_key").and_then(|v| v.as_bool()).unwrap_or(false) && api_key.is_empty() {
            return Err("该 provider 需要 API Key".into());
        }
        if url.is_empty() {
            if let Some(d) = m.get("default_url").and_then(|v| v.as_str()) {
                url = d.to_string();
            }
        }
        if jp_used.is_empty() {
            if let Some(d) = m.get("default_jsonpath_used").and_then(|v| v.as_str()) {
                jp_used = d.to_string();
            }
        }
        if jp_total.is_empty() {
            if let Some(d) = m.get("default_jsonpath_total").and_then(|v| v.as_str()) {
                jp_total = d.to_string();
            }
        }
        if method.is_empty() {
            method = "GET".into();
        }
    } else {
        if !url.starts_with("http") {
            return Err("URL 必须是 http(s) 链接".into());
        }
        if jp_used.is_empty() && jp_total.is_empty() {
            return Err("至少填写 used 或 total 的取值路径".into());
        }
    }
    if method != "GET" && method != "POST" {
        return Err("method 只支持 GET / POST".into());
    }
    if !headers.is_object() {
        return Err("headers 必须是对象".into());
    }
    let interval_min = p
        .get("interval_min")
        .and_then(|v| v.as_u64())
        .unwrap_or(60)
        .max(1);
    Ok(json!({
        "account_id": account_id,
        "provider": provider,
        "api_key": api_key,
        "oauth_tokens": p.get("oauth_tokens").cloned().unwrap_or(json!({})),
        "url": url,
        "method": method,
        "headers": headers,
        "body": body,
        "jsonpath_used": jp_used,
        "jsonpath_total": jp_total,
        "unit": p.get("unit").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "interval_min": interval_min,
        "enabled": p.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
    }))
}

#[tauri::command]
pub fn upsert_usage_config(id: Option<String>, payload: Value) -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    let mut data = store::load_data_raw();
    let mut cfg = match validate_usage_config(&payload, &data) {
        Ok(c) => c,
        Err(e) => return err(e),
    };
    let configs = data["usage_configs"].as_array_mut().unwrap();
    match id {
        Some(cfg_id) => {
            let idx = configs.iter().position(|c| c["id"] == json!(cfg_id));
            match idx {
                Some(i) => {
                    let last_run = configs[i].get("last_run_at").cloned().unwrap_or(json!(""));
                    cfg["id"] = json!(cfg_id);
                    cfg["last_run_at"] = last_run;
                    // 保留已有 oauth_tokens（前端编辑时不一定回传）
                    if cfg["oauth_tokens"].as_object().map(|o| o.is_empty()).unwrap_or(true) {
                        if let Some(old) = configs[i].get("oauth_tokens") {
                            cfg["oauth_tokens"] = old.clone();
                        }
                    }
                    configs[i] = cfg.clone();
                    let _ = store::save_data_raw(&data);
                    ok(json!({ "ok": true, "config": cfg }))
                }
                None => err(format!("用量配置 {cfg_id} 不存在")),
            }
        }
        None => {
            let new_id = store::new_id("uc", configs, "id");
            cfg["id"] = json!(new_id);
            cfg["last_run_at"] = json!("");
            configs.push(cfg.clone());
            let _ = store::save_data_raw(&data);
            ok(json!({ "ok": true, "config": cfg }))
        }
    }
}

#[tauri::command]
pub fn delete_usage_config(id: String) -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    let mut data = store::load_data_raw();
    let configs = data["usage_configs"].as_array().unwrap().clone();
    if !configs.iter().any(|c| c["id"] == json!(id)) {
        return err(format!("用量配置 {id} 不存在"));
    }
    data["usage_configs"] =
        Value::Array(configs.into_iter().filter(|c| c["id"] != json!(id)).collect());
    let _ = store::save_data_raw(&data);
    // 清理缓存
    let mut cache = crate::providers::cache::load_cache();
    cache.as_object_mut().unwrap().remove(&id);
    crate::providers::cache::save_cache(&cache);
    ok(json!({ "ok": true }))
}

// ---- 用量 ----

#[tauri::command]
pub fn get_usage() -> Value {
    let data = store::load_data_raw();
    let cache = crate::providers::cache::load_cache();
    let mut out = Vec::new();
    for c in data["usage_configs"].as_array().unwrap() {
        let mut entry = c.clone();
        entry["cache"] = cache.get(c["id"].as_str().unwrap_or("")).cloned().unwrap_or(Value::Null);
        out.push(entry);
    }
    json!({ "configs": out })
}

#[tauri::command]
pub fn get_usage_providers() -> Value {
    json!({ "providers": crate::providers::all_metadata() })
}

#[tauri::command]
pub async fn test_usage_config(payload: Value) -> Result<Value, ()> {
    Ok(crate::providers::do_fetch(&payload, false).await)
}

#[tauri::command]
pub async fn fetch_usage(id: String) -> Result<Value, ()> {
    let data = store::load_data_raw();
    let cfg = data["usage_configs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == json!(id))
        .cloned();
    let cfg = match cfg {
        Some(c) => c,
        None => return Ok(err("配置不存在")),
    };
    let res = crate::providers::do_fetch(&cfg, true).await;
    if res.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        // 写缓存 + 更新 last_run_at
        let mut cache = crate::providers::cache::load_cache();
        cache
            .as_object_mut()
            .unwrap()
            .insert(id.clone(), res["result"].clone());
        crate::providers::cache::save_cache(&cache);
        let _g = STATE_LOCK.lock().unwrap();
        let mut data = store::load_data_raw();
        for c in data["usage_configs"].as_array_mut().unwrap() {
            if c["id"] == json!(id) {
                c["last_run_at"] = json!(store::now_iso());
                break;
            }
        }
        let _ = store::save_data_raw(&data);
    }
    Ok(res)
}

// ---- 导入 / 重置 ----

#[tauri::command]
pub fn import_data(payload: Value) -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    let mut data = store::load_data_raw();
    let mut n_acc = 0;
    let mut n_rel = 0;
    let mut n_ql = 0;
    let mut n_uc = 0;

    for a in payload.get("accounts").and_then(|v| v.as_array()).unwrap_or(&Vec::new()) {
        let acc = match validate_account(a, &data) {
            Ok(x) => x,
            Err(_) => continue, // Python 版校验失败直接抛错；这里跳过坏条目保证导入不中断
        };
        let accounts = data["accounts"].as_array_mut().unwrap();
        let existing: Vec<String> = accounts
            .iter()
            .filter_map(|x| x["id"].as_str().map(String::from))
            .collect();
        let new_id = match a.get("id").and_then(|v| v.as_str()) {
            Some(orig) if !existing.contains(&orig.to_string()) => orig.to_string(),
            _ => store::new_id("acc", accounts, "id"),
        };
        let mut created = acc;
        created["id"] = json!(new_id);
        created["created_at"] = a
            .get("created_at")
            .cloned()
            .unwrap_or_else(|| json!(store::now_iso()));
        created["updated_at"] = json!(store::now_iso());
        accounts.push(created);
        n_acc += 1;
    }
    for r in payload.get("relations").and_then(|v| v.as_array()).unwrap_or(&Vec::new()) {
        let rel = match validate_relation(r, &data) {
            Ok(x) => x,
            Err(_) => continue,
        };
        let relations = data["relations"].as_array_mut().unwrap();
        let new_id = store::new_id("rel", relations, "id");
        let mut created = rel;
        created["id"] = json!(new_id);
        relations.push(created);
        n_rel += 1;
    }
    for q in payload.get("query_links").and_then(|v| v.as_array()).unwrap_or(&Vec::new()) {
        let ql = match validate_query_link(q) {
            Ok(x) => x,
            Err(_) => continue,
        };
        let links = data["query_links"].as_array_mut().unwrap();
        let new_id = q
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| store::new_id("q", links, "id"));
        let mut created = ql;
        created["id"] = json!(new_id);
        links.push(created);
        n_ql += 1;
    }
    for uc in payload.get("usage_configs").and_then(|v| v.as_array()).unwrap_or(&Vec::new()) {
        let cfg = match validate_usage_config(uc, &data) {
            Ok(x) => x,
            Err(_) => continue,
        };
        let configs = data["usage_configs"].as_array_mut().unwrap();
        let new_id = uc
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| store::new_id("uc", configs, "id"));
        let mut created = cfg;
        created["id"] = json!(new_id);
        created["last_run_at"] = uc
            .get("last_run_at")
            .cloned()
            .unwrap_or_else(|| json!(""));
        configs.push(created);
        n_uc += 1;
    }
    let _ = store::save_data_raw(&data);
    ok(json!({ "ok": true, "imported": { "accounts": n_acc, "relations": n_rel, "query_links": n_ql, "usage_configs": n_uc } }))
}

#[tauri::command]
pub fn reset_data(payload: Value) -> Value {
    if payload.get("confirm").and_then(|v| v.as_str()) != Some("RESET") {
        return err("需在请求体带 confirm=RESET 才可重置");
    }
    let _g = STATE_LOCK.lock().unwrap();
    let df = store::data_file();
    if df.exists() {
        let _ = store::rotate_backup(&df, &store::backup_dir(), store::DATA_BACKUP_KEEP);
    }
    let empty: Value = serde_json::from_str(&{
        let mut v = json!({"version":1,"accounts":[],"relations":[],"query_links":[],"usage_configs":[]});
        v["settings"] = json!({"vault_path": store::default_vault().to_string_lossy(), "name": "账号管家"});
        serde_json::to_string(&v).unwrap()
    })
    .unwrap();
    let _ = store::save_data_raw(&empty);
    ok(json!({ "ok": true }))
}

// ---- OAuth ----

#[tauri::command]
pub async fn grok_device_code_start() -> Result<Value, ()> {
    Ok(crate::providers::grok::device_code_start().await)
}

#[tauri::command]
pub async fn grok_device_code_poll(payload: Value) -> Result<Value, ()> {
    Ok(crate::providers::grok::device_code_poll(payload).await)
}

#[tauri::command]
pub fn oauth_import_from_cli(provider: String) -> Value {
    crate::providers::oauth::import_from_cli(&provider)
}

// ---- Vault ----

#[tauri::command]
pub fn vault_info() -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    let data = store::load_data_raw();
    vault::info(&data)
}

#[tauri::command]
pub fn vault_download() -> Result<(Vec<u8>, String), String> {
    let _g = STATE_LOCK.lock().unwrap();
    let data = store::load_data_raw();
    vault::read_vault(&data)
}

#[tauri::command]
pub fn vault_backups() -> Value {
    let _g = STATE_LOCK.lock().unwrap();
    let data = store::load_data_raw();
    vault::backups(&data)
}

#[tauri::command]
pub fn vault_restore(name: String) -> Result<(Vec<u8>, String), String> {
    let _g = STATE_LOCK.lock().unwrap();
    let data = store::load_data_raw();
    vault::read_backup(&data, &name)
}

#[tauri::command]
pub fn vault_upload(content_b64: String) -> Value {
    use base64::Engine;
    let raw = match base64::engine::general_purpose::STANDARD.decode(&content_b64) {
        Ok(b) => b,
        Err(_) => return err("base64 解码失败"),
    };
    let _g = STATE_LOCK.lock().unwrap();
    let data = store::load_data_raw();
    match vault::upload_vault(&data, &raw) {
        Ok(v) => v,
        Err(e) => err(e),
    }
}

// 全局锁（static，commands 之间共享）
use std::sync::Mutex as StdMutex;
static STATE_LOCK: StdMutex<()> = StdMutex::new(());
