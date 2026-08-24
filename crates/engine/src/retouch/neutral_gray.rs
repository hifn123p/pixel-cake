//! 中性灰磨皮数据流（文档 §4.3）。
//!
//! 链路：人脸检测 → 裁剪缩放 512² → GAN 预测平整/立体中性灰蒙版（ONNX）→
//! 蒙版上采样回原图 → 16bit 柔光合成（先平整后立体）。
//!
//! AI 部分（检测、GAN 预测）依赖 ONNX 模型，当前以 trait 抽象占位，
//! 柔光合成与频率分离已完整落地，可独立测试。

use crate::color::softlight::NeutralGrayCompose;
use crate::image::{ImageBuf, ImageOp};

/// 人脸检测框（归一化坐标 0..1）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// 中性灰蒙版预测器：输入 512² 人脸，输出 `(平整 A, 立体 B)` 中性灰蒙版。
///
/// 生产实现为 ONNX Runtime(CUDA) 推理（文档 §5.1 模型链路），
/// 模型接入后替换 `predict` 内部即可，上层数据流不变。
pub trait MaskPredictor: Send + Sync {
    fn predict(&self, face_512: &ImageBuf) -> (ImageBuf, ImageBuf);
}

/// 中性灰磨皮（文档 §4.3）。
pub struct NeutralGrayRetouch {
    predictor: Box<dyn MaskPredictor>,
}

impl NeutralGrayRetouch {
    pub fn new(predictor: Box<dyn MaskPredictor>) -> Self {
        Self { predictor }
    }

    /// 完整磨皮流程。`ka`(平整)/`kb`(立体) 为 0..100 强度。
    pub fn apply(&self, src: &ImageBuf, ka: u8, kb: u8) -> ImageBuf {
        // TODO(M2): 接入人脸检测模型后，用检测框裁剪；当前以全图为检测框占位。
        let face_512 = resize(src, 512, 512);

        // GAN 预测中性灰蒙版 A/B
        let (mask_a, mask_b) = self.predictor.predict(&face_512);

        // 蒙版上采样回原图尺寸
        let a_full = resize(&mask_a, src.width, src.height);
        let b_full = resize(&mask_b, src.width, src.height);

        // 16bit 柔光合成
        let mut dst = ImageBuf::new(src.width, src.height, src.space);
        NeutralGrayCompose::new(a_full, b_full, ka, kb).apply(src, &mut dst);
        dst
    }
}

/// 最近邻缩放（正确性优先；生产可替换为双线性或 GPU 上采样）。
fn resize(src: &ImageBuf, w: u32, h: u32) -> ImageBuf {
    let mut out = ImageBuf::new(w, h, src.space);
    for c in 0..4 {
        let sp = src.plane(c);
        let op = out.plane_mut(c);
        for y in 0..h {
            let sy = ((y as f32 * src.height as f32) / h as f32) as u32;
            let sy = sy.min(src.height - 1);
            for x in 0..w {
                let sx = ((x as f32 * src.width as f32) / w as f32) as u32;
                let sx = sx.min(src.width - 1);
                op[(y * w + x) as usize] = sp[(sy * src.width + sx) as usize];
            }
        }
    }
    out
}

/// 测试/兜底预测器：输出中性灰 0.5 蒙版（Softlight 对 0.5 恒等，即"无变化"）。
#[derive(Default)]
pub struct NeutralMask;

impl MaskPredictor for NeutralMask {
    fn predict(&self, face_512: &ImageBuf) -> (ImageBuf, ImageBuf) {
        let mut a = ImageBuf::new(face_512.width, face_512.height, face_512.space);
        for c in 0..3 {
            a.plane_mut(c).fill(0.5);
        }
        (a.clone(), a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ColorSpace;

    #[test]
    fn neutral_mask_is_identity() {
        let mut src = ImageBuf::new(8, 8, ColorSpace::Linear);
        src.plane_mut(0).fill(0.3);
        src.plane_mut(1).fill(0.6);
        src.plane_mut(2).fill(0.9);

        let retouch = NeutralGrayRetouch::new(Box::new(NeutralMask));
        let out = retouch.apply(&src, 80, 80);

        // 中性灰蒙版 + 柔光恒等 → 结果应接近原图
        for c in 0..3 {
            let a = src.plane(c);
            let b = out.plane(c);
            for i in 0..a.len() {
                assert!((a[i] - b[i]).abs() < 1e-5, "channel {c} 差异过大");
            }
        }
    }

    #[test]
    fn resize_shapes() {
        let src = ImageBuf::new(100, 80, ColorSpace::Linear);
        let small = resize(&src, 512, 512);
        assert_eq!((small.width, small.height), (512, 512));
        let back = resize(&small, 100, 80);
        assert_eq!((back.width, back.height), (100, 80));
    }
}
