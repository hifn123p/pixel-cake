// 滤镜面板（文档 §4.7）：预设滤镜 LUT + 强度。

import type { Filter } from "../../api/types";

interface Props {
  value: Filter | null;
  onChange: (next: Filter | null) => void;
}

const PRESETS = [
  { id: "none", name: "无" },
  { id: "warm", name: "暖色" },
  { id: "cool", name: "冷色" },
  { id: "bw", name: "黑白" },
  { id: "vivid", name: "鲜艳" },
];

export default function FilterPanel({ value, onChange }: Props) {
  const current = value ?? { lut_id: "none", intensity: 1 };

  function setLut(lutId: string) {
    if (lutId === "none") onChange(null);
    else onChange({ lut_id: lutId, intensity: current.intensity });
  }

  function setIntensity(intensity: number) {
    if (!value) return;
    onChange({ ...value, intensity });
  }

  return (
    <div className="panel">
      <h3>滤镜</h3>
      <label className="row">
        <span>预设</span>
        <select
          value={current.lut_id}
          onChange={(e) => setLut(e.target.value)}
        >
          {PRESETS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      </label>

      {value && (
        <label className="row">
          <span>强度</span>
          <input
            type="range"
            min={0}
            max={100}
            value={Math.round(value.intensity * 100)}
            onChange={(e) => setIntensity(Number(e.target.value) / 100)}
          />
          <b>{Math.round(value.intensity * 100)}</b>
        </label>
      )}
    </div>
  );
}
