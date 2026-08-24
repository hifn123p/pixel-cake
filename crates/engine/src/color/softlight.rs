//! 柔光合成与中性灰磨皮（文档 §4.3 核心公式）。
//!
//! 中性灰磨皮把 GAN 预测的"平整层 A / 立体层 B"中性灰蒙版，用柔光模式与
//! 原图合成，得到无斑驳、保结构的磨皮结果。双参数 ka(平整)、kb(立体) 控制强度。

use crate::image::{ImageBuf, ImageOp, OpCost};

/// Soft Light 混合（W3C compositing spec），输入输出均为 0..1 线性。
#[inline]
pub fn softlight(base: f32, blend: f32) -> f32 {
    let b = base.clamp(0.0, 1.0);
    let s = blend.clamp(0.0, 1.0);
    if s <= 0.5 {
        b - (1.0 - 2.0 * s) * b * (1.0 - b)
    } else {
        let d = if b <= 0.25 {
            ((16.0 * b - 12.0) * b + 4.0) * b
        } else {
            b.sqrt()
        };
        b + (2.0 * s - 1.0) * (d - b)
    }
}

/// 中性灰磨皮单像素合成（文档 §4.3 公式）：
///
/// ```text
/// tmp = Softlight(S, A) * ka + S * (1 - ka)   // 平整层
/// D   = Softlight(tmp, B) * kb + tmp * (1 - kb) // 立体层
/// ```
///
/// `ka` / `kb` 为 0..1 的强度。
#[inline]
pub fn neutral_gray_compose(s: f32, a: f32, b: f32, ka: f32, kb: f32) -> f32 {
    let tmp = softlight(s, a) * ka + s * (1.0 - ka);
    softlight(tmp, b) * kb + tmp * (1.0 - kb)
}

/// 中性灰磨皮合成算子：持有平整蒙版 A 与立体蒙版 B（尺寸须与源一致）。
pub struct NeutralGrayCompose {
    mask_flat: ImageBuf,
    mask_struct: ImageBuf,
    ka: f32,
    kb: f32,
}

impl NeutralGrayCompose {
    /// `ka` / `kb` 为 0..100 的强度（与 UI 滑块一致）。
    pub fn new(mask_flat: ImageBuf, mask_struct: ImageBuf, ka: u8, kb: u8) -> Self {
        Self {
            mask_flat,
            mask_struct,
            ka: ka as f32 / 100.0,
            kb: kb as f32 / 100.0,
        }
    }
}

impl ImageOp for NeutralGrayCompose {
    fn apply(&self, src: &ImageBuf, dst: &mut ImageBuf) {
        assert_eq!(
            (src.width, src.height),
            (self.mask_flat.width, self.mask_flat.height),
            "源与蒙版尺寸不一致"
        );
        for c in 0..3 {
            let sp = src.plane(c);
            let ap = self.mask_flat.plane(c);
            let bp = self.mask_struct.plane(c);
            let dp = dst.plane_mut(c);
            for i in 0..sp.len() {
                dp[i] = neutral_gray_compose(sp[i], ap[i], bp[i], self.ka, self.kb);
            }
        }
        let da = dst.plane_mut(3);
        da.copy_from_slice(src.plane(3));
    }

    fn cost(&self) -> OpCost {
        OpCost {
            vram_bytes: 0,
            weight: 2.0,
            exclusive_gpu: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ColorSpace;

    #[test]
    fn softlight_neutral_is_identity() {
        // blend=0.5 时 Softlight(base, 0.5) == base
        for b in [0.0, 0.2, 0.5, 0.8, 1.0] {
            assert!((softlight(b, 0.5) - b).abs() < 1e-6);
        }
    }

    #[test]
    fn neutral_gray_zero_strength_is_identity() {
        // ka=kb=0 时结果等于原图
        let s = 0.3;
        assert!((neutral_gray_compose(s, 0.9, 0.1, 0.0, 0.0) - s).abs() < 1e-6);
    }

    #[test]
    fn compose_operator_shapes() {
        let src = ImageBuf::new(4, 4, ColorSpace::Linear);
        let a = ImageBuf::new(4, 4, ColorSpace::Linear);
        let b = ImageBuf::new(4, 4, ColorSpace::Linear);
        let mut dst = ImageBuf::new(4, 4, ColorSpace::Linear);
        let op = NeutralGrayCompose::new(a, b, 60, 40);
        op.apply(&src, &mut dst);
        assert_eq!(dst.pixel_count(), 16);
    }
}
