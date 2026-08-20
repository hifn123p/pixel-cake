// 后端 IPC 封装：前端只通过这里与 Rust 通信（文档 §2 ①→②）。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { EngineEvent, Photo, Project, Recipe } from "./types";

export const api = {
  createProject: (name: string) => invoke<Project>("create_project", { name }),

  listProjects: () => invoke<Project[]>("list_projects"),

  importPhotos: (projectId: string, paths: string[]) =>
    invoke<Photo[]>("import_photos", { projectId, paths }),

  listPhotos: (projectId: string) =>
    invoke<Photo[]>("list_photos", { projectId }),

  saveRecipe: (photoId: string, recipe: Recipe) =>
    invoke<void>("save_recipe", { photoId, recipe }),

  getRecipe: (photoId: string) =>
    invoke<Recipe | null>("get_recipe", { photoId }),

  submitRender: (photoId: string, recipe: Recipe, scope: "preview" | "export") =>
    invoke<void>("submit_render", { photoId, recipe, scope }),

  /** 监听引擎事件（进度/完成/错误）。返回取消监听函数。 */
  onEngineEvent: (handler: (e: EngineEvent) => void) =>
    listen<EngineEvent>("engine://event", (ev) => handler(ev.payload)),
};
