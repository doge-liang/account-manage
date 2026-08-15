//! 数据模型 — 对齐 app/data/accounts.json 结构。
//! 兼容策略：宽松反序列化（缺字段给默认），`fields` 等自由字段用 map。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Account {
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub url: String,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub fields: HashMap<String, String>,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn default_status() -> String {
    "active".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Relation {
    #[serde(default)]
    pub from_id: String,
    #[serde(default)]
    pub to_id: String,
    #[serde(default)]
    pub rel_type: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryLink {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageConfig {
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub api_key: String,
    /// OAuth tokens（grok/codex/claude）：access_token / refresh_token / expires_at / oidc_client_id / user_id
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_tokens: Option<HashMap<String, String>>,
    #[serde(default)]
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub jsonpath_used: String,
    #[serde(default)]
    pub jsonpath_total: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default = "default_interval")]
    pub interval_min: u32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub last_run_at: String,
}

fn default_method() -> String {
    "GET".into()
}
fn default_interval() -> u32 {
    60
}
fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub vault_path: String,
    #[serde(default = "default_app_name")]
    pub name: String,
}

fn default_app_name() -> String {
    "账号管家".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppData {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub relations: Vec<Relation>,
    #[serde(default)]
    pub query_links: Vec<QueryLink>,
    #[serde(default)]
    pub usage_configs: Vec<UsageConfig>,
    #[serde(default)]
    pub settings: Settings,
}

fn default_version() -> u32 {
    1
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            version: 1,
            accounts: Vec::new(),
            relations: Vec::new(),
            query_links: Vec::new(),
            usage_configs: Vec::new(),
            settings: Settings::default(),
        }
    }
}
