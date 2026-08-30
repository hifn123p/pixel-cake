//! `scheduler` — 预览/调度层（文档 §2 第②层）。
//!
//! 负责：任务队列（Tokio mpsc）、进度/结果事件广播（broadcast）、显存预算
//! （`GpuBudget`，文档 §5.3 的 8GB 上限约束）。UI 提交 `EngineRequest`，
//! 引擎按显存预算串行/有限并行执行，进度经 `EngineEvent` 回流前端。

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bus::{EngineEvent, EngineRequest, PipelineStep, Scope};
use engine::ai::RetouchEngine;
use engine::base::tone::{CurvePoint, ToneParams};
use engine::color::lut::builtin_filter_lut;
use engine::detect::segment::CLASS_SKIN;
use engine::export::{encode_png, encode_tiff};
use engine::image::{ColorSpace, ImageBuf};
use engine::infer::resize_bilinear;
use engine::pipeline::{process, Pipeline};
use engine::raw::decode_auto;
use engine::retouch::beauty::LiquifyPoint;
use engine::retouch::color_transfer::{build_region_transfer_lut, TransferMode};
use engine::retouch::inpaint::{merge_mask, polygon_to_mask};
use tokio::sync::{broadcast, mpsc};

/// RTX 3070 显存预算：8GB（文档 §5.3）。
pub const VRAM_8GB: usize = 8 * 1024 * 1024 * 1024;

/// 显存预算：约束并发推理，避免多模型同时占满显存。
#[derive(Clone, Debug)]
pub struct GpuBudget {
    total: usize,
    used: Arc<AtomicUsize>,
}

