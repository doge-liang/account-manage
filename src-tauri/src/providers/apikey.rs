//! API Key 类 Provider：GLM / Kimi / MiniMax / DeepSeek / Gemini / DashScope
//! 移植自 server.py L1556-1950。

use crate::jsonpath::parse_numeric;
use serde_json::{json, Value};

fn now_iso() -> String {
    crate::store::now_iso()
}

/// 统一 GET（对齐 _apikey_http_get）。auth: "bearer" | "raw" | (header_name, value)
async fn apikey_get(url: &str, api_key: &str, auth_mode: &str) -> (u16, String, String) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();
    let mut req = client.get(url).header("Accept", "application/json");
    req = match auth_mode {
        "raw" => req.header("Authorization", api_key),
        _ => req.header("Authorization", format!("Bearer {api_key}")),
    };
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            if status >= 400 {
                (status, text, format!("HTTP {status}"))
            } else {
                (status, text, String::new())
            }
        }
        Err(e) => (0, String::new(), e.to_string()),
    }
}

fn no_key() -> Value {
    json!({ "ok": false, "error": "缺少 API Key", "status": 0, "raw": "" })
}

fn bad_json(status: u16, text: &str) -> Value {
    json!({ "ok": false, "error": "响应不是合法 JSON", "status": status, "raw": text.chars().take(500).collect::<String>() })
}

fn http_err(err: &str, status: u16, text: &str) -> Value {
    json!({ "ok": false, "error": err, "status": status, "raw": text.chars().take(500).collect::<String>() })
}

/// GLM Coding Plan — 周配额（unit=6）优先，5h 窗口（unit=3）次之
pub async fn glm_fetch(cfg: &Value) -> Value {
    let api_key = cfg.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
    if api_key.is_empty() {
        return no_key();
    }
    let (status, text, err) =
        apikey_get("https://open.bigmodel.cn/api/monitor/usage/quota/limit", api_key, "raw").await;
    if !err.is_empty() {
        return http_err(&err, status, &text);
    }
    let obj: Value = match serde_json::from_str(&text) {
        Ok(o) => o,
        Err(_) => return bad_json(status, &text),
    };
    // HTTP 200 但 body 报错
    let success = obj.get("success").and_then(|v| v.as_bool()).unwrap_or(true);
    let code = obj.get("code").and_then(|v| v.as_i64());
    if !success || code.map(|c| c != 200).unwrap_or(false) {
        let msg = obj.get("msg").and_then(|v| v.as_str()).unwrap_or("API 返回错误");
        return http_err(msg, status, &text);
    }
    let limits = obj
        .pointer("/data/limits")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let level = obj.pointer("/data/level").cloned();
    let mut weekly_pct: Option<f64> = None;
    let mut session_pct: Option<f64> = None;
    let mut reset_at: Option<String> = None;
    for lim in &limits {
        let unit = lim.get("unit").and_then(|v| v.as_i64()).unwrap_or(0);
        let pct = lim.get("percentage").and_then(|v| parse_numeric(v));
        let rt = ["nextResetTime", "resetTime", "reset_time"]
            .iter()
            .find_map(|k| lim.get(*k).and_then(|v| v.as_str()));
        if unit == 6 {
            if let Some(p) = pct {
                weekly_pct = Some(p);
                reset_at = rt.map(String::from);
            }
        } else if unit == 3 {
            if let Some(p) = pct {
                session_pct = Some(p);
                if reset_at.is_none() {
                    reset_at = rt.map(String::from);
                }
            }
        }
    }
    let percent_used = weekly_pct.or(session_pct);
    let mut result = json!({
        "used": null, "total": null,
        "percent_used": percent_used, "percent_semantics": "used",
        "unit": cfg.get("unit").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("%"),
        "fetched_at": now_iso(),
        "raw_used": null, "raw_total": null,
    });
    if let Some(l) = level {
        result["level"] = json!(l.as_str().unwrap_or("").to_string());
    }
    if let Some(sp) = session_pct {
        result["session_percent"] = json!(sp);
    }
    if let Some(r) = reset_at.as_ref().and_then(|s| crate::providers::normalize_reset_at(&json!(s))) {
        result["reset_at"] = json!(r);
    }
    json!({ "ok": true, "status": status, "result": result })
}

