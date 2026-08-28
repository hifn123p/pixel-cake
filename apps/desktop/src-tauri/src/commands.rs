//! Tauri IPC 命令层：UI 通过 `invoke` 调用这些命令（文档 §2 ①→②）。
//!
//! 薄封装——不做业务逻辑，仅把前端参数转发给 `Store` / `Scheduler`，
//! 错误统一映射为 `String` 返回前端。

use bus::{EngineRequest, Recipe, Scope};
use storage::{Photo, Project};
use tauri::{AppHandle, Manager, State};

use crate::state::AppState;

/// 新建项目（客片组），并在 app_data 下建立项目目录。
#[tauri::command]
pub fn create_project(app: AppHandle, state: State<AppState>, name: String) -> Result<Project, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let lib = dir.join("libraries").join(&name);
    std::fs::create_dir_all(&lib).map_err(|e| e.to_string())?;
    state
        .store
        .create_project(&name, &lib.to_string_lossy())
        .map_err(|e| e.to_string())
}

/// 列出全部项目。
#[tauri::command]
pub fn list_projects(state: State<AppState>) -> Result<Vec<Project>, String> {
    state.store.list_projects().map_err(|e| e.to_string())
}

/// 导入照片（RAW/JPEG 路径列表）。尺寸解码在 engine 接入前以 0 占位。
#[tauri::command]
pub fn import_photos(
    state: State<AppState>,
    project_id: String,
    paths: Vec<String>,
) -> Result<Vec<Photo>, String> {
    // TODO(M1-raw): 尺寸应由 engine::raw 解码得到，此处占位 (0,0)。
    state
        .store
        .import_photos(&project_id, &paths, |_| (0, 0))
        .map_err(|e| e.to_string())
}

/// 列出项目内照片。
#[tauri::command]
pub fn list_photos(state: State<AppState>, project_id: String) -> Result<Vec<Photo>, String> {
    state.store.list_photos(&project_id).map_err(|e| e.to_string())
}

/// 保存某照片的编辑 recipe。
#[tauri::command]
pub fn save_recipe(state: State<AppState>, photo_id: String, recipe: Recipe) -> Result<(), String> {
    state
        .store
        .save_recipe(&photo_id, &recipe)
        .map_err(|e| e.to_string())
}

/// 读取某照片的编辑 recipe。
#[tauri::command]
pub fn get_recipe(state: State<AppState>, photo_id: String) -> Result<Option<Recipe>, String> {
    state.store.get_recipe(&photo_id).map_err(|e| e.to_string())
}

/// 提交一次重算（预览/导出）。scope: "preview" | "export"。
#[tauri::command]
pub async fn submit_render(
    state: State<'_, AppState>,
    photo_id: String,
    recipe: Recipe,
    scope: String,
) -> Result<(), String> {
    let scope = match scope.as_str() {
        "export" => Scope::Export,
        _ => Scope::Preview,
    };
    // 查原图路径（引擎据此解码输入）
    let photo = state
        .store
        .get_photo(&photo_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "照片不存在".to_string())?;
    let req = EngineRequest {
        photo_id,
        raw_path: photo.raw_path,
        recipe,
        scope,
    };
    state.scheduler.enqueue(req).await.map_err(|e| e.to_string())
}

/// 读取某照片的最新预览 PNG 为 base64（前端显示）。
///
/// 安全设计：前端只传 `photo_id`，实际路径由后端从数据库查询并拼接，
/// 不暴露任意文件读取能力。
#[tauri::command]
pub fn read_preview(state: State<AppState>, photo_id: String) -> Result<String, String> {
    let photo = state
        .store
        .get_photo(&photo_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "照片不存在".to_string())?;
    let png_path = format!("{}.out.png", photo.raw_path);
    let bytes = std::fs::read(&png_path).map_err(|e| format!("读取预览失败: {e}"))?;
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &bytes,
    ))
}
