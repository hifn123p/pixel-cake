//! GPEN 人脸皮肤增强（盲修复/磨皮，ONNX）。
//!
//! 后处理参考 FaceFusion `face_enhancer` / roop `Enhance_GPEN`：
//! bbox 裁剪 → resize 512² → RGB + `x*2-1`（[-1,1]）→ 前向输出增强人脸 →
//! `(x+1)/2` 回 [0,1] sRGB → 线性 → resize 回 crop → blend 融合回原图。
//!
//! 用作「磨皮」的皮肤增强替代（专有中性灰 GAN 的开源等价物）。
//!
//! 注意：输入输出约定为 GPEN 标准，需模型文件就位后验证微调。

use ndarray::Array4;
use ort::value::Tensor;

use crate::image::{linear_to_srgb, srgb_to_linear, ColorSpace, ImageBuf};
use crate::infer::{resize_bilinear, InferSession};

/// GPEN 人脸增强器。
pub struct Enhancer {
    session: InferSession,
}

/// GPEN 输入尺寸。
const MODEL_SIZE: u32 = 512;

impl Enhancer {
    /// 从 `.onnx` 模型文件创建增强器（CUDA EP）。
    pub fn new(model_path: &str) -> Result<Self, String> {
        Ok(Self {
            session: InferSession::from_file_cuda(model_path)?,
        })
    }

    /// 增强 `bbox` 内的人脸皮肤，返回增强后的全图（bbox 区域按 `blend` 融合）。
    ///
    /// `blend` 为增强结果权重 0..1（1 = 全增强，0 = 原图）。
    pub fn enhance(
        &mut self,
        img: &ImageBuf,
        bbox: [f32; 4],
        blend: f32,
    ) -> Result<ImageBuf, String> {
        // 1. bbox 裁剪（clamp 到图像边界）
        let x1 = bbox[0].floor().max(0.0) as u32;
        let y1 = bbox[1].floor().max(0.0) as u32;
        let x2 = bbox[2].ceil().min(img.width as f32) as u32;
        let y2 = bbox[3].ceil().min(img.height as f32) as u32;
        if x2 <= x1 || y2 <= y1 {
            return Ok(img.clone());
        }
        let crop_w = x2 - x1;
        let crop_h = y2 - y1;
        let crop = crop_region(img, x1, y1, crop_w, crop_h);

        // 2. resize 512² + 预处理
        let resized = resize_bilinear(&crop, MODEL_SIZE, MODEL_SIZE);
        let tensor = enhance_preprocess(&resized)?;

        // 3. 前向
        let outputs = self.session.run("input", tensor, &["output"])?;
        let (_shape, data) = &outputs[0]; // [1, 3, 512, 512]

        // 4. 后处理 → 线性 RGB 512 图像
        let enhanced_512 = enhance_postprocess(data, MODEL_SIZE, MODEL_SIZE);

        // 5. resize 回 crop 尺寸
        let enhanced_crop = resize_bilinear(&enhanced_512, crop_w, crop_h);

        // 6. 贴回原图（blend 融合，仅 bbox 区域）
        let blend = blend.clamp(0.0, 1.0);
        let mut out = img.clone();
        for y in 0..crop_h {
            for x in 0..crop_w {
                let o = img.pixel(x1 + x, y1 + y);
                let e = enhanced_crop.pixel(x, y);
                out.set_pixel(
                    x1 + x,
                    y1 + y,
                    [
                        o[0] * (1.0 - blend) + e[0] * blend,
                        o[1] * (1.0 - blend) + e[1] * blend,
                        o[2] * (1.0 - blend) + e[2] * blend,
                        o[3],
                    ],
                );
            }
        }
        Ok(out)
    }
}

/// bbox 区域裁剪。
fn crop_region(img: &ImageBuf, x1: u32, y1: u32, w: u32, h: u32) -> ImageBuf {
    let mut dst = ImageBuf::new(w, h, img.space);
    for y in 0..h {
        for x in 0..w {
            dst.set_pixel(x, y, img.pixel(x1 + x, y1 + y));
        }
    }
    dst
}

/// GPEN 预处理：RGB + 线性转 sRGB + `x*2-1`（[-1,1]）。
fn enhance_preprocess(img: &ImageBuf) -> Result<Tensor<f32>, String> {
    let size = MODEL_SIZE as usize;
    let mut data = Vec::with_capacity(size * size * 3);
    for c in 0..3 {
        for y in 0..MODEL_SIZE {
            for x in 0..MODEL_SIZE {
                let srgb = linear_to_srgb(img.pixel(x, y)[c].clamp(0.0, 1.0));
                data.push(srgb * 2.0 - 1.0);
            }
        }
    }
    let arr = Array4::from_shape_vec((1, 3, size, size), data).map_err(|e| e.to_string())?;
    Tensor::from_array(arr).map_err(|e| e.to_string())
}

/// GPEN 后处理：`(x+1)/2` 回 [0,1] sRGB → 线性 RGB。
fn enhance_postprocess(data: &[f32], h: u32, w: u32) -> ImageBuf {
    let mut img = ImageBuf::new(w, h, ColorSpace::Linear);
    for y in 0..h {
        for x in 0..w {
            let mut rgb = [0.0f32; 3];
            for c in 0..3 {
                let v = data[(c * h as usize + y as usize) * w as usize + x as usize];
                let srgb = (v.clamp(-1.0, 1.0) + 1.0) / 2.0;
                rgb[c] = srgb_to_linear(srgb);
            }
            img.set_pixel(x, y, [rgb[0], rgb[1], rgb[2], 1.0]);
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postprocess_maps_range() {
        // 输入 -1 → 0（黑），+1 → 1（白），0 → 0.5 灰度
        let data = [-1.0f32, 0.0, 1.0];
        let img = enhance_postprocess(&data, 1, 1);
        assert!(img.pixel(0, 0)[0] < 1e-4); // -1 → 黑
        assert!(img.pixel(0, 0)[1] < 1e-4);
        assert!(img.pixel(0, 0)[2] < 1e-4);
    }

    #[test]
    fn crop_region_copies() {
        let mut img = ImageBuf::new(4, 4, ColorSpace::Linear);
        for y in 0..4 {
            for x in 0..4 {
                img.set_pixel(x, y, [x as f32 / 3.0, y as f32 / 3.0, 0.0, 1.0]);
            }
        }
        let crop = crop_region(&img, 1, 1, 2, 2);
        assert_eq!(crop.pixel(0, 0), img.pixel(1, 1));
        assert_eq!(crop.pixel(1, 1), img.pixel(2, 2));
    }
}
