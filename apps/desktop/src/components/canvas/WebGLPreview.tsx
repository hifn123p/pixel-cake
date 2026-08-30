// WebGL2 实时预览：基础调色 + 滤镜在 GPU 上即时渲染，滑块零延迟。
//
// 与后端引擎的数学完全一致（`crates/engine/src/base/tone.rs` + `color/lut.rs` 内置滤镜）：
//   白平衡 → 曝光 → 对比度 → 饱和度 → 曲线 → 暗角 → 颗粒 → 滤镜 LUT
// 底图来自后端 Scope::Base（仅解码 + 降采样，未调色），由本组件逐帧叠加实时参数。
// AI 依赖的操作（磨皮/美型/祛瑕/追色）不走本组件，仍由后端全链路生成 PNG 预览。

import { useEffect, useRef, useState } from "react";
import type { Recipe } from "../../api/types";

interface Props {
  /** 底图（base64 data URL，未调色）。 */
  imgSrc: string | null;
  /** 实时调色参数（仅使用 base 与 filter 字段）。 */
  recipe: Recipe;
  photoName: string | null;
  onImageClick: (nx: number, ny: number) => void;
}

const MAX_CURVE_PTS = 32;

const VERT = `#version 300 es
in vec2 aPos;
in vec2 aUv;
out vec2 vUv;
void main() {
  gl_Position = vec4(aPos, 0.0, 1.0);
  vUv = aUv;
}`;

const FRAG = `#version 300 es
precision highp float;

in vec2 vUv;
out vec4 outColor;

uniform sampler2D uTex;
uniform vec2 uTexSize;

// 基础调色（引擎 tone.rs 逐项对应）
uniform float uExposure;     // EV
uniform float uContrast;     // -100..100
uniform float uSaturation;   // -100..100
uniform float uTemperature;  // -100..100
uniform float uTint;         // -100..100
uniform vec2 uCurvePts[${MAX_CURVE_PTS}]; // 曲线锚点
uniform int  uCurveLen;
uniform float uVignette;     // 0..100
uniform float uGrain;        // 0..100

// 滤镜（引擎 lut.rs 内置滤镜：通道倍乘 / 黑白）
uniform float uFilterMode;   // 0=无 1=通道倍乘 2=黑白
uniform vec3 uFilterMults;
uniform float uFilterAmt;    // 强度 0..1

// 曲线分段线性查找（与 curve_lookup 一致）
float curveLookup(float t) {
  if (uCurveLen <= 0) return t;
  t = clamp(t, 0.0, 1.0);
  for (int i = 0; i < ${MAX_CURVE_PTS - 1}; i++) {
    if (i + 1 >= uCurveLen) break;
    vec2 p0 = uCurvePts[i];
    vec2 p1 = uCurvePts[i + 1];
    if (t >= p0.x && t <= p1.x) {
      float seg = p1.x - p0.x;
      if (seg <= 0.0) return p1.y;
      return p0.y + (p1.y - p0.y) * ((t - p0.x) / seg);
    }
  }
  if (t <= uCurvePts[0].x) return uCurvePts[0].y;
  return uCurvePts[uCurveLen - 1].y;
}

// 确定性噪声（与引擎 hash_noise 一致，像素级）
float hashNoise(float x, float y) {
  uint hx = uint(x) * 374761393u;
  uint hy = uint(y) * 668265263u;
  uint h = hx ^ hy;
  h = (h ^ (h >> 13u)) * 1274126177u;
  h ^= h >> 16u;
  return (float(h) / 4294967295.0) * 2.0 - 1.0;
}

void main() {
  vec3 c = texture(uTex, vUv).rgb;

  // 白平衡：温度影响 R/B，色调影响 G
  float temp = uTemperature / 100.0;
  float tint = uTint / 100.0;
  c.r *= 1.0 + temp * 0.5;
  c.b *= 1.0 - temp * 0.5;
  c.g *= 1.0 + tint * 0.3;

  // 曝光：2^EV
  c *= exp2(uExposure);

  // 对比度（中点 0.5）
  c = (c - 0.5) * (1.0 + uContrast / 100.0) + 0.5;

  // 饱和度（Rec.709 亮度）
  float sf = 1.0 + uSaturation / 100.0;
  float gray = dot(c, vec3(0.2126, 0.7152, 0.0722));
  c = clamp(gray + (c - gray) * sf, 0.0, 1.0);

  // 曲线（RGB 三通道）
  c = vec3(curveLookup(c.r), curveLookup(c.g), curveLookup(c.b));

  // 暗角：中心 1，角落 1 - strength/100
  if (uVignette > 0.0) {
    vec2 n = vUv - 0.5;
    float d2 = dot(n, n);
    float d = sqrt(d2 / 0.5);
    float v = 1.0 - (uVignette / 100.0) * d * d;
    c *= v;
  }

  // 颗粒（三通道同源噪声）
  if (uGrain > 0.0) {
    vec2 pc = floor(vUv * uTexSize);
    float n = hashNoise(pc.x, pc.y) * (uGrain / 100.0) * 0.5;
    c = clamp(c + n, 0.0, 1.0);
  }

  // 滤镜（管线第 6 步：基础调色之后）
  if (uFilterMode > 0.5) {
    if (uFilterMode > 1.5) {
      float l = dot(c, vec3(0.299, 0.587, 0.114)); // BT.601 亮度（黑白滤镜）
      c = mix(c, vec3(l), uFilterAmt);
    } else {
      c = mix(c, c * uFilterMults, uFilterAmt);
    }
  }

  outColor = vec4(clamp(c, 0.0, 1.0), 1.0);
}`;

