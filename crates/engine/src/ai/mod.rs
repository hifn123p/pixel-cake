//! AI 推理门面：持有各检测/增强模型会话，为处理管线提供 AI 预测。
//!
//! 模型文件不入库，用户应用内下载到 `models_dir`。缺失的模型对应功能
//! 自动降级（检测返回 `None`），管线退化为纯参数处理。
//!
//! 当前接入：SCRFD 人脸检测（磨皮/美型的前置），由 5 点关键点生成
//! 「大眼」液化点，驱动美型 warp；2DFAN4 68 点关键点（精细美型）；
//! BiSeNet 19 类人脸解析（追色分区）。

use std::path::Path;

use crate::detect::face::{DetectedFace, FaceDetector};
use crate::detect::landmark::Landmarker;
use crate::detect::segment::{SegMask, Segmenter};
use crate::image::ImageBuf;
use crate::retouch::beauty::LiquifyPoint;

/// AI 门面：惰性加载可用模型（缺失则对应检测返回 `None`）。
pub struct RetouchEngine {
    face_detector: Option<FaceDetector>,
    landmarker: Option<Landmarker>,
    segmenter: Option<Segmenter>,
}

impl RetouchEngine {
    /// 从模型目录构建；`scrfd_2.5g.onnx` / `2dfan4.onnx` / `bisenet_resnet_34.onnx`
    /// 存在才加载对应检测器。
    pub fn new(models_dir: &Path) -> Self {
        let face_path = models_dir.join("scrfd_2.5g.onnx");
        let face_detector = if face_path.exists() {
            match FaceDetector::new(face_path.to_str().unwrap_or_default()) {
                Ok(d) => Some(d),
                Err(e) => {
                    eprintln!("[retouch] 加载人脸检测器失败，磨皮/美型将降级: {e}");
                    None
                }
            }
        } else {
            None
        };

        let landmark_path = models_dir.join("2dfan4.onnx");
        let landmarker = if landmark_path.exists() {
            match Landmarker::new(landmark_path.to_str().unwrap_or_default()) {
                Ok(l) => Some(l),
                Err(e) => {
                    eprintln!("[retouch] 加载关键点检测器失败，精细美型将降级: {e}");
                    None
                }
            }
        } else {
            None
        };

        let segment_path = models_dir.join("bisenet_resnet_34.onnx");
        let segmenter = if segment_path.exists() {
            match Segmenter::new(segment_path.to_str().unwrap_or_default()) {
                Ok(s) => Some(s),
                Err(e) => {
                    eprintln!("[retouch] 加载人脸解析器失败，追色分区将降级: {e}");
                    None
                }
            }
        } else {
            None
        };

        Self {
            face_detector,
            landmarker,
            segmenter,
        }
    }

    /// 是否已加载人脸检测器。
    pub fn has_face_detector(&self) -> bool {
        self.face_detector.is_some()
    }

    /// 是否已加载关键点检测器。
    pub fn has_landmarker(&self) -> bool {
        self.landmarker.is_some()
    }

    /// 是否已加载人脸解析器。
    pub fn has_segmenter(&self) -> bool {
        self.segmenter.is_some()
    }

    /// 检测图像中的所有人脸（模型未加载或推理失败时返回 `None`）。
    pub fn detect_faces(&mut self, img: &ImageBuf) -> Option<Vec<DetectedFace>> {
        self.face_detector
            .as_mut()
            .and_then(|d| d.detect(img).ok())
    }

    /// 对单张人脸做 68 点关键点检测（模型未加载或推理失败时返回 `None`）。
    pub fn detect_landmarks(
        &mut self,
        img: &ImageBuf,
        bbox: [f32; 4],
    ) -> Option<[[f32; 2]; 68]> {
        self.landmarker
            .as_mut()
            .and_then(|l| l.detect(img, bbox).ok())
    }

    /// 对单张人脸做 19 类分割（模型未加载或推理失败时返回 `None`）。
    pub fn segment_face(&mut self, img: &ImageBuf, bbox: [f32; 4]) -> Option<SegMask> {
        self.segmenter
            .as_mut()
            .and_then(|s| s.segment(img, bbox).ok())
    }

    /// 由 5 点关键点生成「大眼」液化点（归一化坐标）。
    ///
    /// 对每只眼睛在上下左右 4 个方向放置向外推的控制点，形成放大效果。
    /// 眼睛半径按两眼间距估算，推拉强度与半径成比例。
    pub fn auto_beauty_points(faces: &[DetectedFace], w: u32, h: u32) -> Vec<LiquifyPoint> {
        let wf = w.max(1) as f32;
        let hf = h.max(1) as f32;
        let mut pts = Vec::new();

        for f in faces {
            // landmarks[0]=左眼, [1]=右眼（SCRFD 5 点顺序）
            let (lx, ly) = (f.landmarks[0][0], f.landmarks[0][1]);
            let (rx, ry) = (f.landmarks[1][0], f.landmarks[1][1]);
            let eye_dist = ((rx - lx).powi(2) + (ry - ly).powi(2)).sqrt().max(1.0);
            let r = eye_dist / 6.0; // 眼睛半径估算
            let k = r * 0.12; // 推拉强度

            for (cx, cy) in [(lx, ly), (rx, ry)] {
                // 四方向向外推（大眼）
                let dirs = [
                    (cx - r, cy, -k, 0.0),
                    (cx + r, cy, k, 0.0),
                    (cx, cy - r, 0.0, -k),
                    (cx, cy + r, 0.0, k),
                ];
                for (px, py, dx, dy) in dirs {
                    pts.push(LiquifyPoint {
                        x: px / wf,
                        y: py / hf,
                        dx: dx / wf,
                        dy: dy / hf,
                        radius: r / wf,
                    });
                }
            }
        }
        pts
    }

