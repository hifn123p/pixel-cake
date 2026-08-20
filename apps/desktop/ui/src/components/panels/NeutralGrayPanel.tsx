// 中性灰磨皮面板（文档 §4.3）：双参数 ka(平整)、kb(立体)。

import type { NeutralGray } from "../../api/types";

interface Props {
  value: NeutralGray;
  onChange: (next: NeutralGray) => void;
}

export default function NeutralGrayPanel({ value, onChange }: Props) {
  const set = (patch: Partial<NeutralGray>) => onChange({ ...value, ...patch });

  return (
    <div className="panel">
      <h3>中性灰磨皮</h3>
      <label className="row">
        <input
          type="checkbox"
          checked={value.enabled}
          onChange={(e) => set({ enabled: e.target.checked })}
        />
        启用
      </label>

      <label className="row">
        <span>平整 ka</span>
        <input
          type="range"
          min={0}
          max={100}
          value={value.ka}
          onChange={(e) => set({ ka: Number(e.target.value) })}
        />
        <b>{value.ka}</b>
      </label>

      <label className="row">
        <span>立体 kb</span>
        <input
          type="range"
          min={0}
          max={100}
          value={value.kb}
          onChange={(e) => set({ kb: Number(e.target.value) })}
        />
        <b>{value.kb}</b>
      </label>

      <label className="row">
        <span>模式</span>
        <select
          value={value.mode}
          onChange={(e) => set({ mode: e.target.value as NeutralGray["mode"] })}
        >
          <option value="Dual">平整 + 立体</option>
          <option value="FlatOnly">仅平整</option>
          <option value="StructureOnly">仅立体</option>
        </select>
      </label>
    </div>
  );
}
