# Bohack-ExplainCraft

ExplainCraft 是一个 AI 视频创作桌面应用。Vue 工作台、Rust 制作后端和 Tauri 桌面壳在同一个
仓库中开发，`website/` 则是拥有独立依赖与构建配置的静态官网，发布到 GitHub Pages。

桌面发行说明见 [v0.1.0](front/docs/RELEASE-0.1.0.md)。应用名称为 Lingjing（灵镜），
标识固定为 `com.explaincraft.lingjing`；所有安装包使用 `ExplainCraft_版本_平台` 文件名。
桌面构建会执行前端、Rust 和安装后自检。GitHub Actions 的 `Publish desktop release`
单独接收已验证的构建编号：`preview` 允许部分平台，`stable` 要求 Windows x64、Apple Silicon
Mac 和 Intel Mac 全部通过。
发布流程拒绝替换已有正式 Release、移动已有标签或覆盖不同内容的同名安装包。

```text
Bohack-ExplainCraft/
├── Cargo.toml              Cargo workspace，统一组织 Rust 工程
├── src-tauri/              单一 Rust crate：Tauri commands、SQLite、任务与模型供应商
├── front/                  Vue 3 + TypeScript：仅 Tauri 工作台
├── website/                独立 Vite 官网：自包含页面、素材与 Pages 构建配置
├── shared/public/media/    工作台使用的本地示例媒体（官网素材在 website/public/media）
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
`--features quercus`（保留兼容参数）。FFmpeg 等额外要求见 [后端说明](src-tauri/README.md)。

也可以从仓库根目录执行 Cargo 检查：

```bash
npm --prefix front run build:studio
cargo test --workspace
cargo check --workspace --all-targets
```

制作流程由应用内 skills 约束：主题 → 风格 → 时长 → SVG/HTML → 配音 → BGM →
方案确认 → 文案确认 → 分镜确认 → 制作确认 → 初版确认 → 可选剪辑 → 交付。
输入框旁不再放秒数下拉框；选择保存在同一任务内，刷新或重启可以恢复。

视觉模型生成受约束的 SVG/SMIL 或离线 HTML/CSS，本地输出镜头后由 FFmpeg 合成基础 MP4、
旁白、BGM 与字幕。HTML 路径额外需要系统 Chrome。**初版生成前不调用 Quercus**；
初版可播放后，用户明确选择继续剪辑才调用 Quercus 并保存第二个版本。
工程包导出属于单独的显式请求，保留 OVE / OTIO / FCPXML 能力。

配音每次提供 API、本地 breezeTTS2、纯 BGM 三项。缺少本地模型时询问是否打开下载设置，
必须再点同意才下载；API / 本地配音超时按约定退回 BGM，页面展示原因。
API 默认 45 秒、本地默认 90 秒，可用 `VOICE_TIMEOUT_SECONDS` 调整。

除 Vibe 知识大赏外，模板已换为 Video Shotcraft 镜头配方适配，随代码保存上游提交和 Apache-2.0
许可证；当前运行 SVG/HTML 路径，未引入上游完整 Remotion 工作台。
详细闭环设计、来源、检查与实际边界见 [引导制作说明](front/docs/GUIDED_PRODUCTION.md)。

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

`v0.1.0` 标签对应首个正式桌面发行版；源码、应用版本和安装包保持一致。版本约定见
[版本说明](front/docs/VERSIONING.md)。

`src-tauri/src/oak/` 的 Quercus 工具层来源于 OakVideoEditorCommunity/example-mcp，
采用 GPL-3.0-or-later，许可证和上游说明随源码一并保留。
