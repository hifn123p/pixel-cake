//! 应用状态：持有数据层（`Store`）与调度层（`Scheduler`），并启动事件转发。

use scheduler::Scheduler;
use storage::Store;
use tauri::{AppHandle, Manager};

/// 全局应用状态（文档 §2：数据层 + 预览/调度层）。
pub struct AppState {
    pub store: Store,
    pub scheduler: Scheduler,
}

/// 在 Tauri `setup` 阶段初始化：打开 SQLite、创建调度器、启动事件转发、注入状态。
pub fn init(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&dir)?;
    let db = dir.join("pixcake.db");

    let store = Store::open(&db)?;
    let scheduler = Scheduler::new();

    // 先取出事件接收端再 move 调度器进状态，避免借用冲突。
    crate::events::spawn_forwarder(app.clone(), scheduler.subscribe());

    app.manage(AppState { store, scheduler });
    Ok(())
}
