//! `engine` — 核心引擎层（文档 §2 第③层，重算）。
//!
//! 本 crate 承载全部重算力：16bit 图像处理管线、ONNX Runtime(CUDA) 推理、
//! 美型/祛瑕/追色算子。已落地 `image`（16bit 缓冲）、`color`（柔光/频率分离）、
//! `retouch`（中性灰磨皮骨架）；`raw` / `ort` / `base` / `gpu` 随里程碑补入。

pub mod color;
pub mod image;
pub mod retouch;
