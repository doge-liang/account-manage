//! Grok Build provider + 设备码 OAuth（RFC 8628）。
//! 移植 _grok_refresh_and_fetch / _grok_device_code_start / _grok_device_code_poll。
//!
//! Cloudflare 注意（memory）：Python urllib 被 CF 拦、curl 能过。
//! Rust reqwest 用 rustls——TLS 指纹与浏览器/curl 不同，若被拦降级为调系统 curl.exe。

use crate::providers::oauth;
use serde_json::{json, Value};

const GROK_OAUTH_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const GROK_OAUTH_SCOPE: &str = "openid profile email offline_access grok-cli:access api:access conversations:read conversations:write";
const GROK_DEVICE_CODE_URL: &str = "https://auth.x.ai/oauth2/device/code";
const GROK_TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";

pub async fn refresh_and_fetch(cfg: &Value) -> Value {
    let path = oauth::grok_auth_path();
    let tok = match oauth::get_tokens(cfg, &path, oauth::grok_extractor) {
        Some(t) => t,
        None => {
            return json!({ "ok": false, "error": "未找到 OAuth token（请先从 CLI 导入或用 Grok CLI 登录）", "status": 0, "raw": "" })
        }
    };
    let mut access_token = tok.access_token.clone();
    let refresh_token = tok.refresh_token.clone();
    let oidc_client_id = tok
        .extras
        .get("oidc_client_id")
        .and_then(|v| v.as_str())
        .unwrap_or(GROK_OAUTH_CLIENT_ID)
        .to_string();
    let user_id = tok.extras.get("user_id").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // JWT exp 过期判断
    let token_expired = jwt_expired(&access_token);

    if token_expired && !refresh_token.is_empty() {
        match refresh_grok(&refresh_token, &oidc_client_id, cfg, &user_id).await {
            Ok(t) => access_token = t,
            Err(e) => return json!({ "ok": false, "error": format!("token 刷新失败: {e}"), "status": 0, "raw": "" }),
        }
    }

    // billing 查询：优先 reqwest，CF 拦截则降级 curl.exe
    let (status, text, err) = billing_request(&access_token, &user_id).await;
    let (mut status, mut text, mut err) = (status, text, err);
    if (status == 401 || status == 403) && !refresh_token.is_empty() {
        match refresh_grok(&refresh_token, &oidc_client_id, cfg, &user_id).await {
            Ok(t) => {
                access_token = t;
                let r = billing_request(&access_token, &user_id).await;
                status = r.0;
                text = r.1;
                err = r.2;
            }
            Err(e) => return json!({ "ok": false, "error": format!("token 刷新失败: {e}"), "status": status, "raw": text.chars().take(500).collect::<String>() }),
        }
    }
    if !err.is_empty() {
        return json!({ "ok": false, "error": err, "status": status, "raw": text.chars().take(500).collect::<String>() });
    }

    let billing: Value = match serde_json::from_str(&text) {
        Ok(o) => o,
        Err(_) => return json!({ "ok": false, "error": "billing 响应不是合法 JSON", "status": status, "raw": text.chars().take(500).collect::<String>() }),
    };

    let config = billing.get("config").cloned().unwrap_or(json!({}));
    let credit_pct = config.get("creditUsagePercent").and_then(|v| v.as_f64());
    // creditUsagePercent 缺失 = 本周期没用量 = 0%
    let percent_used = credit_pct.map(|p| (p * 10.0).round() / 10.0).unwrap_or(0.0);

    let mut result = json!({
        "used": null, "total": null,
        "percent_used": percent_used, "percent_semantics": "used",
        "unit": cfg.get("unit").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("%"),
        "fetched_at": crate::store::now_iso(),
        "raw_used": credit_pct,
        "raw_total": null,
    });
    if let Some(pb) = config.get("prepaidBalance").and_then(|v| v.as_object()) {
        if let Some(val) = pb.get("val") {
            if !val.is_null() {
                result["prepaid_balance"] = json!(val.to_string());
            }
        }
    }
    if let Some(bpe) = config.get("billingPeriodEnd") {
        if !bpe.is_null() {
            result["billing_period_end"] = bpe.clone();
        }
    }
    if let Some(pu) = config.get("productUsage").and_then(|v| v.as_array()) {
        let products: Vec<String> = pu
            .iter()
            .map(|p| {
                format!(
                    "{}: {}%",
                    p.get("product").and_then(|v| v.as_str()).unwrap_or(""),
                    p.get("usagePercent").map(|v| v.to_string()).unwrap_or_default()
                )
            })
            .collect();
        if !products.is_empty() {
            result["products"] = json!(products.join("; "));
        }
    }
    json!({ "ok": true, "status": status, "result": result })
}

