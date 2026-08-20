//! 引擎事件转发：把 `scheduler` 产生的 `EngineEvent` 推送到前端。
//!
//! 前端通过 `listen('engine://event', ...)` 接收进度/完成/错误事件（文档 §7）。

use bus::EngineEvent;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

pub const EVENT_NAME: &str = "engine://event";

/// 启动后台转发任务：订阅调度器事件流，emit 到 WebView。
pub fn spawn_forwarder(app: AppHandle, mut rx: broadcast::Receiver<EngineEvent>) {
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(evt) => {
                    if let Err(e) = app.emit(EVENT_NAME, &evt) {
                        eprintln!("[events] emit failed: {e}");
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}
