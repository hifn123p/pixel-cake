//! AI 推理门面：持有各检测/增强模型会话，为处理管线提供 AI 预测。
//!
//! 模型文件不入库，用户应用内下载到 `models_dir`。缺失的模型对应功能
//! 自动降级（检测返回 `None`），管线退化为纯参数处理。
//!
//! 当前接入：SCRFD 人脸检测（磨皮/美型的前置），由 5 点关键点生成
//! 「大眼」液化点，驱动美型 warp；2DFAN4 68 点关键点（精细美型）。

use std::path::Path;

use crate::detect::face::{DetectedFace, FaceDetector};
use crate::detect::landmark::Landmarker;
use crate::image::ImageBuf;
use crate::retouch::beauty::LiquifyPoint;

/// AI 门面：惰性加载可用模型（缺失则对应检测返回 `None`）。
pub struct RetouchEngine {
    face_detector: Option<FaceDetector>,
    landmarker: Option<Landmarker>,
}

impl RetouchEngine {
    /// 从模型目录构建；`scrfd_2.5g.onnx` / `2dfan4.onnx` 存在才加载对应检测器。
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

        Self {
            face_detector,
            landmarker,
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
}
