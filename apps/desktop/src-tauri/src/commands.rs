//! Tauri IPC 命令层：UI 通过 `invoke` 调用这些命令（文档 §2 ①→②）。
//!
//! 薄封装——不做业务逻辑，仅把前端参数转发给 `Store` / `Scheduler`，
//! 错误统一映射为 `String` 返回前端。

use std::collections::BTreeMap;

use bus::{EngineRequest, Recipe, Scope};
use storage::{Photo, Preset, Project};
use tauri::{AppHandle, Manager, State};

use crate::state::AppState;

/// 打开已有照片目录：扫描目录内的 RAW/JPEG/PNG，建立（或复用同名）项目并导入。
///
/// 常规 Windows 应用的「打开项目」：让用户通过目录对话框选择一个照片文件夹，
/// 应用直接把它变成可编辑项目，无需手动逐条输入路径。
#[tauri::command]
pub fn open_project(
    state: State<AppState>,
    dir: String,
) -> Result<(Project, Vec<Photo>), String> {
    let dir_path = std::path::PathBuf::from(&dir);
    if !dir_path.is_dir() {
        return Err(format!("不是有效目录: {dir}"));
    }

    // 1. 扫描目录（非递归）收集支持的图片文件。
    const EXTS: &[&str] = &[
        "cr2", "arw", "nef", "dng", "orf", "rw2", "pef", "raf", "srw", "3fr", "raw",
        "jpg", "jpeg", "png", "ppm",
    ];
    let mut files: Vec<String> = Vec::new();
    let rd = std::fs::read_dir(&dir_path).map_err(|e| format!("读取目录失败: {e}"))?;
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if EXTS.contains(&ext.as_str()) {
            files.push(path.to_string_lossy().into_owned());
        }
    }
    if files.is_empty() {
        return Err(format!("目录中没有找到照片（{dir}）"));
    }

    // 2. 项目名 = 目录名；同名已存在则直接复用，否则新建（root_path 指向该目录）。
    let name = dir_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "打开项目".into());
    let mut project = None;
    for p in state.store.list_projects().map_err(|e| e.to_string())? {
        if p.name == name {
            project = Some(p);
            break;
        }
    }
    let project = match project {
        Some(p) => p,
        None => state
            .store
            .create_project(&name, &dir)
            .map_err(|e| e.to_string())?,
    };

    // 3. 只导入尚未入库的照片（按 raw_path 去重）。
    let existing = state
        .store
        .list_photos(&project.id)
        .map_err(|e| e.to_string())?;
    let known: std::collections::HashSet<String> =
        existing.iter().map(|p| p.raw_path.clone()).collect();
    let fresh: Vec<String> = files.into_iter().filter(|f| !known.contains(f)).collect();
    let mut photos = existing;
    if !fresh.is_empty() {
        let imported = state
            .store
            .import_photos(&project.id, &fresh, |p| {
                engine::raw::probe_dimensions(p).unwrap_or((0, 0))
            })
            .map_err(|e| e.to_string())?;
        photos.extend(imported);
    }

    Ok((project, photos))
}

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

/// 导入照片（RAW/JPEG 路径列表）。尺寸经 engine 探测（读头，不全量解码）。
#[tauri::command]
pub fn import_photos(
    state: State<AppState>,
    project_id: String,
    paths: Vec<String>,
) -> Result<Vec<Photo>, String> {
    state
        .store
        .import_photos(&project_id, &paths, |p| {
            engine::raw::probe_dimensions(p).unwrap_or((0, 0))
        })
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
///
/// 可选参数（设置面板可全局配置，单次也可覆盖）：
/// - `export_dir`：导出目录（None = 与源文件同目录）
/// - `preview_max_edge`：预览最长边（None = 设置默认 1600）
/// - `export_format`：导出格式（"tiff" | "png"，None = 设置默认 "tiff"）
#[tauri::command]
pub async fn submit_render(
    state: State<'_, AppState>,
    photo_id: String,
    recipe: Recipe,
    scope: String,
    export_dir: Option<String>,
    preview_max_edge: Option<u32>,
    export_format: Option<String>,
) -> Result<(), String> {
    let scope = match scope.as_str() {
        "export" => Scope::Export,
        "base" => Scope::Base,
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
        export_dir,
        preview_max_edge,
        export_format,
    };
    state.scheduler.enqueue(req).await.map_err(|e| e.to_string())
}

/// 读取全部用户设置（key-value）。前端负责默认值合并。
#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<BTreeMap<String, String>, String> {
    // 返回所有非空设置；缺失项由前端用默认值兜底。
    let mut out = BTreeMap::new();
    for key in [
        "theme",
        "preview_max_edge",
        "export_format",
        "export_dir",
        "import_dir",
        "last_project",
    ] {
        if let Ok(Some(v)) = state.store.get_setting(key) {
            if !v.is_empty() {
                out.insert(key.to_string(), v);
            }
        }
    }
    Ok(out)
}

/// 保存一条用户设置。
#[tauri::command]
pub fn save_setting(state: State<AppState>, key: String, value: String) -> Result<(), String> {
    state.store.set_setting(&key, &value).map_err(|e| e.to_string())
}

/// 在系统文件管理器中显示指定路径（Windows 资源管理器，选中文件或打开目录）。
#[tauri::command]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    use std::process::Command;
    if std::env::consts::OS == "windows" {
        Command::new("explorer")
            .arg("/select,")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("打开资源管理器失败: {e}"))?;
    } else {
        // 非 Windows 平台：尝试用 opener 打开所在目录（不阻塞）。
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = tauri_plugin_opener::open_path(parent.to_string_lossy().as_ref(), None::<&str>);
        }
    }
    Ok(())
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

/// 读取某照片的 WebGL 实时预览底图 PNG 为 base64（仅解码、未调色）。
///
/// 路径规则同 `read_preview`：前端只传 `photo_id`，后端查询拼接 `{raw_path}.base.png`。
#[tauri::command]
pub fn read_base(state: State<AppState>, photo_id: String) -> Result<String, String> {
    let photo = state
        .store
        .get_photo(&photo_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "照片不存在".to_string())?;
    let png_path = format!("{}.base.png", photo.raw_path);
    let bytes = std::fs::read(&png_path).map_err(|e| format!("读取底图失败: {e}"))?;
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &bytes,
    ))
}

/// 保存当前编辑参数为预设。
#[tauri::command]
pub fn save_preset_cmd(
    state: State<AppState>,
    name: String,
    recipe: Recipe,
) -> Result<Preset, String> {
    state
        .store
        .save_preset(&name, "我的样片", &recipe, None)
        .map_err(|e| e.to_string())
}

/// 列出全部预设。
#[tauri::command]
pub fn list_presets_cmd(state: State<AppState>) -> Result<Vec<Preset>, String> {
    state.store.list_presets().map_err(|e| e.to_string())
}