/// billing 请求：reqwest → CF 拦截(403/挑战页) → 降级 curl.exe
async fn billing_request(access_token: &str, user_id: &str) -> (u16, String, String) {
    let url = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("x-userid", user_id)
        .header("x-grok-client-mode", "xai-grok-cli")
        .header("Accept", "application/json")
        .send()
        .await;
    match resp {
        Ok(r) => {
            let status = r.status().as_u16();
            let text = r.text().await.unwrap_or_default();
            // CF 挑战页特征：403 + HTML（非 JSON）→ 降级 curl
            if status == 403 && !text.trim_start().starts_with('{') {
                return billing_request_curl(access_token, user_id).await;
            }
            if status >= 400 {
                (status, text, format!("HTTP {status}"))
            } else {
                (status, text, String::new())
            }
        }
        Err(_) => {
            // reqwest 失败也降级
            billing_request_curl(access_token, user_id).await
        }
    }
}

/// 降级：系统 curl.exe（Win10+ 自带）。curl 的 TLS 指纹可通过 CF。
async fn billing_request_curl(access_token: &str, user_id: &str) -> (u16, String, String) {
    let url = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
    let output = tokio::process::Command::new("curl")
        .args([
            "-s", "-w", "\n%{http_code}", url,
            "-H", &format!("Authorization: Bearer {access_token}"),
            "-H", &format!("x-userid: {user_id}"),
            "-H", "x-grok-client-mode: xai-grok-cli",
            "-H", "Accept: application/json",
            "--max-time", "15",
        ])
        .output()
        .await;
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let parts: Vec<&str> = stdout.rsplitn(2, '\n').collect();
            if parts.len() == 2 {
                let code = parts[0].trim().parse::<u16>().unwrap_or(0);
                let body = parts[1].to_string();
                if code >= 400 {
                    (code, body, format!("HTTP {code}"))
                } else {
                    (code, body, String::new())
                }
            } else {
                (0, stdout, String::new())
            }
        }
        Err(e) => (0, String::new(), format!("curl 降级也失败: {e}")),
    }
}

fn jwt_expired(access_token: &str) -> bool {
    use base64::Engine;
    let parts: Vec<&str> = access_token.split('.').collect();
    if parts.len() < 2 {
        return true; // 无法解析 → 尝试刷新
    }
    let payload_b64 = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE
        .decode(format!("{payload_b64}===").trim_end_matches('='))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload_b64));
    match decoded {
        Ok(bytes) => {
            let payload: Value = match serde_json::from_slice(&bytes) {
                Ok(p) => p,
                Err(_) => return true,
            };
            let exp = payload.get("exp").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            now > exp
        }
        Err(_) => true,
    }
}

