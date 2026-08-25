//! 美型液化 warp（文档 §4.4）。
//!
//! 通过控制点（位置 + 位移 + 半径）对图像做局部推拉变形，
//! 类似 Photoshop 液化。关键点检测（468 点/人体关键点）依赖 ONNX 模型，
//! 由模型输出的控制点驱动本模块的几何变形。

use crate::image::{ColorSpace, ImageBuf};

/// 液化控制点（归一化坐标 0..1）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquifyPoint {
    pub x: f32,
    pub y: f32,
    /// 位移（归一化，相对图像尺寸）。
    pub dx: f32,
    pub dy: f32,
    /// 影响半径（归一化）。
    pub radius: f32,
}

/// 液化：对每个输出像素累积控制点的推拉位移，反向采样原图。
pub fn liquify(src: &ImageBuf, points: &[LiquifyPoint]) -> ImageBuf {
    let mut dst = ImageBuf::new(src.width, src.height, ColorSpace::Linear);
    let w = src.width as f32;
    let h = src.height as f32;
    for y in 0..src.height {
        for x in 0..src.width {
            let px = x as f32 / w;
            let py = y as f32 / h;
            let mut mx = 0.0f32;
            let mut my = 0.0f32;
            for p in points {
                let dx = px - p.x;
                let dy = py - p.y;
                let d2 = dx * dx + dy * dy;
                let r2 = p.radius * p.radius;
                if r2 > 0.0 && d2 < r2 {
                    let t = d2.sqrt() / p.radius;
                    // 平滑衰减（离控制点越近影响越大）
                    let weight = (1.0 - t * t) * (1.0 - t * t);
                    mx += p.dx * weight;
                    my += p.dy * weight;
                }
            }
            let c = sample_bilinear(src, (px - mx) * w, (py - my) * h);
            dst.set_pixel(x, y, c);
        }
    }
    dst
}

/// 双线性采样（像素坐标，边界 clamp）。
pub fn sample_bilinear(img: &ImageBuf, x: f32, y: f32) -> [f32; 4] {
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn no_points_is_identity() {
        let src = solid(16, 16, [0.4, 0.5, 0.6, 1.0]);
        let out = liquify(&src, &[]);
        let p = out.pixel(5, 5);
        assert!((p[0] - 0.4).abs() < 1e-4);
        assert!((p[1] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn zero_displacement_is_identity() {
        let src = solid(16, 16, [0.4, 0.5, 0.6, 1.0]);
        let pt = LiquifyPoint { x: 0.5, y: 0.5, dx: 0.0, dy: 0.0, radius: 0.3 };
        let out = liquify(&src, &[pt]);
        let p = out.pixel(5, 5);
        assert!((p[0] - 0.4).abs() < 1e-3);
    }

    #[test]
    fn bilinear_sampling_edges() {
        let src = solid(4, 4, [0.7, 0.3, 0.2, 1.0]);
        // 越界坐标被 clamp，不 panic
        let c = sample_bilinear(&src, -5.0, 99.0);
        assert!((c[0] - 0.7).abs() < 1e-4);
    }
}