/// Kimi for Coding — limits[].detail（5h 窗口） + usage（周配额）
pub async fn kimi_fetch(cfg: &Value) -> Value {
    let api_key = cfg.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
    if api_key.is_empty() {
        return no_key();
    }
    let (status, text, err) = apikey_get("https://api.kimi.com/coding/v1/usages", api_key, "bearer").await;
    if !err.is_empty() {
        return http_err(&err, status, &text);
    }
    let obj: Value = match serde_json::from_str(&text) {
        Ok(o) => o,
        Err(_) => return bad_json(status, &text),
    };
    let mut session_pct: Option<f64> = None;
    let mut reset_at: Option<String> = None;
    // 5h 窗口：limits[].detail
    if let Some(limits) = obj.get("limits").and_then(|v| v.as_array()) {
        for item in limits {
            let detail = match item.get("detail").and_then(|v| v.as_object()) {
                Some(d) => d,
                None => continue,
            };
            let lim = detail.get("limit").and_then(|v| parse_numeric(v));
            let rem = detail.get("remaining").and_then(|v| parse_numeric(v));
            if let (Some(l), Some(r)) = (lim, rem) {
                if l > 0.0 {
                    session_pct = Some(((1.0 - r / l) * 100.0 * 10.0).round() / 10.0);
                    reset_at = ["resetTime", "reset_time"]
                        .iter()
                        .find_map(|k| detail.get(*k).and_then(|v| v.as_str()))
                        .map(String::from);
                    break;
                }
            }
        }
    }
    // 周配额：usage
    let mut percent_used: Option<f64> = None;
    let mut used_val: Option<f64> = None;
    let mut total_val: Option<f64> = None;
    if let Some(usage) = obj.get("usage").and_then(|v| v.as_object()) {
        let wlim = usage.get("limit").and_then(|v| parse_numeric(v));
        let wrem = usage.get("remaining").and_then(|v| parse_numeric(v));
        if let (Some(l), Some(r)) = (wlim, wrem) {
            if l > 0.0 {
                percent_used = Some(((1.0 - r / l) * 100.0 * 10.0).round() / 10.0);
                used_val = Some(((l - r) * 10.0).round() / 10.0);
                total_val = Some(l);
                let wreset = ["resetTime", "reset_time"]
                    .iter()
                    .find_map(|k| usage.get(*k).and_then(|v| v.as_str()));
                if wreset.is_some() {
                    reset_at = wreset.map(String::from);
                }
            }
        }
    }
    if percent_used.is_none() {
        percent_used = session_pct;
    }
    let mut result = json!({
        "used": used_val, "total": total_val,
        "percent_used": percent_used, "percent_semantics": "used",
        "unit": cfg.get("unit").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("%"),
        "fetched_at": now_iso(),
        "raw_used": used_val, "raw_total": total_val,
    });
    if let Some(sp) = session_pct {
        result["session_percent"] = json!(sp);
    }
    if let Some(r) = reset_at.as_ref().and_then(|s| crate::providers::normalize_reset_at(&json!(s))) {
        result["reset_at"] = json!(r);
    }
    json!({ "ok": true, "status": status, "result": result })
}

