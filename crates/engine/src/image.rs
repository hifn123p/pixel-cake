//! 16bit 图像缓冲与算子抽象（文档 §5.2「16bit 图像处理管线」基座）。
//!
//! 全链路统一以 `f32` 平面存储，色彩空间为**线性/ProPhoto**，避免多步调整的
//! 累积误差与色带。所有算子实现 [`ImageOp`]，由调度器按 [`OpCost`] 估算
//! 显存/耗时，决定串行或分块执行。

use std::fmt;

/// 内部色彩空间。除 `SRgb` 用于 8bit 编解码边界外，管线内部只用
/// `Linear` / `ProPhoto` / `Lab`（追色用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    /// sRGB（gamma 编码，仅编解码边界）。
    SRgb,
    /// 线性 RGB（D65）。
    Linear,
    /// ProPhoto / ROMM RGB（D50，宽色域，导出保真）。
    ProPhoto,
    /// CIELAB（追色 μ/σ 迁移）。
    Lab,
}

/// 16bit 图像缓冲：RGBA 四平面，每像素 `f32`。
#[derive(Debug, Clone, PartialEq)]
pub struct ImageBuf {
    pub width: u32,
    pub height: u32,
    pub space: ColorSpace,
    planes: [Vec<f32>; 4],
}

/// 单图最大像素数上限（约 2 亿，远超 8K 照片），防止恶意/损坏头导致 OOM。
pub const MAX_PIXELS: usize = 200_000_000;

impl ImageBuf {
    /// 以指定色彩空间新建缓冲，像素初值为 0。
    ///
    /// 超出 [`MAX_PIXELS`] 的尺寸直接 panic（防溢出与 OOM）。
    pub fn new(width: u32, height: u32, space: ColorSpace) -> Self {
        let n = (width as usize)
            .checked_mul(height as usize)
            .filter(|&n| n <= MAX_PIXELS)
            .expect("图像尺寸非法（过大或溢出）");
        Self {
            width,
            height,
            space,
            planes: [
                vec![0.0; n],
                vec![0.0; n],
                vec![0.0; n],
                vec![1.0; n], // alpha 默认 1.0
            ],
        }
    }

    /// 从 8bit sRGB 解码为线性 RGB（RAW 之外的导入边界）。
    ///
    /// `bytes` 长度不足时 panic（防止越界读取）。
    pub fn from_srgb_rgba8(width: u32, height: u32, bytes: &[u8]) -> Self {
        let n = (width as usize)
            .checked_mul(height as usize)
            .filter(|&n| n <= MAX_PIXELS)
            .expect("图像尺寸非法（过大或溢出）");
        assert!(bytes.len() >= n * 4, "RGBA8 数据长度不足");
        let mut buf = Self::new(width, height, ColorSpace::Linear);
        for i in 0..n {
            let o = i * 4;
            buf.planes[0][i] = srgb_to_linear(bytes[o] as f32 / 255.0);
            buf.planes[1][i] = srgb_to_linear(bytes[o + 1] as f32 / 255.0);
            buf.planes[2][i] = srgb_to_linear(bytes[o + 2] as f32 / 255.0);
            buf.planes[3][i] = bytes[o + 3] as f32 / 255.0;
        }
        buf
    }

    pub fn pixel_count(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    /// 读取某像素的 RGBA（线性空间，`f32`）。
    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> [f32; 4] {
        let i = (y as usize) * (self.width as usize) + (x as usize);
        [
            self.planes[0][i],
            self.planes[1][i],
            self.planes[2][i],
            self.planes[3][i],
        ]
    }

    /// 写入某像素。
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, c: [f32; 4]) {
        let i = (y as usize) * (self.width as usize) + (x as usize);
        self.planes[0][i] = c[0];
        self.planes[1][i] = c[1];
        self.planes[2][i] = c[2];
        self.planes[3][i] = c[3];
    }

    /// 以行优先暴露单个平面（供算子/着色器/FFI 直接消费）。
    #[inline]
    pub fn plane(&self, ch: usize) -> &[f32] {
        &self.planes[ch]
    }