/// 刷新 token 并回写（config 模式 → cfg.oauth_tokens；file 模式 → auth.json）
async fn refresh_grok(
    refresh_token: &str,
    client_id: &str,
    cfg: &Value,
    user_id: &str,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(GROK_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(format!(
            "grant_type=refresh_token&refresh_token={refresh_token}&client_id={client_id}"
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let refreshed: Value = resp.json().await.map_err(|e| e.to_string())?;
    let access = refreshed
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("响应缺少 access_token")?
        .to_string();
    let new_refresh = refreshed
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or(refresh_token)
        .to_string();
    let expires_in = refreshed.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(21600);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let expires_at = (now + expires_in as u64).to_string();

    // 回写
    if let Some(cfg_id) = cfg.get("id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        oauth::save_tokens_to_cfg(
            cfg_id,
            json!({
                "access_token": access,
                "refresh_token": new_refresh,
                "expires_at": expires_at,
                "oidc_client_id": client_id,
                "user_id": user_id,
            }),
        );
    } else {
        // file 模式：回写 ~/.grok/auth.json
        let path = oauth::grok_auth_path();
        let file_data: Option<Value> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());
        if let Some(mut file_data) = file_data {
            if let Some(obj) = file_data.as_object_mut() {
                let first_key: Option<String> = obj.keys().next().cloned();
                if let Some(k) = first_key {
                    obj[&k]["key"] = json!(access);
                    obj[&k]["refresh_token"] = json!(new_refresh);
                    obj[&k]["expires_at"] = json!(expires_at);
                    let tmp = path.with_extension("json.tmp");
                    if std::fs::write(&tmp, serde_json::to_string_pretty(&file_data).unwrap_or_default()).is_ok() {
                        let _ = std::fs::rename(&tmp, &path);
                    }
                }
            }
        }
    }
    Ok(access)
}

// ---- 设备码流程 ----

pub async fn device_code_start() -> Value {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();
    let resp = match client
        .post(GROK_DEVICE_CODE_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(format!("client_id={}&scope={}", GROK_OAUTH_CLIENT_ID, GROK_OAUTH_SCOPE.replace(' ', "+")))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }),
    };
    if !resp.status().is_success() {
        return json!({ "ok": false, "error": format!("HTTP {}", resp.status()) });
    }
    let obj: Value = match resp.json().await {
        Ok(o) => o,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }),
    };
    json!({
        "ok": true,
        "data": {
            "device_code": obj.get("device_code").and_then(|v| v.as_str()).unwrap_or(""),
            "user_code": obj.get("user_code").and_then(|v| v.as_str()).unwrap_or(""),
            "verification_uri": obj.get("verification_uri").and_then(|v| v.as_str()).unwrap_or(""),
            "verification_uri_complete": obj.get("verification_uri_complete").and_then(|v| v.as_str()).unwrap_or(""),
            "expires_in": obj.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(1800),
            "interval": obj.get("interval").and_then(|v| v.as_i64()).unwrap_or(5),
        }
    })
}

pub async fn device_code_poll(payload: Value) -> Value {
    let device_code = payload
        .get("device_code")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if device_code.is_empty() {
        return json!({ "ok": false, "error": "缺少 device_code" });
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();
    let resp = client
        .post(GROK_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code&client_id={}&device_code={}",
            GROK_OAUTH_CLIENT_ID, device_code
        ))
        .send()
        .await;
    let (status_ok, body) = match resp {
        Ok(r) => {
            let ok = r.status().is_success();
            (ok, r.text().await.unwrap_or_default())
        }
        Err(e) => return json!({ "ok": false, "status": "error", "error": e.to_string() }),
    };
    let obj: Value = match serde_json::from_str(&body) {
        Ok(o) => o,
        Err(_) => return json!({ "ok": false, "status": "error", "error": "响应不是合法 JSON" }),
    };
    if status_ok {
        if let Some(access) = obj.get("access_token").and_then(|v| v.as_str()) {
            return json!({
                "ok": true, "status": "success",
                "tokens": {
                    "access_token": access,
                    "refresh_token": obj.get("refresh_token").and_then(|v| v.as_str()).unwrap_or(""),
                    "expires_in": obj.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600),
                    "id_token": obj.get("id_token").and_then(|v| v.as_str()).unwrap_or(""),
                }
            });
        }
    }
    // 错误分支
    let err_code = obj.get("error").and_then(|v| v.as_str()).unwrap_or("");
    if err_code == "authorization_pending" || err_code == "slow_down" {
        return json!({ "ok": true, "status": "pending", "error": err_code });
    }
    json!({
        "ok": false, "status": "error",
        "error": obj.get("error_description").and_then(|v| v.as_str()).unwrap_or(err_code),
    })
}
