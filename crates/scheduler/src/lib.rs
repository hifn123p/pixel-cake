//! `scheduler` — 预览/调度层（文档 §2 第②层）。
//!
//! 负责：任务队列（Tokio mpsc）、进度/结果事件广播（broadcast）、显存预算
//! （`GpuBudget`，文档 §5.3 的 8GB 上限约束）。UI 提交 `EngineRequest`，
//! 引擎按显存预算串行/有限并行执行，进度经 `EngineEvent` 回流前端。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bus::{EngineEvent, EngineRequest, PipelineStep, Scope};
use engine::base::tone::{CurvePoint, ToneParams};
use engine::export::encode_tiff;
use engine::pipeline::{process, Pipeline};
use engine::raw::decode_ppm;
use engine::retouch::beauty::LiquifyPoint;
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
    pub fn new() -> Self {
        Self::with_budget(VRAM_8GB)
    }

    pub fn with_budget(total_bytes: usize) -> Self {
        let (tx_req, mut rx_req) = mpsc::channel::<EngineRequest>(256);
        let (tx_evt, _) = broadcast::channel::<EngineEvent>(64);
        let budget = GpuBudget::new(total_bytes);

        let evt = tx_evt.clone();
        let bgt = budget.clone();
        tokio::spawn(async move {
            while let Some(req) = rx_req.recv().await {
                run_one(&evt, &bgt, req).await;
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
        Self::new()
    }
}

/// 单条请求的处理：按显存预算串行，解码 → 管线 → 导出，发进度与完成事件。
async fn run_one(evt: &broadcast::Sender<EngineEvent>, budget: &GpuBudget, req: EngineRequest) {
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
    let result = process_request(&req);

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
                proxy_updated: req.scope == Scope::Preview,
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

/// 执行一次重算：解码输入 → Recipe 转管线 → 16bit 处理 → TIFF 导出。
/// 返回导出文件路径。
fn process_request(req: &EngineRequest) -> Result<String, String> {
    // 1. 读文件 + 解码（PPM 占位；LibRaw 接入后按扩展名分发）
    let bytes = std::fs::read(&req.raw_path).map_err(|e| format!("读取原图失败: {e}"))?;
    let img = decode_ppm(&bytes).map_err(|e| format!("解码失败: {e}"))?;

    // 2. Recipe → Pipeline
    let pipeline = recipe_to_pipeline(&req.recipe);

    // 3. 16bit 全链路处理
    let result = process(&img, &pipeline);

    // 4. 导出 16bit TIFF
    let tiff = encode_tiff(&result);
    let out_path = format!("{}.out.tiff", req.raw_path);
    std::fs::write(&out_path, &tiff).map_err(|e| format!("写导出文件失败: {e}"))?;

    Ok(out_path)
}

/// 把 `bus::Recipe` 映射为引擎管线参数。
/// AI 依赖的部分（磨皮蒙版/追色 LUT/祛瑕 mask/滤镜 LUT）需模型与资源加载，
/// 当前留空占位，待接入后填充。
fn recipe_to_pipeline(recipe: &bus::Recipe) -> Pipeline {
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

    // TODO(engine): 以下依赖 AI 模型 / 资源加载，接入后填充：
    // - neutral_gray：GAN 预测平整/立体蒙版 → Pipeline::neutral_gray
    // - color：语义分割 + Lab 迁移烘焙 → Pipeline::color_lut
    // - inpaint：多边形栅格化为 mask → Pipeline::inpaint_mask
    // - filter：按 lut_id 加载 .cube → Pipeline::filter_lut

    p
}
