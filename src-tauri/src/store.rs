//! 数据层 — load/save/原子写/备份轮换。移植自 server.py L64-142。
//!
//! 数据目录解析：
//! - dev（`cargo tauri dev`）：仓库根的 app/data/（与 Python 版共用）
//! - portable（直接跑 exe）：exe 所在目录的 app/data/

use crate::models::AppData;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DATA_BACKUP_KEEP: usize = 10;

/// 解析数据目录。cfg(debug_assertions) 时用编译期仓库根（dev），
/// 否则用 exe 所在目录（portable）。
pub fn data_dir() -> PathBuf {
    #[cfg(debug_assertions)]
    {
        // dev: src-tauri/ 的上级 = 仓库根
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest.parent().unwrap().join("app").join("data")
    }
    #[cfg(not(debug_assertions))]
    {
        // portable: exe 旁的 app/data/
        let exe = std::env::current_exe().expect("无法定位 exe 路径");
        exe.parent()
            .unwrap()
            .join("app")
            .join("data")
    }
}

pub fn data_file() -> PathBuf {
    data_dir().join("accounts.json")
}

pub fn backup_dir() -> PathBuf {
    data_dir().join("backups")
}

pub fn usage_cache_file() -> PathBuf {
    data_dir().join("usage_cache.json")
}

/// 默认 vault 路径（仓库根/exe 旁 的 keepassxc/vaults/ai-keys.kdbx）
pub fn default_vault() -> PathBuf {
    project_root().join("keepassxc").join("vaults").join("ai-keys.kdbx")
}

pub fn project_root() -> PathBuf {
    data_dir().parent().unwrap().parent().unwrap().to_path_buf()
}

fn empty_data_json() -> String {
    let mut v = serde_json::json!({
        "version": 1,
        "accounts": [],
        "relations": [],
        "query_links": [],
        "usage_configs": [],
    });
    let obj = v.as_object_mut().unwrap();
    obj.insert(
        "settings".into(),
        serde_json::json!({
            "vault_path": default_vault().to_string_lossy(),
            "name": "账号管家",
        }),
    );
    serde_json::to_string_pretty(&v).unwrap()
}

/// 加载数据：损坏时回退最新备份，再不行返回空。
/// 返回原始 Value（保持未知字段不丢，与 Python 版行为一致——直接整棵 JSON 读写）。
pub fn load_data_raw() -> Value {
    let df = data_file();
    if !df.exists() {
        return serde_json::from_str(&empty_data_json()).unwrap();
    }
    match fs::read_to_string(&df) {
        Ok(s) => match serde_json::from_str::<Value>(&s) {
            Ok(mut v) => {
                normalize(&mut v);
                v
            }
            Err(_) => load_from_backup(),
        },
        Err(_) => load_from_backup(),
    }
}

fn load_from_backup() -> Value {
    if let Some(b) = latest_backup(&backup_dir()) {
        if let Ok(s) = fs::read_to_string(&b) {
            if let Ok(mut v) = serde_json::from_str::<Value>(&s) {
                normalize(&mut v);
                return v;
            }
        }
    }
    serde_json::from_str(&empty_data_json()).unwrap()
}

/// 对齐 Python setdefault 行为：补默认键。
fn normalize(v: &mut Value) {
    let obj = match v.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    obj.entry("version").or_insert(serde_json::json!(1));
    obj.entry("accounts").or_insert(serde_json::json!([]));
    obj.entry("relations").or_insert(serde_json::json!([]));
    obj.entry("query_links").or_insert(serde_json::json!([]));
    obj.entry("usage_configs").or_insert(serde_json::json!([]));
    let settings = obj
        .entry("settings")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(s) = settings.as_object_mut() {
        s.entry("vault_path")
            .or_insert(serde_json::json!(default_vault().to_string_lossy()));
        s.entry("name").or_insert(serde_json::json!("账号管家"));
    }
}

/// 保存：内容无变化跳过；有变化先轮换备份，再 tmp+rename 原子写（Syncthing 友好）。
pub fn save_data_raw(data: &Value) -> std::io::Result<()> {
    let dir = data_dir();
    fs::create_dir_all(&dir)?;
    let df = data_file();
    let new = serde_json::to_string_pretty(data).unwrap_or_default();

    if df.exists() {
        if let Ok(old) = fs::read_to_string(&df) {
            if old.trim() == new.trim() {
                return Ok(()); // 无变化
            }
        }
        let _ = rotate_backup(&df, &backup_dir(), DATA_BACKUP_KEEP);
    }

    let tmp = dir.join("accounts.json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(new.as_bytes())?;
        f.flush()?;
        #[cfg(unix)]
        f.sync_all()?;
    }
    #[cfg(windows)]
    {
        // Windows 上 rename 前确保写入落盘
        use std::os::windows::io::AsRawHandle;
        let _ = AsRawHandle::as_raw_handle(&fs::File::open(&tmp)?);
    }
    fs::rename(&tmp, &df)?;
    Ok(())
}

/// 类型化加载（tests 用）
pub fn load_data() -> AppData {
    serde_json::from_value(load_data_raw()).unwrap_or_default()
}

/// 备份轮换：把当前文件复制为 backups/accounts-YYYYmmdd-HHMMSS.json，保留最近 keep 份。
pub fn rotate_backup(src: &Path, bdir: &Path, keep: usize) -> std::io::Result<PathBuf> {
    fs::create_dir_all(bdir)?;
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
    // Python 版格式 accounts-YYYYmmdd-HHMMSS.json（毫秒部分去掉点号 → accounts-...-SSmmm.json）
    let ts = ts.replace('.', "");
    let dst = bdir.join(format!("accounts-{ts}.json"));
    fs::copy(src, &dst)?;
    // 清理超出 keep 的最旧备份
    let mut backups = list_backups(bdir);
    while backups.len() > keep {
        let oldest = backups.remove(0);
        let _ = fs::remove_file(&oldest);
    }
    Ok(dst)
}

/// 列出备份文件（按文件名升序 = 时间升序）
pub fn list_backups(bdir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = match fs::read_dir(bdir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("accounts-") && n.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    v.sort();
    v
}

/// 最新备份
pub fn latest_backup(bdir: &Path) -> Option<PathBuf> {
    list_backups(bdir).pop()
}

/// sha256（vault 校验用，移植 _sha256）
pub fn sha256_hex(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(path)?;
    let mut h = Sha256::new();
    h.update(&bytes);
    let out = h.finalize();
    Ok(out.iter().map(|b| format!("{b:02x}")).collect())
}

/// 新 id：前缀-4位序号（对齐 Python _new_id：acc-0001 / uc-0001 / q-0001 / rel-0001）
pub fn new_id(prefix: &str, items: &[Value], id_key: &str) -> String {
    let mut max = 0u32;
    for it in items {
        if let Some(id) = it.get(id_key).and_then(|v| v.as_str()) {
            if let Some(num) = id.rsplit('-').next().and_then(|s| s.parse::<u32>().ok()) {
                if num > max {
                    max = num;
                }
            }
        }
    }
    format!("{prefix}-{:04}", max + 1)
}

/// 当前 UTC ISO 时间（对齐 Python _now：+00:00 格式）
pub fn now_iso() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.6f+00:00")
        .to_string()
}
