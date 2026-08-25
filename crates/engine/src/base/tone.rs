//! 基础调色算子（文档 §4.7）：曝光/对比度/白平衡/饱和度。
//!
//! 在 16bit 线性空间直接计算，避免 8bit 精度损失。

use crate::image::{ImageBuf, ImageOp, OpCost};

/// 基础调色参数（-100..100 为 UI 惯用范围，曝光为 EV）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToneParams {
    /// 曝光 EV，-5..5。
    pub exposure: f32,
    /// 对比度 -100..100。
    pub contrast: f32,
    /// 饱和度 -100..100。
    pub saturation: f32,
    /// 色温 -100..100（正=暖，负=冷）。
    pub temperature: f32,
    /// 色调 -100..100。
    pub tint: f32,
}

impl Default for ToneParams {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            temperature: 0.0,
            tint: 0.0,
        }
    }
}

/// 曝光增益：`2^ev`。
#[inline]
pub fn exposure_gain(ev: f32) -> f32 {
    2.0f32.powf(ev)
}

/// 对比度因子：-100→0，0→1，100→2。
#[inline]
pub fn contrast_factor(c: f32) -> f32 {
    1.0 + c / 100.0
}

/// 饱和度因子：-100→0，0→1，100→2。
#[inline]
pub fn saturation_factor(s: f32) -> f32 {
    1.0 + s / 100.0
}

/// 对单个像素应用调色（RGB 线性 0..1）。
#[inline]
pub fn tone_pixel(rgb: [f32; 3], p: &ToneParams) -> [f32; 3] {
    let [mut r, mut g, mut b] = rgb;

    // 白平衡：温度影响 R/B，色调影响 G
    let temp = p.temperature / 100.0;
    let tint = p.tint / 100.0;
    r *= 1.0 + temp * 0.5;
    b *= 1.0 - temp * 0.5;
    g *= 1.0 + tint * 0.3;

    // 曝光
    let ev = exposure_gain(p.exposure);
    r *= ev;
    g *= ev;
    b *= ev;

    // 对比度（中点 0.5）
    let cf = contrast_factor(p.contrast);
    r = (r - 0.5) * cf + 0.5;
    g = (g - 0.5) * cf + 0.5;
    b = (b - 0.5) * cf + 0.5;

    // 饱和度
    let sf = saturation_factor(p.saturation);
    let gray = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    r = gray + (r - gray) * sf;
    g = gray + (g - gray) * sf;
    b = gray + (b - gray) * sf;

    [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]
}

/// 基础调色算子。
pub struct ToneAdjust {
    pub params: ToneParams,
}

impl ToneAdjust {
    pub fn new(params: ToneParams) -> Self {
        Self { params }
    }
}

impl ImageOp for ToneAdjust {
    fn apply(&self, src: &ImageBuf, dst: &mut ImageBuf) {
        for y in 0..src.height {
            for x in 0..src.width {
                let p = src.pixel(x, y);
                let c = tone_pixel([p[0], p[1], p[2]], &self.params);
                dst.set_pixel(x, y, [c[0], c[1], c[2], p[3]]);
            }
        }
    }

    fn cost(&self) -> OpCost {
        OpCost {
            vram_bytes: 0,
            weight: 1.0,
            exclusive_gpu: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_params_is_identity() {
        let p = ToneParams::default();
        let out = tone_pixel([0.3, 0.5, 0.7], &p);
        assert!((out[0] - 0.3).abs() < 1e-4);
        assert!((out[1] - 0.5).abs() < 1e-4);
        assert!((out[2] - 0.7).abs() < 1e-4);
    }

    #[test]
    fn exposure_brightens() {
        let p = ToneParams { exposure: 1.0, ..Default::default() };
        let out = tone_pixel([0.5, 0.5, 0.5], &p);
        assert!((out[0] - 1.0).abs() < 1e-3); // 0.5 * 2^1 = 1.0
    }

    #[test]
    fn saturation_grayscale() {
        let p = ToneParams { saturation: -100.0, ..Default::default() };
        let out = tone_pixel([0.8, 0.4, 0.2], &p);
        // 完全去饱和 → R=G=B
        assert!((out[0] - out[1]).abs() < 1e-3);
        assert!((out[1] - out[2]).abs() < 1e-3);
    }
}