    /// 可变平面引用。
    #[inline]
    pub fn plane_mut(&mut self, ch: usize) -> &mut [f32] {
        &mut self.planes[ch]
    }
}

/// 算子开销估算（供调度器做显存/耗时决策，文档 §5.3）。
#[derive(Debug, Clone, Copy, Default)]
pub struct OpCost {
    /// 估算峰值显存字节（如分块推理的 tile 大小）。
    pub vram_bytes: usize,
    /// 相对耗时权重（1 = 轻量，>1 为重算）。
    pub weight: f32,
    /// 是否需要独占 GPU 推理资源（AI 推理为 true）。
    pub exclusive_gpu: bool,
}

/// 图像算子：原地/出图算子统一协议。
pub trait ImageOp: Send + Sync {
    fn apply(&self, src: &ImageBuf, dst: &mut ImageBuf);
    fn cost(&self) -> OpCost;
}

impl fmt::Debug for dyn ImageOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ImageOp")
    }
}

// ───────────────────────── 色彩空间转换 ─────────────────────────

/// sRGB gamma → 线性（IEC 61966-2-1）。
#[inline]
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// 线性 → sRGB gamma。
#[inline]
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// 线性 RGB(D65) → CIELAB（D65 参考白，追色迁移在 Lab 空间进行）。
pub fn rgb_to_lab(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb;
    // 线性 sRGB → XYZ(D65)
    let x = r * 0.412_456_4 + g * 0.357_576_1 + b * 0.180_437_5;
    let y = r * 0.212_672_9 + g * 0.715_152_2 + b * 0.072_175_0;
    let z = r * 0.019_333_9 + g * 0.119_192_0 + b * 0.950_304_1;

    // D65 参考白
    let xn = 0.950_47;
    let yn = 1.0;
    let zn = 1.088_83;

    let fx = lab_f(x / xn);
    let fy = lab_f(y / yn);
    let fz = lab_f(z / zn);

    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIELAB → 线性 RGB(D65)。
pub fn lab_to_rgb(lab: [f32; 3]) -> [f32; 3] {
    let [l, a, b] = lab;
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;

    let x = 0.950_47 * lab_f_inv(fx);
    let y = 1.0 * lab_f_inv(fy);
    let z = 1.088_83 * lab_f_inv(fz);

    // XYZ(D65) → 线性 sRGB
    let r = x * 3.240_454_2 + y * -1.537_138_5 + z * -0.498_531_4;
    let g = x * -0.969_266_0 + y * 1.876_010_8 + z * 0.041_556_0;
    let b = x * 0.055_643_4 + y * -0.204_025_9 + z * 1.057_225_2;

    [r, g, b]
}

#[inline]
fn lab_f(t: f32) -> f32 {
    const D: f32 = 6.0 / 29.0;
    if t > D * D * D {
        t.cbrt()
    } else {
        t / (3.0 * D * D) + 4.0 / 29.0
    }
}

#[inline]
fn lab_f_inv(t: f32) -> f32 {
    const D: f32 = 6.0 / 29.0;
    if t > D {
        t * t * t
    } else {
        3.0 * D * D * (t - 4.0 / 29.0)
    }
}

// 线性 RGB(D65) → XYZ(D50) 与 ProPhoto 的完整转换矩阵见后续 `color::space`
// 扩展；此处先落 Lab 链路，ProPhoto 由 `ColorSpace` 变体占位。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_linear_roundtrip() {
        for &v in &[0.0, 0.01, 0.1, 0.5, 0.9, 1.0] {
            assert!((srgb_to_linear(linear_to_srgb(v)) - v).abs() < 1e-5);
        }
    }

    #[test]
    fn lab_roundtrip_neutral() {
        let rgb = [0.3, 0.3, 0.3];
        let lab = rgb_to_lab(rgb);
        let back = lab_to_rgb(lab);
        for i in 0..3 {
            assert!((back[i] - rgb[i]).abs() < 1e-3);
        }
    }

    #[test]
    fn image_buf_shape() {
        let b = ImageBuf::new(10, 20, ColorSpace::Linear);
        assert_eq!(b.pixel_count(), 200);
        assert_eq!(b.pixel(0, 0)[3], 1.0);
    }
}
