//! BiSeNet 人脸解析（19 类语义分割，ONNX）。
//!
//! 后处理参考 FaceFusion `face_parser` / UniFace `bisenet`：
//! bbox 裁剪 → resize 512² → RGB + `(x-mean)/std`（ImageNet mean/std）→
//! 前向输出 `[1,19,512,512]` logits → argmax 得类别 → resize 回原图。
//!
//! 用于追色分区（皮肤/发/唇等区域做 Lab 迁移）。
//!
//! 注意：输入输出约定为 BiSeNet 标准，需模型文件就位后验证微调。

use ndarray::Array4;
use ort::value::Tensor;

use crate::image::{linear_to_srgb, ColorSpace, ImageBuf};
use crate::infer::{resize_bilinear, InferSession};

/// 类别常量（BiSeNet 19 类，dlib/FAN 约定）。
pub const CLASS_SKIN: u8 = 1;
pub const CLASS_HAIR: u8 = 17;
pub const CLASS_UPPER_LIP: u8 = 12;
pub const CLASS_LOWER_LIP: u8 = 13;

/// 分割结果：原图尺寸的类别 mask（每像素 0..18）。
#[derive(Debug, Clone, PartialEq)]
pub struct SegMask {
    pub width: u32,
    pub height: u32,
    /// 行优先类别索引。
    pub classes: Vec<u8>,
}

impl SegMask {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            classes: vec![0; (width as usize) * (height as usize)],
        }
    }

    #[inline]
    pub fn class_at(&self, x: u32, y: u32) -> u8 {
        self.classes[(y as usize) * (self.width as usize) + (x as usize)]
    }
}

/// BiSeNet 人脸解析器。
pub struct Segmenter {
    session: InferSession,
}

/// BiSeNet 输入尺寸。
const MODEL_SIZE: u32 = 512;
/// 类别数。
const NUM_CLASSES: usize = 19;

impl Segmenter {
    /// 从 `.onnx` 模型文件创建解析器（CUDA EP）。
    pub fn new(model_path: &str) -> Result<Self, String> {
        Ok(Self {
            session: InferSession::from_file_cuda(model_path)?,
        })
    }

    /// 对 `bbox` 内的人脸做 19 类分割，返回原图尺寸的类别 mask。
    pub fn segment(&mut self, img: &ImageBuf, bbox: [f32; 4]) -> Result<SegMask, String> {
        // 1. bbox 裁剪（clamp 到图像边界）
        let x1 = bbox[0].floor().max(0.0) as u32;
        let y1 = bbox[1].floor().max(0.0) as u32;
        let x2 = bbox[2].ceil().min(img.width as f32) as u32;
        let y2 = bbox[3].ceil().min(img.height as f32) as u32;
        if x2 <= x1 || y2 <= y1 {
            return Ok(SegMask::new(img.width, img.height));
        }
        let crop_w = x2 - x1;
        let crop_h = y2 - y1;
        let crop = crop_region(img, x1, y1, crop_w, crop_h);

        // 2. resize 到 512² 并预处理
        let resized = resize_bilinear(&crop, MODEL_SIZE, MODEL_SIZE);
        let tensor = segment_preprocess(&resized)?;

        // 3. 前向
        let outputs = self.session.run("input", tensor, &["output"])?;
        let (_shape, data) = &outputs[0]; // [1, 19, 512, 512]

        // 4. argmax → 512² 类别
        let small = argmax_mask(data, MODEL_SIZE as usize, MODEL_SIZE as usize);

        // 5. resize 回 crop 尺寸（最近邻，保持类别）
        let crop_mask = SegMask {
            width: crop_w,
            height: crop_h,
            classes: resize_nearest(&small, MODEL_SIZE, MODEL_SIZE, crop_w, crop_h),
        };

        // 6. 放回原图坐标（其余区域 background=0）
        let mut full = SegMask::new(img.width, img.height);
        for y in 0..crop_h {
            for x in 0..crop_w {
                let v = crop_mask.classes[(y * crop_w + x) as usize];
                full.classes[((y1 + y) * img.width + (x1 + x)) as usize] = v;
            }
        }
        Ok(full)
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

/// BiSeNet 预处理：RGB + `(x-mean)/std`（ImageNet 归一化）。
fn segment_preprocess(img: &ImageBuf) -> Result<Tensor<f32>, String> {
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];
    let size = MODEL_SIZE as usize;
    let mut data = Vec::with_capacity(size * size * 3);
    for c in 0..3 {
        for y in 0..MODEL_SIZE {
            for x in 0..MODEL_SIZE {
                let srgb = linear_to_srgb(img.pixel(x, y)[c].clamp(0.0, 1.0));
                data.push((srgb - MEAN[c]) / STD[c]);
            }
        }
    }
    let arr = Array4::from_shape_vec((1, 3, size, size), data).map_err(|e| e.to_string())?;
    Tensor::from_array(arr).map_err(|e| e.to_string())
}

/// argmax 得类别 mask（输入 `[1, C, H, W]` 展平，行优先）。
fn argmax_mask(data: &[f32], h: usize, w: usize) -> Vec<u8> {
    let mut mask = vec![0u8; h * w];
    for y in 0..h {
        for x in 0..w {
            let mut best = 0u8;
            let mut best_v = f32::MIN;
            for c in 0..NUM_CLASSES {
                let v = data[(c * h + y) * w + x];
                if v > best_v {
                    best_v = v;
                    best = c as u8;
                }
            }
            mask[y * w + x] = best;
        }
    }
    mask
}

/// 最近邻 resize（用于类别 mask）。
fn resize_nearest(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut dst = vec![0u8; (dw * dh) as usize];
    for y in 0..dh {
        for x in 0..dw {
            let sx = (x as f32 / dw as f32 * sw as f32) as u32;
            let sy = (y as f32 / dh as f32 * sh as f32) as u32;
            dst[(y * dw + x) as usize] = src[(sy * sw + sx) as usize];
        }
    }
    dst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argmax_picks_max_class() {
        // [1, 3, 1, 1]：3 类，单像素，logit [0.1, 0.9, 0.5] → 类别 1
        let data = vec![0.1, 0.9, 0.5];
        let mask = argmax_mask(&data, 1, 1);
        assert_eq!(mask[0], 1);
    }

    #[test]
    fn resize_nearest_upscales() {
        // 2x2 → 4x4 最近邻
        let src = vec![1u8, 2, 3, 4];
        let dst = resize_nearest(&src, 2, 2, 4, 4);
        assert_eq!(dst.len(), 16);
        assert_eq!(dst[0], 1); // (0,0) → src(0,0)
        assert_eq!(dst[2 * 4 + 0], 3); // (0,2) → src(0,1)
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
        assert_eq!(crop.width, 2);
        assert_eq!(crop.pixel(0, 0), img.pixel(1, 1));
        assert_eq!(crop.pixel(1, 1), img.pixel(2, 2));
    }
}
