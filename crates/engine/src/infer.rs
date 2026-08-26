//! ONNX Runtime 推理封装（CUDA EP）。
//!
//! 作为人脸检测 / 语义分割 / 关键点 / 磨皮等 AI 算子的共用基础设施：
//! 封装 Session 加载（CUDA 执行提供者）、输入/输出张量转换，
//! 以及 `ImageBuf`（线性 RGB 16bit）→ NCHW `f32` 张量的预处理。
//!
//! 依赖 `ort` crate（默认含 `download-binaries`：构建时下载 ORT 二进制，
//! `copy-dylibs`：复制 `onnxruntime*.dll` 到 target；`cuda` feature 启用 CUDA EP）。
//!
//! 注意：CUDA EP 需运行时环境满足 CUDA ≥ 13.2 + cuDNN ≥ 9.23（见 ort 文档）。

use ndarray::Array4;
use ort::session::Session;
use ort::{ep, inputs, Tensor};

use crate::image::{linear_to_srgb, ColorSpace, ImageBuf};

/// 推理会话：持有 ONNX Runtime Session（注册 CUDA 执行提供者）。
pub struct InferSession {
    session: Session,
}

impl InferSession {
    /// 从 `.onnx` 文件加载模型，注册 CUDA 执行提供者。
    ///
    /// 注册失败时 ort 默认静默回退 CPU；若希望失败即报错，可改用
    /// `ep::CUDA::default().build().error_on_failure()`。
    pub fn from_file_cuda(path: &str) -> Result<Self, String> {
        let session = Session::builder()
            .map_err(|e| e.to_string())?
            .with_execution_providers([ep::CUDA::default().build()])
            .map_err(|e| e.to_string())?
            .commit_from_file(path)
            .map_err(|e| e.to_string())?;
        Ok(Self { session })
    }

    /// 单输入推理：输入一个命名 `f32` 张量，返回各命名输出的（shape, 展平数据）。
    ///
    /// 输出按 `output_names` 顺序返回，每个元素为 `(维度列表, 行优先展平的 f32)`，
    /// 由调用方按模型约定做后处理（解码 bbox / 关键点 / 蒙版等）。
    pub fn run(
        &self,
        input_name: &str,
        tensor: Tensor<f32>,
        output_names: &[&str],
    ) -> Result<Vec<(Vec<usize>, Vec<f32>)>, String> {
        let outputs = self
            .session
            .run(inputs![input_name => tensor])
            .map_err(|e| e.to_string())?;

        let mut result = Vec::with_capacity(output_names.len());
        for &name in output_names {
            let arr = outputs[name]
                .try_extract_array::<f32>()
                .map_err(|e| format!("提取输出 {name}: {e}"))?;
            let shape = arr.shape().to_vec();
            let data = arr.iter().copied().collect::<Vec<f32>>();
            result.push((shape, data));
        }
        Ok(result)
    }
}

/// 把 `ImageBuf` 双线性 resize 到 `(w, h)`，输出 NCHW `f32` 张量数组。
///
/// 预处理约定：线性 RGB → sRGB → clamp 到 `[0, 1]`，RGB 顺序（绝大多数
/// 视觉模型在 sRGB 域训练）。如需其他归一化（如 `[-1, 1]`）由调用方再变换。
pub fn image_to_nchw(img: &ImageBuf, w: u32, h: u32) -> Result<Array4<f32>, String> {
    let resized = resize_bilinear(img, w, h);
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for c in 0..3 {
        for y in 0..h {
            for x in 0..w {
                let v = resized.pixel(x, y)[c];
                let srgb = linear_to_srgb(v.clamp(0.0, 1.0));
                data.push(srgb);
            }
        }
    }
    Array4::from_shape_vec((1, 3, h as usize, w as usize), data).map_err(|e| e.to_string())
}

/// 双线性 resize（保持 RGBA 四通道，色彩空间不变）。
fn resize_bilinear(src: &ImageBuf, dst_w: u32, dst_h: u32) -> ImageBuf {
    let mut dst = ImageBuf::new(dst_w, dst_h, src.space);
    if src.width == 0 || src.height == 0 {
        return dst;
    }
    let sx = src.width as f32 / dst_w as f32;
    let sy = src.height as f32 / dst_h as f32;

    for y in 0..dst_h {
        for x in 0..dst_w {
            let fx = ((x as f32 + 0.5) * sx - 0.5).max(0.0);
            let fy = ((y as f32 + 0.5) * sy - 0.5).max(0.0);
            let x0 = fx.floor() as u32;
            let y0 = fy.floor() as u32;
            let x1 = (x0 + 1).min(src.width - 1);
            let y1 = (y0 + 1).min(src.height - 1);
            let tx = fx - x0 as f32;
            let ty = fy - y0 as f32;

            let mut out = [0.0; 4];
            for c in 0..4 {
                let p00 = src.pixel(x0, y0)[c];
                let p10 = src.pixel(x1, y0)[c];
                let p01 = src.pixel(x0, y1)[c];
                let p11 = src.pixel(x1, y1)[c];
                out[c] = p00 * (1.0 - tx) * (1.0 - ty)
                    + p10 * tx * (1.0 - ty)
                    + p01 * (1.0 - tx) * ty
                    + p11 * tx * ty;
            }
            dst.set_pixel(x, y, out);
        }
    }
    dst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_identity_keeps_pixels() {
        let mut src = ImageBuf::new(2, 2, ColorSpace::Linear);
        src.set_pixel(0, 0, [0.0, 0.0, 0.0, 1.0]);
        src.set_pixel(1, 0, [1.0, 1.0, 1.0, 1.0]);
        src.set_pixel(0, 1, [0.5, 0.5, 0.5, 1.0]);
        src.set_pixel(1, 1, [0.25, 0.25, 0.25, 1.0]);

        let dst = resize_bilinear(&src, 2, 2);
        assert_eq!(dst.pixel(0, 0)[0], 0.0);
        assert_eq!(dst.pixel(1, 0)[0], 1.0);
    }

    #[test]
    fn resize_upscale_averages() {
        // 2x2 纯色 → 4x4 应保持纯色（双线性插值在纯色下不变）
        let mut src = ImageBuf::new(2, 2, ColorSpace::Linear);
        for y in 0..2 {
            for x in 0..2 {
                src.set_pixel(x, y, [0.3, 0.6, 0.9, 1.0]);
            }
        }
        let dst = resize_bilinear(&src, 4, 4);
        let p = dst.pixel(2, 2);
        assert!((p[0] - 0.3).abs() < 1e-5);
        assert!((p[1] - 0.6).abs() < 1e-5);
        assert!((p[2] - 0.9).abs() < 1e-5);
    }

    #[test]
    fn image_to_nchw_shape_and_order() {
        // 1x1 纯红（线性 1.0）→ 2x2，应输出 [1,3,2,2]，R 通道≈1，G/B≈0
        let mut src = ImageBuf::new(1, 1, ColorSpace::Linear);
        src.set_pixel(0, 0, [1.0, 0.0, 0.0, 1.0]);
        let arr = image_to_nchw(&src, 2, 2).unwrap();
        assert_eq!(arr.shape(), &[1, 3, 2, 2]);
        assert!(arr[[0, 0, 0, 0]] > 0.99); // R
        assert!(arr[[0, 1, 0, 0]] < 0.01); // G
        assert!(arr[[0, 2, 0, 0]] < 0.01); // B
    }
}
