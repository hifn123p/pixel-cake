//! 3D LUT（文档 §4.6/§4.7：追色结果烘焙为 3D LUT，供预览与批量复用）。
//!
//! 色彩变换被离散化为 `size³` 的查找表，运行时三线性插值查询，
//! 避免对每个像素重复执行昂贵的色彩迁移计算。

use crate::image::{ColorSpace, ImageBuf};

/// 3D LUT：`size × size × size` 的表，每个条目为输出 RGB（0..1 线性）。
pub struct Lut3D {
    pub size: u32,
    table: Vec<[f32; 3]>,
}

impl Lut3D {
    /// 恒等 LUT（输出 = 输入）。
    pub fn identity(size: u32) -> Self {
        let n = size;
        let mut table = Vec::with_capacity((n * n * n) as usize);
        for r in 0..n {
            for g in 0..n {
                for b in 0..n {
                    let d = (n - 1) as f32;
                    table.push([r as f32 / d, g as f32 / d, b as f32 / d]);
                }
            }
        }
        Self { size: n, table }
    }

    /// 从映射函数构建：对每个格点调用 `f(输入RGB) -> 输出RGB`。
    pub fn from_fn(size: u32, f: impl Fn([f32; 3]) -> [f32; 3]) -> Self {
        let n = size;
        let mut table = Vec::with_capacity((n * n * n) as usize);
        for r in 0..n {
            for g in 0..n {
                for b in 0..n {
                    let d = (n - 1) as f32;
                    let input = [r as f32 / d, g as f32 / d, b as f32 / d];
                    table.push(f(input));
                }
            }
        }
        Self { size: n, table }
    }

    /// 从 Adobe `.cube` 3D LUT 文本解析（滤镜库加载）。
    /// `.cube` 条目按 red 最快排序，此处映射到内部表序（b 最快）。
    pub fn from_cube(text: &str) -> Result<Self, String> {
        let mut size = 0u32;
        let mut entries: Vec<[f32; 3]> = Vec::new();

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            match parts.next() {
                Some("LUT_3D_SIZE") => {
                    size = parts
                        .next()
                        .ok_or_else(|| "LUT_3D_SIZE 缺值".to_string())?
                        .parse()
                        .map_err(|_| "LUT_3D_SIZE 非数字".to_string())?;
                }
                Some("TITLE") | Some("DOMAIN_MIN") | Some("DOMAIN_MAX") | Some("LUT_1D_SIZE") => {
                    continue;
                }
                Some(first) => {
                    let mut vals = Vec::with_capacity(3);
                    if let Ok(v) = first.parse::<f32>() {
                        vals.push(v);
                    }
                    for p in parts {
                        if let Ok(v) = p.parse::<f32>() {
                            vals.push(v);
                        }
                    }
                    if vals.len() >= 3 {
                        entries.push([vals[0], vals[1], vals[2]]);
                    }
                }
                None => {}
            }
        }

        if size == 0 {
            return Err("缺少 LUT_3D_SIZE".to_string());
        }
        let n = size;
        let expected = (n * n * n) as usize;
        if entries.len() < expected {
            return Err(format!("条目不足: 期望 {expected}，实际 {}", entries.len()));
        }

        let mut table = vec![[0.0f32; 3]; expected];
        for (i, e) in entries.iter().take(expected).enumerate() {
            let r = (i % n as usize) as u32;
            let g = ((i / n as usize) % n as usize) as u32;
            let b = (i / (n as usize * n as usize)) as u32;
            table[((r * n + g) * n + b) as usize] = *e;
        }