/// MiniMax Coding Plan — model_remains[] 里 model_name=="general"，剩余% 反转为已用%
pub async fn minimax_fetch(cfg: &Value) -> Value {
    let api_key = cfg.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
    if api_key.is_empty() {
        return no_key();
    }
    let (status, text, err) = apikey_get(
        "https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains",
        api_key,
        "bearer",
    )
    .await;
    if !err.is_empty() {
        return http_err(&err, status, &text);
    }
    let obj: Value = match serde_json::from_str(&text) {
        Ok(o) => o,
        Err(_) => return bad_json(status, &text),
    };
    // base_resp 错误
    if let Some(sc) = obj.pointer("/base_resp/status_code").and_then(|v| v.as_i64()) {
        if sc != 0 {
            let msg = if sc == 1004 {
                "认证失败/cookie 缺失（1004）：请确认使用的是 Coding Plan 专用订阅 key（非普通 API key）"
            } else {
                obj.pointer("/base_resp/status_msg").and_then(|v| v.as_str()).unwrap_or("API 返回错误")
            };
            return http_err(msg, status, &text);
        }
    }
    let model_remains = obj.get("model_remains").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let general = model_remains
        .iter()
        .find(|m| m.get("model_name").and_then(|v| v.as_str()) == Some("general"));
    let mut percent_used: Option<f64> = None;
    let mut session_pct: Option<f64> = None;
    let mut reset_at: Option<String> = None;
    if let Some(g) = general {
        // 5h 窗口
        let remain5 = [
            g.get("current_interval_remaining_percent"),
            g.get("currentIntervalRemainingPercent"),
        ]
        .into_iter()
        .flatten()
        .find_map(|v| parse_numeric(v));
        if let Some(r5) = remain5 {
            session_pct = Some(((100.0 - r5) * 10.0).round() / 10.0);
            reset_at = ["end_time", "endTime"]
                .iter()
                .find_map(|k| g.get(*k).and_then(|v| v.as_str()))
                .map(String::from);
        }
        // 周配额（status==1 才激活）
        let weekly_status = ["current_weekly_status", "currentWeeklyStatus"]
            .iter()
            .find_map(|k| g.get(*k))
            .cloned();
        if weekly_status == Some(json!(1)) {
            let remain_w = [
                g.get("current_weekly_remaining_percent"),
                g.get("currentWeeklyRemainingPercent"),
            ]
            .into_iter()
            .flatten()
            .find_map(|v| parse_numeric(v));
            if let Some(rw) = remain_w {
                percent_used = Some(((100.0 - rw) * 10.0).round() / 10.0);
                let wreset = ["weekly_end_time", "weeklyEndTime"]
                    .iter()
                    .find_map(|k| g.get(*k).and_then(|v| v.as_str()));
                if wreset.is_some() {
                    reset_at = wreset.map(String::from);
                }
            }
        }
    }
    if percent_used.is_none() {
        percent_used = session_pct;
    }
    let mut result = json!({
        "used": null, "total": null,
        "percent_used": percent_used, "percent_semantics": "used",
        "unit": cfg.get("unit").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("%"),
        "fetched_at": now_iso(),
        "raw_used": null, "raw_total": null,
    });
    if let Some(sp) = session_pct {
        result["session_percent"] = json!(sp);
    }
    if let Some(r) = reset_at.as_ref().and_then(|s| crate::providers::normalize_reset_at(&json!(s))) {
        result["reset_at"] = json!(r);
    }
    json!({ "ok": true, "status": status, "result": result })
}

fn g_get(v: &Value) -> &Value {
    v
}

/// DeepSeek 余额
pub async fn deepseek_fetch(cfg: &Value) -> Value {
    let api_key = cfg.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
    if api_key.is_empty() {
        return no_key();
    }
    let (status, text, err) = apikey_get("https://api.deepseek.com/user/balance", api_key, "bearer").await;
    if !err.is_empty() {
        return http_err(&err, status, &text);
    }
    let obj: Value = match serde_json::from_str(&text) {
        Ok(o) => o,
        Err(_) => return bad_json(status, &text),
    };
    let is_available = obj.get("is_available").cloned();
    let infos = obj.get("balance_infos").and_then(|v| v.as_array());
    let (balance_str, currency, granted, topped) = match infos.and_then(|a| a.first()) {
        Some(info) => (
            info.get("total_balance").and_then(|v| v.as_str()).unwrap_or(""),
            info.get("currency").and_then(|v| v.as_str()).unwrap_or(""),
            info.get("granted_balance").and_then(|v| v.as_str()).unwrap_or(""),
            info.get("topped_up_balance").and_then(|v| v.as_str()).unwrap_or(""),
        ),
        None => ("", "", "", ""),
    };
    let balance_val = parse_numeric(&json!(balance_str));
    let mut result = json!({
        "used": null, "total": balance_val,
        "percent_used": null, "percent_semantics": "remaining",
        "unit": if currency.is_empty() { cfg.get("unit").and_then(|v| v.as_str()).unwrap_or("") } else { currency },
        "fetched_at": now_iso(),
        "raw_used": null, "raw_total": balance_str,
    });
    if let Some(av) = is_available {
        result["is_available"] = json!(av.as_bool().unwrap_or(false));
    }
    if !granted.is_empty() {
        result["granted_balance"] = json!(granted);
    }
    if !topped.is_empty() {
        result["topped_up_balance"] = json!(topped);
    }
    json!({ "ok": true, "status": status, "result": result })
}

