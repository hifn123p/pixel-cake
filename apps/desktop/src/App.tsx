import { useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { api } from "./api/client";
import { defaultRecipe, type Photo, type Project, type Recipe } from "./api/types";
import Canvas, { type Point2D } from "./components/canvas/Canvas";
import SettingsModal, {
  DEFAULT_SETTINGS,
  type AppSettings,
} from "./components/SettingsModal";
import BasePanel from "./components/panels/BasePanel";
import BeautyPanel from "./components/panels/BeautyPanel";
import ColorPanel from "./components/panels/ColorPanel";
import FilterPanel from "./components/panels/FilterPanel";
import InpaintPanel from "./components/panels/InpaintPanel";
import ModelManager from "./components/panels/ModelManager";
import NeutralGrayPanel from "./components/panels/NeutralGrayPanel";
import PresetPanel from "./components/panels/PresetPanel";

/** 导入对话框支持的图片格式过滤器。 */
const PHOTO_FILTERS = [
  {
    name: "照片",
    extensions: [
      "cr2", "arw", "nef", "dng", "orf", "rw2", "pef", "raf", "srw", "3fr", "raw",
      "jpg", "jpeg", "png", "ppm",
    ],
  },
  { name: "所有文件", extensions: ["*"] },
];

export default function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [photos, setPhotos] = useState<Photo[]>([]);
  const [photo, setPhoto] = useState<Photo | null>(null);
  const [recipe, setRecipe] = useState<Recipe>(defaultRecipe());
  const [progress, setProgress] = useState<number | null>(null);
  const [previewSrc, setPreviewSrc] = useState<string | null>(null);
  const [drawing, setDrawing] = useState(false);
  const [draftPoints, setDraftPoints] = useState<Point2D[]>([]);
  const [newName, setNewName] = useState("");
  const [about, setAbout] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [batchInfo, setBatchInfo] = useState<string | null>(null);
  const renderTimer = useRef<number | null>(null);
  const batchTotal = useRef(0);
  const batchDone = useRef(0);

  // 启动：加载项目列表 + 用户设置；订阅引擎事件。
  useEffect(() => {
    api.listProjects().then(setProjects).catch(console.error);

    api
      .getSettings()
      .then((kv) => {
        setSettings((prev) => ({
          ...prev,
          theme: kv.theme === "light" ? "light" : "dark",
          previewMaxEdge: kv.preview_max_edge ? Number(kv.preview_max_edge) : prev.previewMaxEdge,
          exportFormat: kv.export_format === "png" ? "png" : "tiff",
          exportDir: kv.export_dir ?? "",
          importDir: kv.import_dir ?? "",
        }));
      })
      .catch(console.error);

    const unlisten = api.onEngineEvent((e) => {
      if (e.type === "progress") setProgress(e.pct);
      else if (e.type === "done") {
        setProgress(null);
        // 批量导出进度：累计完成数
        if (batchTotal.current > 0) {
          batchDone.current += 1;
          if (batchDone.current >= batchTotal.current) {
            setBatchInfo(null);
            batchTotal.current = 0;
            batchDone.current = 0;
          } else {
            setBatchInfo(`批量导出 ${batchDone.current}/${batchTotal.current}`);
          }
        }
        // 读取 PNG 预览（后端按 photo_id 查询路径）
        api
          .readPreview(e.photo_id)
          .then((b64) => setPreviewSrc(`data:image/png;base64,${b64}`))
          .catch(console.error);
      } else if (e.type === "error") {
        console.error(e.message);
        setError(e.message);
        setProgress(null);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // 主题切换：作用于 CSS 变量（html[data-theme]）。
  useEffect(() => {
    document.documentElement.dataset.theme = settings.theme;
    api.saveSetting("theme", settings.theme).catch(console.error);
  }, [settings.theme]);

  // 设置变更即持久化（除主题外由上方统一处理）。
  function updateSettings(next: AppSettings) {
    setSettings(next);
    api.saveSetting("preview_max_edge", String(next.previewMaxEdge)).catch(console.error);
    api.saveSetting("export_format", next.exportFormat).catch(console.error);
    api.saveSetting("export_dir", next.exportDir).catch(console.error);
    api.saveSetting("import_dir", next.importDir).catch(console.error);
  }

  /** 打开项目：原生目录对话框选择照片文件夹，后端扫描建项目并导入。 */
  async function openProject() {
    try {
      const dir = await openDialog({
        directory: true,
        multiple: false,
        title: "打开照片文件夹",
        defaultPath: settings.importDir || undefined,
      });
      if (typeof dir !== "string" || !dir) return;
      setBusy(true);
      setError(null);
      const [proj, ph] = await api.openProject(dir);
      setProjects(await api.listProjects());
      setProjectId(proj.id);
      setPhotos(ph);
      setPhoto(null);
      setPreviewSrc(null);
      api.saveSetting("last_project", proj.id).catch(console.error);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function createProject() {
    if (!newName.trim()) return;
    try {
      await api.createProject(newName.trim());
      setProjects(await api.listProjects());
      setNewName("");
    } catch (e) {
      setError(String(e));
    }
  }

  async function selectProject(id: string) {
    setProjectId(id);
    setPhoto(null);
    setPreviewSrc(null);
    try {
      setPhotos(await api.listPhotos(id));
    } catch (e) {
      setError(String(e));
    }
  }

  async function selectPhoto(p: Photo) {
    setPhoto(p);
    try {
      const r = await api.getRecipe(p.id);
      setRecipe(r ?? defaultRecipe());
      setDraftPoints([]);
      setDrawing(false);
    } catch (e) {
      setError(String(e));
    }
  }

  /** 导入照片：原生文件对话框多选。 */
  async function importPhotos() {
    if (!projectId) {
      setError("请先选择或打开一个项目");
      return;
    }
    try {
      const picked = await openDialog({
        directory: false,
        multiple: true,
        title: "导入照片",
        filters: PHOTO_FILTERS,
        defaultPath: settings.importDir || undefined,
      });
      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      const files = paths.filter((p): p is string => typeof p === "string");
      if (files.length === 0) return;
      setBusy(true);
      setError(null);
      await api.importPhotos(projectId, files);
      setPhotos(await api.listPhotos(projectId));
      if (files.length > 0) {
        const dir = files[0].split(/[\\/]/).slice(0, -1).join("/");
        if (dir && !settings.importDir) {
          updateSettings({ ...settings, importDir: dir });
        }
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  // 编辑即保存，并提交代理图预览（文档 §4.8：预览与重算靠 recipe 解耦）。
  // 预览重算带 200ms 防抖，避免滑块拖动时高频触发后端全链路。
  function updateRecipe(next: Recipe) {
    setRecipe(next);
    if (!photo) return;
    api.saveRecipe(photo.id, next).catch(console.error);
    if (renderTimer.current) window.clearTimeout(renderTimer.current);
    renderTimer.current = window.setTimeout(() => {
      api
        .submitRender(photo.id, next, "preview", {
          previewMaxEdge: settings.previewMaxEdge,
        })
        .catch(console.error);
    }, 200);
  }

  /** 导出当前照片：原生目录对话框选择导出位置（可选）。 */
  async function exportPhoto() {
    if (!photo) return;
    let dir = settings.exportDir || undefined;
    if (!dir) {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: "选择导出目录",
      });
      if (typeof picked === "string" && picked) dir = picked;
      else return;
    }
    try {
      setError(null);
      await api.submitRender(photo.id, recipe, "export", {
        exportDir: dir,
        exportFormat: settings.exportFormat,
      });
    } catch (e) {
      setError(String(e));
    }
  }

  // 批量导出：对当前项目全部照片应用当前 recipe，统一风格导出（后台队列逐个处理）。
  async function exportAll() {
    if (!projectId || photos.length === 0) return;
    const picked = await openDialog({
      directory: true,
      multiple: false,
      title: "选择批量导出目录",
    });
    if (typeof picked !== "string" || !picked) return;
    batchTotal.current = photos.length;
    batchDone.current = 0;
    setBatchInfo(`批量导出 0/${photos.length}`);
    photos.forEach((p) =>
      api
        .submitRender(p.id, recipe, "export", {
          exportDir: picked,
          exportFormat: settings.exportFormat,
        })
        .catch((e) => setError(String(e)))
    );
  }

  /** 在资源管理器中定位导出结果。 */
  async function revealResult() {
    if (!photo) return;
    const ext = settings.exportFormat === "png" ? "png" : "tiff";
    try {
      await api.revealInExplorer(`${photo.raw_path}.out.${ext}`);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="app-shell">
      <header className="menubar">
        <span className="brand">像素蛋糕</span>
        <nav className="menu">
          <span className="menu-group">文件</span>
          <button onClick={openProject}>打开项目…</button>
          <button onClick={importPhotos} disabled={!projectId}>
            导入照片…
          </button>
          <button disabled={!photo} onClick={exportPhoto}>
            导出…
          </button>
          <button disabled={!projectId || photos.length === 0} onClick={exportAll}>
            批量导出…
          </button>
          <span className="menu-sep" />
          <span className="menu-group">视图</span>
          <button onClick={() => setSettingsOpen(true)}>设置…</button>
          <button onClick={() => setAbout(true)}>关于</button>
        </nav>
        <span className="menubar-right">
          {busy ? "处理中…" : `v0.2.0 · ${settings.theme === "light" ? "浅色" : "深色"}`}
        </span>
      </header>

      <div className="app">
        <aside className="sidebar">
          <div className="block">
            <input
              placeholder="新建项目名"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && createProject()}
            />
            <button onClick={createProject}>新建</button>
          </div>
          <button className="btn-outline block-full" onClick={openProject}>
            打开照片文件夹…
          </button>

          <h4 className="sidebar-title">项目</h4>
          {projects.length === 0 ? (
            <p className="empty-hint">还没有项目。新建或打开一个照片文件夹。</p>
          ) : (
            <ul className="list">
              {projects.map((p) => (
                <li
                  key={p.id}
                  className={p.id === projectId ? "active" : ""}
                  onClick={() => selectProject(p.id)}
                  title={p.root_path}
                >
                  {p.name}
                </li>
              ))}
            </ul>
          )}

          <h4 className="sidebar-title">照片</h4>
          {projectId && photos.length === 0 ? (
            <p className="empty-hint">项目内还没有照片，点上方「导入照片…」。</p>
          ) : (
            <ul className="list">
              {photos.map((p) => (
                <li
                  key={p.id}
                  className={p.id === photo?.id ? "active" : ""}
                  onClick={() => selectPhoto(p)}
                  title={p.raw_path}
                >
                  {p.raw_path.split(/[\\/]/).pop()}
                  {p.width > 0 && (
                    <span className="muted">
                      {p.width}×{p.height}
                    </span>
                  )}
                  <span className={`status status-${p.status}`}>{p.status}</span>
                </li>
              ))}
            </ul>
          )}
        </aside>

        <main className="canvas-area">
          <Canvas
            photoName={photo?.raw_path ?? null}
            previewSrc={previewSrc}
            progress={progress}
            drawing={drawing}
            draftPoints={draftPoints}
            onImageClick={(nx, ny) => setDraftPoints((p) => [...p, { x: nx, y: ny }])}
          />
        </main>

        <aside className="inspector">
          <NeutralGrayPanel
            value={recipe.neutral_gray}
            onChange={(v) => updateRecipe({ ...recipe, neutral_gray: v })}
          />
          <BeautyPanel
            value={recipe.beauty}
            onChange={(v) => updateRecipe({ ...recipe, beauty: v })}
          />
          <ColorPanel
            value={recipe.color}
            onChange={(v) => updateRecipe({ ...recipe, color: v })}
            defaultPath={settings.importDir || undefined}
          />
          <InpaintPanel
            value={recipe.inpaint}
            onChange={(v) => updateRecipe({ ...recipe, inpaint: v })}
            drawing={drawing}
            onDrawingChange={setDrawing}
            draftPoints={draftPoints}
            onDraftChange={setDraftPoints}
          />
          <BasePanel
            value={recipe.base}
            onChange={(v) => updateRecipe({ ...recipe, base: v })}
          />
          <FilterPanel
            value={recipe.filter}
            onChange={(v) => updateRecipe({ ...recipe, filter: v })}
          />
          <div className="export-row">
            <button
              className="export-btn"
              disabled={!photo}
              onClick={exportPhoto}
            >
              导出（全分辨率）
            </button>
            <button
              className="btn-ghost square"
              title="在资源管理器中显示导出结果"
              disabled={!photo}
              onClick={revealResult}
            >
              📂
            </button>
          </div>

          <PresetPanel currentRecipe={recipe} onApplyRecipe={updateRecipe} />

          <ModelManager />
        </aside>
      </div>

      <footer className="statusbar">
        <span>{photo ? `当前：${photo.raw_path.split(/[\\/]/).pop()}` : "未选择照片"}</span>
        {progress !== null && <span>处理中 {Math.round(progress)}%</span>}
        {batchInfo && <span>{batchInfo}</span>}
        {error && (
          <span className="statusbar-error" title={error}>
            ⚠ {error.slice(0, 60)}
          </span>
        )}
        <span className="statusbar-right">本地 GPU 推理</span>
      </footer>

      {about && (
        <div className="modal-mask" onClick={() => setAbout(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2>像素蛋糕</h2>
            <p className="muted">版本 0.2.0</p>
            <p>
              本地 AI 修图客户端：中性灰磨皮、AI 美型、祛瑕、AI 追色、基础调色、滤镜。
            </p>
            <p className="muted">
              数据全部本地存储，AI 推理在本地 NVIDIA GPU（RTX 3070）完成，无需联网。
            </p>
            <button onClick={() => setAbout(false)}>关闭</button>
          </div>
        </div>
      )}

      {settingsOpen && (
        <SettingsModal
          settings={settings}
          onSettings={updateSettings}
          onClose={() => setSettingsOpen(false)}
        />
      )}
    </div>
  );
}
