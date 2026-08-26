//! SCRFD 人脸检测器（InsightFace SCRFD-2.5G，ONNX）。
//!
//! 后处理参考 FaceFusion / InsightFace 的 SCRFD 解码逻辑：
//! 3 个特征尺度（stride 8/16/32），distance-based bbox 与关键点回归，
//! 输出 9 个张量（score/bbox/kps 各 3 尺度），最后 NMS 去重。
//!
//! 注意：模型输入输出约定（输出名、通道顺序、归一化）为 InsightFace SCRFD
//! 标准约定，实际运行需模型文件就位后验证微调。

use ndarray::Array4;
use ort::value::Tensor;

use crate::image::{linear_to_srgb, ImageBuf};
use crate::infer::{resize_bilinear, InferSession};

/// 检测到的人脸（坐标为原图像素坐标）。
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedFace {
    /// 边界框 `[x1, y1, x2, y2]`（原图像素）。
    pub bbox: [f32; 4],
    /// 置信度。
    pub score: f32,
    /// 5 点关键点（双眼、鼻尖、嘴角），原图像素坐标。
    pub landmarks: [[f32; 2]; 5],
}

/// SCRFD 人脸检测器。
pub struct FaceDetector {
    session: InferSession,
    input_size: u32,
    score_threshold: f32,
    iou_threshold: f32,
}

/// 特征金字塔 stride（8/16/32）。
const FEATURE_STRIDES: [u32; 3] = [8, 16, 32];
/// 每个特征位置 anchor 数。
const NUM_ANCHORS: usize = 2;

impl FaceDetector {
    /// 从 `.onnx` 模型文件创建检测器（CUDA EP）。
    pub fn new(model_path: &str) -> Result<Self, String> {
        Ok(Self {
            session: InferSession::from_file_cuda(model_path)?,
            input_size: 640,
            score_threshold: 0.5,
            iou_threshold: 0.4,
        })
    }

    /// 检测图像中的所有人脸。
    pub fn detect(&mut self, img: &ImageBuf) -> Result<Vec<DetectedFace>, String> {
        let size = self.input_size;
        let tensor = scrfd_preprocess(img, size)?;

        let output_names = [
            "score_8", "score_16", "score_32", "bbox_8", "bbox_16", "bbox_32", "kps_8",
            "kps_16", "kps_32",
        ];
        let outputs = self.session.run("input", tensor, &output_names)?;

        let ratio_x = img.width as f32 / size as f32;
        let ratio_y = img.height as f32 / size as f32;

        let mut faces = Vec::new();
        for (i, &stride) in FEATURE_STRIDES.iter().enumerate() {
            let (score_shape, score_data) = &outputs[i];
            let (_bbox_shape, bbox_data) = &outputs[i + 3];
            let (_kps_shape, kps_data) = &outputs[i + 6];

            // score: [1, 2, H, W]
            let h = score_shape[2];
            let w = score_shape[3];

            for ah in 0..h {
                for aw in 0..w {
                    let cx = (aw as f32 + 0.5) * stride as f32;
                    let cy = (ah as f32 + 0.5) * stride as f32;

                    for a in 0..NUM_ANCHORS {
                        let s = score_data[nchw_idx(2, h, w, a, ah, aw)];
                        if s < self.score_threshold {
                            continue;
                        }

                        // bbox 通道：anchor a 的 [left, top, right, bottom]
                        let b0 = a * 4;
                        let left = bbox_data[nchw_idx(8, h, w, b0, ah, aw)] * stride as f32;
                        let top = bbox_data[nchw_idx(8, h, w, b0 + 1, ah, aw)] * stride as f32;
                        let right = bbox_data[nchw_idx(8, h, w, b0 + 2, ah, aw)] * stride as f32;
                        let bottom = bbox_data[nchw_idx(8, h, w, b0 + 3, ah, aw)] * stride as f32;

                        let bbox = [
                            (cx - left) * ratio_x,
                            (cy - top) * ratio_y,
                            (cx + right) * ratio_x,
                            (cy + bottom) * ratio_y,
                        ];

                        // kps 通道：anchor a 的 5 点 (dx, dy)
                        let k0 = a * 10;
                        let mut landmarks = [[0.0f32; 2]; 5];
                        for k in 0..5 {
                            let dx =
                                kps_data[nchw_idx(20, h, w, k0 + k * 2, ah, aw)] * stride as f32;
                            let dy = kps_data[nchw_idx(20, h, w, k0 + k * 2 + 1, ah, aw)]
                                * stride as f32;
                            landmarks[k] = [(cx + dx) * ratio_x, (cy + dy) * ratio_y];
                        }

                        faces.push(DetectedFace {
                            bbox,
                            score: s,
                            landmarks,
                        });
                    }
                }
            }
        }

        Ok(nms(faces, self.iou_threshold))
    }
}