        Ok(Self { size: n, table })
    }

    #[inline]
    fn get(&self, r: u32, g: u32, b: u32) -> [f32; 3] {
        let n = self.size;
        self.table[((r * n + g) * n + b) as usize]
    }

    /// 三线性插值查询单个像素。
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let t = n - 1;
        let fx = rgb[0].clamp(0.0, 1.0) * t as f32;
        let fy = rgb[1].clamp(0.0, 1.0) * t as f32;
        let fz = rgb[2].clamp(0.0, 1.0) * t as f32;
        let x0 = fx.floor() as u32;
        let y0 = fy.floor() as u32;
        let z0 = fz.floor() as u32;
        let x1 = (x0 + 1).min(t);
        let y1 = (y0 + 1).min(t);
        let z1 = (z0 + 1).min(t);
        let dx = fx - x0 as f32;
        let dy = fy - y0 as f32;
        let dz = fz - z0 as f32;

        let c000 = self.get(x0, y0, z0);
        let c100 = self.get(x1, y0, z0);
        let c010 = self.get(x0, y1, z0);
        let c110 = self.get(x1, y1, z0);
        let c001 = self.get(x0, y0, z1);
        let c101 = self.get(x1, y0, z1);
        let c011 = self.get(x0, y1, z1);
        let c111 = self.get(x1, y1, z1);

        let mut out = [0.0; 3];
        for i in 0..3 {
            let c00 = c000[i] * (1.0 - dx) + c100[i] * dx;
            let c10 = c010[i] * (1.0 - dx) + c110[i] * dx;
            let c01 = c001[i] * (1.0 - dx) + c101[i] * dx;
            let c11 = c011[i] * (1.0 - dx) + c111[i] * dx;
            let c0 = c00 * (1.0 - dy) + c10 * dy;
            let c1 = c01 * (1.0 - dy) + c11 * dy;
            out[i] = c0 * (1.0 - dz) + c1 * dz;
        }
        out
    }

    /// 对整个图像应用 LUT（RGB 三通道逐像素）。
    pub fn apply_image(&self, src: &ImageBuf) -> ImageBuf {
        let mut dst = ImageBuf::new(src.width, src.height, ColorSpace::Linear);
        for y in 0..src.height {
            for x in 0..src.width {
                let p = src.pixel(x, y);
                let o = self.apply([p[0], p[1], p[2]]);
                dst.set_pixel(x, y, [o[0], o[1], o[2], p[3]]);
            }
        }
        dst
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_lut_is_identity() {
        let lut = Lut3D::identity(33);
        for &v in &[[0.0, 0.0, 0.0], [0.5, 0.5, 0.5], [0.3, 0.7, 0.9], [1.0, 1.0, 1.0]] {
            let out = lut.apply(v);
            for i in 0..3 {
                assert!((out[i] - v[i]).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn lut_inverts() {
        // 构建一个"反转"LUT：out = 1 - in
        let lut = Lut3D::from_fn(17, |c| [1.0 - c[0], 1.0 - c[1], 1.0 - c[2]]);
        let out = lut.apply([0.25, 0.5, 0.75]);
        assert!((out[0] - 0.75).abs() < 1e-3);
        assert!((out[1] - 0.5).abs() < 1e-3);
        assert!((out[2] - 0.25).abs() < 1e-3);
    }

    #[test]
    fn cube_identity() {
        // size=2 的恒等 .cube（条目 red 最快）
        let text = "\
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
        let lut = Lut3D::from_cube(text).unwrap();
        assert_eq!(lut.size, 2);
        let out = lut.apply([0.5, 0.5, 0.5]);
        assert!((out[0] - 0.5).abs() < 1e-3);
        assert!((out[1] - 0.5).abs() < 1e-3);
        assert!((out[2] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn cube_with_comments_and_title() {
        let text = "\
# 注释行
TITLE \"test\"
LUT_3D_SIZE 2
DOMAIN_MIN 0.0 0.0 0.0
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
        let lut = Lut3D::from_cube(text).unwrap();
        assert_eq!(lut.size, 2);
    }

    #[test]
    fn cube_missing_size_errors() {
        let text = "0.0 0.0 0.0\n1.0 1.0 1.0\n";
        assert!(Lut3D::from_cube(text).is_err());
    }
}
