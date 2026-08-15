// 账号管家 AccountHub — Tauri 2 桌面版
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod jsonpath;
mod models;
mod providers;
mod scheduler;
mod store;
mod vault;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // 后台用量调度器
            scheduler::spawn(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_data,
            commands::save_settings,
            commands::upsert_account,
            commands::delete_account,
            commands::upsert_relation,
            commands::delete_relation,
            commands::upsert_query_link,
            commands::delete_query_link,
            commands::get_usage,
            commands::get_usage_providers,
            commands::upsert_usage_config,
            commands::delete_usage_config,
            commands::test_usage_config,
            commands::fetch_usage,
            commands::import_data,
            commands::reset_data,
            commands::grok_device_code_start,
            commands::grok_device_code_poll,
            // commands::oauth_import_from_cli, // 前端未接线（Python 版同样未实现），保留命令定义待用
            commands::vault_info,
            commands::vault_download,
            commands::vault_backups,
            commands::vault_restore,
            commands::vault_upload,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn main() {
    run();
}