/// SCRFD 预处理：双线性 resize → 线性转 sRGB → BGR 顺序 → `(x*255-127.5)/128` 归一化到 `[-1,1]`。
fn scrfd_preprocess(img: &ImageBuf, size: u32) -> Result<Tensor<f32>, String> {
    let resized = resize_bilinear(img, size, size);
    let mut data = Vec::with_capacity((size * size * 3) as usize);
    // BGR 顺序（SCRFD 输入为 BGR）
    for &ch in &[2usize, 1, 0] {
        for y in 0..size {
            for x in 0..size {
                let v = resized.pixel(x, y)[ch];
                let srgb = linear_to_srgb(v.clamp(0.0, 1.0));
                let val = (srgb * 255.0 - 127.5) / 128.0;
                data.push(val);
            }
        }
    }
    let arr = Array4::from_shape_vec((1, 3, size as usize, size as usize), data)
        .map_err(|e| e.to_string())?;
    Tensor::from_array(arr).map_err(|e| e.to_string())
}

/// NCHW（batch=1）行优先索引：`data[(ci * H + hi) * W + wi]`。
#[inline]
fn nchw_idx(_c: usize, h: usize, w: usize, ci: usize, hi: usize, wi: usize) -> usize {
    (ci * h + hi) * w + wi
}

/// 贪心 NMS：按 score 降序，抑制 IoU 超过阈值的重叠框。
fn nms(mut faces: Vec<DetectedFace>, iou_threshold: f32) -> Vec<DetectedFace> {
    faces.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut suppressed = vec![false; faces.len()];
    let mut keep = Vec::new();
    for i in 0..faces.len() {
        if suppressed[i] {
            continue;
        }
        keep.push(faces[i].clone());
        for j in (i + 1)..faces.len() {
            if !suppressed[j] && iou(&faces[i].bbox, &faces[j].bbox) > iou_threshold {
                suppressed[j] = true;
            }
        }
    }
    keep
}

/// 两个 bbox 的 IoU。
fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let x1 = a[0].max(b[0]);
    let y1 = a[1].max(b[1]);
    let x2 = a[2].min(b[2]);
    let y2 = a[3].min(b[3]);
    let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let area_a = (a[2] - a[0]).max(0.0) * (a[3] - a[1]).max(0.0);
    let area_b = (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let union = area_a + area_b - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nchw_indexing() {
        // [1, 2, 3, 4]：ci=1, hi=2, wi=3 → (1*3 + 2)*4 + 3 = 23
        assert_eq!(nchw_idx(2, 3, 4, 1, 2, 3), 23);
        assert_eq!(nchw_idx(2, 3, 4, 0, 0, 0), 0);
    }

    #[test]
    fn iou_computation() {
        let a = [0.0, 0.0, 2.0, 2.0];
        let b = [1.0, 1.0, 3.0, 3.0];
        // 交集 = 1x1 = 1，并集 = 4 + 4 - 1 = 7
        assert!((iou(&a, &b) - 1.0 / 7.0).abs() < 1e-5);
        // 不相交
        let c = [10.0, 10.0, 11.0, 11.0];
        assert_eq!(iou(&a, &c), 0.0);
    }

    #[test]
    fn nms_suppresses_overlap() {
        let f1 = DetectedFace {
            bbox: [0.0, 0.0, 2.0, 2.0],
            score: 0.9,
            landmarks: [[0.0; 2]; 5],
        };
        let f2 = DetectedFace {
            bbox: [0.0, 0.0, 1.8, 1.8],
            score: 0.6,
            landmarks: [[0.0; 2]; 5],
        };
        let f3 = DetectedFace {
            bbox: [10.0, 10.0, 12.0, 12.0],
            score: 0.8,
            landmarks: [[0.0; 2]; 5],
        };
        let kept = nms(vec![f1, f2, f3], 0.4);
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().all(|f| f.score > 0.7));
    }
}
