# Bohack-ExplainCraft

ExplainCraft 是一个 AI 视频创作桌面应用。Vue 工作台、Rust 制作后端和 Tauri 桌面壳在同一个
仓库中开发，产品官网则作为独立静态站点发布到 GitHub Pages。

```text
Bohack-ExplainCraft/
├── Cargo.toml              Cargo workspace，统一组织 Rust 工程
├── src-tauri/              单一 Rust crate：Tauri commands、SQLite、任务与模型供应商
├── front/                  Vue 3 + TypeScript：官网与 Tauri 工作台两套入口
│   ├── index.html          独立官网入口
│   └── studio.html         Tauri 工作台入口
└── .github/workflows/      自动检查与 GitHub Pages 发布
```

桌面壳与业务后端位于同一个 `lingjing-desktop` crate。工作台通过 `#[tauri::command]`
调用进程内业务状态，没有 TCP 监听或独立 Web 服务。项目、任务和生成文件保存在系统应用数据目录。

## 桌面端开发

需要 Node.js 24.12+、Rust，以及对应操作系统的
[Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。

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

Windows 如需同时启用 quercus 剪辑引擎，可使用
`npm run desktop:dev:windows` 和 `npm run desktop:build:windows`；脚本会加载 MSVC 环境并传递
`--features quercus`。FFmpeg 等额外要求见 [后端说明](src-tauri/README.md)。

也可以从仓库根目录执行 Cargo 检查：

```bash
npm --prefix front run build:studio
cargo test --workspace
cargo check --workspace --all-targets
```

## 官网开发

官网开发服务与桌面工作台互不影响：

```bash
cd front
npm run dev
```

打开 <http://127.0.0.1:5173>。官网上的创作入口统一指向
[GitHub Releases](https://github.com/LDFei/Bohack-ExplainCraft/releases)，不会在 Pages 上运行演示工作台。

推送到 `main` 后，`.github/workflows/pages.yml` 执行 `npm run build:website`，只把
`front/dist/website/` 发布到 GitHub Pages。仓库 Pages 的 Source 需选择 **GitHub Actions**。

## 测试

Rust 后端只随 Tauri 桌面应用运行，不提供独立 Web 服务。前端测试与两种构建均从 `front/` 执行：

```bash
npm test
npm run test:e2e
npm run build:website
npm run check:website
npm run build:studio
```

更多说明：

- [前端与桌面端开发](front/README.md)
- [Rust 后端与模型供应商](src-tauri/README.md)
- [前后端接口约定](front/docs/API.md)
- [页面设计](front/docs/FRONTEND_DESIGN.md)

原始第一版仍由 `v0.1.0` 标签保存；不要重写该标签或已有远端历史。
