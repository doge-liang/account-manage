//! OAuth 基础设施 — 对齐 _oauth_get_tokens / _oauth_save_tokens_to_cfg / import_from_cli。

use serde_json::{json, Value};
use std::path::PathBuf;

/// 读取 token：优先 cfg["oauth_tokens"]，fallback 本地 CLI 文件。
/// 返回 (access, refresh, extras, source)。
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub extras: Value,       // auth_data 中除 access/refresh 外的字段
    pub source: &'static str, // "config" | "file"
}

pub fn get_tokens(cfg: &Value, local_path: &PathBuf, extractor: fn(&Value) -> Value) -> Option<Tokens> {
    let ot = cfg.get("oauth_tokens").unwrap_or(&Value::Null);
    if let Some(o) = ot.as_object() {
        let access = o.get("access_token").and_then(|v| v.as_str()).unwrap_or("");
        let refresh = o.get("refresh_token").and_then(|v| v.as_str()).unwrap_or("");
        if !access.is_empty() || !refresh.is_empty() {
            let mut extras = json!({});
            for (k, v) in o {
                if k != "access_token" && k != "refresh_token" {
                    extras[k] = v.clone();
                }
            }
            return Some(Tokens {
                access_token: access.to_string(),
                refresh_token: refresh.to_string(),
                extras,
                source: "config",
            });
        }
    }
    // fallback 文件
    if !local_path.exists() {
        return None;
    }
    let file_data: Value = std::fs::read_to_string(local_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())?;
    let extracted = extractor(&file_data);
    let access = extracted.get("access_token").and_then(|v| v.as_str()).unwrap_or("");
    let refresh = extracted.get("refresh_token").and_then(|v| v.as_str()).unwrap_or("");
    if access.is_empty() && refresh.is_empty() {
        return None;
    }
    let mut extras = json!({});
    if let Some(o) = extracted.as_object() {
        for (k, v) in o {
            if k != "access_token" && k != "refresh_token" {
                extras[k] = v.clone();
            }
        }
    }
    Some(Tokens {
        access_token: access.to_string(),
        refresh_token: refresh.to_string(),
        extras,
        source: "file",
    })
}

/// 刷新后回写 usage_config 的 oauth_tokens（对齐 _oauth_save_tokens_to_cfg）
pub fn save_tokens_to_cfg(cfg_id: &str, tokens: Value) {
    use std::sync::MutexGuard;
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g: MutexGuard<()> = LOCK.lock().unwrap();
    let mut data = crate::store::load_data_raw();
    if let Some(configs) = data.get_mut("usage_configs").and_then(|v| v.as_array_mut()) {
        if let Some(c) = configs.iter_mut().find(|c| c["id"] == json!(cfg_id)) {
            c["oauth_tokens"] = tokens;
            let _ = crate::store::save_data_raw(&data);
        }
    }
}

/// Codex extractor: auth.json → {tokens: {access_token, refresh_token, account_id}}
pub fn codex_extractor(file_data: &Value) -> Value {
    let t = file_data.get("tokens").cloned().unwrap_or(json!({}));
    json!({
        "access_token": t.get("access_token").and_then(|v| v.as_str()).unwrap_or(""),
        "refresh_token": t.get("refresh_token").and_then(|v| v.as_str()).unwrap_or(""),
        "account_id": t.get("account_id").and_then(|v| v.as_str()).unwrap_or(""),
    })
}

/// Claude extractor: .credentials.json → {claudeAiOauth: {accessToken, refreshToken, ...}}
pub fn claude_extractor(file_data: &Value) -> Value {
    let a = file_data.get("claudeAiOauth").cloned().unwrap_or(json!({}));
    json!({
        "access_token": a.get("accessToken").and_then(|v| v.as_str()).unwrap_or(""),
        "refresh_token": a.get("refreshToken").and_then(|v| v.as_str()).unwrap_or(""),
        "expires_at": a.get("expiresAt"),
        "subscription_type": a.get("subscriptionType"),
    })
}

/// Grok extractor: auth.json → {issuer_key: {key, refresh_token, oidc_client_id, user_id}}
pub fn grok_extractor(file_data: &Value) -> Value {
    let issuer = file_data.as_object().and_then(|o| o.values().next());
    let ad = issuer.cloned().unwrap_or(json!({}));
    json!({
        "access_token": ad.get("key").and_then(|v| v.as_str()).unwrap_or(""),
        "refresh_token": ad.get("refresh_token").and_then(|v| v.as_str()).unwrap_or(""),
        "oidc_client_id": ad.get("oidc_client_id").and_then(|v| v.as_str()).unwrap_or(""),
        "user_id": ad.get("user_id").and_then(|v| v.as_str()).unwrap_or(""),
    })
}

pub fn codex_auth_path() -> PathBuf {
    home().join(".codex").join("auth.json")
}
pub fn claude_auth_path() -> PathBuf {
    home().join(".claude").join(".credentials.json")
}
pub fn grok_auth_path() -> PathBuf {
    home().join(".grok").join("auth.json")
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// POST /api/usage-configs/import-oauth — 从本地 CLI 文件导入 token 到前端（暂存，保存时写入 cfg）
pub fn import_from_cli(provider: &str) -> Value {
    let (path, extractor): (PathBuf, fn(&Value) -> Value) = match provider {
        "chatgpt_codex" => (codex_auth_path(), codex_extractor),
        "claude_code" => (claude_auth_path(), claude_extractor),
        "grok_build" => (grok_auth_path(), grok_extractor),
        _ => return json!({ "ok": false, "error": format!("未知 provider: {provider}") }),
    };
    if !path.exists() {
        return json!({ "ok": false, "error": format!("未找到 {}（请先登录对应 CLI）", path.display()) });
    }
    match std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
    {
        Some(file_data) => {
            let extracted = extractor(&file_data);
            let access = extracted.get("access_token").and_then(|v| v.as_str()).unwrap_or("");
            let refresh = extracted.get("refresh_token").and_then(|v| v.as_str()).unwrap_or("");
            if access.is_empty() && refresh.is_empty() {
                return json!({ "ok": false, "error": "文件中未找到 token" });
            }
            json!({ "ok": true, "oauth_tokens": extracted, "source": path.to_string_lossy() })
        }
        None => json!({ "ok": false, "error": "读取/解析 CLI 认证文件失败" }),
    }
}
