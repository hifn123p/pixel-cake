// 像素蛋糕 Windows 11 客户端（本地修图，3070 GPU 推理）。
// 发布版隐藏控制台窗口。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod models;
mod state;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::create_project,
            commands::open_project,
            commands::list_projects,
            commands::import_photos,
            commands::list_photos,
            commands::save_recipe,
            commands::get_recipe,
            commands::submit_render,
            commands::read_preview,
            commands::read_base,
            commands::save_preset_cmd,
            commands::list_presets_cmd,
            commands::get_settings,
            commands::save_setting,
            commands::reveal_in_explorer,
            models::list_model_packages_cmd,
            models::download_model_cmd,
        ])
        .setup(|app| {
            state::init(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("像素蛋糕客户端启动失败");
}
