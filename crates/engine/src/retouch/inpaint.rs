//! 祛瑕 / 背景修复（文档 §4.5）。
//!
//! 由 mask 标记的瑕疵区域，用邻域非瑕疵像素的加权平均填充并迭代扩散。
//! 这是 AI inpaint 网络接入前的兜底实现；生产由 ONNX 推理（如 MIGAN/轻量扩散）
//! 替代，接口 `inpaint(src, mask)` 不变。

use crate::image::{ColorSpace, ImageBuf};

/// 简单 inpaint：mask 区域用邻域非 mask 像素均值填充，迭代 `iterations` 次扩散。
pub fn inpaint(src: &ImageBuf, mask: &ImageBuf, iterations: u32) -> ImageBuf {
    let mut dst = src.clone();
    for _ in 0..iterations {
        dst = inpaint_once(&dst, mask);
    }
    dst
}

/// 单次扩散：mask 内像素取邻域非 mask 像素的均值。
fn inpaint_once(img: &ImageBuf, mask: &ImageBuf) -> ImageBuf {
    let mut out = img.clone();
    for y in 0..img.height {
        for x in 0..img.width {
            if mask.pixel(x, y)[0] < 0.5 {
                continue;
            }
            let mut sum = [0.0f32; 4];
            let mut cnt = 0.0f32;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= img.width as i32 || ny >= img.height as i32 {
                        continue;
                    }
                    if mask.pixel(nx as u32, ny as u32)[0] >= 0.5 {
                        continue;
                    }
                    let p = img.pixel(nx as u32, ny as u32);
                    for i in 0..4 {
                        sum[i] += p[i];
                    }
                    cnt += 1.0;
                }
            }
            if cnt > 0.0 {
                let mut p = [0.0f32; 4];
                for i in 0..4 {
                    p[i] = sum[i] / cnt;
                }
                out.set_pixel(x, y, p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_mask_is_identity() {
        let mut src = ImageBuf::new(8, 8, ColorSpace::Linear);
        for y in 0..8 {
            for x in 0..8 {
                src.set_pixel(x, y, [0.5, 0.5, 0.5, 1.0]);
            }
        }
        let mask = ImageBuf::new(8, 8, ColorSpace::Linear);
        let out = inpaint(&src, &mask, 3);
        assert!((out.pixel(3, 3)[0] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn mask_pixel_gets_filled() {
        // 中心像素是 mask，周围是 0.8，填充后中心应接近 0.8
        let mut src = ImageBuf::new(5, 5, ColorSpace::Linear);
        for y in 0..5 {
            for x in 0..5 {
                src.set_pixel(x, y, [0.8, 0.8, 0.8, 1.0]);
            }
        }
        src.set_pixel(2, 2, [0.0, 0.0, 0.0, 1.0]);
        let mut mask = ImageBuf::new(5, 5, ColorSpace::Linear);
        mask.set_pixel(2, 2, [1.0, 1.0, 1.0, 1.0]);

        let out = inpaint(&src, &mask, 3);
        assert!((out.pixel(2, 2)[0] - 0.8).abs() < 0.1);
    }
}
