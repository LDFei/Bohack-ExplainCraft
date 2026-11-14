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

- 文案、对话、分镜与 SVG 动画共用 llm1，不单独配置视觉模型。
- 视觉场景以受约束的 SVG/SMIL 输出，本地逐帧渲染后交给 Quercus。
- 新项目默认使用本地程序化配乐：按风格合成和弦、贝斯、琶音与轻打击乐，直接生成 WAV。
- 也可关闭音乐，或改用 MiniMax、Stable Audio、硅基流动、曲库和上传音频。
- Qwen-TTS、本地 IndexTTS 或 breezeTTS2 负责配音。
- Quercus 负责预览剪辑与 MP4 导出，不直接复制示例成片。

“工作台设置 → API 配置”用于选择供应商、base URL、模型与密钥。保存通过
`get_model_settings` / `update_model_settings` commands 写入应用配置并热替换供应商，
新任务立即生效；密钥不会回显到页面。

选择“自定义 OpenAI
Compatible”时，可填写自己的 Endpoint 和模型名称；Endpoint 必须包含完整 HTTP(S) 地址。
填写 API Key 后会自动请求该 Endpoint 的 `/models`，模型名可从下拉列表选择，也可手动输入。
配音可选择 Qwen-TTS、硅基流动 TTS、本地 IndexTTS 或 breezeTTS2。
每次创作可在作品设置中选择或手填声线；旧音色 ID 会迁移到当前默认声线。
目标时长支持 15～300 秒，超过 300 秒直接返回 `DURATION_TOO_LONG`，不会静默截断。

## 工作台功能

- 项目历史、素材上传、对话创作与修改、追问、取消和失败重试。
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
