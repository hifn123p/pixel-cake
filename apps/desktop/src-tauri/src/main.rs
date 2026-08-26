// 像素蛋糕 Windows 11 客户端（本地修图，3070 GPU 推理）。
// 发布版隐藏控制台窗口。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod models;
mod state;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::create_project,
            commands::list_projects,
            commands::import_photos,
            commands::list_photos,
            commands::save_recipe,
            commands::get_recipe,
            commands::submit_render,
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
