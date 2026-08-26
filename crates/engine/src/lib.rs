//! `engine` — 核心引擎层（文档 §2 第③层，重算）。
//!
//! 本 crate 承载全部重算力：16bit 图像处理管线、ONNX Runtime(CUDA) 推理、
//! 美型/祛瑕/追色算子。已落地 `image`（16bit 缓冲）、`color`（柔光/频率分离/LUT）、
//! `retouch`（磨皮/美型/祛瑕/追色）、`base`（基础调色）、`pipeline`（全链路）、
//! `export`（16bit TIFF）、`raw`（LibRaw 解码）、`infer`（ORT 推理封装）、
//! `detect`（人脸检测）、`ai`（AI 门面）。

pub mod ai;
pub mod base;
pub mod color;
pub mod detect;
pub mod export;
pub mod image;
pub mod infer;
pub mod pipeline;
pub mod raw;
pub mod retouch;
