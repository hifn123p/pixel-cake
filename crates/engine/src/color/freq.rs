//! 频率分离（高低频）——磨皮的经典基础（文档 §5.2 频率分离算子）。
//!
//! 高斯模糊得到低频层（肤色/明暗），原图减低频得高频层（纹理/毛孔）。
//! 磨皮时只处理低频层，再重组回高频，从而"去瑕保纹理"。

use crate::image::ImageBuf;

/// separable 高斯模糊：先水平后垂直，仅作用于 RGB 三通道，alpha 直接复制。
pub fn gaussian_blur(src: &ImageBuf, sigma: f32) -> ImageBuf {
    let kernel = gaussian_kernel(sigma);
    let r = (kernel.len() / 2) as i32;

    // 水平 pass
    let mut h = ImageBuf::new(src.width, src.height, src.space);
    for c in 0..3 {
        let sp = src.plane(c);
        let hp = h.plane_mut(c);
        for y in 0..src.height as i32 {
            for x in 0..src.width as i32 {
                let mut acc = 0.0;
                for (k, &w) in kernel.iter().enumerate() {
                    let sx = (x + k as i32 - r).clamp(0, src.width as i32 - 1);
                    acc += sp[(y * src.width as i32 + sx) as usize] * w;
                }
                hp[(y * src.width as i32 + x) as usize] = acc;
            }
        }
    }

    // 垂直 pass
    let mut out = ImageBuf::new(src.width, src.height, src.space);
    for c in 0..3 {
        let hp = h.plane(c);
        let op = out.plane_mut(c);
        for y in 0..src.height as i32 {
            for x in 0..src.width as i32 {
                let mut acc = 0.0;
                for (k, &w) in kernel.iter().enumerate() {
                    let sy = (y + k as i32 - r).clamp(0, src.height as i32 - 1);
                    acc += hp[(sy * src.width as i32 + x) as usize] * w;
                }
                op[(y * src.width as i32 + x) as usize] = acc;
            }
        }
    }
    out.plane_mut(3).copy_from_slice(src.plane(3));
    out
}

/// 一维高斯核（归一化）。
fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let r = (sigma * 3.0).ceil() as i32;
    let mut k = Vec::with_capacity((2 * r + 1) as usize);
    let mut sum = 0.0;
    for i in -r..=r {
        let v = (-(i as f32).powi(2) / (2.0 * sigma * sigma)).exp();
        k.push(v);
        sum += v;
    }
    for v in k.iter_mut() {
        *v /= sum;
    }
    k
}

/// 高低频分离结果。`high` 为 `src - low`，可含负值（16bit float 支持）。
pub struct FrequencySplit {
    pub low: ImageBuf,
    pub high: ImageBuf,
}

/// 分离：`low = gaussian_blur(src)`，`high = src - low`。
pub fn split_frequency(src: &ImageBuf, sigma: f32) -> FrequencySplit {
    let low = gaussian_blur(src, sigma);
    let mut high = ImageBuf::new(src.width, src.height, src.space);
    for c in 0..4 {
        let sp = src.plane(c);
        let lp = low.plane(c);
        let hp = high.plane_mut(c);
        for i in 0..sp.len() {
            hp[i] = sp[i] - lp[i];
        }
    }
    FrequencySplit { low, high }
}

/// 重组：`low + high`。
pub fn recombine(low: &ImageBuf, high: &ImageBuf) -> ImageBuf {
    let mut out = ImageBuf::new(low.width, low.height, low.space);
    for c in 0..4 {
        let lp = low.plane(c);
        let hp = high.plane(c);
        let op = out.plane_mut(c);
        for i in 0..lp.len() {
            op[i] = lp[i] + hp[i];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ColorSpace;

    fn flat(width: u32, height: u32, v: f32) -> ImageBuf {
        let mut b = ImageBuf::new(width, height, ColorSpace::Linear);
        for c in 0..3 {
            b.plane_mut(c).fill(v);
        }
        b
    }

    #[test]
    fn blur_preserves_constant() {
        let src = flat(16, 16, 0.5);
        let out = gaussian_blur(&src, 2.0);
        let p = out.plane(0);
        assert!((p[0] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn split_recombine_roundtrip() {
        let src = flat(8, 8, 0.6);
        let split = split_frequency(&src, 1.5);
        let back = recombine(&split.low, &split.high);
        let a = src.plane(0);
        let b = back.plane(0);
        for i in 0..a.len() {
            assert!((a[i] - b[i]).abs() < 1e-4);
        }
    }

    #[test]
    fn kernel_normalized() {
        let k = gaussian_kernel(2.0);
        let sum: f32 = k.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
    }
}
