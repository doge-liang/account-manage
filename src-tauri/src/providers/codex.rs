//! ChatGPT Codex provider — 移植 _codex_refresh_and_fetch。

use crate::jsonpath::{eval, parse_numeric};
use crate::providers::oauth;
use serde_json::{json, Value};

const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

pub async fn refresh_and_fetch(cfg: &Value) -> Value {
    // token 来源：cfg.oauth_tokens 优先（多账号），fallback ~/.codex/auth.json
    let path = oauth::codex_auth_path();
    let tok = match oauth::get_tokens(cfg, &path, oauth::codex_extractor) {
        Some(t) => t,
        None => {
            return json!({ "ok": false, "error": format!("未找到 {}（请先用 Codex CLI 登录）", path.display()), "status": 0, "raw": "" })
        }
    };
    let account_id = tok.extras.get("account_id").and_then(|v| v.as_str()).unwrap_or("");
    let mut access_token = tok.access_token.clone();
    let refresh_token = tok.refresh_token.clone();

    let url = cfg
        .get("url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("https://chatgpt.com/backend-api/codex/usage");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    let mut do_request = |tok: &str| -> reqwest::Result<reqwest::Response> {
        // client.request 需要 &self，闭包捕获可变引用不优雅——直接内联循环替代
        let _ = tok;
        unreachable!()
    };
    let _ = &mut do_request;

    // 第一次请求
    let mut resp = client
        .get(url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("ChatGPT-Account-Id", account_id)
        .header("User-Agent", "codex-cli/1.0")
        .header("Accept", "application/json")
        .header("originator", "codex_cli_rs")
        .send()
        .await;

    // 401/403 → 刷新重试
    let need_refresh = matches!(&resp, Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403)
        || matches!(&resp, Err(_));
    if need_refresh && !refresh_token.is_empty() {
        match refresh_codex(&refresh_token).await {
            Ok(new_access) => {
                access_token = new_access;
                resp = client
                    .get(url)
                    .header("Authorization", format!("Bearer {access_token}"))
                    .header("ChatGPT-Account-Id", account_id)
                    .header("User-Agent", "codex-cli/1.0")
                    .header("Accept", "application/json")
                    .header("originator", "codex_cli_rs")
                    .send()
                    .await;
            }
            Err(e) => {
                return json!({ "ok": false, "error": format!("token 刷新失败: {e}"), "status": 401, "raw": "" })
            }
        }
    }

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return json!({ "ok": false, "error": e.to_string(), "status": 0, "raw": "" }),
    };
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if status >= 400 {
        return json!({ "ok": false, "error": format!("HTTP {status}"), "status": status, "raw": text.chars().take(500).collect::<String>() });
    }
    let obj: Value = match serde_json::from_str(&text) {
        Ok(o) => o,
        Err(_) => return json!({ "ok": false, "error": "响应不是合法 JSON", "status": status, "raw": text.chars().take(500).collect::<String>() }),
    };

    let jp_used = cfg
        .get("jsonpath_used")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("$.rate_limit.primary_window.used_percent");
    let jp_total = cfg.get("jsonpath_total").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("");
    let used_raw = eval(jp_used, &obj);
    let total_raw = if jp_total.is_empty() { None } else { eval(jp_total, &obj) };
    let used_val = used_raw.as_ref().and_then(parse_numeric);
    let total_val = total_raw.as_ref().and_then(parse_numeric);

    let percent_used = if let (Some(u), Some(t)) = (used_val, total_val) {
        if t > 0.0 { Some((u / t * 100.0 * 10.0).round() / 10.0) } else { None }
    } else if let Some(u) = used_val {
        Some((u * 10.0).round() / 10.0)
    } else {
        None
    };

    let mut result = json!({
        "used": if total_val.is_some() { used_val } else { None },
        "total": total_val,
        "percent_used": percent_used,
        "percent_semantics": "used",
        "unit": cfg.get("unit").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("%"),
        "fetched_at": crate::store::now_iso(),
        "raw_used": used_raw,
        "raw_total": total_raw,
    });
    if let Some(credits) = obj.get("credits").and_then(|v| v.as_object()) {
        if let Some(bal) = credits.get("balance") {
            if !bal.is_null() {
                result["credits_balance"] = json!(bal.to_string());
            }
        }
    }
    if let Some(pt) = obj.get("plan_type") {
        if !pt.is_null() {
            result["plan_type"] = pt.clone();
        }
    }
    if let Some(pw) = obj.pointer("/rate_limit/primary_window") {
        if let Some(r) = pw.get("reset_at") {
            if !r.is_null() {
                result["reset_at"] = json!(r.to_string());
            }
        }
    }
    json!({ "ok": true, "status": status, "result": result })
}

async fn refresh_codex(refresh_token: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post("https://auth.openai.com/oauth/token")
        .header("Content-Type", "application/json")
        .json(&json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": CODEX_CLIENT_ID,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let refreshed: Value = resp.json().await.map_err(|e| e.to_string())?;
    refreshed
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| "响应缺少 access_token".to_string())
}
