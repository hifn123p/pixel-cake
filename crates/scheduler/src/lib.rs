//! `scheduler` — 预览/调度层（文档 §2 第②层）。
//!
//! 负责：任务队列（Tokio mpsc）、进度/结果事件广播（broadcast）、显存预算
//! （`GpuBudget`，文档 §5.3 的 8GB 上限约束）。UI 提交 `EngineRequest`，
//! 引擎按显存预算串行/有限并行执行，进度经 `EngineEvent` 回流前端。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bus::{EngineEvent, EngineRequest, PipelineStep, Scope};
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

/// 单条请求的处理：按显存预算串行，发进度与完成事件。
///
/// 当前为 M1 骨架占位——真实推理在 `engine` crate 接入后替换
/// `// TODO(engine)` 处的调用，预算由各算子的 `OpCost::vram_bytes` 驱动。
async fn run_one(evt: &broadcast::Sender<EngineEvent>, budget: &GpuBudget, req: EngineRequest) {
    let photo_id = req.photo_id.clone();

    // 占位：估算本次重算峰值显存（后续由 engine 算子 cost 计算）。
    let est_vram = 512 * 512 * 4 * 4;
    if !budget.try_acquire(est_vram) {
        // 显存不足 → 等待（简化：直接发错误；真实实现应入队等待）。
        let _ = evt.send(EngineEvent::Error {
            photo_id: photo_id.clone(),
            code: bus::ErrorCode::OutOfMemory,
            message: "显存不足，请稍后重试".into(),
        });
        return;
    }

    let steps = match req.scope {
        Scope::Preview => {
            // 代理图只跑轻算子链，不经 AI 推理。
            vec![PipelineStep::BaseTone, PipelineStep::Filter, PipelineStep::Encode]
        }
        Scope::Export => vec![
            PipelineStep::RawDecode,
            PipelineStep::NeutralGray,
            PipelineStep::BeautyWarp,
            PipelineStep::Inpaint,
            PipelineStep::ColorLut,
            PipelineStep::BaseTone,
            PipelineStep::Filter,
            PipelineStep::Encode,
        ],
    };

    let total = steps.len() as f32;
    for (i, step) in steps.into_iter().enumerate() {
        let pct = (i as f32 / total) * 100.0;
        // TODO(engine): 此处调用 engine 管线的对应算子。
        let _ = evt.send(EngineEvent::Progress {
            photo_id: photo_id.clone(),
            step,
            pct,
        });
    }

    budget.release(est_vram);
    let _ = evt.send(EngineEvent::Done {
        photo_id,
        result_path: None, // TODO(engine): 由导出模块写入
        proxy_updated: req.scope == Scope::Preview,
    });
}
