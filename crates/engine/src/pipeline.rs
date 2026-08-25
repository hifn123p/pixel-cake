//! 16bit 全链路处理管线（文档 §5.2 算子链）。
//!
//! 按固定顺序串联各精修算子：
//! 中性灰磨皮 → 美型 warp → 祛瑕 → 追色(LUT) → 基础调色 → 滤镜(LUT)。
//!
//! AI 预测（人脸检测、GAN 蒙版、分割）在上层（scheduler/engine 推理层）完成，
//! 其结果（蒙版/控制点/LUT）作为本管线的输入，使管线保持"纯图像处理"。

use crate::base::tone::{ToneAdjust, ToneParams};
use crate::color::lut::Lut3D;
use crate::color::softlight::NeutralGrayCompose;
use crate::image::{ImageBuf, ImageOp};
use crate::retouch::beauty::{liquify, LiquifyPoint};
use crate::retouch::inpaint::inpaint;

/// 中性灰磨皮的输入（蒙版已由 GAN 预测 + 上采样到原图尺寸）。
pub struct NeutralGrayInput {
    pub mask_flat: ImageBuf,
    pub mask_struct: ImageBuf,
    pub ka: u8,
    pub kb: u8,
}

/// 16bit 全链路管线参数。
#[derive(Default)]
pub struct Pipeline {
    /// 中性灰磨皮（蒙版已预测）。
    pub neutral_gray: Option<NeutralGrayInput>,
    /// 美型液化控制点。
    pub beauty_points: Vec<LiquifyPoint>,
    /// 祛瑕 mask（瑕疵区域为 1）。
    pub inpaint_mask: Option<ImageBuf>,
    /// 追色 LUT。
    pub color_lut: Option<Lut3D>,
    /// 基础调色。
    pub tone: ToneParams,
    /// 滤镜 LUT。
    pub filter_lut: Option<Lut3D>,
}

/// 按算子链顺序处理。
pub fn process(src: &ImageBuf, p: &Pipeline) -> ImageBuf {
    let mut img = src.clone();

    // 1. 中性灰磨皮（柔光合成）
    if let Some(ng) = &p.neutral_gray {
        let op = NeutralGrayCompose::new(
            ng.mask_flat.clone(),
            ng.mask_struct.clone(),
            ng.ka,
            ng.kb,
        );
        let mut next = ImageBuf::new(src.width, src.height, src.space);
        op.apply(&img, &mut next);
        img = next;
    }

    // 2. 美型液化
    if !p.beauty_points.is_empty() {
        img = liquify(&img, &p.beauty_points);
    }

    // 3. 祛瑕
    if let Some(mask) = &p.inpaint_mask {
        img = inpaint(&img, mask, 4);
    }

    // 4. 追色 LUT
    if let Some(lut) = &p.color_lut {
        img = lut.apply_image(&img);
    }

    // 5. 基础调色
    if p.tone != ToneParams::default() {
        let op = ToneAdjust::new(p.tone);
        let mut next = ImageBuf::new(src.width, src.height, src.space);
        op.apply(&img, &mut next);
        img = next;
    }

    // 6. 滤镜 LUT
    if let Some(lut) = &p.filter_lut {
        img = lut.apply_image(&img);
    }

    img
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ColorSpace;

    fn solid(w: u32, h: u32, c: [f32; 4]) -> ImageBuf {
        let mut b = ImageBuf::new(w, h, ColorSpace::Linear);
        for y in 0..h {
            for x in 0..w {
                b.set_pixel(x, y, c);
            }
        }
        b
    }

    #[test]
    fn empty_pipeline_is_identity() {
        let src = solid(8, 8, [0.4, 0.5, 0.6, 1.0]);
        let out = process(&src, &Pipeline::default());
        let p = out.pixel(3, 3);
        assert!((p[0] - 0.4).abs() < 1e-4);
        assert!((p[1] - 0.5).abs() < 1e-4);
        assert!((p[2] - 0.6).abs() < 1e-4);
    }

    #[test]
    fn tone_step_applies() {
        let src = solid(8, 8, [0.5, 0.5, 0.5, 1.0]);
        let p = Pipeline {
            tone: ToneParams { exposure: 1.0, ..Default::default() },
            ..Default::default()
        };
        let out = process(&src, &p);
        assert!((out.pixel(3, 3)[0] - 1.0).abs() < 1e-3);
    }
}
