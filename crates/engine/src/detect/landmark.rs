//! 2DFAN4 人脸关键点检测器（68 点，ONNX）。
//!
//! 后处理参考 FaceFusion `face_landmarker.py`：
//! 以 bbox 做相似仿射裁剪到 256×256（scale=195/bbox_size，bbox 中心对齐），
//! BGR + `/255` 归一化，输出 `landmarks [1,68,3]`（x/y 值域 0..64，实测验证），
//! `/64*256` 映射回 256 空间后逆仿射回原图坐标。
//!
//! 实测（Jonny001/Models-Pack-01 2dfan4.onnx）：输入名 `input`、输出名 `landmarks`，
//! 另有一冗余输出 `heatmaps [1,68,64,64]`（本模块不消费）。

use ndarray::Array4;
use ort::value::Tensor;

use crate::image::{linear_to_srgb, ImageBuf};
use crate::infer::InferSession;

/// 2DFAN4 关键点检测器（68 点）。
pub struct Landmarker {
    session: InferSession,
}

/// 2DFAN4 输入尺寸。
const MODEL_SIZE: u32 = 256;

impl Landmarker {
    /// 从 `.onnx` 模型文件创建检测器（CUDA EP）。
    pub fn new(model_path: &str) -> Result<Self, String> {
        Ok(Self {
            session: InferSession::from_file(model_path)?,
        })
    }

    /// 对 `bbox` 内的人脸做 68 点关键点检测，返回原图像素坐标。
    pub fn detect(&mut self, img: &ImageBuf, bbox: [f32; 4]) -> Result<[[f32; 2]; 68], String> {
        let (scale, translation) = face_affine_params(bbox, MODEL_SIZE);

        // 1. 相似仿射裁剪到 256×256
        let crop = warp_face(img, scale, translation, MODEL_SIZE);

        // 2. 预处理：BGR + /255（[0,1]）
        let tensor = landmark_preprocess(&crop)?;

        // 3. 前向（实测输出名为 `landmarks`，非 `landmarks_xyscore`）
        let outputs = self.session.run("input", tensor, &["landmarks"])?;
        let (_shape, data) = &outputs[0]; // [1, 68, 3]

        // 4. 后处理：x/y /64*256 → 逆仿射回原图
        let mut landmarks = [[0.0f32; 2]; 68];
        for i in 0..68 {
            let x64 = data[i * 3];
            let y64 = data[i * 3 + 1];
            let x256 = x64 / 64.0 * MODEL_SIZE as f32;
            let y256 = y64 / 64.0 * MODEL_SIZE as f32;
            landmarks[i] = [
                (x256 - translation[0]) / scale,
                (y256 - translation[1]) / scale,
            ];
        }
        Ok(landmarks)
    }
}

/// 计算 bbox → 256 裁剪的相似仿射参数。
///
/// 映射：`crop = src * scale + translation`（FaceFusion `warp_face_by_translation`），
/// 使 bbox 最长边缩放到 195 像素、bbox 中心对齐到 crop 中心。
fn face_affine_params(bbox: [f32; 4], size: u32) -> (f32, [f32; 2]) {
    let bbox_size = (bbox[2] - bbox[0]).max(bbox[3] - bbox[1]).max(1.0);
    let scale = 195.0 / bbox_size;
    let translation = [
        (size as f32 - (bbox[2] + bbox[0]) * scale) * 0.5,
        (size as f32 - (bbox[3] + bbox[1]) * scale) * 0.5,
    ];
    (scale, translation)
}

/// 相似仿射裁剪：`crop(x,y) = src((x - t)/scale, (y - t)/scale)`。
fn warp_face(img: &ImageBuf, scale: f32, translation: [f32; 2], crop_size: u32) -> ImageBuf {
    let mut dst = ImageBuf::new(crop_size, crop_size, img.space);
    for y in 0..crop_size {
        for x in 0..crop_size {
            let sx = (x as f32 - translation[0]) / scale;
            let sy = (y as f32 - translation[1]) / scale;
            dst.set_pixel(x, y, sample_bilinear(img, sx, sy));
        }
    }
    dst
}

/// 双线性采样（像素坐标，边界 clamp）。
fn sample_bilinear(img: &ImageBuf, x: f32, y: f32) -> [f32; 4] {
    let x = x.clamp(0.0, img.width as f32 - 1.0);
    let y = y.clamp(0.0, img.height as f32 - 1.0);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(img.width - 1);
    let y1 = (y0 + 1).min(img.height - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let p00 = img.pixel(x0, y0);
    let p10 = img.pixel(x1, y0);
    let p01 = img.pixel(x0, y1);
    let p11 = img.pixel(x1, y1);
    let mut out = [0.0f32; 4];
    for i in 0..4 {
        let top = p00[i] * (1.0 - fx) + p10[i] * fx;
        let bot = p01[i] * (1.0 - fx) + p11[i] * fx;
        out[i] = top * (1.0 - fy) + bot * fy;
    }
    out
}

/// 2DFAN4 预处理：BGR 顺序 + 线性转 sRGB（[0,1]）。
fn landmark_preprocess(img: &ImageBuf) -> Result<Tensor<f32>, String> {
    let size = MODEL_SIZE as usize;
    let mut data = Vec::with_capacity(size * size * 3);
    for &ch in &[2usize, 1, 0] {
        for y in 0..MODEL_SIZE {
            for x in 0..MODEL_SIZE {
                let v = img.pixel(x, y)[ch];
                data.push(linear_to_srgb(v.clamp(0.0, 1.0)));
            }
        }
    }
    let arr = Array4::from_shape_vec((1, 3, size, size), data).map_err(|e| e.to_string())?;
    Tensor::from_array(arr).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ColorSpace;

    #[test]
    fn affine_params_center_bbox() {
        // bbox [100,100,200,200]（100×100），应缩放到 195 并居中到 256
        let (scale, t) = face_affine_params([100.0, 100.0, 200.0, 200.0], 256);
        assert!((scale - 1.95).abs() < 1e-4);
        // bbox 中心 (150,150) 映射到 crop 中心 (128,128)
        let cx = 150.0 * scale + t[0];
        let cy = 150.0 * scale + t[1];
        assert!((cx - 128.0).abs() < 1e-3);
        assert!((cy - 128.0).abs() < 1e-3);
    }

    #[test]
    fn warp_face_identity() {
        // scale=1, translation=0 → 裁剪等于原图（尺寸相同时）
        let mut src = ImageBuf::new(4, 4, ColorSpace::Linear);
        for y in 0..4 {
            for x in 0..4 {
                src.set_pixel(x, y, [x as f32 / 3.0, y as f32 / 3.0, 0.0, 1.0]);
            }
        }
        let dst = warp_face(&src, 1.0, [0.0, 0.0], 4);
        assert_eq!(dst.pixel(2, 2), src.pixel(2, 2));
    }
}
