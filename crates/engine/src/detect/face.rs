//! SCRFD 人脸检测器（InsightFace SCRFD-2.5G，ONNX）。
//!
//! 后处理参考 FaceFusion / InsightFace 的 SCRFD 解码逻辑：
//! 3 个特征尺度（stride 8/16/32），distance-based bbox 与关键点回归，
//! 最后 NMS 去重。
//!
//! 输入输出约定（已实测验证，模型文件 Jonny001/Models-Pack-01 scrfd_2.5g.onnx）：
//! - 输入：BGR，`(x*255-127.5)/128` 归一化到 `[-1,1]`，640×640，输入名 `input`。
//! - 输出：9 个张量，**输出名按序 `"0".."8"`**（非 `score_8` 等命名），
//!   每尺度展平为 `[N, C]` 布局（N = H·W·2，`[h, w, anchor]` 行优先）：
//!   `0/1/2` = score（[N,1]），`3/4/5` = bbox（[N,4]），`6/7/8` = kps（[N,10]）。
//!   bbox/kps 为 distance 回归值（单位 stride），score 已是 sigmoid 概率。
//! - 实测：hwa 展平序（`(hi*W+wi)*2+a`）能解出正确人脸框。

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
            session: InferSession::from_file(model_path)?,
            input_size: 640,
            score_threshold: 0.5,
            iou_threshold: 0.4,
        })
    }

    /// 检测图像中的所有人脸。
    pub fn detect(&mut self, img: &ImageBuf) -> Result<Vec<DetectedFace>, String> {
        let size = self.input_size;
        let tensor = scrfd_preprocess(img, size)?;

        // 实测：该模型输出名是 "0".."8"（0-2=score, 3-5=bbox, 6-8=kps，按 stride 8/16/32 分组），
        // 且每尺度是展平 [N, C] 布局（N = H*W*2，[h, w, anchor] 行优先），不是 NCHW。
        let output_names = ["0", "1", "2", "3", "4", "5", "6", "7", "8"];
        let outputs = self.session.run("input", tensor, &output_names)?;

        let ratio_x = img.width as f32 / size as f32;
        let ratio_y = img.height as f32 / size as f32;

        let mut faces = Vec::new();
        for (i, &stride) in FEATURE_STRIDES.iter().enumerate() {
            let (score_shape, score_data) = &outputs[i];
            let (_bbox_shape, bbox_data) = &outputs[i + 3];
            let (_kps_shape, kps_data) = &outputs[i + 6];

            // 展平布局：N = H*W*2，行优先 [h, w, anchor]
            let h = (size / stride) as usize;
            let w = (size / stride) as usize;
            debug_assert_eq!(score_shape.len(), 2);
            debug_assert_eq!(score_shape[0], h * w * NUM_ANCHORS);

            for ah in 0..h {
                for aw in 0..w {
                    let base = (ah * w + aw) * NUM_ANCHORS;

                    for a in 0..NUM_ANCHORS {
                        let n = base + a;
                        let s = score_data[n];
                        if s < self.score_threshold {
                            continue;
                        }

                        // bbox 通道：anchor a 的 [left, top, right, bottom]（distance 回归，单位 stride）
                        let b0 = n * 4;
                        let left = bbox_data[b0] * stride as f32;
                        let top = bbox_data[b0 + 1] * stride as f32;
                        let right = bbox_data[b0 + 2] * stride as f32;
                        let bottom = bbox_data[b0 + 3] * stride as f32;

                        let cx = (aw as f32 + 0.5) * stride as f32;
                        let cy = (ah as f32 + 0.5) * stride as f32;
                        let bbox = [
                            (cx - left) * ratio_x,
                            (cy - top) * ratio_y,
                            (cx + right) * ratio_x,
                            (cy + bottom) * ratio_y,
                        ];

                        // kps 通道：anchor a 的 5 点 (dx, dy)（distance 回归，单位 stride）
                        let k0 = n * 10;
                        let mut landmarks = [[0.0f32; 2]; 5];
                        for k in 0..5 {
                            let dx = kps_data[k0 + k * 2] * stride as f32;
                            let dy = kps_data[k0 + k * 2 + 1] * stride as f32;
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
    fn flat_hwa_indexing() {
        // 展平 [h, w, anchor] 布局：h=2, w=3, anchors=2，n = (hi*w+wi)*2 + a
        // (hi=1, wi=2, a=1) → (1*3+2)*2+1 = 11
        let h = 2usize;
        let w = 3usize;
        let anchors = 2usize;
        let idx = |hi: usize, wi: usize, a: usize| (hi * w + wi) * anchors + a;
        assert_eq!(idx(1, 2, 1), 11);
        assert_eq!(idx(0, 0, 0), 0);
        assert_eq!(idx(1, 2, 0), 10);
        // 总数 = h*w*anchors
        assert_eq!(h * w * anchors, 12);
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
