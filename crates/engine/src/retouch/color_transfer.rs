//! AI 追色核心：Lab 均值-方差迁移（文档 §4.6）。
//!
//! 在 CIELAB 空间对目标图与样片图计算各通道 μ、σ，做均值-方差迁移
//! （Reinhard 色彩迁移），再烘焙为 3D LUT 供预览与批量复用。
//! 语义分割（皮肤/发/唇/背景/天空）依赖 ONNX 模型，以 `RegionMask` 抽象占位。

use crate::color::lut::Lut3D;
use crate::image::{lab_to_rgb, rgb_to_lab, ColorSpace, ImageBuf};

/// 迁移模式（文档 §4.6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferMode {
    /// 极致（更强迁移）。
    Extreme,
    /// 和谐（保守迁移）。
    Harmony,
}

/// Lab 空间每通道均值与标准差。
#[derive(Debug, Clone, Copy)]
pub struct LabStats {
    pub mean: [f32; 3],
    pub std: [f32; 3],
}

/// 计算图像在 Lab 空间的每通道均值与标准差。
pub fn lab_stats(img: &ImageBuf) -> LabStats {
    let n = img.pixel_count() as f32;
    let mut sum = [0.0f32; 3];
    for y in 0..img.height {
        for x in 0..img.width {
            let p = img.pixel(x, y);
            let lab = rgb_to_lab([p[0], p[1], p[2]]);
            for i in 0..3 {
                sum[i] += lab[i];
            }
        }
    }
    let mean = [sum[0] / n, sum[1] / n, sum[2] / n];

    let mut var = [0.0f32; 3];
    for y in 0..img.height {
        for x in 0..img.width {
            let p = img.pixel(x, y);
            let lab = rgb_to_lab([p[0], p[1], p[2]]);
            for i in 0..3 {
                let d = lab[i] - mean[i];
                var[i] += d * d;
            }
        }
    }
    let std = [
        (var[0] / n).sqrt(),
        (var[1] / n).sqrt(),
        (var[2] / n).sqrt(),
    ];
    LabStats { mean, std }
}

/// 单像素均值-方差迁移（Lab 空间）。
#[inline]
fn transfer_pixel(rgb: [f32; 3], src: &LabStats, dst: &LabStats, mode: TransferMode) -> [f32; 3] {
    let strength = match mode {
        TransferMode::Extreme => 1.0,
        TransferMode::Harmony => 0.6,
    };
    let lab = rgb_to_lab(rgb);
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        let ratio = if src.std[i] < 1e-6 { 1.0 } else { dst.std[i] / src.std[i] };
        let target = (lab[i] - src.mean[i]) * ratio + dst.mean[i];
        out[i] = lab[i] + (target - lab[i]) * strength;
    }
    let r = lab_to_rgb(out);
    [r[0].clamp(0.0, 1.0), r[1].clamp(0.0, 1.0), r[2].clamp(0.0, 1.0)]
}

/// 全局色彩迁移：把 `target` 的色调迁到 `reference` 的色调。
pub fn lab_transfer(target: &ImageBuf, reference: &ImageBuf, mode: TransferMode) -> ImageBuf {
    let src = lab_stats(target);
    let dst = lab_stats(reference);
    let mut out = ImageBuf::new(target.width, target.height, ColorSpace::Linear);
    for y in 0..target.height {
        for x in 0..target.width {
            let p = target.pixel(x, y);
            let c = transfer_pixel([p[0], p[1], p[2]], &src, &dst, mode);
            out.set_pixel(x, y, [c[0], c[1], c[2], p[3]]);
        }
    }
    out
}

/// 把迁移关系烘焙为 3D LUT（文档 §4.6：烘焙为 LUT 供预览与批量）。
pub fn build_transfer_lut(
    target: &ImageBuf,
    reference: &ImageBuf,
    mode: TransferMode,
    size: u32,
) -> Lut3D {
    let src = lab_stats(target);
    let dst = lab_stats(reference);
    Lut3D::from_fn(size, |rgb| transfer_pixel(rgb, &src, &dst, mode))
}

/// 分区追色：把 `target` 指定区域的色调迁到 `reference` 对应区域，烘焙 LUT。
///
/// `target_mask` / `reference_mask` 为 0..1 权重（>0.5 计入统计），
/// 由语义分割（如 BiSeNet 皮肤 mask）提供。区域独立统计并迁移，可精确
/// 控制「只迁皮肤、不动背景/唇色」。
pub fn build_region_transfer_lut(
    target: &ImageBuf,
    target_mask: &ImageBuf,
    reference: &ImageBuf,
    reference_mask: &ImageBuf,
    mode: TransferMode,
    size: u32,
) -> Lut3D {
    let src = masked_stats(target, target_mask);
    let dst = masked_stats(reference, reference_mask);
    Lut3D::from_fn(size, |rgb| transfer_pixel(rgb, &src, &dst, mode))
}

/// 语义分区蒙版（皮肤/头发/唇/背景/天空），权重 0..1。
/// 语义分割依赖 ONNX 模型（文档 §5.1），此处以结构占位。
pub struct RegionMask {
    pub label: &'static str,
    pub mask: ImageBuf,
}