/** 滤镜 → shader 参数（与引擎 builtin_filter_lut 的系数一致）。 */
function filterUniforms(
  filter: Recipe["filter"]
): { mode: number; mults: [number, number, number]; amt: number } {
  if (!filter) return { mode: 0, mults: [1, 1, 1], amt: 0 };
  const amt = Math.max(0, Math.min(1, filter.intensity));
  switch (filter.lut_id) {
    case "warm":
      return { mode: 1, mults: [1.08, 1.02, 0.9], amt };
    case "cool":
      return { mode: 1, mults: [0.9, 1.0, 1.1], amt };
    case "vivid":
      return { mode: 1, mults: [1.15, 1.15, 1.15], amt };
    case "bw":
      return { mode: 2, mults: [1, 1, 1], amt };
    default:
      return { mode: 0, mults: [1, 1, 1], amt };
  }
}

/** 从 recipe 提取 shader 参数。 */
function extractParams(recipe: Recipe) {
  const b = recipe.base;
  const pts = b.curves.slice(0, MAX_CURVE_PTS);
  const f = filterUniforms(recipe.filter);
  return {
    exposure: b.exposure,
    contrast: b.contrast,
    saturation: b.hsl.saturation,
    temperature: b.temperature,
    tint: b.tint,
    curves: pts,
    vignette: b.vignette,
    grain: b.grain,
    filterMode: f.mode,
    filterMults: f.mults,
    filterAmt: f.amt,
  };
}

