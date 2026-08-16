//! Claude Code provider — 移植 _claude_refresh_and_fetch。
//! api.anthropic.com/api/oauth/usage 返回 {five_hour: {utilization}, seven_day: {utilization}}。

use crate::providers::oauth;
use serde_json::{json, Value};

pub async fn refresh_and_fetch(cfg: &Value) -> Value {
    let path = oauth::claude_auth_path();
    let tok = match oauth::get_tokens(cfg, &path, oauth::claude_extractor) {
        Some(t) => t,
        None => {
            return json!({ "ok": false, "error": format!("未找到 {}（请先用 Claude Code 登录）", path.display()), "status": 0, "raw": "" })
        }
    };
    let mut access_token = tok.access_token.clone();
    let refresh_token = tok.refresh_token.clone();
    let subscription_type = tok
        .extras
        .get("subscription_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // 过期判断（expiresAt 是毫秒时间戳）
    let expires_at = tok.extras.get("expires_at").and_then(|v| v.as_f64());
    let token_expired = expires_at
        .map(|e| {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0);
            now_ms > e
        })
        .unwrap_or(false);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    let fetch_usage = |tok: String| {
        let client = &client;
        async move {
            client
                .get("https://api.anthropic.com/api/oauth/usage")
                .header("Authorization", format!("Bearer {tok}"))
                .header("Content-Type", "application/json")
                .send()
                .await
        }
    };

    if token_expired && !refresh_token.is_empty() {
        match refresh_via_cli(&path).await {
            Ok(t) => access_token = t,
            Err(e) => return json!({ "ok": false, "error": format!("token 刷新失败: {e}"), "status": 0, "raw": "" }),
        }
    }

    let mut resp = fetch_usage(access_token.clone()).await;
    if matches!(&resp, Ok(r) if r.status().as_u16() == 401) && !refresh_token.is_empty() {
        // 401 → 先试 CLI 刷新（claude.ai/oauth/token 被 Cloudflare 拦截，直接 HTTP 刷新不可行）
        match refresh_via_cli(&path).await {
            Ok(t) => {
                access_token = t;
                resp = fetch_usage(access_token.clone()).await;
            }
            Err(e) => return json!({ "ok": false, "error": format!("token 刷新失败: {e}"), "status": 401, "raw": "" }),
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
    let usage: Value = match serde_json::from_str(&text) {
        Ok(o) => o,
        Err(_) => return json!({ "ok": false, "error": "usage 响应不是合法 JSON", "status": status, "raw": text.chars().take(500).collect::<String>() }),
    };

    let weekly_util = usage.pointer("/seven_day/utilization").and_then(|v| v.as_f64());
    let session_util = usage.pointer("/five_hour/utilization").and_then(|v| v.as_f64());

    let (percent_used, window, reset_at) = if let Some(w) = weekly_util {
        (
            Some((w * 10.0).round() / 10.0),
            Some("weekly"),
            usage.pointer("/seven_day/resets_at").and_then(|v| v.as_str()).map(String::from),
        )
    } else if let Some(s) = session_util {
        (
            Some((s * 10.0).round() / 10.0),
            Some("session"),
            usage.pointer("/five_hour/resets_at").and_then(|v| v.as_str()).map(String::from),
        )
    } else {
        (None, None, None)
    };

    let mut result = json!({
        "used": null, "total": null,
        "percent_used": percent_used, "percent_semantics": "used",
        "unit": cfg.get("unit").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("%"),
        "fetched_at": crate::store::now_iso(),
        "raw_used": weekly_util.or(session_util),
        "raw_total": null,
        "subscription_type": subscription_type,
    });
    if let Some(w) = window {
        result["window"] = json!(w);
    }
    if let Some(r) = reset_at {
        result["reset_at"] = json!(r);
    }
    if let Some(s) = session_util {
        result["session_percent"] = json!((s * 10.0).round() / 10.0);
    }
    json!({ "ok": true, "status": status, "result": result })
}

/// 通过系统 claude CLI 刷新 token：
/// claude.ai/oauth/token 端点有 Cloudflare 浏览器指纹校验，HTTP 客户端无法直接刷新。
/// CLI 自带完整指纹能通过 —— 跑一次最小请求（-p hi）让它刷新并回写 credentials.json，
/// 然后重读文件拿新 token。已装 Claude Code 的机器上这是最可靠的路径。
async fn refresh_via_cli(cred_path: &std::path::Path) -> Result<String, String> {
    let out = tokio::process::Command::new("claude")
        .args(["-p", "hi", "--max-turns", "1"])
        .output()
        .await
        .map_err(|e| format!("无法启动 claude CLI（{e}）。请安装 Claude Code 或手动运行一次 `claude` 登录"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("claude CLI 刷新失败: {}", stderr.chars().take(200).collect::<String>()));
    }
    // CLI 回写 credentials.json 后重读
    let creds: Value = std::fs::read_to_string(cred_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or("重读 credentials.json 失败")?;
    creds
        .pointer("/claudeAiOauth/accessToken")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| "credentials.json 中未找到新 accessToken".to_string())
}
