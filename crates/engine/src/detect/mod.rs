//! 检测算子（依赖 ONNX 推理）。
//!
//! - `face`：SCRFD 人脸检测（bbox + 5 点关键点）
//! - `landmark`：2DFAN4 68 点关键点（精细美型）

pub mod face;
pub mod landmark;
