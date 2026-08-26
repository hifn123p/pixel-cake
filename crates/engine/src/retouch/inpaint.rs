//! 祛瑕 / 背景修复（文档 §4.5）。
//!
//! 由 mask 标记的瑕疵区域，用邻域非瑕疵像素的加权平均填充并迭代扩散。
//! 这是 AI inpaint 网络接入前的兜底实现；生产由 ONNX 推理（如 MIGAN/轻量扩散）
//! 替代，接口 `inpaint(src, mask)` 不变。

use crate::image::{ColorSpace, ImageBuf};

/// 把归一化坐标多边形栅格化为 mask（多边形内像素为 1）。
/// `polygon` 为 `[[x, y], ...]`，坐标 0..1。
pub fn polygon_to_mask(w: u32, h: u32, polygon: &[[f32; 2]]) -> ImageBuf {
    let pts: Vec<(f32, f32)> = polygon
        .iter()
        .map(|p| (p[0] * w as f32, p[1] * h as f32))
        .collect();
    let mut mask = ImageBuf::new(w, h, ColorSpace::Linear);
    for y in 0..h {
        for x in 0..w {
            if point_in_polygon((x as f32 + 0.5, y as f32 + 0.5), &pts) {
                mask.set_pixel(x, y, [1.0, 1.0, 1.0, 1.0]);
            }
        }
    }
    mask
}

/// 把 `src` mask 与 `add` mask 做逻辑或（写入 src）。
pub fn merge_mask(src: &mut ImageBuf, add: &ImageBuf) {
    for y in 0..src.height {
        for x in 0..src.width {
            if add.pixel(x, y)[0] >= 0.5 {
                src.set_pixel(x, y, [1.0, 1.0, 1.0, 1.0]);
            }
        }
    }
}

/// 射线法判断点是否在多边形内。
fn point_in_polygon(p: (f32, f32), poly: &[(f32, f32)]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if ((yi > p.1) != (yj > p.1)) && (p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

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

    #[test]
    fn polygon_to_mask_center_inside() {
        // 归一化坐标的方块覆盖中心区域
        let poly = [[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]];
        let mask = polygon_to_mask(8, 8, &poly);
        // 中心 (4,4) 在多边形内
        assert!(mask.pixel(4, 4)[0] >= 0.5);
        // 角落 (0,0) 在多边形外
        assert!(mask.pixel(0, 0)[0] < 0.5);
    }

    #[test]
    fn polygon_degenerate_is_empty() {
        let poly = [[0.5, 0.5], [0.6, 0.6]]; // 少于 3 点
        let mask = polygon_to_mask(8, 8, &poly);
        assert!(mask.pixel(4, 4)[0] < 0.5);
    }
}
