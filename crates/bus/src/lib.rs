//! `bus` — 全程编辑协议（UI ⇄ 引擎 ⇄ 存储共享）。
//!
//! `Recipe` 是 UI 与核心引擎之间唯一的编辑契约：预览层用它在代理图上即时渲染，
//! 引擎层用它在 16bit 全链路重算，存储层把它序列化为 JSON 落 SQLite。
//! 所有类型实现 `Serialize + Deserialize`，可直接 `serde_json` 往返。

use serde::{Deserialize, Serialize};

/// 中性灰磨皮参数（文档 §4.3）：双参数 ka(平整)、kb(立体)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NeutralGray {
    pub enabled: bool,
    /// 平整强度 0–100。
    pub ka: u8,
    /// 立体强度 0–100。
    pub kb: u8,
    pub mode: NeutralGrayMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NeutralGrayMode {
    /// 平整 + 立体 双蒙版合成（先平整后立体）。
    Dual,
    /// 仅平整（纯磨皮）。
    FlatOnly,
    /// 仅立体（只保留光影结构）。
    StructureOnly,
}

/// AI 美型 / 液化参数（文档 §4.4）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Beauty {
    pub enabled: bool,
    /// 瘦脸 0–100。
    pub face_slim: f32,
    /// 瘦身 0–100。
    pub body_slim: f32,
    /// 天鹅颈 0–100。
    pub neck_slim: f32,
    /// 面部丰盈 0–100。
    pub face_full: f32,
    /// 五官微调（用户拖拽控制点的局部位移，归一化坐标）。
    pub manual_points: Vec<ControlPoint>,
}

/// 用户手动拖拽的控制点（归一化 [0,1] 坐标 + 位移向量）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ControlPoint {
    pub x: f32,
    pub y: f32,
    pub dx: f32,
    pub dy: f32,
}

/// 祛瑕区域（文档 §4.5）：mask 以多边形坐标存 recipe。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InpaintRegion {
    /// 归一化多边形顶点（用户画笔描边采集）。
    pub polygon: Vec<Point>,
    pub kind: InpaintKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InpaintKind {
    /// 祛痣/祛斑。
    Blemish,
    /// 祛纹身。
    Tattoo,
    /// 背景瑕疵消除。
    Background,
    /// 牙齿修复。
    Teeth,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// AI 追色 / 调色参数（文档 §4.6）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub enabled: bool,
    /// 烘焙后的 3D LUT 引用（存储在本地目录，此处存路径/ID）。
    pub lut_ref: Option<String>,
    /// 分区独立追色强度 0–1。
    pub per_region_strength: f32,
    /// 追色模式。
    pub mode: ColorTransferMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorTransferMode {
    /// 极致（更强迁移）。
    Extreme,
    /// 和谐（保守迁移）。
    Harmony,
}

/// 基础调色参数（文档 §4.7）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Base {
    /// 曝光 EV，-5..5。
    pub exposure: f32,
    /// 对比度 -100..100。
    pub contrast: f32,
    /// 白平衡色温 -100..100。
    pub temperature: f32,
    /// 白平衡色调 -100..100。
    pub tint: f32,
    /// 曲线（若干锚点，归一化）。
    pub curves: Vec<Point>,
    pub hsl: Hsl,
    /// 颗粒 0–100。
    pub grain: f32,
    /// 暗角 0–100。
    pub vignette: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hsl {
    pub hue: f32,
    pub saturation: f32,
    pub lightness: f32,
}

/// 滤镜（文档 §4.7：70+ 滤镜，插件化 .cube / 参数 JSON）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    /// 滤镜/预设 LUT ID。
    pub lut_id: String,
    /// 强度 0–1。
    pub intensity: f32,
}

/// 完整编辑协议（文档 §7）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    pub neutral_gray: NeutralGray,
    pub beauty: Beauty,
    pub inpaint: Vec<InpaintRegion>,
    pub color: Color,
    pub base: Base,
    pub filter: Option<Filter>,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            neutral_gray: NeutralGray {
                enabled: false,
                ka: 0,
                kb: 0,
                mode: NeutralGrayMode::Dual,
            },
            beauty: Beauty {
                enabled: false,
                face_slim: 0.0,
                body_slim: 0.0,
                neck_slim: 0.0,
                face_full: 0.0,
                manual_points: Vec::new(),
            },
            inpaint: Vec::new(),
            color: Color {
                enabled: false,
                lut_ref: None,
                per_region_strength: 1.0,
                mode: ColorTransferMode::Harmony,
            },
            base: Base {
                exposure: 0.0,
                contrast: 0.0,
                temperature: 0.0,
                tint: 0.0,
                curves: Vec::new(),
                hsl: Hsl {
                    hue: 0.0,
                    saturation: 0.0,
                    lightness: 0.0,
                },
                grain: 0.0,
                vignette: 0.0,
            },
            filter: None,
        }
    }
}

/// 重算范围（文档 §7）：代理图预览 or 全分辨率导出。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    Preview,
    Export,
}

/// UI → 引擎：提交一次重算（文档 §7）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineRequest {
    pub photo_id: String,
    /// 原始文件路径（RAW/JPEG/代理图），引擎据此解码输入。
    pub raw_path: String,
    pub recipe: Recipe,
    pub scope: Scope,
}

/// 引擎 → UI：进度/结果事件（文档 §7）。
///
/// 以 `type` 字段判别（`progress` / `done` / `error`），便于前端监听。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    Progress {
        photo_id: String,
        step: PipelineStep,
        pct: f32,
    },
    Done {
        photo_id: String,
        result_path: Option<String>,
        proxy_updated: bool,
    },
    Error {
        photo_id: String,
        code: ErrorCode,
        message: String,
    },
}

/// 16bit 管线步骤（用于进度反馈，文档 §5.2 算子链）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineStep {
    RawDecode,
    NeutralGray,
    BeautyWarp,
    Inpaint,
    ColorLut,
    BaseTone,
    Filter,
    Encode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    Io,
    Decode,
    Inference,
    OutOfMemory,
    CudaUnavailable,
    InvalidRecipe,
}

impl Recipe {
    /// 序列化为 JSON（存储层直接落 SQLite `recipe.data` 列）。
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Recipe 序列化不应失败")
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}
