//! `engine` — 核心引擎层（文档 §2 第③层，重算）。
//!
//! 本 crate 承载全部重算力：16bit 图像处理管线、ONNX Runtime(CUDA) 推理、
//! 美型/祛瑕/追色算子。已落地 `image`（16bit 缓冲）、`color`（柔光/频率分离/LUT）、
//! `retouch`（磨皮/美型/祛瑕/追色）、`base`（基础调色）、`pipeline`（全链路）、
//! `export`（16bit TIFF）。`raw` / `ort` / `gpu` 随模型与 FFI 接入补入。

pub mod base;
pub mod color;
pub mod export;
pub mod image;
pub mod pipeline;
pub mod retouch;
