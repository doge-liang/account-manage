//! 后台用量调度器 — 对齐 _usage_scheduler（每 30s 扫描，按 interval_min 判断到期）。

use crate::providers;
use crate::store;
use serde_json::Value;
use std::time::Duration;
use tauri::AppHandle;

pub fn spawn(_app: AppHandle) {
    // Tauri 默认不在 tokio runtime 上 — 独立线程建 runtime 跑调度器
    std::thread::spawn(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build scheduler runtime");
        rt.block_on(async { loop_tick().await });
    });
}

async fn loop_tick() {
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let _ = tick().await;
    }
}

async fn tick() -> Result<(), Box<dyn std::error::Error>> {
    let data = store::load_data_raw();
    let mut cache = providers::cache::load_cache();
    let now = chrono::Utc::now();
    let mut changed = false;

    for c in data
        .get("usage_configs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
    {
        if !c.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true) {
            continue;
        }
        let interval = c
            .get("interval_min")
            .and_then(|v| v.as_u64())
            .unwrap_or(60)
            .max(1);
        let cfg_id = c.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if cfg_id.is_empty() {
            continue;
        }
        // 到期判断
        let due = match cache
            .get(&cfg_id)
            .and_then(|e| e.get("fetched_at"))
            .and_then(|v| v.as_str())
        {
            None => true,
            Some(last) => match chrono::DateTime::parse_from_rfc3339(last) {
                Ok(lt) => (now - lt.with_timezone(&chrono::Utc)).num_seconds() >= (interval as i64) * 60,
                Err(_) => true,
            },
        };
        if !due {
            continue;
        }
        let res = providers::do_fetch(&c, true).await;
        if res.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            if let Some(obj) = cache.as_object_mut() {
                obj.insert(cfg_id.clone(), res["result"].clone());
            }
            changed = true;
            // 更新 last_run_at
            let mut d2 = store::load_data_raw();
            if let Some(configs) = d2.get_mut("usage_configs").and_then(|v| v.as_array_mut()) {
                for cc in configs.iter_mut() {
                    if cc["id"] == Value::String(cfg_id.clone()) {
                        cc["last_run_at"] = Value::String(store::now_iso());
                        break;
                    }
                }
            }
            let _ = store::save_data_raw(&d2);
        }
    }
    if changed {
        providers::cache::save_cache(&cache);
    }
    Ok(())
}
