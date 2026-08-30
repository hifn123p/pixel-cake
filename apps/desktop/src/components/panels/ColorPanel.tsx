// AI 追色面板（文档 §4.6）：参考样片 + 追色模式。
// 参考样片支持原生文件对话框选择，或手动输入路径。

import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Color, ColorTransferMode } from "../../api/types";

interface Props {
  value: Color;
  onChange: (next: Color) => void;
  /** 文件对话框起始目录（设置里的默认导入目录）。 */
  defaultPath?: string;
}

export default function ColorPanel({ value, onChange, defaultPath }: Props) {
  const set = (patch: Partial<Color>) => onChange({ ...value, ...patch });

  async function pickReference() {
    const picked = await openDialog({
      directory: false,
      multiple: false,
      title: "选择追色参考样片",
      defaultPath,
      filters: [
        { name: "照片", extensions: ["jpg", "jpeg", "png", "cr2", "arw", "nef", "dng"] },
        { name: "所有文件", extensions: ["*"] },
      ],
    });
    if (typeof picked === "string" && picked) {
      set({ reference_path: picked });
    }
  }

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
        <span>参考样片</span>
        <input
          type="text"
          value={value.reference_path ?? ""}
          onChange={(e) => set({ reference_path: e.target.value || null })}
          placeholder="目标色调参考图路径"
        />
        <button className="btn-mini" onClick={pickReference}>
          选择…
        </button>
      </label>
      {value.reference_path && (
        <div className="muted row-path" title={value.reference_path}>
          {value.reference_path.split(/[\\/]/).pop()}
        </div>
      )}

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
