// AI 追色面板（文档 §4.6）：参考样片 + 追色模式。

import type { Color, ColorTransferMode } from "../../api/types";

interface Props {
  value: Color;
  onChange: (next: Color) => void;
}

export default function ColorPanel({ value, onChange }: Props) {
  const set = (patch: Partial<Color>) => onChange({ ...value, ...patch });

  return (
    <div className="panel">
      <h3>AI 追色</h3>
      <label className="row">
        <input
          type="checkbox"
          checked={value.enabled}
          onChange={(e) => set({ enabled: e.target.checked })}
        />
        启用
      </label>

      <label className="row">
        <span>参考样片路径</span>
        <input
          type="text"
          value={value.reference_path ?? ""}
          onChange={(e) => set({ reference_path: e.target.value || null })}
          placeholder="目标色调参考图路径"
        />
      </label>

      <label className="row">
        <span>模式</span>
        <select
          value={value.mode}
          onChange={(e) => set({ mode: e.target.value as ColorTransferMode })}
        >
          <option value="Harmony">和谐（保守）</option>
          <option value="Extreme">极致（强烈）</option>
        </select>
      </label>
    </div>
  );
}