export default function WebGLPreview({
  imgSrc,
  recipe,
  photoName,
  onImageClick,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [failed, setFailed] = useState(false);
  const [loading, setLoading] = useState(false);

  const glRef = useRef<WebGL2RenderingContext | null>(null);
  const progRef = useRef<WebGLProgram | null>(null);
  const locRef = useRef<Record<string, WebGLUniformLocation | null>>({});
  const texRef = useRef<WebGLTexture | null>(null);
  const texSizeRef = useRef<{ w: number; h: number }>({ w: 0, h: 0 });
  const paramsRef = useRef(extractParams(recipe));
  const needRenderRef = useRef(true);
  const rafRef = useRef(0);

  // ── 初始化 GL 程序（仅一次）──────────────────────────
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const gl = canvas.getContext("webgl2", {
      alpha: false,
      antialias: false,
      preserveDrawingBuffer: false,
    });
    if (!gl) {
      setFailed(true);
      return;
    }
    glRef.current = gl;

    const compile = (type: number, src: string) => {
      const sh = gl.createShader(type)!;
      gl.shaderSource(sh, src);
      gl.compileShader(sh);
      if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
        throw new Error(gl.getShaderInfoLog(sh) ?? "shader compile error");
      }
      return sh;
    };

    try {
      const vs = compile(gl.VERTEX_SHADER, VERT);
      const fs = compile(gl.FRAGMENT_SHADER, FRAG);
      const prog = gl.createProgram()!;
      gl.attachShader(prog, vs);
      gl.attachShader(prog, fs);
      gl.linkProgram(prog);
      if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
        throw new Error(gl.getProgramInfoLog(prog) ?? "program link error");
      }
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      progRef.current = prog;

      // 全屏三角形（flipY 后 vUv.y=1 对应图像顶部）
      const buf = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, buf);
      gl.bufferData(
        gl.ARRAY_BUFFER,
        new Float32Array([
          -1, -1, 0, 0,
           3, -1, 2, 0,
          -1,  3, 0, 2,
        ]),
        gl.STATIC_DRAW
      );
      const aPos = gl.getAttribLocation(prog, "aPos");
      const aUv = gl.getAttribLocation(prog, "aUv");
      gl.enableVertexAttribArray(aPos);
      gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 16, 0);
      gl.enableVertexAttribArray(aUv);
      gl.vertexAttribPointer(aUv, 2, gl.FLOAT, false, 16, 8);

      // uniform 位置缓存
      const names = [
        "uTex", "uTexSize", "uExposure", "uContrast", "uSaturation",
        "uTemperature", "uTint", "uCurvePts", "uCurveLen", "uVignette",
        "uGrain", "uFilterMode", "uFilterMults", "uFilterAmt",
      ];
      for (const n of names) {
        locRef.current[n] = gl.getUniformLocation(prog, n);
      }

      gl.useProgram(prog);
      gl.uniform1i(locRef.current.uTex, 0);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
      gl.disable(gl.DEPTH_TEST);
    } catch (e) {
      console.error("WebGL 初始化失败:", e);
      setFailed(true);
    }

    return () => {
      cancelAnimationFrame(rafRef.current);
      const g = glRef.current;
      if (g) {
        if (texRef.current) g.deleteTexture(texRef.current);
        if (progRef.current) g.deleteProgram(progRef.current);
      }
      glRef.current = null;
    };
  }, []);

  // ── 参数变更：更新 ref 并请求重绘（rAF 节流）──────────
  useEffect(() => {
    paramsRef.current = extractParams(recipe);
    requestRender();
  }, [recipe]);

  // ── 底图加载：上传纹理 ──────────────────────────────
  useEffect(() => {
    const gl = glRef.current;
    if (!gl) return;
    if (!imgSrc) {
      needRenderRef.current = true;
      return;
    }
    let alive = true;
    setLoading(true);
    const img = new Image();
    img.onload = () => {
      if (!alive || !glRef.current) return;
      const g = glRef.current;
      if (texRef.current) g.deleteTexture(texRef.current);
      const tex = g.createTexture();
      g.bindTexture(g.TEXTURE_2D, tex);
      g.texImage2D(g.TEXTURE_2D, 0, g.RGBA, g.RGBA, g.UNSIGNED_BYTE, img);
      g.texParameteri(g.TEXTURE_2D, g.TEXTURE_MIN_FILTER, g.LINEAR);
      g.texParameteri(g.TEXTURE_2D, g.TEXTURE_MAG_FILTER, g.LINEAR);
      g.texParameteri(g.TEXTURE_2D, g.TEXTURE_WRAP_S, g.CLAMP_TO_EDGE);
      g.texParameteri(g.TEXTURE_2D, g.TEXTURE_WRAP_T, g.CLAMP_TO_EDGE);
      texRef.current = tex;
      texSizeRef.current = { w: img.naturalWidth, h: img.naturalHeight };
      setLoading(false);
      fitCanvas();
      requestRender();
    };
    img.onerror = () => {
      if (alive) setLoading(false);
    };
    img.src = imgSrc;
    return () => {
      alive = false;
    };
  }, [imgSrc]);

  // ── 容器尺寸变化：重算画布适配（contain）──────────────
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => fitCanvas());
    ro.observe(el);
    return () => ro.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function fitCanvas() {
    const g = glRef.current;
    const el = containerRef.current;
    const canvas = canvasRef.current;
    if (!g || !el || !canvas) return;
    const { w, h } = texSizeRef.current;
    if (w <= 0 || h <= 0) return;
    const cw = el.clientWidth || 1;
    const ch = el.clientHeight || 1;
    const scale = Math.min(cw / w, ch / h);
    const dw = Math.max(1, Math.round(w * scale));
    const dh = Math.max(1, Math.round(h * scale));
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.style.width = `${dw}px`;
    canvas.style.height = `${dh}px`;
    canvas.width = Math.round(dw * dpr);
    canvas.height = Math.round(dh * dpr);
    g.viewport(0, 0, canvas.width, canvas.height);
  }

  function requestRender() {
    if (rafRef.current) return;
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = 0;
      renderFrame();
    });
  }

  function renderFrame() {
    const g = glRef.current;
    if (!g || !progRef.current) return;
    const loc = locRef.current;
    const p = paramsRef.current;
    const { w, h } = texSizeRef.current;
    if (w <= 0) return; // 底图未就绪

    g.useProgram(progRef.current);
    g.uniform2f(loc.uTexSize, w, h);
    g.uniform1f(loc.uExposure, p.exposure);
    g.uniform1f(loc.uContrast, p.contrast);
    g.uniform1f(loc.uSaturation, p.saturation);
    g.uniform1f(loc.uTemperature, p.temperature);
    g.uniform1f(loc.uTint, p.tint);
    if (p.curves.length > 0) {
      const arr = new Float32Array(MAX_CURVE_PTS * 2);
      p.curves.forEach((pt, i) => {
        arr[i * 2] = pt.x;
        arr[i * 2 + 1] = pt.y;
      });
      g.uniform2fv(loc.uCurvePts, arr);
    }
    g.uniform1i(loc.uCurveLen, p.curves.length);
    g.uniform1f(loc.uVignette, p.vignette);
    g.uniform1f(loc.uGrain, p.grain);
    g.uniform1f(loc.uFilterMode, p.filterMode);
    g.uniform3f(loc.uFilterMults, p.filterMults[0], p.filterMults[1], p.filterMults[2]);
    g.uniform1f(loc.uFilterAmt, p.filterAmt);

    g.activeTexture(g.TEXTURE0);
    g.bindTexture(g.TEXTURE_2D, texRef.current);
    g.drawArrays(g.TRIANGLES, 0, 3);
  }

  function handleClick(e: React.MouseEvent) {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const nx = (e.clientX - rect.left) / rect.width;
    const ny = (e.clientY - rect.top) / rect.height;
    onImageClick(Math.max(0, Math.min(1, nx)), Math.max(0, Math.min(1, ny)));
  }

  // ── 渲染：WebGL 画布 or 降级 <img> ──────────────────
  if (failed) {
    return imgSrc ? (
      <div className="canvas-image">
        <img src={imgSrc} alt={photoName ?? "预览"} className="preview-img" draggable={false} />
      </div>
    ) : (
      <div className="canvas-empty">
        <p>导入照片开始编辑</p>
        <p className="muted">WebGL2 不可用，已降级为静态预览</p>
      </div>
    );
  }

  return (
    <div className="canvas-image canvas-webgl" ref={containerRef}>
      <canvas
        ref={canvasRef}
        className="preview-img"
        draggable={false}
        onClick={handleClick}
      />
      {loading && (
        <div className="canvas-hint">底图加载中…</div>
      )}
    </div>
  );
}
