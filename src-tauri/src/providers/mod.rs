//! Provider 注册与统一入口 — 对齐 server.py PROVIDER_FETCHERS + _usage_do_fetch。

pub mod apikey;
pub mod cache;
pub mod claude;
pub mod codex;
pub mod grok;
pub mod oauth;

use serde_json::{json, Value};

/// USAGE_PROVIDERS 静态元数据（对齐 Python 字典，含 base_url/docs_url）
pub fn metadata(key: &str) -> Option<Value> {
    let v = match key {
        "chatgpt_codex" => json!({
            "key": "chatgpt_codex",
            "label": "ChatGPT Codex (OAuth)",
            "description": "自动读取 ~/.codex/auth.json，刷新 token，查询 codex 用量",
            "vendor_filter": "OpenAI",
            "default_url": "https://chatgpt.com/backend-api/codex/usage",
            "default_jsonpath_used": "$.rate_limit.primary_window.used_percent",
            "default_jsonpath_total": "",
            "default_unit": "%",
            "default_interval_min": 30,
            "base_url": "https://api.openai.com/v1",
            "docs_url": "https://platform.openai.com/docs/api-reference",
        }),
        "claude_code" => json!({
            "key": "claude_code",
            "label": "Claude Code (OAuth)",
            "description": "自动读取 ~/.claude/.credentials.json，刷新 token，发送探测请求读取 rate-limit 响应头",
            "vendor_filter": "Anthropic",
            "default_url": "",
            "default_jsonpath_used": "",
            "default_jsonpath_total": "",
            "default_unit": "%",
            "default_interval_min": 30,
            "base_url": "https://api.anthropic.com",
            "docs_url": "https://docs.anthropic.com/en/api/getting-started",
        }),
        "grok_build" => json!({
            "key": "grok_build",
            "label": "Grok Build (OAuth)",
            "description": "自动读取 ~/.grok/auth.json，刷新 token，查询 xAI 用量",
            "vendor_filter": "xAI",
            "default_url": "",
            "default_jsonpath_used": "",
            "default_jsonpath_total": "",
            "default_unit": "%",
            "default_interval_min": 30,
            "base_url": "https://api.x.ai/v1",
            "docs_url": "https://docs.x.ai/docs",
        }),
        "glm_coding" => json!({
            "key": "glm_coding",
            "label": "GLM Coding Plan (API Key)",
            "description": "通过 API Key 查询智谱 GLM Coding Plan 配额（open.bigmodel.cn）",
            "vendor_filter": "Zhipu",
            "requires_api_key": true,
            "default_unit": "%",
            "default_interval_min": 30,
            "base_url": "https://open.bigmodel.cn/api/paas/v4",
            "docs_url": "https://open.bigmodel.cn/dev/api",
        }),
        "kimi_coding" => json!({
            "key": "kimi_coding",
            "label": "Kimi for Coding (API Key)",
            "description": "通过 API Key 查询 Kimi Coding Plan 周配额（api.kimi.com）",
            "vendor_filter": "Moonshot",
            "requires_api_key": true,
            "default_unit": "%",
            "default_interval_min": 30,
            "base_url": "https://api.moonshot.cn/v1",
            "docs_url": "https://platform.moonshot.cn/docs",
        }),
        "minimax_coding" => json!({
            "key": "minimax_coding",
            "label": "MiniMax Coding Plan (API Key)",
            "description": "通过 API Key 查询 MiniMax Coding Plan 配额（api.minimaxi.com）",
            "vendor_filter": "MiniMax",
            "requires_api_key": true,
            "default_unit": "%",
            "default_interval_min": 30,
            "base_url": "https://api.minimaxi.com/v1",
            "docs_url": "https://platform.minimaxi.com/document",
        }),
        "deepseek_balance" => json!({
            "key": "deepseek_balance",
            "label": "DeepSeek 余额 (API Key)",
            "description": "通过 API Key 查询 DeepSeek 账户余额（api.deepseek.com/user/balance）",
            "vendor_filter": "DeepSeek",
            "requires_api_key": true,
            "default_unit": "",
            "default_interval_min": 60,
            "base_url": "https://api.deepseek.com/v1",
            "docs_url": "https://api-docs.deepseek.com/",
        }),
        "gemini_models" => json!({
            "key": "gemini_models",
            "label": "Gemini Key 验证 (API Key)",
            "description": "通过 API Key 调用 /v1beta/models 验证密钥有效性并列出可用模型（无用量配额 API）",
            "vendor_filter": "Google",
            "requires_api_key": true,
            "default_unit": "",
            "default_interval_min": 60,
            "base_url": "https://generativelanguage.googleapis.com/v1beta",
            "docs_url": "https://ai.google.dev/gemini-api/docs",
        }),
        "dashscope_balance" => json!({
            "key": "dashscope_balance",
            "label": "阿里百炼配额 (API Key)",
            "description": "通过 API Key 调用 /api/v1/quotas 验证密钥有效性并返回模型配额（dashscope.aliyuncs.com）",
            "vendor_filter": "阿里云",
            "requires_api_key": true,
            "default_unit": "",
            "default_interval_min": 60,
            "base_url": "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "docs_url": "https://help.aliyun.com/zh/dashscope/",
        }),
        _ => return None,
    };
    Some(v)
}

