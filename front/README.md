# ExplainCraft Tauri 工作台

`front/` 只保存 Tauri 工作台的 Vue、TypeScript、样式与测试。独立静态官网位于
[`website/`](../website/README.md)，共享示例媒体位于 `shared/public/media/`。

工作台没有 `DemoApi`、本地伪造任务或首页入口。项目、对话、上传、任务、版本与导出全部通过
Tauri commands 访问真实 Rust 后端。

## 桌面端开发

需要 Node.js 24.12+、Rust、Tauri 2 平台依赖、FFmpeg 开发包和 Quercus 编译环境。

```bash
npm ci
npm run desktop:dev
```

Tauri 会执行 `npm run dev:studio` 并打开工作台。macOS 可设置 `FFMPEG_DIR=/opt/homebrew`。
Windows 的 MSVC 环境使用：

```powershell
npm run desktop:dev:windows
npm run desktop:build:windows
```

## 真实生成链路

- 主题提交后，按应用内 skills 依次确认风格、时长、SVG/HTML、API / breezeTTS2 /
  BGM、音乐、方案、文案和分镜。
- 选择题由后端持久化，前端显示选项与确认按钮；没有回答不会执行下一步。固定秒数下拉框已移除。
- SVG/SMIL 本地逐帧渲染；HTML/CSS 在系统 Chrome 中离线定帧，后者需要 Node.js 24+、Chrome 和 FFmpeg。
- FFmpeg 先合成基础成片与字幕。用户看完初版、明确同意后才调用 Quercus 生成后续版本。
- API 与本地配音均有超时限制，超时转为字幕加 BGM；不会把失败说成配音成功。
- breezeTTS2 模型缺失时打开下载设置；下载需要用户明确点击同意。
- 音乐可用本地程序化 BGM、已上传音频，或不使用音乐；选择纯 BGM 时需保留背景音乐。
- 三个 Shotcraft 模板来自已核对的镜头配方适配；Vibe 保留知识讲解结构，并服从同一套选择约束。

“工作台设置 → API 配置”保存创作 API、云端配音 API 与本地 ComfyUI 配置，
通过 `get_model_settings` / `update_model_settings` 写入本机，密钥不会回显。
自定义 OpenAI Compatible Endpoint 需填写完整地址与模型；声线可选择或手填。
对话时长提供 15、30、60、120 秒、内容推荐，以及用户原话中 15～300 秒内的指定值。

## 工作台功能

- 新建话题、结束当前话题、项目历史、素材上传、对话修改、逐步选择、取消与失败重试。
- 制作步骤内展示后端操作、镜头进度、耗时与超时降级；等待选择时允许上传素材。
- 脚本、音色、语速、音乐、音量、字幕、风格和比例设置。
- 成片播放、历史版本、MP4 导出下载、项目改名和删除。
- 三栏自定义布局、侧栏收缩、四套主题与发送快捷键。

## 验证

```bash
npm run format
npm run format:check
npm test
npm run build:studio
```

Rust workspace 检查从仓库根目录执行：

```bash
cargo fmt --all --check
cargo test --workspace
```

接口说明见 [API 文档](docs/API.md)，官网开发见 [website/README.md](../website/README.md)。

本轮设计与验证见 [引导制作闭环](docs/GUIDED_PRODUCTION.md)。
