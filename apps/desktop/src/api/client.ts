// 后端 IPC 封装：前端只通过这里与 Rust 通信（文档 §2 ①→②）。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { EngineEvent, ModelPackage, Photo, Preset, Project, Recipe } from "./types";

/** 渲染参数（设置面板可全局配置；单次调用可覆盖）。 */
export interface RenderOptions {
  exportDir?: string | null;
  previewMaxEdge?: number | null;
  exportFormat?: "tiff" | "png" | null;
}

export const api = {
  createProject: (name: string) => invoke<Project>("create_project", { name }),

  /** 打开已有照片目录：后端扫描建项目并导入，返回 (项目, 照片列表)。 */
  openProject: (dir: string) => invoke<[Project, Photo[]]>("open_project", { dir }),

  listProjects: () => invoke<Project[]>("list_projects"),

  importPhotos: (projectId: string, paths: string[]) =>
    invoke<Photo[]>("import_photos", { projectId, paths }),

  listPhotos: (projectId: string) =>
    invoke<Photo[]>("list_photos", { projectId }),

  saveRecipe: (photoId: string, recipe: Recipe) =>
    invoke<void>("save_recipe", { photoId, recipe }),

  getRecipe: (photoId: string) =>
    invoke<Recipe | null>("get_recipe", { photoId }),

  submitRender: (
    photoId: string,
    recipe: Recipe,
    scope: "preview" | "export",
    opts?: RenderOptions
  ) =>
    invoke<void>("submit_render", {
      photoId,
      recipe,
      scope,
      exportDir: opts?.exportDir ?? null,
      previewMaxEdge: opts?.previewMaxEdge ?? null,
      exportFormat: opts?.exportFormat ?? null,
    }),

  /** 读取某照片的最新预览 PNG 为 base64（后端按 photo_id 查路径，安全）。 */
  readPreview: (photoId: string) => invoke<string>("read_preview", { photoId }),

  /** 保存当前编辑参数为预设。 */
  savePreset: (name: string, recipe: Recipe) =>
    invoke<Preset>("save_preset_cmd", { name, recipe }),

  /** 列出全部预设。 */
  listPresets: () => invoke<Preset[]>("list_presets_cmd"),

  /** 读取全部用户设置（缺失项由前端默认值兜底）。 */
  getSettings: () => invoke<Record<string, string>>("get_settings"),

  /** 保存一条用户设置。 */
  saveSetting: (key: string, value: string) =>
    invoke<void>("save_setting", { key, value }),

  /** 在系统文件管理器中显示文件/目录。 */
  revealInExplorer: (path: string) => invoke<void>("reveal_in_explorer", { path }),

  /** 模型资源包：列出清单 + 本地状态。 */
  listModelPackages: () => invoke<ModelPackage[]>("list_model_packages_cmd"),

  /** 模型资源包：下载。返回本地路径。 */
  downloadModel: (id: string) => invoke<string>("download_model_cmd", { id }),

  /** 监听引擎事件（进度/完成/错误）。返回取消监听函数。 */
  onEngineEvent: (handler: (e: EngineEvent) => void) =>
    listen<EngineEvent>("engine://event", (ev) => handler(ev.payload)),
};