pub fn all_metadata() -> Vec<Value> {
    [
        "chatgpt_codex", "claude_code", "grok_build", "glm_coding", "kimi_coding",
        "minimax_coding", "deepseek_balance", "gemini_models", "dashscope_balance",
    ]
    .iter()
    .filter_map(|k| metadata(k))
    .collect()
}

fn now_iso() -> String {
    crate::store::now_iso()
}

/// reset_at 归一化：epoch 秒/毫秒字符串或数字 → ISO 8601；ISO 字符串原样通过。
/// 各家 API 返回格式不一（Codex=epoch秒、GLM/Kimi=ISO、Grok=ISO），统一后前端才能算时间进度。
pub fn normalize_reset_at(v: &Value) -> Option<String> {
    let s = match v {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    if s.is_empty() {
        return None;
    }
    // 纯数字 → epoch（秒 10 位 / 毫秒 13 位）
    if s.chars().all(|c| c.is_ascii_digit()) {
        let num: i64 = s.parse().ok()?;
        let secs = if num > 1_000_000_000_000 { num / 1000 } else { num };
        let dt = chrono::DateTime::from_timestamp(secs, 0)?;
        return Some(dt.to_rfc3339());
    }
    // 已经是 ISO → 原样
    if s.contains('T') || s.contains('-') {
        return Some(s);
    }
    None
}

pub fn err_result(err: &str, status: u16, raw: &str) -> Value {
    json!({ "ok": false, "error": err, "status": status, "raw": raw.chars().take(500).collect::<String>() })
}

/// 统一抓取入口 — 对齐 _usage_do_fetch：
/// 内置 provider 走专用路径，否则自定义 URL+JSONPath。
/// `persist`: true=写缓存（正式抓取），false=测试。
pub async fn do_fetch(cfg: &Value, _persist: bool) -> Value {
    let provider = cfg.get("provider").and_then(|v| v.as_str()).unwrap_or("");
    let res = match provider {
        "chatgpt_codex" => codex::refresh_and_fetch(cfg).await,
        "claude_code" => claude::refresh_and_fetch(cfg).await,
        "grok_build" => grok::refresh_and_fetch(cfg).await,
        "glm_coding" => apikey::glm_fetch(cfg).await,
        "kimi_coding" => apikey::kimi_fetch(cfg).await,
        "minimax_coding" => apikey::minimax_fetch(cfg).await,
        "deepseek_balance" => apikey::deepseek_fetch(cfg).await,
        "gemini_models" => apikey::gemini_fetch(cfg).await,
        "dashscope_balance" => apikey::dashscope_fetch(cfg).await,
        _ => custom_fetch(cfg).await,
    };
    res
}

/// 自定义中转站 — 对齐 _usage_do_fetch 的 custom 分支
async fn custom_fetch(cfg: &Value) -> Value {
    let url = cfg.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let method = cfg.get("method").and_then(|v| v.as_str()).unwrap_or("GET").to_uppercase();
    let body = cfg.get("body").and_then(|v| v.as_str()).unwrap_or("");
    let jp_used = cfg.get("jsonpath_used").and_then(|v| v.as_str()).unwrap_or("");
    let jp_total = cfg.get("jsonpath_total").and_then(|v| v.as_str()).unwrap_or("");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();
    let mut req = client.request(
        if method == "POST" { reqwest::Method::POST } else { reqwest::Method::GET },
        url,
    );
    for (k, v) in cfg.get("headers").and_then(|v| v.as_object()).unwrap_or(&serde_json::Map::new()) {
        if let (Some(k), Some(v)) = (k.strip_prefix("<"), None::<&str>) {
            let _ = (k, v);
        }
        if let Some(vs) = v.as_str() {
            req = req.header(k.as_str(), vs);
        }
    }
    if method == "POST" && !body.is_empty() {
        req = req.header("Content-Type", "application/json").body(body.to_string());
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return err_result(&e.to_string(), 0, ""),
    };
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if status >= 400 {
        return err_result(&format!("HTTP {status}"), status, &text);
    }
    let obj: Value = match serde_json::from_str(&text) {
        Ok(o) => o,
        Err(_) => return err_result("响应不是合法 JSON", status, &text),
    };
    let used_raw = if jp_used.is_empty() { None } else { crate::jsonpath::eval(jp_used, &obj) };
    let total_raw = if jp_total.is_empty() { None } else { crate::jsonpath::eval(jp_total, &obj) };
    let used = used_raw.as_ref().and_then(crate::jsonpath::parse_numeric);
    let mut used_v = used;
    let total = total_raw.as_ref().and_then(crate::jsonpath::parse_numeric);
    let percent_used = if let (Some(u), Some(t)) = (used, total) {
        if t > 0.0 { Some((u / t * 100.0 * 10.0).round() / 10.0) } else { None }
    } else if let Some(u) = used {
        used_v = None;
        Some((u * 10.0).round() / 10.0)
    } else {
        None
    };
    json!({
        "ok": true, "status": status,
        "result": {
            "used": used_v, "total": total,
            "percent_used": percent_used, "percent_semantics": "used",
            "unit": cfg.get("unit").and_then(|v| v.as_str()).unwrap_or(""),
            "fetched_at": now_iso(),
            "raw_used": used_raw, "raw_total": total_raw,
        }
    })
}

pub use err_result as make_err;