impl GpuBudget {
    pub fn new(total_bytes: usize) -> Self {
        Self {
            total: total_bytes,
            used: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// 尝试占用 `bytes` 显存；成功返回 true（用于决定是否串行）。
    pub fn try_acquire(&self, bytes: usize) -> bool {
        let mut cur = self.used.load(Ordering::Relaxed);
        loop {
            if cur + bytes > self.total {
                return false;
            }
            match self.used.compare_exchange_weak(
                cur,
                cur + bytes,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => cur = actual,
            }
        }
    }

    pub fn release(&self, bytes: usize) {
        self.used.fetch_sub(bytes, Ordering::SeqCst);
    }

    pub fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> usize {
        self.total
    }
}

/// 任务调度器。需在 Tokio runtime 上下文内创建（Tauri 后端自带 async runtime）。
pub struct Scheduler {
    tx_req: mpsc::Sender<EngineRequest>,
    tx_evt: broadcast::Sender<EngineEvent>,
    budget: GpuBudget,
}

impl Scheduler {
    /// 创建调度器；`models_dir` 为模型目录（缺失模型则 AI 功能降级）。
    pub fn new(models_dir: impl AsRef<Path>) -> Self {
        Self::with_budget(VRAM_8GB, models_dir)
    }

    pub fn with_budget(total_bytes: usize, models_dir: impl AsRef<Path>) -> Self {
        let (tx_req, mut rx_req) = mpsc::channel::<EngineRequest>(256);
        let (tx_evt, _) = broadcast::channel::<EngineEvent>(64);
        let budget = GpuBudget::new(total_bytes);

        let models_dir = models_dir.as_ref().to_path_buf();
        let evt = tx_evt.clone();
        let bgt = budget.clone();
        tokio::spawn(async move {
            // AI 门面在工作线程内创建一次，跨请求复用（模型加载开销仅在首次）。
            let engine = Arc::new(Mutex::new(RetouchEngine::new(&models_dir)));
            while let Some(req) = rx_req.recv().await {
                run_one(&evt, &bgt, &engine, req).await;
            }
        });

        Self {
            tx_req,
            tx_evt,
            budget,
        }
    }

    /// 提交一次重算（UI → 引擎）。
    pub async fn enqueue(&self, req: EngineRequest) -> Result<(), mpsc::error::SendError<EngineRequest>> {
        self.tx_req.send(req).await
    }

    /// 订阅进度/结果事件（引擎 → UI）。
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.tx_evt.subscribe()
    }

    pub fn budget(&self) -> &GpuBudget {
        &self.budget
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        // 默认空模型目录：AI 功能降级为纯参数处理。
        Self::new(Path::new(""))
    }
}

/// 单条请求的处理：按显存预算串行，解码 → 管线 → 导出，发进度与完成事件。
async fn run_one(
    evt: &broadcast::Sender<EngineEvent>,
    budget: &GpuBudget,
    engine: &Arc<Mutex<RetouchEngine>>,
    req: EngineRequest,
) {
    let photo_id = req.photo_id.clone();

    // 显存预算：占位估算（后续由 engine 各算子 OpCost 驱动）。
    let est_vram = 512 * 512 * 4 * 4;
    if !budget.try_acquire(est_vram) {
        let _ = evt.send(EngineEvent::Error {
            photo_id: photo_id.clone(),
            code: bus::ErrorCode::OutOfMemory,
            message: "显存不足，请稍后重试".into(),
        });
        return;
    }

    let _ = evt.send(EngineEvent::Progress {
        photo_id: photo_id.clone(),
        step: PipelineStep::RawDecode,
        pct: 0.0,
    });

    // 解码 + 处理 + 导出（同步，M1 骨架；后续改用 spawn_blocking 承接 CPU 密集）。
    let result = process_request(&req, engine);

    budget.release(est_vram);

    match result {
        Ok(out_path) => {
            let _ = evt.send(EngineEvent::Progress {
                photo_id: photo_id.clone(),
                step: PipelineStep::Encode,
                pct: 100.0,
            });
            let _ = evt.send(EngineEvent::Done {
                photo_id,
                result_path: Some(out_path),
                proxy_updated: matches!(req.scope, Scope::Preview | Scope::Base),
            });
        }
        Err(message) => {
            let _ = evt.send(EngineEvent::Error {
                photo_id,
                code: bus::ErrorCode::Decode,
                message,
            });
        }
    }
}

/// 执行一次重算：解码输入 → AI 检测 → Recipe 转管线 → 16bit 处理 → TIFF 导出。
/// 返回导出文件路径。
fn process_request(req: &EngineRequest, engine: &Arc<Mutex<RetouchEngine>>) -> Result<String, String> {
    // 1. 读文件 + 解码（按扩展名分发：PPM / LibRaw）
    let bytes = std::fs::read(&req.raw_path).map_err(|e| format!("读取原图失败: {e}"))?;
    let mut img = decode_auto(&req.raw_path, &bytes).map_err(|e| format!("解码失败: {e}"))?;

    // 预览 / 底图模式：缩小到代理尺寸（最长边可配置，默认 1600px），显著提升编辑响应速度；
    // 导出模式保持全分辨率。
    if matches!(req.scope, Scope::Preview | Scope::Base) {
        let max_edge = req.preview_max_edge.unwrap_or(1600).max(320);
        if img.width.max(img.height) > max_edge {
            let scale = max_edge as f32 / img.width.max(img.height) as f32;
            let w = ((img.width as f32 * scale).round() as u32).max(1);
            let h = ((img.height as f32 * scale).round() as u32).max(1);
            img = resize_bilinear(&img, w, h);
        }
    }

    // Base 模式：仅解码 + 降采样，不跑管线（WebGL 实时预览底图）。
    if req.scope == Scope::Base {
        let png_path = format!("{}.base.png", req.raw_path);
        let png = encode_png(&img);
        std::fs::write(&png_path, &png).map_err(|e| format!("写底图失败: {e}"))?;
        return Ok(png_path);
    }

    // 2. Recipe → Pipeline
    let mut pipeline = recipe_to_pipeline(&req.recipe, img.width, img.height);

    // 3. AI：检测人脸 → 美型液化点（原图关键点）+ 磨皮（GPEN 替换 img）
    {
        let mut eng = engine.lock().expect("engine mutex poisoned");
        if let Some(faces) = eng.detect_faces(&img) {
            // 3a. 美型：68 点瘦脸/大眼（基于原图关键点），关键点模型缺失时回退 5 点大眼
            let mut beauty = Vec::new();
            for f in &faces {
                if let Some(lm) = eng.detect_landmarks(&img, f.bbox) {
                    beauty.extend(RetouchEngine::face_beauty_points(
                        &lm,
                        img.width,
                        img.height,
                        req.recipe.beauty.face_slim,
                    ));
                }
            }
            if beauty.is_empty() {
                beauty = RetouchEngine::auto_beauty_points(&faces, img.width, img.height);
            }
            pipeline.beauty_points.extend(beauty);

            // 3b. 磨皮：GPEN 皮肤增强（blend 强度 = ka/100）
            if req.recipe.neutral_gray.enabled && req.recipe.neutral_gray.ka > 0 {
                let blend = req.recipe.neutral_gray.ka as f32 / 100.0;
                for f in &faces {
                    if let Some(enhanced) = eng.enhance_face(&img, f.bbox, blend) {
                        img = enhanced;
                    }
                }
            }

            // 3c. 追色：参考图皮肤色调 → 烘焙分区迁移 LUT（单脸）
            if req.recipe.color.enabled {
                if let Some(ref_path) = req.recipe.color.reference_path.as_ref() {
                    if let Some(target_bbox) = faces.first().map(|f| f.bbox) {
                        if let Ok(ref_bytes) = std::fs::read(ref_path) {
                            if let Ok(reference) = decode_auto(ref_path, &ref_bytes) {
                                if let Some(ref_faces) = eng.detect_faces(&reference) {
                                    if let Some(ref_bbox) = ref_faces.first().map(|f| f.bbox) {
                                        let target_mask = eng
                                            .segment_face(&img, target_bbox)
                                            .map(|m| m.to_mask(&[CLASS_SKIN]));
                                        let ref_mask = eng
                                            .segment_face(&reference, ref_bbox)
                                            .map(|m| m.to_mask(&[CLASS_SKIN]));
                                        if let (Some(tm), Some(rm)) = (target_mask, ref_mask) {
                                            let mode = match req.recipe.color.mode {
                                                bus::ColorTransferMode::Extreme => {
                                                    TransferMode::Extreme
                                                }
                                                bus::ColorTransferMode::Harmony => {
                                                    TransferMode::Harmony
                                                }
                                            };
                                            pipeline.color_lut = Some(build_region_transfer_lut(
                                                &img,
                                                &tm,
                                                &reference,
                                                &rm,
                                                mode,
                                                33,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. 16bit 全链路处理
    let result = process(&img, &pipeline);

    // 5. 导出：预览仅输出 8bit PNG（小图，快）；导出输出 16bit TIFF + PNG（全分辨率）。
    //    导出目录可选：`req.export_dir` 指定时写入该目录，否则与源文件同目录。
    let src_stem = Path::new(&req.raw_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".into());
    let out_ext = if req.export_format.as_deref() == Some("png") {
        "png"
    } else {
        "tiff"
    };
    let out_path = match &req.export_dir {
        Some(dir) if !dir.trim().is_empty() => {
            std::fs::create_dir_all(dir).map_err(|e| format!("创建导出目录失败: {e}"))?;
            Path::new(dir)
                .join(format!("{src_stem}.out.{out_ext}"))
                .to_string_lossy()
                .into_owned()
        }
        _ => format!("{}.out.{out_ext}", req.raw_path),
    };
    // PNG 预览始终写在源文件旁（read_preview 依此读取）。
    let png_path = format!("{}.out.png", req.raw_path);
    let export_png = req.export_format.as_deref() == Some("png");
    if req.scope == Scope::Preview {
        let png = encode_png(&result);
        std::fs::write(&png_path, &png).map_err(|e| format!("写预览文件失败: {e}"))?;
    } else {
        if export_png {
            // 仅 PNG 导出：全分辨率 8bit PNG（无 16bit TIFF）。
            let png = encode_png(&result);
            std::fs::write(&out_path, &png).map_err(|e| format!("写导出文件失败: {e}"))?;
        } else {
            let tiff = encode_tiff(&result);
            std::fs::write(&out_path, &tiff).map_err(|e| format!("写导出文件失败: {e}"))?;
        }
        let png = encode_png(&result);
        std::fs::write(&png_path, &png).map_err(|e| format!("写预览文件失败: {e}"))?;
    }

    Ok(out_path)
}

/// 把 `bus::Recipe` 映射为引擎管线参数。
/// AI 依赖的部分（磨皮蒙版/追色 LUT/滤镜 LUT）需模型与资源加载，
/// 当前留空占位；祛瑕多边形栅格化已落地。
fn recipe_to_pipeline(recipe: &bus::Recipe, w: u32, h: u32) -> Pipeline {
    let mut p = Pipeline::default();

    // 基础调色（纯参数，直接映射）
    p.tone = ToneParams {
        exposure: recipe.base.exposure,
        contrast: recipe.base.contrast,
        saturation: recipe.base.hsl.saturation,
        temperature: recipe.base.temperature,
        tint: recipe.base.tint,
        curves: recipe
            .base
            .curves
            .iter()
            .map(|pt| CurvePoint { x: pt.x, y: pt.y })
            .collect(),
        grain: recipe.base.grain,
        vignette: recipe.base.vignette,
    };

    // 美型：用户手动拖拽控制点可直接映射为液化点
    if recipe.beauty.enabled {
        p.beauty_points = recipe
            .beauty
            .manual_points
            .iter()
            .map(|cp| LiquifyPoint {
                x: cp.x,
                y: cp.y,
                dx: cp.dx,
                dy: cp.dy,
                radius: 0.12,
            })
            .collect();
    }

    // 祛瑕：多边形栅格化为 mask（多区域合并）
    if !recipe.inpaint.is_empty() {
        let mut mask = ImageBuf::new(w, h, ColorSpace::Linear);
        for region in &recipe.inpaint {
            let poly: Vec<[f32; 2]> = region
                .polygon
                .iter()
                .map(|pt| [pt.x, pt.y])
                .collect();
            let m = polygon_to_mask(w, h, &poly);
            merge_mask(&mut mask, &m);
        }
        p.inpaint_mask = Some(mask);
    }

    // 滤镜：内置预设 LUT（warm/cool/bw/vivid）；.cube 文件加载后续接入。
    // 强度 0..1：向恒等 LUT 混合（修复导出与 UI 强度不一致）。
    if let Some(filter) = &recipe.filter {
        if let Some(lut) = builtin_filter_lut(&filter.lut_id) {
            p.filter_lut = Some(lut.blended(filter.intensity));
        }
    }

    // TODO(engine): 以下依赖 AI 模型 / 资源加载，接入后填充：
    // - neutral_gray：GAN 预测平整/立体蒙版 → Pipeline::neutral_gray
    // - color：语义分割 + Lab 迁移烘焙 → Pipeline::color_lut（已在 process_request 现场烘焙）

    p
}
