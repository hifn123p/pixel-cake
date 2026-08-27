// 画布：显示代理图预览 + 进度 + 祛瑕多边形绘制叠加层。

import { useRef } from "react";

export interface Point2D {
  x: number;
  y: number;
}

interface Props {
  photoName: string | null;
  previewSrc: string | null;
  progress: number | null;
  /** 是否处于祛瑕绘制模式。 */
  drawing: boolean;
  /** 草稿顶点（归一化 0..1）。 */
  draftPoints: Point2D[];
  /** 点击图像回调（归一化坐标）。 */
  onImageClick: (nx: number, ny: number) => void;
}

export default function Canvas({
  photoName,
  previewSrc,
  progress,
  drawing,
  draftPoints,
  onImageClick,
}: Props) {
  const imgRef = useRef<HTMLImageElement>(null);

  function handleClick(e: React.MouseEvent) {
    if (!drawing || !imgRef.current) return;
    const rect = imgRef.current.getBoundingClientRect();
    const nx = (e.clientX - rect.left) / rect.width;
    const ny = (e.clientY - rect.top) / rect.height;
    onImageClick(Math.max(0, Math.min(1, nx)), Math.max(0, Math.min(1, ny)));
  }

  const poly = draftPoints.map((p) => `${p.x * 100},${p.y * 100}`).join(" ");

  return (
    <div className="canvas">
      {previewSrc ? (
        <div className="canvas-image" onClick={handleClick}>
          <img
            ref={imgRef}
            src={previewSrc}
            alt={photoName ?? "预览"}
            className="preview-img"
            draggable={false}
          />
          {drawing && (
            <svg
              className="overlay"
              viewBox="0 0 100 100"
              preserveAspectRatio="none"
            >
              {draftPoints.length >= 2 && (
                <polygon
                  points={poly}
                  fill="rgba(255,0,0,0.15)"
                  stroke="#ff4444"
                  strokeWidth="0.6"
                />
              )}
              {draftPoints.map((p, i) => (
                <circle
                  key={i}
                  cx={p.x * 100}
                  cy={p.y * 100}
                  r="1.2"
                  fill="#ff4444"
                />
              ))}
            </svg>
          )}
          {progress !== null && (
            <div className="progress">
              <div className="progress-bar" style={{ width: `${progress}%` }} />
            </div>
          )}
        </div>
      ) : photoName ? (
        <div className="canvas-image">
          <span className="canvas-hint">{photoName}</span>
          {progress !== null && (
            <div className="progress">
              <div className="progress-bar" style={{ width: `${progress}%` }} />
            </div>
          )}
        </div>
      ) : (
        <div className="canvas-empty">
          <p>导入照片开始编辑</p>
          <p className="muted">WebGL2 实时预览引擎将在后续版本接入</p>
        </div>
      )}
    </div>
  );
}
