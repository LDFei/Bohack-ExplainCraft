# Bohack-ExplainCraft

黑客松的 AI 教育赛道项目。

当前第一版提供「灵镜 · AI 视频创作空间」的 Vue 3 + TypeScript 前端演示。根据 [业务接口文档](docs/API.md) 和 [页面设计说明](docs/FRONTEND_DESIGN.md) 实现，暂用“灵镜”作为展示名称。

第一版基线使用标签 `v0.1.0` 保存，后续改造在 `develop` 分支进行。需要从第一版重新开始时，新建一个基于该标签的分支即可；具体命令见 [版本保存与重新改造](docs/VERSIONING.md)。

![视频创作工作台](docs/screenshots/workspace.png)

## 本地运行

需要 Node.js 24.12 或更高版本。首次运行安装依赖：

```bash
git clone https://github.com/LDFei/Bohack-ExplainCraft.git
cd Bohack-ExplainCraft
npm ci
npm run dev
```

浏览器访问 [http://127.0.0.1:5173](http://127.0.0.1:5173)。打开首页会进入最近的项目；第一次运行会显示已完成的咖啡店案例。点击左侧“新建创作”可以从空白开始。开发服务按需启动，没有部署到外部平台。

## 可以演示的功能

- 三栏工作台：项目历史、AI 对话、作品预览与设置；手机上切换“对话 / 作品”。
- 一句话发起制作，逐阶段展示进度，支持停止、失败后重试和补充信息后继续。
- 图片、视频、音频上传；校验文件大小和可解码性，支持拖入、移除待发送附件。
- 对话修改生成新版本；旧版本保留，并以只读方式查看历史脚本和设置。
- 脚本编辑、音色、语速、音乐、音量、字幕、风格和比例设置；保存草稿后明确提示需要重新生成。
- 本地示例视频播放、720p MP4 导出和下载；三种画面比例均有真实可播放的文件。
- 仅讨论模式、灵感模板、项目筛选、改名与确认删除。
- 创作进度和记录刷新恢复；项目数据保存在 localStorage，上传文件保存在 IndexedDB。

“使用指南”内可一键体验失败重试和补充信息两个演示情景。

## 演示范围

当前实现的是前端演示，**没有接通真实 AI、TTS、剪辑服务或 Rust 后端**。页面清楚标注演示模式，生成使用内置的咖啡、旅行、产品三个主题的 30 秒示例视频。修改意见和设置会更新草稿及版本数据，内置成片不会根据任意提示词真正重绘、重配音或应用特效；上传画面会出现在素材和分镜面板中，不会自动合成进内置视频。

示例视频采用本地图片制作，有轻微运镜和自行合成的背景音，提供 9:16、16:9、1:1 三种比例、720p、30fps。目标时长可选择 15/30/60 秒以展示交互，演示视频实际长度固定 30 秒。试听提供示例背景音乐，没有真实 TTS 音色试听。

本机数据仅属于当前浏览器；清除网站数据后，演示记录和上传文件会丢失。此版本没有实现真实账户、跨设备同步和外部分享。

## 后续接入 Rust

页面通过 `src/services/api.ts` 的 StudioApi 接口访问数据，演示服务与 HTTP 服务分开。类型定义位于 `src/domain/types.ts`，主要路径与 API.md 保持一致。

复制 `.env.example` 为 `.env.local`，把 `VITE_STUDIO_MODE` 改为 `http`，配置 `VITE_API_BASE_URL`，再重启开发服务。HTTP 适配已经映射项目、消息、素材、草稿、任务、事件、视频版本和导出接口；需要后端按文档提供实际响应，登录系统随后接入。当前没有可联调的 Rust 服务，因此 HTTP 模式尚未进行端到端验收。

鉴权从登录模块写入当前会话的 `lingjing_access_token` 读取。`VITE_API_TOKEN` 只供本机开发联调，在生产构建中不读取；正式接入应改为项目登录模块提供短期令牌。

HTTP 事件流使用带鉴权的 fetch，收到持久化变更后停止旧连接、重取快照，再按新游标订阅；断线时重取快照恢复。首版 HTTP 列表仅请求前 100 条，较长历史的游标翻页、逐字流式展示及真实媒体地址续期需结合后端继续联调。当前本地演示不依赖外部网络，资源全部随项目打包。

## 验证与构建

在项目根目录执行：

```bash
npm test
npm run test:e2e
npm run build
```

浏览器测试使用本机 Google Chrome。没有 Chrome 的环境可先安装 Chrome，或修改 `playwright.config.ts` 使用已安装的 Playwright Chromium。测试会在需要时启动本地开发服务。

构建结果在 `dist/`。验证生产构建可运行 `npm run preview`，访问 [http://127.0.0.1:4173](http://127.0.0.1:4173)。部署时需将 `/create` 和 `/projects/*` 等前端路由回退到 index.html。

页面截图：[完整工作台](docs/screenshots/workspace.png)、[新建创作](docs/screenshots/new-creation.png)、[手机作品预览](docs/screenshots/mobile.png)。素材来源见 [MEDIA_SOURCES.md](docs/MEDIA_SOURCES.md)。
