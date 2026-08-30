// 设置面板（常规 Windows 应用设置）：主题 / 预览尺寸 / 导出格式 / 默认目录。
import { open } from "@tauri-apps/plugin-dialog";

export interface AppSettings {
  theme: "dark" | "light";
  previewMaxEdge: number;
  exportFormat: "tiff" | "png";
  exportDir: string;
  importDir: string;
}

export const DEFAULT_SETTINGS: AppSettings = {
  theme: "dark",
  previewMaxEdge: 1600,
  exportFormat: "tiff",
  exportDir: "",
  importDir: "",
};

interface Props {
  settings: AppSettings;
  onSettings: (s: AppSettings) => void;
  onClose: () => void;
}

export default function SettingsModal({ settings, onSettings, onClose }: Props) {
  const set = (patch: Partial<AppSettings>) => onSettings({ ...settings, ...patch });

  async function pickDir(
    key: "exportDir" | "importDir",
    title: string,
    defaultPath?: string
  ) {
    const dir = await open({
      directory: true,
      multiple: false,
      title,
      defaultPath,
    });
    if (typeof dir === "string" && dir) {
      set({ [key]: dir } as Partial<AppSettings>);
    }
  }

  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal modal-wide" onClick={(e) => e.stopPropagation()}>
        <h2 className="modal-title">设置</h2>

        <div className="settings-group">
          <div className="settings-row">
            <span className="settings-label">界面主题</span>
            <div className="seg">
              <button
                className={settings.theme === "dark" ? "seg-active" : ""}
                onClick={() => set({ theme: "dark" })}
              >
                深色
              </button>
              <button
                className={settings.theme === "light" ? "seg-active" : ""}
                onClick={() => set({ theme: "light" })}
              >
                浅色
              </button>
            </div>
          </div>

          <div className="settings-row">
            <span className="settings-label">预览尺寸</span>
            <select
              value={settings.previewMaxEdge}
              onChange={(e) => set({ previewMaxEdge: Number(e.target.value) })}
            >
              <option value={800}>小（800px，最快）</option>
              <option value={1200}>中（1200px）</option>
              <option value={1600}>大（1600px，默认）</option>
              <option value={2400}>超大（2400px，最清晰）</option>
            </select>
          </div>

          <div className="settings-row">
            <span className="settings-label">导出格式</span>
            <select
              value={settings.exportFormat}
              onChange={(e) => set({ exportFormat: e.target.value as "tiff" | "png" })}
            >
              <option value="tiff">16bit TIFF（默认，无损）</option>
              <option value="png">8bit PNG（通用）</option>
            </select>
          </div>

          <div className="settings-row">
            <span className="settings-label">默认导出目录</span>
            <div className="settings-path">
              <input
                value={settings.exportDir}
                placeholder="留空 = 与源文件同目录"
                onChange={(e) => set({ exportDir: e.target.value })}
              />
              <button onClick={() => pickDir("exportDir", "选择默认导出目录")}>
                浏览…
              </button>
            </div>
          </div>

          <div className="settings-row">
            <span className="settings-label">默认导入目录</span>
            <div className="settings-path">
              <input
                value={settings.importDir}
                placeholder="留空 = 使用上次位置"
                onChange={(e) => set({ importDir: e.target.value })}
              />
              <button onClick={() => pickDir("importDir", "选择默认导入目录")}>
                浏览…
              </button>
            </div>
          </div>
        </div>

        <div className="modal-actions">
          <button className="btn-ghost" onClick={onClose}>
            取消
          </button>
          <button onClick={onClose}>完成</button>
        </div>
      </div>
    </div>
  );
}
