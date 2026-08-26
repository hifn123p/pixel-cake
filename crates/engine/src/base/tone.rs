//! 基础调色算子（文档 §4.7）：曝光/对比度/白平衡/饱和度/曲线/颗粒/暗角。
//!
//! 在 16bit 线性空间直接计算，避免 8bit 精度损失。

use crate::image::{ImageBuf, ImageOp, OpCost};

/// 曲线锚点（归一化 0..1）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurvePoint {
    pub x: f32,
    pub y: f32,
}

/// 基础调色参数（-100..100 为 UI 惯用范围，曝光为 EV）。
#[derive(Debug, Clone, PartialEq)]
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
    /// 曲线锚点（空 = 直通）。
    pub curves: Vec<CurvePoint>,
    /// 颗粒 0..100。
    pub grain: f32,
    /// 暗角 0..100。
    pub vignette: f32,
}

impl Default for ToneParams {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            temperature: 0.0,
            tint: 0.0,
            curves: Vec::new(),
            grain: 0.0,
            vignette: 0.0,
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

/// 对单个像素应用像素级调色（曝光/对比度/白平衡/饱和度；RGB 线性 0..1）。
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

/// 曲线查找（锚点间分段线性插值）。
#[inline]
pub fn curve_lookup(curves: &[CurvePoint], t: f32) -> f32 {
    if curves.is_empty() {
        return t;
    }
    let t = t.clamp(0.0, 1.0);
    for i in 0..curves.len() - 1 {
        let p0 = curves[i];
        let p1 = curves[i + 1];
        if t >= p0.x && t <= p1.x {
            let seg = p1.x - p0.x;
            if seg <= 0.0 {
                return p1.y;
            }
            let u = (t - p0.x) / seg;
            return p0.y + (p1.y - p0.y) * u;
        }
    }
    if t <= curves[0].x {
        curves[0].y
    } else {
        curves[curves.len() - 1].y
    }
}

/// 暗角衰减因子（中心 1，边缘变小）。
#[inline]
pub fn vignette_factor(x: u32, y: u32, w: u32, h: u32, strength: f32) -> f32 {
    let cx = (x as f32 + 0.5) / w as f32 - 0.5;
    let cy = (y as f32 + 0.5) / h as f32 - 0.5;
    let d2 = cx * cx + cy * cy;
    let d = (d2 / 0.5).sqrt(); // 0（中心）..1（角落）
    let v = strength / 100.0;
    1.0 - v * d * d
}

/// 确定性伪随机噪声（-1..1），用于颗粒。
#[inline]
pub fn hash_noise(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    (h as f32 / 4_294_967_295.0) * 2.0 - 1.0
}

/// 基础调色算子（含曲线/颗粒/暗角）。
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
        let w = src.width;
        let h = src.height;
        let grain = self.params.grain / 100.0;
        for y in 0..h {
            for x in 0..w {
                let p = src.pixel(x, y);
                let mut c = tone_pixel([p[0], p[1], p[2]], &self.params);

                // 曲线（RGB 三通道）
                if !self.params.curves.is_empty() {
                    c = [
                        curve_lookup(&self.params.curves, c[0]),
                        curve_lookup(&self.params.curves, c[1]),
                        curve_lookup(&self.params.curves, c[2]),
                    ];
                }

                // 暗角
                if self.params.vignette != 0.0 {
                    let v = vignette_factor(x, y, w, h, self.params.vignette);
                    c[0] *= v;
                    c[1] *= v;
                    c[2] *= v;
                }

                // 颗粒（三通道同源噪声，简化）
                if grain > 0.0 {
                    let n = hash_noise(x, y) * grain * 0.5;
                    c[0] = (c[0] + n).clamp(0.0, 1.0);
                    c[1] = (c[1] + n).clamp(0.0, 1.0);
                    c[2] = (c[2] + n).clamp(0.0, 1.0);
                }

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
        assert!((out[0] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn saturation_grayscale() {
        let p = ToneParams { saturation: -100.0, ..Default::default() };
        let out = tone_pixel([0.8, 0.4, 0.2], &p);
        assert!((out[0] - out[1]).abs() < 1e-3);
        assert!((out[1] - out[2]).abs() < 1e-3);
    }

    #[test]
    fn empty_curve_is_identity() {
        let c = [];
        assert!((curve_lookup(&c, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn curve_linear_midpoint() {
        // 两点曲线 (0,0) -> (1,1) 是直线，中点不变
        let c = [CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }];
        assert!((curve_lookup(&c, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn curve_boosts_midtones() {
        // 中点提升到 0.75
        let c = [CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 0.5, y: 0.75 }, CurvePoint { x: 1.0, y: 1.0 }];
        assert!((curve_lookup(&c, 0.5) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn vignette_center_unchanged() {
        // 中心强度衰减应为 1（不变）
        let f = vignette_factor(4, 4, 9, 9, 100.0);
        assert!((f - 1.0).abs() < 1e-3);
    }

    #[test]
    fn vignette_corner_darkened() {
        // 角落应明显变暗
        let f = vignette_factor(0, 0, 9, 9, 100.0);
        assert!(f < 0.5);
    }
}