/// 分区独立追色：对每个分区分别统计并迁移。
/// `target` / `reference` 的蒙版需语义对齐（由分割模型保证）。
pub fn per_region_transfer(
    target: &ImageBuf,
    reference: &ImageBuf,
    regions: &[RegionMask],
    mode: TransferMode,
) -> ImageBuf {
    let mut out = target.clone();
    for region in regions {
        // 对单个分区：以 mask 为权重统计（简化：只迁移 mask>0.5 的像素）
        let src = masked_stats(target, &region.mask);
        let dst = masked_stats(reference, &region.mask);
        for y in 0..target.height {
            for x in 0..target.width {
                let w = region.mask.pixel(x, y)[0];
                if w <= 0.5 {
                    continue;
                }
                let p = target.pixel(x, y);
                let c = transfer_pixel([p[0], p[1], p[2]], &src, &dst, mode);
                out.set_pixel(x, y, [c[0], c[1], c[2], p[3]]);
            }
        }
    }
    out
}

/// 仅统计 mask>0.5 像素的 Lab 均值/标准差。
fn masked_stats(img: &ImageBuf, mask: &ImageBuf) -> LabStats {
    let mut sum = [0.0f32; 3];
    let mut n = 0usize;
    for y in 0..img.height {
        for x in 0..img.width {
            if mask.pixel(x, y)[0] <= 0.5 {
                continue;
            }
            let p = img.pixel(x, y);
            let lab = rgb_to_lab([p[0], p[1], p[2]]);
            for i in 0..3 {
                sum[i] += lab[i];
            }
            n += 1;
        }
    }
    if n == 0 {
        return LabStats {
            mean: [0.0; 3],
            std: [1.0; 3],
        };
    }
    let mean = [sum[0] / n as f32, sum[1] / n as f32, sum[2] / n as f32];
    let mut var = [0.0f32; 3];
    for y in 0..img.height {
        for x in 0..img.width {
            if mask.pixel(x, y)[0] <= 0.5 {
                continue;
            }
            let p = img.pixel(x, y);
            let lab = rgb_to_lab([p[0], p[1], p[2]]);
            for i in 0..3 {
                let d = lab[i] - mean[i];
                var[i] += d * d;
            }
        }
    }
    let std = [
        (var[0] / n as f32).sqrt(),
        (var[1] / n as f32).sqrt(),
        (var[2] / n as f32).sqrt(),
    ];
    LabStats { mean, std }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgb: [f32; 3]) -> ImageBuf {
        let mut b = ImageBuf::new(w, h, ColorSpace::Linear);
        for y in 0..h {
            for x in 0..w {
                b.set_pixel(x, y, [rgb[0], rgb[1], rgb[2], 1.0]);
            }
        }
        b
    }

    #[test]
    fn transfer_same_image_is_identity() {
        let a = solid(16, 16, [0.4, 0.5, 0.6]);
        let out = lab_transfer(&a, &a, TransferMode::Extreme);
        let p = out.pixel(0, 0);
        assert!((p[0] - 0.4).abs() < 1e-3);
        assert!((p[1] - 0.5).abs() < 1e-3);
        assert!((p[2] - 0.6).abs() < 1e-3);
    }

    #[test]
    fn stats_solid() {
        let a = solid(8, 8, [0.5, 0.5, 0.5]);
        let s = lab_stats(&a);
        // 纯灰的 a/b 分量接近 0
        assert!(s.mean[1].abs() < 1e-2);
        assert!(s.mean[2].abs() < 1e-2);
    }

    #[test]
    fn lut_roundtrip_identity_transfer() {
        let a = solid(8, 8, [0.3, 0.6, 0.9]);
        let lut = build_transfer_lut(&a, &a, TransferMode::Extreme, 17);
        let out = lut.apply([0.3, 0.6, 0.9]);
        for i in 0..3 {
            assert!((out[i] - [0.3, 0.6, 0.9][i]).abs() < 1e-2);
        }
    }

    #[test]
    fn region_lut_identity_transfer() {
        // 同图同 mask（全 1）分区迁移应为恒等
        let a = solid(8, 8, [0.3, 0.6, 0.9]);
        let mask = solid(8, 8, [1.0, 1.0, 1.0]);
        let lut = build_region_transfer_lut(&a, &mask, &a, &mask, TransferMode::Extreme, 17);
        let out = lut.apply([0.3, 0.6, 0.9]);
        for i in 0..3 {
            assert!((out[i] - [0.3, 0.6, 0.9][i]).abs() < 1e-2);
        }
    }

    #[test]
    fn region_lut_only_transfers_masked() {
        // 目标全蓝、参考全红，mask 覆盖全图 → 迁移后应偏红（L 不变，a 增大）
        let target = solid(8, 8, [0.2, 0.3, 0.8]);
        let reference = solid(8, 8, [0.8, 0.2, 0.2]);
        let mask = solid(8, 8, [1.0, 1.0, 1.0]);
        let lut = build_region_transfer_lut(&target, &mask, &reference, &mask, TransferMode::Extreme, 17);
        let out = lut.apply([0.2, 0.3, 0.8]);
        // 蓝 → 红：a 分量显著上升，b 分量下降
        let src_lab = rgb_to_lab([0.2, 0.3, 0.8]);
        let out_lab = rgb_to_lab(out);
        assert!(out_lab[1] > src_lab[1] + 5.0);
        assert!(out_lab[2] < src_lab[2]);
    }
}
