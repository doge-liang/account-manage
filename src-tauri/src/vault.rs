//! KeePassXC vault 文件操作 — 移植自 server.py _vault_* 系列。
//! KDBX 魔数校验、下载路径、备份轮换、上传校验。

use crate::store;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

pub const KDBX_MAGIC: [u8; 4] = [0x03, 0xD9, 0xA2, 0x9A];
pub const VAULT_BACKUP_KEEP: usize = 10;

/// 从 settings 解析 vault 路径
pub fn vault_path(data: &Value) -> PathBuf {
    let p = data
        .pointer("/settings/vault_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if p.is_empty() {
        store::default_vault()
    } else {
        PathBuf::from(p)
    }
}

/// info：path/exists/size/mtime/sha256/valid_kdbx
pub fn info(data: &Value) -> Value {
    let vp = vault_path(data);
    let mut o = json!({
        "path": vp.to_string_lossy(),
        "exists": vp.exists(),
    });
    if vp.exists() {
        if let Ok(md) = fs::metadata(&vp) {
            o["size"] = json!(md.len());
            o["mtime"] = json!(chrono::DateTime::<chrono::Local>::from(
                md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            )
            .to_rfc3339());
        }
        o["sha256"] = json!(store::sha256_hex(&vp).unwrap_or_default());
        let head = fs::read(&vp)
            .map(|b| {
                if b.len() >= 4 {
                    b[..4] == KDBX_MAGIC
                } else {
                    false
                }
            })
            .unwrap_or(false);
        o["valid_kdbx"] = json!(head);
    }
    o
}

/// 读取 vault 二进制（下载）。返回 (bytes, filename)
pub fn read_vault(data: &Value) -> Result<(Vec<u8>, String), String> {
    let vp = vault_path(data);
    if !vp.exists() {
        return Err("密钥库文件不存在".into());
    }
    let name = vp
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "ai-keys.kdbx".into());
    fs::read(&vp).map(|b| (b, name)).map_err(|e| e.to_string())
}

/// 上传替换：校验魔数 + 大小限制 + 备份旧文件 + 原子写
pub fn upload_vault(data: &Value, raw: &[u8]) -> Result<Value, String> {
    if raw.is_empty() {
        return Err("上传内容为空".into());
    }
    if raw.len() > 64 * 1024 * 1024 {
        return Err("文件超过 64MB 限制".into());
    }
    if raw.len() < 4 || raw[..4] != KDBX_MAGIC {
        return Err("不是有效的 KDBX 文件（文件头不匹配），已拒绝替换".into());
    }
    let vp = vault_path(data);
    if let Some(parent) = vp.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if vp.exists() {
        let bdir = vp.parent().unwrap().join("backups");
        let _ = store::rotate_backup(&vp, &bdir, VAULT_BACKUP_KEEP);
    }
    let tmp = vp.with_extension("kdbx.tmp");
    fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &vp).map_err(|e| e.to_string())?;
    Ok(json!({
        "ok": true,
        "path": vp.to_string_lossy(),
        "sha256": store::sha256_hex(&vp).unwrap_or_default(),
    }))
}

/// 备份列表
pub fn backups(data: &Value) -> Value {
    let vp = vault_path(data);
    let bdir = vp.parent().unwrap().join("backups");
    let mut items = Vec::new();
    if bdir.exists() {
        let stem = vp.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let ext = vp.extension().and_then(|s| s.to_str()).unwrap_or("kdbx");
        let mut v: Vec<PathBuf> = fs::read_dir(&bdir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n.starts_with(stem) && n.ends_with(ext))
                            .unwrap_or(false)
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v.reverse();
        for b in v {
            let md = fs::metadata(&b).ok();
            items.push(json!({
                "name": b.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                "size": md.as_ref().map(|m| m.len()).unwrap_or(0),
                "mtime": md
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .map(|t| chrono::DateTime::<chrono::Local>::from(t).to_rfc3339())
                    .unwrap_or_default(),
                "sha256": store::sha256_hex(&b).unwrap_or_default(),
            }));
        }
    }
    json!({ "backups": items, "dir": bdir.to_string_lossy() })
}

/// 读取指定备份（下载）。路径穿越防护。
pub fn read_backup(data: &Value, name: &str) -> Result<(Vec<u8>, String), String> {
    let vp = vault_path(data);
    let bdir = vp.parent().unwrap().join("backups");
    let target = bdir.join(name);
    let canon = target.canonicalize().map_err(|_| "备份不存在".to_string())?;
    let base = bdir.canonicalize().map_err(|_| "备份不存在".to_string())?;
    if !canon.starts_with(&base) || !canon.is_file() {
        return Err("备份不存在".into());
    }
    let fname = canon
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    fs::read(&canon).map(|b| (b, fname)).map_err(|e| e.to_string())
}
