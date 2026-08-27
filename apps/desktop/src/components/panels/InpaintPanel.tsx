// 祛瑕面板（文档 §4.5）：画布上绘制多边形区域 + 区域管理。

import type { InpaintKind, InpaintRegion, Point } from "../../api/types";
import type { Point2D } from "../canvas/Canvas";

interface Props {
  value: InpaintRegion[];
  onChange: (next: InpaintRegion[]) => void;
  drawing: boolean;
  onDrawingChange: (d: boolean) => void;
  draftPoints: Point2D[];
  onDraftChange: (pts: Point2D[]) => void;
}

export default function InpaintPanel({
  value,
  onChange,
  drawing,
  onDrawingChange,
  draftPoints,
  onDraftChange,
}: Props) {
  function finish() {
    if (draftPoints.length < 3) {
      onDraftChange([]);
      onDrawingChange(false);
      return;
    }
    const polygon: Point[] = draftPoints.map((p) => ({ x: p.x, y: p.y }));
    onChange([...value, { polygon, kind: "Blemish" as InpaintKind }]);
    onDraftChange([]);
    onDrawingChange(false);
  }

  function remove(i: number) {
    onChange(value.filter((_, idx) => idx !== i));
  }

  return (
    <div className="panel">
      <h3>祛瑕</h3>
      {drawing ? (
        <div className="row">
          <span>已选 {draftPoints.length} 个顶点</span>
          <button onClick={finish} disabled={draftPoints.length < 3}>
            完成区域
          </button>
          <button onClick={() => { onDraftChange([]); onDrawingChange(false); }}>
            取消
          </button>
        </div>
      ) : (
        <button onClick={() => onDrawingChange(true)}>绘制祛瑕区域</button>
      )}

      <ul className="list">
        {value.map((r, i) => (
          <li key={i} className="row">
            <span>区域 {i + 1}（{r.polygon.length} 点）</span>
            <button onClick={() => remove(i)}>删除</button>
          </li>
        ))}
      </ul>
    </div>
  );
}