    /// 由 68 点关键点生成「瘦脸 + 大眼」液化点（归一化坐标）。
    ///
    /// - 瘦脸：下颌轮廓点（索引 3..=13）朝脸部中心（鼻尖 33）向内推；
    /// - 大眼：双眼轮廓点（36..=41 / 42..=47）从眼中心向外推。
    ///
    /// 68 点顺序为 dlib/FAN 标准（0-16 轮廓、17-26 眉、27-35 鼻、36-47 眼、48-67 嘴）。
    pub fn face_beauty_points(landmarks: &[[f32; 2]; 68], w: u32, h: u32) -> Vec<LiquifyPoint> {
        let wf = w.max(1) as f32;
        let hf = h.max(1) as f32;
        let mut pts = Vec::new();

        // 脸部中心：鼻尖（索引 33）
        let (cx, cy) = (landmarks[33][0], landmarks[33][1]);

        // 1. 瘦脸：下颌轮廓向内推
        for i in 3..=13usize {
            let (px, py) = (landmarks[i][0], landmarks[i][1]);
            let vx = px - cx;
            let vy = py - cy;
            let dist = (vx * vx + vy * vy).sqrt().max(1.0);
            let k = dist * 0.02; // 推拉强度随距中心距离增大
            pts.push(LiquifyPoint {
                x: px / wf,
                y: py / hf,
                dx: -vx / dist * k / wf,
                dy: -vy / dist * k / hf,
                radius: (dist * 0.5 / wf).max(0.01),
            });
        }

        // 2. 大眼：双眼轮廓向外推
        for eye_range in [36..=41usize, 42..=47usize] {
            let mut ex = 0.0f32;
            let mut ey = 0.0f32;
            for i in eye_range.clone() {
                ex += landmarks[i][0];
                ey += landmarks[i][1];
            }
            ex /= 6.0;
            ey /= 6.0;
            for i in eye_range {
                let (px, py) = (landmarks[i][0], landmarks[i][1]);
                let vx = px - ex;
                let vy = py - ey;
                let dist = (vx * vx + vy * vy).sqrt().max(1.0);
                let k = dist * 0.08;
                pts.push(LiquifyPoint {
                    x: px / wf,
                    y: py / hf,
                    dx: vx / dist * k / wf,
                    dy: vy / dist * k / hf,
                    radius: dist / wf,
                });
            }
        }

        pts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(lx: f32, ly: f32, rx: f32, ry: f32) -> DetectedFace {
        DetectedFace {
            bbox: [0.0, 0.0, 100.0, 100.0],
            score: 0.9,
            landmarks: [[lx, ly], [rx, ry], [50.0, 50.0], [40.0, 70.0], [60.0, 70.0]],
        }
    }

    #[test]
    fn auto_beauty_points_count() {
        // 一张脸 → 2 眼 × 4 方向 = 8 个液化点
        let faces = vec![face(40.0, 40.0, 60.0, 40.0)];
        let pts = RetouchEngine::auto_beauty_points(&faces, 100, 100);
        assert_eq!(pts.len(), 8);
    }

    #[test]
    fn auto_beauty_points_normalized() {
        let faces = vec![face(40.0, 40.0, 60.0, 40.0)];
        let pts = RetouchEngine::auto_beauty_points(&faces, 100, 100);
        for p in &pts {
            assert!((0.0..=1.0).contains(&p.x));
            assert!((0.0..=1.0).contains(&p.y));
            assert!(p.radius > 0.0);
        }
    }

    #[test]
    fn face_beauty_points_count() {
        // 构造 68 点：鼻尖 (50,50)，下颌向外放，双眼各 6 点
        let mut lm = [[50.0f32, 50.0]; 68];
        for i in 3..=13 {
            lm[i] = [50.0 + (i as f32 - 8.0) * 5.0, 70.0];
        }
        for i in 36..=41 {
            lm[i] = [40.0 + (i as f32 - 36.0) * 2.0, 40.0];
        }
        for i in 42..=47 {
            lm[i] = [60.0 + (i as f32 - 42.0) * 2.0, 40.0];
        }
        // 11 个瘦脸点 + 12 个大眼点
        let pts = RetouchEngine::face_beauty_points(&lm, 100, 100);
        assert_eq!(pts.len(), 23);
        for p in &pts {
            assert!((0.0..=1.0).contains(&p.x));
            assert!((0.0..=1.0).contains(&p.y));
        }
    }
}
