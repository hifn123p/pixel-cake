// 基础调色面板（文档 §4.7）：曝光/对比度/白平衡。

import type { Base } from "../../api/types";

interface Props {
  value: Base;
  onChange: (next: Base) => void;
}

export default function BasePanel({ value, onChange }: Props) {
  const set = (patch: Partial<Base>) => onChange({ ...value, ...patch });

  return (
    <div className="panel">
      <h3>基础调色</h3>
      <label className="row">
        <span>曝光</span>
        <input
          type="range"
          min={-5}
          max={5}
          step={0.1}
          value={value.exposure}
          onChange={(e) => set({ exposure: Number(e.target.value) })}
        />
        <b>{value.exposure.toFixed(1)}</b>
      </label>

      <label className="row">
        <span>对比度</span>
        <input
          type="range"
          min={-100}
          max={100}
          value={value.contrast}
          onChange={(e) => set({ contrast: Number(e.target.value) })}
        />
        <b>{value.contrast}</b>
      </label>

      <label className="row">
        <span>色温</span>
        <input
          type="range"
          min={-100}
          max={100}
          value={value.temperature}
          onChange={(e) => set({ temperature: Number(e.target.value) })}
        />
        <b>{value.temperature}</b>
      </label>

      <label className="row">
        <span>色调</span>
        <input
          type="range"
          min={-100}
          max={100}
          value={value.tint}
          onChange={(e) => set({ tint: Number(e.target.value) })}
        />
        <b>{value.tint}</b>
      </label>
    </div>
  );
}