/// Gemini key 验证 + 模型列表
pub async fn gemini_fetch(cfg: &Value) -> Value {
    let api_key = cfg.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
    if api_key.is_empty() {
        return no_key();
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();
    let resp = match client
        .get("https://generativelanguage.googleapis.com/v1beta/models")
        .header("x-goog-api-key", api_key)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return http_err(&e.to_string(), 0, ""),
    };
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if status >= 400 {
        // 解析 Google 错误格式
        let mut err_msg = format!("HTTP {status}");
        if let Ok(eo) = serde_json::from_str::<Value>(&text) {
            if let Some(m) = eo.pointer("/error/message").and_then(|v| v.as_str()) {
                err_msg = m.to_string();
            }
        }
        return http_err(&err_msg, status, &text);
    }
    let obj: Value = match serde_json::from_str(&text) {
        Ok(o) => o,
        Err(_) => return bad_json(status, &text),
    };
    let models = obj.get("models").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let names: Vec<String> = models
        .iter()
        .filter_map(|m| {
            let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let n = name.strip_prefix("models/").unwrap_or(name);
            if n.is_empty() { None } else { Some(n.to_string()) }
        })
        .collect();
    let mut result = json!({
        "used": null, "total": null,
        "percent_used": null, "percent_semantics": "remaining",
        "unit": cfg.get("unit").and_then(|v| v.as_str()).unwrap_or(""),
        "fetched_at": now_iso(),
        "raw_used": null, "raw_total": null,
    });
    if !names.is_empty() {
        result["model_count"] = json!(names.len());
        let mut s = names.iter().take(10).cloned().collect::<Vec<_>>().join(", ");
        if names.len() > 10 {
            s.push_str("...");
        }
        result["models"] = json!(s);
    }
    json!({ "ok": true, "status": status, "result": result })
}

/// DashScope 配额
pub async fn dashscope_fetch(cfg: &Value) -> Value {
    let api_key = cfg.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
    if api_key.is_empty() {
        return no_key();
    }
    let (status, text, err) =
        apikey_get("https://dashscope.aliyuncs.com/api/v1/quotas", api_key, "bearer").await;
    if !err.is_empty() {
        return http_err(&err, status, &text);
    }
    let obj: Value = match serde_json::from_str(&text) {
        Ok(o) => o,
        Err(_) => return bad_json(status, &text),
    };
    if obj.get("success") == Some(&json!(false)) {
        let msg = obj.get("message").and_then(|v| v.as_str()).unwrap_or("API 返回错误");
        return http_err(msg, status, &text);
    }
    let quotas = obj
        .pointer("/output/quotas")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut summary: Vec<String> = Vec::new();
    for q in &quotas {
        let model = q.get("model").and_then(|v| v.as_str()).unwrap_or("");
        let ml = q.get("model_limit");
        let usage_limit = ml.and_then(|m| m.get("usage_limit")).and_then(|v| v.as_str()).or_else(|| {
            ml.and_then(|m| m.get("usage_limit")).and_then(|v| v.as_i64()).map(|_| "")
        });
        if !model.is_empty() {
            if let (Some(ul), Some(m)) = (usage_limit, ml) {
                let field = m.get("usage_limit_field").and_then(|v| v.as_str()).unwrap_or("");
                let period = m.get("usage_limit_period").and_then(|v| v.as_str()).unwrap_or("");
                summary.push(format!("{model}:{ul}{field}/{period}d"));
            }
        }
    }
    let mut result = json!({
        "used": null, "total": null,
        "percent_used": null, "percent_semantics": "remaining",
        "unit": cfg.get("unit").and_then(|v| v.as_str()).unwrap_or(""),
        "fetched_at": now_iso(),
        "raw_used": null, "raw_total": null,
    });
    if !summary.is_empty() {
        result["model_count"] = json!(summary.len());
        let mut s = summary.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
        if summary.len() > 8 {
            s.push_str("...");
        }
        result["models"] = json!(s);
    }
    json!({ "ok": true, "status": status, "result": result })
}
