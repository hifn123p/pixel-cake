# 像素蛋糕（Pixel Cake）

> 本地 AI 修图桌面客户端 · Windows 11 · 数据与推理全在本地

复刻「像素蛋糕」核心修图能力：**中性灰磨皮、AI 美型、祛瑕、AI 追色、基础调色、滤镜**。基于 Tauri 2 + Rust 构建，AI 推理经 ONNX Runtime（CUDA EP）在本地 NVIDIA GPU 上完成，无需联网、无需 Python。

## 特性

- 🧱 **中性灰磨皮** — 平整/立体双蒙版，或 GPEN 皮肤增强
- 🎯 **AI 美型** — SCRFD 人脸检测 + 2DFAN4 68 点关键点，瘦脸 / 大眼液化
- 🩹 **祛瑕** — 画布绘制多边形区域，区域填充修复
- 🎨 **AI 追色** — BiSeNet 皮肤分区 + Lab 均值-方差迁移，参考样片一键追色
- 🎛️ **基础调色** — 曝光 / 对比度 / 色温 / 色调 / HSL / 曲线 / 颗粒 / 暗角
- ⚡ **WebGL2 实时预览** — 基础调色与滤镜在 GPU 上即时渲染，滑块零延迟；AI 功能（磨皮/美型/祛瑕/追色）自动回退后端全链路预览
- 🌈 **滤镜** — 内置预设（暖色 / 冷色 / 黑白 / 鲜艳）+ `.cube` LUT 支持，强度 0–100% 生效
- 📷 **RAW 支持** — LibRaw 解码 CR2 / ARW / NEF / DNG 等 10 种相机格式，另支持 JPEG / PNG
- 🗂️ **打开照片文件夹** — 原生目录对话框选择文件夹，扫描建项目并自动导入
- 🧩 **原生文件对话框** — 导入照片、追色参考样片、选择导出目录均为系统对话框
- ⚙️ **设置面板** — 深/浅色主题、预览尺寸、导出格式（16bit TIFF / PNG）、默认目录
- 🔒 **全本地** — 数据存 SQLite + 本地目录，AI 推理不离开你的 GPU

## 技术栈

| 层 | 技术 |
|---|---|
| 外壳 | Tauri 2（Rust） |
| 前端 | React 18 + TypeScript + Vite 5 |
| 引擎 | Rust，16bit f32 全链路 |
| AI 推理 | ONNX Runtime（CUDA EP）：SCRFD / 2DFAN4 / BiSeNet / GPEN |
| RAW 解码 | LibRaw（vendored 静态编译） |
| 存储 | SQLite（rusqlite bundled） |
| 打包 | NSIS 安装包（GitHub Actions CI 产出） |

## 架构

```
┌─────────────────────────────────────────────┐
│  React UI（面板 / 画布 / 菜单栏 / 状态栏）     │
│   └─ tauri-plugin-dialog（原生文件对话框）     │
└────────────────────┬────────────────────────┘
                     │ Tauri IPC (commands)
┌────────────────────▼────────────────────────┐
│  Scheduler（Tokio 队列 + GpuBudget 8GB）     │
│    ├── 解码（LibRaw / PPM / image crate）     │
│    ├── AI 门面 RetouchEngine（模型惰性加载）   │
│    │     ├── SCRFD 人脸检测                   │
│    │     ├── 2DFAN4 68 点关键点 → 美型液化     │
│    │     ├── BiSeNet 皮肤分割 → 追色 LUT      │
│    │     └── GPEN 皮肤增强（磨皮）            │
│    └── 16bit 全链路 pipeline → TIFF / PNG    │
└─────────────────────────────────────────────┘
```

## 环境要求

- **运行**：Windows 10/11 + [VC++ 运行库](https://aka.ms/vs/17/release/vc_redist.x64.exe) + NVIDIA 驱动（CUDA ≥ 13.2 / cuDNN ≥ 9.23，由驱动版本覆盖）
- **开发**：Rust stable + Node.js 18+ + MSVC Build Tools

## 构建与运行

```bash
# 前端依赖
cd apps/desktop && npm install

# 开发模式（热重载）
npm run tauri dev

# 打包 NSIS 安装包
npm run tauri build
# 产物：target/release/bundle/nsis/*.exe
```

CI（`.github/workflows/ci.yml`）会在 `windows-latest` 上执行 `cargo build` + `cargo test` + `cargo clippy` + NSIS 打包，安装包作为 artifact 上传。

## 使用

1. **打开照片文件夹** — 菜单「文件 → 打开项目…」，或左侧「打开照片文件夹…」，选择一个存有 RAW/JPEG/PNG 的目录，应用自动建项目并导入全部照片
2. **新建项目** — 左侧输入客片组名称，点「新建」
3. **导入照片** — 菜单「文件 → 导入照片…」用系统对话框多选照片，或直接点选项目内照片
4. **下载模型** — 首次使用 AI 功能前，在右侧「模型下载」面板下载 4 个 ONNX 模型
5. **编辑** — 磨皮 / 美型 / 追色 / 祛瑕 / 调色 / 滤镜，编辑即自动预览。仅调色与滤镜时走 **WebGL2 实时预览**（GPU 即时渲染，拖动滑块零延迟）；启用 AI 操作后自动切回后端全链路预览
6. **设置** — 菜单「视图 → 设置…」：切换深/浅色主题、预览清晰度、导出格式（16bit TIFF / PNG）、默认导出/导入目录
7. **导出** — 菜单或右侧「导出（全分辨率）」输出 16bit TIFF；「批量导出…」把当前项目全部照片按统一风格导出到所选目录

> 模型下载：SCRFD（人脸检测）、2DFAN4（关键点）、BiSeNet（分割）、GPEN（磨皮），均来自开源的 FaceFusion 模型仓库，经 hf-mirror 镜像下载。

## 项目结构

```
crates/
  bus/        编辑协议（Recipe / EngineRequest / EngineEvent）
  engine/     核心引擎（图像 / 算子 / AI 检测 / 解码 / 导出）
  scheduler/  调度层（任务队列 / 显存预算 / AI 门面）
  storage/    SQLite 数据层（项目 / 照片 / recipe / 预设 / 设置）
apps/desktop/
  src-tauri/  Tauri 后端（IPC 命令 / 模型下载 / 事件转发 / 原生对话框）
  src/        React 前端（面板 / 画布 / 菜单栏 / 设置 / 状态栏）
scripts/      CI 图标生成
```

## 里程碑

| 里程碑 | 状态 |
|---|---|
| M1 骨架（项目/导入/存储） | ✅ |
| M2 中性灰磨皮 | ✅ |
| M3 AI 追色 | ✅ |
| M4 美型 / 祛瑕 | ✅ |
| M5 16bit 全链路 + 导出 | ✅ |
| M6 打磨（设置 / 原生对话框 / 主题 / WebGL2 实时预览 / UX） | 🔶 进行中 |

## 许可证

本项目为专有软件（`UNLICENSED`），保留所有权利。
