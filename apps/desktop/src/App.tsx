import { useEffect, useState } from "react";
import { api } from "./api/client";
import { defaultRecipe, type Photo, type Project, type Recipe } from "./api/types";
import Canvas from "./components/canvas/Canvas";
import BasePanel from "./components/panels/BasePanel";
import BeautyPanel from "./components/panels/BeautyPanel";
import ColorPanel from "./components/panels/ColorPanel";
import ModelManager from "./components/panels/ModelManager";
import NeutralGrayPanel from "./components/panels/NeutralGrayPanel";

export default function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [photos, setPhotos] = useState<Photo[]>([]);
  const [photo, setPhoto] = useState<Photo | null>(null);
  const [recipe, setRecipe] = useState<Recipe>(defaultRecipe());
  const [progress, setProgress] = useState<number | null>(null);
  const [newName, setNewName] = useState("");
  const [importPaths, setImportPaths] = useState("");

  useEffect(() => {
    api.listProjects().then(setProjects).catch(console.error);

    const unlisten = api.onEngineEvent((e) => {
      if (e.type === "progress") setProgress(e.pct);
      else if (e.type === "done") setProgress(null);
      else if (e.type === "error") {
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
  function updateRecipe(next: Recipe) {
    setRecipe(next);
    if (photo) {
      api.saveRecipe(photo.id, next).catch(console.error);
      api.submitRender(photo.id, next, "preview").catch(console.error);
    }
  }

  return (
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
              <span className={`status status-${p.status}`}>{p.status}</span>
            </li>
          ))}
        </ul>
      </aside>

      <main className="canvas-area">
        <Canvas photoName={photo?.raw_path ?? null} progress={progress} />
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
        <BasePanel
          value={recipe.base}
          onChange={(v) => updateRecipe({ ...recipe, base: v })}
        />
        <button
          className="export-btn"
          disabled={!photo}
          onClick={() => photo && api.submitRender(photo.id, recipe, "export")}
        >
          导出（全分辨率）
        </button>

        <ModelManager />
      </aside>
    </div>
  );
}
