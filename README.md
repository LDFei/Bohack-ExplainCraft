# Bohack-ExplainCraft

ExplainCraft 是一个 AI 视频创作桌面应用。Vue 工作台、Rust 制作后端和 Tauri 桌面壳在同一个
仓库中开发，`website/` 则是拥有独立依赖与构建配置的静态官网，发布到 GitHub Pages。

```text
Bohack-ExplainCraft/
├── Cargo.toml              Cargo workspace，统一组织 Rust 工程
├── src-tauri/              单一 Rust crate：Tauri commands、SQLite、任务与模型供应商
├── front/                  Vue 3 + TypeScript：仅 Tauri 工作台
├── website/                独立 Vite 官网：依赖、测试与 Pages 构建配置
├── shared/public/media/    官网与工作台共享的本地示例媒体
└── .github/workflows/      自动检查与 GitHub Pages 发布
```

桌面壳与业务后端位于同一个 `lingjing-desktop` crate。工作台通过 `#[tauri::command]`
调用进程内业务状态，没有 TCP 监听或独立 Web 服务。项目、任务和生成文件保存在系统应用数据目录。

## 桌面端开发

需要 Node.js 24.12+、Rust、FFmpeg 开发包，以及对应操作系统的
[Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。macOS 构建时设置
`FFMPEG_DIR=/opt/homebrew`。

```bash
git clone https://github.com/LDFei/Bohack-ExplainCraft.git
cd Bohack-ExplainCraft/front
npm ci
npm run desktop:dev
```

Tauri 会执行 `npm run dev:studio` 并打开工作台。打包桌面安装包：

```bash
cd front
npm run desktop:build
```

Windows 的 quercus/MSVC 开发环境可使用
`npm run desktop:dev:windows` 和 `npm run desktop:build:windows`；脚本会加载 MSVC 环境并传递
`--features quercus`（当前已为默认 feature）。FFmpeg 等额外要求见 [后端说明](src-tauri/README.md)。

也可以从仓库根目录执行 Cargo 检查：

```bash
npm --prefix front run build:studio
cargo test --workspace
cargo check --workspace --all-targets
```

视觉模型输出受约束的 SVG/SMIL 动画（每个镜头都会被检测是否真的有运动，静止图会重画或自动补推镜），本地逐帧渲染后由 Quercus 剪辑和导出。
多段旁白与烧录字幕会进入最终成片；导出时可选包含全部素材和 OVE / OTIO / FCPXML 的 ZIP 工程包。
OpenAI、阿里云百炼、DeepSeek、MiniMax、硅基流动与 GLM 的 base URL 已预置；
音乐支持 MiniMax、Stable Audio 和硅基流动，配音支持 Qwen-TTS、本地 IndexTTS 与 breezeTTS2。
`src-tauri/src/oak/` 提供 50 个可通过 OpenAI `tool_calls` 驱动的 Quercus 工具。

## 独立官网开发

官网是独立的 Vite 项目，不包含工作台或后端：

```bash
cd website
npm ci
npm run dev
```

打开 <http://127.0.0.1:5174>。官网上的创作入口统一指向
[GitHub Releases](https://github.com/LDFei/Bohack-ExplainCraft/releases)，不会在 Pages 上运行演示工作台。

推送到 `main` 后，`.github/workflows/pages.yml` 在 `website/` 执行 `npm run build`，只把
`website/dist/` 发布到 GitHub Pages。仓库 Pages 的 Source 需选择 **GitHub Actions**。

## 测试

Rust 后端只随 Tauri 桌面应用运行，不提供独立 Web 服务。工作台检查从 `front/` 执行：

```bash
npm test
npm run build:studio
```

官网检查从 `website/` 执行：

```bash
npm run build
npm run check:website
npm run test:e2e
```

更多说明：

- [前端与桌面端开发](front/README.md)
- [Rust 后端与模型供应商](src-tauri/README.md)
- [前后端接口约定](front/docs/API.md)
- [页面设计](front/docs/FRONTEND_DESIGN.md)

原始第一版仍由 `v0.1.0` 标签保存；不要重写该标签或已有远端历史。

`src-tauri/src/oak/` 的 Quercus 工具层来源于 OakVideoEditorCommunity/example-mcp，
采用 GPL-3.0-or-later，许可证和上游说明随源码一并保留。
