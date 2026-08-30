//! 模型资源包管理（设置项「资源包下载」）。
//!
//! 模型文件不入库，应用内按需下载到 `app_data_dir/models/`。
//! 清单硬编码在此，URL 使用 Hugging Face 镜像 hf-mirror.com（国内可达）。
//! size_bytes 为实测文件大小（2026-08-30 验证：4 个 URL 均可达，
//! SCRFD/2DFAN4/BiSeNet/GPEN 输入输出约定与引擎代码一致）。

use std::path::PathBuf;
use std::sync::LazyLock;

use bus::{ModelSpec, ModelStatus};
use tauri::{AppHandle, Manager};

/// 模型资源包清单（开源小模型，ONNX 格式，来源 FaceFusion 模型仓库）。
pub static MODEL_PACKAGES: LazyLock<Vec<ModelSpec>> = LazyLock::new(|| {
    vec![
    ModelSpec {
        id: "scrfd_2.5g".into(),
        name: "人脸检测 SCRFD 2.5G".into(),
        url: "https://hf-mirror.com/Jonny001/Models-Pack-01/resolve/main/scrfd_2.5g.onnx".into(),
        size_bytes: 3_295_067,
        purpose: "磨皮/美型第一步的人脸检测".into(),
    },
    ModelSpec {
        id: "bisenet_resnet_34".into(),
        name: "人脸解析 BiSeNet".into(),
        url: "https://hf-mirror.com/Jonny001/Models-Pack-01/resolve/main/bisenet_resnet_34.onnx".into(),
        size_bytes: 93_632_546,
        purpose: "追色分区（皮肤/发/唇/背景）".into(),
    },
    ModelSpec {
        id: "2dfan4".into(),
        name: "人脸关键点 2DFAN4".into(),
        url: "https://hf-mirror.com/Jonny001/Models-Pack-01/resolve/main/2dfan4.onnx".into(),
        size_bytes: 97_904_803,
        purpose: "美型液化的关键点检测".into(),
    },
    ModelSpec {
        id: "gpen_bfr_512".into(),
        name: "皮肤增强 GPEN".into(),
        url: "https://hf-mirror.com/Jonny001/Models-Pack-01/resolve/main/gpen_bfr_512.onnx".into(),
        size_bytes: 284_340_240,
        purpose: "磨皮（皮肤平滑/增强）".into(),
    },
    ]
});

/// 模型目录：`app_data_dir/models`。
pub fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("models"))
}

fn model_path(dir: &PathBuf, id: &str) -> PathBuf {
    dir.join(format!("{id}.onnx"))
}

/// 列出模型清单 + 本地状态。
pub fn list_model_packages(app: &AppHandle) -> Result<Vec<(ModelSpec, ModelStatus)>, String> {
    let dir = models_dir(app)?;
    Ok(MODEL_PACKAGES
        .iter()
        .map(|spec| {
            let path = model_path(&dir, &spec.id);
            let status = if path.exists() {
                ModelStatus::Downloaded {
                    path: path.to_string_lossy().into_owned(),
                }
            } else {
                ModelStatus::NotDownloaded
            };
            (spec.clone(), status)
        })
        .collect())
}

/// 下载指定模型（同步，阻塞；由 spawn_blocking 包裹调用）。
pub fn download_model(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let spec = MODEL_PACKAGES
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("未知模型: {id}"))?;
    let dir = models_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建模型目录失败: {e}"))?;
    let path = model_path(&dir, &spec.id);

    let resp = ureq::get(&spec.url).call().map_err(|e| format!("下载失败: {e}"))?;
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&path).map_err(|e| format!("创建文件失败: {e}"))?;
    std::io::copy(&mut reader, &mut file).map_err(|e| format!("写入失败: {e}"))?;

    Ok(path)
}

/// 查询模型资源包清单及本地状态。
#[tauri::command]
pub fn list_model_packages_cmd(app: AppHandle) -> Result<Vec<(ModelSpec, ModelStatus)>, String> {
    list_model_packages(&app)
}

/// 下载模型资源包（异步命令，阻塞下载在 spawn_blocking 中执行）。
#[tauri::command]
pub async fn download_model_cmd(app: AppHandle, id: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        download_model(&app, &id).map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("下载任务失败: {e}"))?
}
