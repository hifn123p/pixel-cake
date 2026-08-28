import { useEffect, useRef, useState } from "react";
import { api } from "./api/client";
import { defaultRecipe, type Photo, type Project, type Recipe } from "./api/types";
import Canvas, { type Point2D } from "./components/canvas/Canvas";
import BasePanel from "./components/panels/BasePanel";
import BeautyPanel from "./components/panels/BeautyPanel";
import ColorPanel from "./components/panels/ColorPanel";
import FilterPanel from "./components/panels/FilterPanel";
import InpaintPanel from "./components/panels/InpaintPanel";
import ModelManager from "./components/panels/ModelManager";
import NeutralGrayPanel from "./components/panels/NeutralGrayPanel";
import PresetPanel from "./components/panels/PresetPanel";

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
  const [importPaths, setImportPaths] = useState("");
  const [about, setAbout] = useState(false);
  const [batchInfo, setBatchInfo] = useState<string | null>(null);
  const renderTimer = useRef<number | null>(null);
  const batchTotal = useRef(0);
  const batchDone = useRef(0);

  useEffect(() => {
    api.listProjects().then(setProjects).catch(console.error);

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
        setProgress(null);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  async function createProject() {
    if (!newName.trim()) return;
    await api.createProject(newName.trim());
    setProjects(await api.listProjects());
    setNewName("");
  }

  async function selectProject(id: string) {
    setProjectId(id);
    setPhoto(null);
    setPhotos(await api.listPhotos(id));
  }

  async function selectPhoto(p: Photo) {
    setPhoto(p);
    const r = await api.getRecipe(p.id);
    setRecipe(r ?? defaultRecipe());
  }

  async function importPhotos() {
    if (!projectId || !importPaths.trim()) return;
    const paths = importPaths
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    await api.importPhotos(projectId, paths);
    setPhotos(await api.listPhotos(projectId));
    setImportPaths("");
  }

  // 编辑即保存，并提交代理图预览（文档 §4.8：预览与重算靠 recipe 解耦）。
  // 预览重算带 200ms 防抖，避免滑块拖动时高频触发后端全链路。
  function updateRecipe(next: Recipe) {
    setRecipe(next);
    if (!photo) return;
    api.saveRecipe(photo.id, next).catch(console.error);
    if (renderTimer.current) window.clearTimeout(renderTimer.current);
    renderTimer.current = window.setTimeout(() => {
      api.submitRender(photo.id, next, "preview").catch(console.error);
    }, 200);
  }

  // 批量导出：对当前项目全部照片应用当前 recipe，统一风格导出（后台队列逐个处理）。
  function exportAll() {
    if (!projectId || photos.length === 0) return;
    batchTotal.current = photos.length;
    batchDone.current = 0;
    setBatchInfo(`批量导出 0/${photos.length}`);
    photos.forEach((p) => api.submitRender(p.id, recipe, "export").catch(console.error));
  }

  return (
    <div className="app-shell">
      <header className="menubar">
        <span className="brand">像素蛋糕</span>
        <nav className="menu">
          <button onClick={() => document.getElementById("import-input")?.focus()}>
            导入照片
          </button>
          <button
            disabled={!photo}
            onClick={() => photo && api.submitRender(photo.id, recipe, "export")}
          >
            导出
          </button>
          <button disabled={!projectId || photos.length === 0} onClick={exportAll}>
            批量导出
          </button>
          <button onClick={() => setAbout(true)}>关于</button>
        </nav>
        <span className="menubar-right">v0.1.0</span>
      </header>

      <div className="app">
        <aside className="sidebar">
          <div className="block">
            <input
              placeholder="新建项目名"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
            />
            <button onClick={createProject}>新建项目</button>
          </div>

          <ul className="list">
            {projects.map((p) => (
              <li
                key={p.id}
                className={p.id === projectId ? "active" : ""}
                onClick={() => selectProject(p.id)}
              >
                {p.name}
              </li>
            ))}
          </ul>

          <div className="block">
            <input
              id="import-input"
              placeholder="导入路径（逗号分隔）"
              value={importPaths}
              onChange={(e) => setImportPaths(e.target.value)}
            />
            <button onClick={importPhotos} disabled={!projectId}>
              导入照片
            </button>
          </div>

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
          <button
            className="export-btn"
            disabled={!photo}
            onClick={() => photo && api.submitRender(photo.id, recipe, "export")}
          >
            导出（全分辨率）
          </button>

          <PresetPanel currentRecipe={recipe} onApplyRecipe={updateRecipe} />

          <ModelManager />
        </aside>
      </div>

      <footer className="statusbar">
        <span>{photo ? `当前：${photo.raw_path.split(/[\\/]/).pop()}` : "未选择照片"}</span>
        {progress !== null && <span>处理中 {Math.round(progress)}%</span>}
        {batchInfo && <span>{batchInfo}</span>}
        <span className="statusbar-right">本地 GPU 推理</span>
      </footer>

      {about && (
        <div className="modal-mask" onClick={() => setAbout(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2>像素蛋糕</h2>
            <p className="muted">版本 0.1.0</p>
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
    </div>
  );
}
