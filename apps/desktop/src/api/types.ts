// 与 Rust `bus` crate 一一对应的前端类型（serde JSON 往返）。

export type NeutralGrayMode = "Dual" | "FlatOnly" | "StructureOnly";

export interface NeutralGray {
  enabled: boolean;
  ka: number;
  kb: number;
  mode: NeutralGrayMode;
}

export interface ControlPoint {
  x: number;
  y: number;
  dx: number;
  dy: number;
}

export interface Beauty {
  enabled: boolean;
  face_slim: number;
  body_slim: number;
  neck_slim: number;
  face_full: number;
  manual_points: ControlPoint[];
}

export type InpaintKind = "Blemish" | "Tattoo" | "Background" | "Teeth";

export interface Point {
  x: number;
  y: number;
}

export interface InpaintRegion {
  polygon: Point[];
  kind: InpaintKind;
}

export type ColorTransferMode = "Extreme" | "Harmony";

export interface Color {
  enabled: boolean;
  lut_ref: string | null;
  reference_path: string | null;
  per_region_strength: number;
  mode: ColorTransferMode;
}

export interface Hsl {
  hue: number;
  saturation: number;
  lightness: number;
}

export interface Base {
  exposure: number;
  contrast: number;
  temperature: number;
  tint: number;
  curves: Point[];
  hsl: Hsl;
  grain: number;
  vignette: number;
}

export interface Filter {
  lut_id: string;
  intensity: number;
}

export interface Recipe {
  neutral_gray: NeutralGray;
  beauty: Beauty;
  inpaint: InpaintRegion[];
  color: Color;
  base: Base;
  filter: Filter | null;
}

/** 预设（保存的编辑参数，recipe_json 为序列化 Recipe）。 */
export interface Preset {
  id: string;
  name: string;
  scope: string;
  recipe_json: string;
  lut_path: string | null;
}

export function defaultRecipe(): Recipe {
  return {
    neutral_gray: { enabled: false, ka: 0, kb: 0, mode: "Dual" },
    beauty: {
      enabled: false,
      face_slim: 0,
      body_slim: 0,
      neck_slim: 0,
      face_full: 0,
      manual_points: [],
    },
    inpaint: [],
    color: {
      enabled: false,
      lut_ref: null,
      reference_path: null,
      per_region_strength: 1,
      mode: "Harmony",
    },
    base: {
      exposure: 0,
      contrast: 0,
      temperature: 0,
      tint: 0,
      curves: [],
      hsl: { hue: 0, saturation: 0, lightness: 0 },
      grain: 0,
      vignette: 0,
    },
    filter: null,
  };
}

export interface Project {
  id: string;
  name: string;
  created_at: number;
  root_path: string;
  thumb: string | null;
}

export type PhotoStatus = "pending" | "retouched" | "exported";

export interface Photo {
  id: string;
  project_id: string;
  raw_path: string;
  proxy_path: string | null;
  result_path: string | null;
  width: number;
  height: number;
  status: PhotoStatus;
  created_at: number;
}

export type PipelineStep =
  | "RawDecode"
  | "NeutralGray"
  | "BeautyWarp"
  | "Inpaint"
  | "ColorLut"
  | "BaseTone"
  | "Filter"
  | "Encode";

export type EngineEvent =
  | { type: "progress"; photo_id: string; step: PipelineStep; pct: number }
  | { type: "done"; photo_id: string; result_path: string | null; proxy_updated: boolean }
  | { type: "error"; photo_id: string; code: string; message: string };

// 模型资源包（对应 bus::ModelSpec / ModelStatus）
export interface ModelSpec {
  id: string;
  name: string;
  url: string;
  size_bytes: number;
  purpose: string;
}

export type ModelStatus =
  | { state: "not_downloaded" }
  | { state: "downloading"; progress: number }
  | { state: "downloaded"; path: string };

export type ModelPackage = [ModelSpec, ModelStatus];
