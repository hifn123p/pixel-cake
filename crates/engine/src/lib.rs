//! `engine` — 核心引擎层（文档 §2 第③层，重算）。
//!
//! 本 crate 承载全部重算力：16bit 图像处理管线、ONNX Runtime(CUDA) 推理、
//! 美型/祛瑕/追色算子。当前先落地 16bit 图像缓冲与算子抽象（`image` 模块），
//! 其余模块（`raw` / `ort` / `retouch` / `base` / `gpu`）随里程碑逐步补入。

pub mod image;
