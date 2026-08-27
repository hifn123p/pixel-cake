// AI 美型面板（文档 §4.4）：瘦脸等液化参数。

import type { Beauty } from "../../api/types";

interface Props {
  value: Beauty;
  onChange: (next: Beauty) => void;
}

export default function BeautyPanel({ value, onChange }: Props) {
  const set = (patch: Partial<Beauty>) => onChange({ ...value, ...patch });

  return (
    <div className="panel">
      <h3>AI 美型</h3>
      <label className="row">
        <input
          type="checkbox"
          checked={value.enabled}
          onChange={(e) => set({ enabled: e.target.checked })}
        />
        启用
      </label>

      <label className="row">
        <span>瘦脸</span>
        <input
          type="range"
          min={0}
          max={100}
          value={value.face_slim}
          onChange={(e) => set({ face_slim: Number(e.target.value) })}
        />
        <b>{value.face_slim}</b>
      </label>

      <p className="muted">瘦身 / 天鹅颈 / 面部丰盈 待引擎实现</p>
    </div>
  );
}
