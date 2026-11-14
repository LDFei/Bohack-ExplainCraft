# 灵镜后端（Rust）

按 [业务接口文档](../front/docs/API.md) 实现的 Rust 后端：Tauri commands 处理工作台调用，SQLite 持久化，后台 worker
编排视频制作流程。四个模型与剪辑渲染器都可以独立切换：

它是根目录 Cargo workspace 中唯一的 crate。Tauri 桌面壳与全部业务实现共用
`lingjing_desktop_lib`，后端只在桌面应用进程内运行，没有独立 Web 服务入口。

| 环节                    | 实现                                         | 说明          |
| ----------------------- | -------------------------------------------- | ------------- |
| llm1 文案 / 分镜 / 剪辑 | `siliconflow`：GLM-5.3 + 50 个 Quercus 工具  | 主题模板      |
| llm1 SVG 动画           | 与文案共用 llm1                              | 本地逐帧渲染  |
| llm3 背景音乐           | MiniMax / Stable Audio / 硅基流动            | 示例背景音    |
| llm4 配音               | 阿里云 Qwen-TTS / 本地 IndexTTS / breezeTTS2 | 静音占位      |
| 剪辑与导出              | `quercus`（默认）                            | 真实 MP4 输出 |

## 快速开始

需要 Node.js、Rust、Tauri 平台依赖、FFmpeg 与 Quercus 编译环境。macOS 可设置
`FFMPEG_DIR=/opt/homebrew`，并通过 `brew install ffmpeg mpg123 zstd` 安装编解码库；Linux 还需要
`libmpg123-dev` 和 `libzstd-dev`。Quercus 固定为 0.7.2，其 Unix 链接配置位于根目录 `.cargo/config.toml`。
Windows 的依赖准备见下文。
该配置也为 RustRover 和直接执行的 Cargo 命令提供 `FFMPEG_DIR=/opt/homebrew` 默认值；
非 Homebrew 环境可显式设置自己的 `FFMPEG_DIR`，会覆盖这个默认值。

```bash
cd front
npm ci
npm run desktop:dev
```

Tauri 会初始化进程内后端并打开工作台，所有业务操作都通过 `#[tauri::command]` 完成。
真实模型缺少密钥或本地服务未启动时会返回明确错误，不会用演示结果代替成功。

### Windows 构建依赖

- FFmpeg 8 开发包：把 BtbN 的 `ffmpeg-n8.1-*-win64-gpl-shared` 解压为与仓库同级的
  `ffmpeg-btbn-8.1`（含 `include/`、`lib/*.lib`、`bin/*.dll`）。`tauri.ps1` / `cargo.ps1`
  未设置 `FFMPEG_DIR` 时优先使用它，否则回退到旧的 `ffmpeg-msvc`。
- libclang：`LIBCLANG_PATH` 指向 MSYS2 的 `ucrt64\bin`；脚本同时把该目录放到 `PATH` 最前。
  否则 Git 自带的 MinGW DLL 会先被加载，`libclang.dll` 以 `LoadLibraryExW failed` 失败。
- `vendor/ffmpeg-sys-next`：由根目录 `[patch.crates-io]` 引用，包含两项 Windows 修复：
  屏蔽 clang `amxintrin.h` 的 `__tile1024i` 类型（E0587），以及让 bindgen 在 256 MB 栈的
  线程中运行（release 下 `STATUS_STACK_OVERFLOW`）。
- 运行时需要 `ffmpeg-btbn-8.1\bin`（或 MSYS2 `ucrt64\bin`）在 `PATH` 中，供 FFmpeg
  命令行与 Quercus 链接的 DLL 使用。

## 使用真实模型

1. **配置**：复制 `.env.example` 为 `.env`，填写硅基流动密钥和配音参考音。`.env` 已加入 `.gitignore`，
   密钥只保存在本机。含空格或反斜杠的值（Windows 路径、`BREEZE_MODEL`）要用单引号包裹。
2. **启动 ComfyUI**（breezeTTS2 配音）：

   ```powershell
   cd D:\software\ComfyUI
   .\.venv\Scripts\python.exe main.py --listen 127.0.0.1 --port 8188
   ```

   参考音会先转成 24 kHz 单声道再上传，这样 ComfyUI 环境里不需要 `torchaudio`。

3. **构建桌面应用**（quercus 渲染需要 FFmpeg 开发包）：

   ```powershell
   cd front
   npm run desktop:build:windows
   ```

   请使用 release 构建：调试版编译的 quercus 渲染慢一个数量级。启动日志会显示各环节使用的供应商和模型，
   密钥只显示“已设置 / 未设置”。

实测一次 20 秒竖屏成片（3 个镜头）各环节耗时：选题、文案、分镜各约 10～15 秒；3 张图并发约 25 秒；
本地配音每句约 1 分钟；背景音乐约 2 分钟，与配音同时进行；剪辑指令约 25 秒；渲染时间取决于编译模式。

## 制作流程与模型分工

```
选题 ──► 文案 ──► 分镜 ──┬─► SVG 画面（llm1）─┬─► 剪辑方案 ──► 调色字幕 ──► 渲染输出
        （llm1）         └─► 音频（llm3 背景音 + llm4 配音）┘    （llm1）     （预设）      （剪辑库）
```

| 阶段 key     | 流程环节      | 接口（`src/providers/mod.rs`）                                  | 实现说明                       |
| ------------ | ------------- | --------------------------------------------------------------- | ------------------------------ |
| `planning`   | 1. 选题       | `ScriptModel::plan` / `revise`（llm1）                          | 关键词识别咖啡 / 旅行 / 产品   |
| `script`     | 2. 文案       | `ScriptModel::write_script`（llm1）                             | 主题模板文案                   |
| `storyboard` | 2. 分镜       | `ScriptModel::storyboard`（llm1）                               | 按句拆镜头、平分时长           |
| `visuals`    | 3. SVG 场景   | `VisualModel::generate_shot`（与文案共用 llm1）                 | SVG/SMIL 逐帧渲染              |
| `audio`      | 4. 音频       | `MusicModel::compose`（llm3）+ `VoiceModel::synthesize`（llm4） | 示例背景音 + 静音 WAV 占位配音 |
| `edit`       | 5. 剪辑       | `ScriptModel::edit_plan`（llm1）输出剪辑指令                    | 按分镜生成标准剪辑指令         |
| `finishing`  | 6. 调色与特效 | 用户选择的预设写入剪辑方案（转场同步到指令）                    | —                              |
| `render`     | 输出          | `Renderer::render` / `export`                                   | quercus：SVG 镜头剪辑与导出    |

画面和音频两个阶段并行执行，剪辑等待两者完成。仅讨论模式调用 `ScriptModel::chat`。

### 剪辑：LLM 输出指令，quercus 执行

剪辑阶段由 llm1 输出一串 JSON 剪辑指令（`src/providers/edit_ops.rs` 的 `EditOp`）：

- `create_sequence`：创建画布；
- `import_media`：导入素材；
- `place_clip`：把素材放到视频 / 音频轨的指定时间段；
- `add_crossfade`：在相邻镜头之间加淡化转场。

这是给 LLM 的工具调用格式，真实模型可以直接按这个格式输出。渲染器 `quercus`（`src/providers/quercus.rs`）逐条把指令翻译成
[quercus-engine](https://docs.rs/quercus-engine-rs) 的 `EditCommand`，在它的非线性剪辑工程里搭出时间线，最后导出
H.264 / AAC 的 MP4。
每段旁白都有独立片段 ID。字幕会生成 SRT 和透明 PNG，直接放到 Quercus 视频轨，
与画面在同一次编码中输出，不再进行字幕二次编码，也不启用会覆盖黑屏的原生 Text 字幕层。
SVG 镜头默认优先使用 VideoToolbox / NVENC / QSV / AMF，失败后回退到 `libx264 veryfast`；
日志会打印实际编码器。可用 `FFMPEG_HW_ENCODER=software` 强制软件编码，或指定
`videotoolbox` / `nvenc` / `qsv` / `amf`。Quercus 自身也按同样顺序尝试硬件编码。
同规格 MP4 导出直接复用已渲染文件；选择工程包时还会保存 OVE / OTIO / FCPXML，
连同全部相关素材、上传文件和独立字幕打入使用相对路径的 ZIP。

quercus 是默认 feature 和默认渲染器，需要 FFmpeg 开发包。可用 `--no-default-features`
显式关闭它以只做轻量检查；预览和工程包都会通过 Quercus 执行。

```powershell
$env:PROVIDER_RENDERER = 'quercus'
cd front
npm run desktop:build:windows
```

`tauri.ps1` 会自动设置 `FFMPEG_DIR`（同级目录 `ffmpeg-msvc`）和 `LIBCLANG_PATH`（MSYS2）。

## 视觉、音乐、配音与 50 个工具

- `PROVIDER_LLM1=openai|aliyun|deepseek|minimax|siliconflow|glm`：创作与 SVG 共用 llm1，只输出受约束的
  SVG/SMIL，后端验证并逐帧渲染为镜头 MP4，不使用文生视频。
- `LLM1_BASE_URL` 可覆盖六家预设；`LLM1_API_KEY` 和 `LLM1_MODEL` 配置密钥与模型。
- `PROVIDER_LLM1=custom` 使用用户自定义 OpenAI Compatible Endpoint，此时必须设置 `LLM1_BASE_URL`。
- `PROVIDER_LLM3=minimax` 使用 MiniMax `music_generation`，`stable_audio` 使用 Stability AI，
  `siliconflow` 保留 MiniMax-H3 音轨方案。
- `PROVIDER_LLM4=qwen` 使用阿里云 Qwen-TTS，`indextts` 使用本地 OpenAI `audio/speech`
  兼容接口，`breeze` 保留 ComfyUI 语音克隆。
- `PROVIDER_LLM4=siliconflow` 使用硅基流动 TTS，独立配置 `SILICONFLOW_TTS_*`。
- `list_models` command 通过 OpenAI Compatible `/models` 获取模型下拉候选，支持手动覆盖模型名。
- `TTS_VOICES` 配置可选声线，作品设置允许每次重新选择或手填；不再做供应商之间的声线名称猜测。
- 目标时长支持 15～300 秒，超过后返回 `DURATION_TOO_LONG`。
- `src/oak/` 包含参考项目的 50 个 Quercus 工具及 GPL 声明。llm1 可通过 OpenAI `tool_calls`
  创建工程、操作时间线、预览、导出、撤销与重做，最终剪辑指令仍经过本地校验。

### 接入真实模型

1. 在 `src/providers/` 新建文件，实现对应 trait（例如 `impl ScriptModel for MyLlm`）。
2. 在 `Providers::from_config` 的 `pick` 中按名称注册。
3. 设置环境变量切换，例如 `PROVIDER_LLM1=my-llm`。

供应商只负责“输入 → 结果 / 媒体文件”，不访问数据库。文件保存、版本、进度、取消、重试都由编排层处理。错误使用
`ProviderError`，其中的 `message` 必须能直接展示给用户。

## 代码结构

| 路径              | 职责                                                                     |
| ----------------- | ------------------------------------------------------------------------ |
| `src/commands.rs` | 20 个 Tauri commands、原始字节上传、事件轮询与自定义媒体协议             |
| `src/api/`        | 进程内业务处理器：项目、消息、草稿、素材、任务、版本导出与签名媒体       |
| `src/db.rs`       | SQLite 表结构与读写；状态变更和事件在同一事务提交                        |
| `src/ops.rs`      | 接口层和 worker 共用的状态变更：建任务、写消息、结束任务                 |
| `src/worker.rs`   | 认领队列任务（attempt 作为 fencing token）、重启恢复、追问超时、事件清理 |
| `src/pipeline.rs` | 八个阶段的编排、检查点复用、取消与失败处理                               |
| `src/providers/`  | 模型与渲染器接口及真实供应商实现                                         |
| `src/media.rs`    | 文件存储、HMAC 签名地址、文件头识别、ffprobe 解析                        |
| `src/catalog.rs`  | `/capabilities` 内容与新项目默认草稿                                     |

### 检查点与复用

每个阶段完成后，产物会写回草稿（`draft_revision + 1`），并从 `invalidated_stages` 中移除。

- 失败重试、对话修改、手动保存设置后更新，都只重跑仍然失效的阶段，其余显示为“沿用已有结果”。例如“只改配音”会跳过画面阶段；“第二个镜头换一下”只重新生成那一个镜头。
- 配音按“旁白 + 音色 + 语速 + 时长”生成指纹，内容没变就复用。

### 数据与文件

- 数据库：系统应用数据目录下的 `explaincraft.sqlite3`
- 上传、生成和渲染的文件：系统应用数据目录下的 `storage/<project_id>/`

可通过 `DATA_DIR` 或单独的路径环境变量覆盖。删除对应应用数据目录即可重置。

## 配置（环境变量）

| 变量                            | 默认值                          | 说明                                                       |
| ------------------------------- | ------------------------------- | ---------------------------------------------------------- |
| `DATA_DIR`                      | 系统应用数据目录                | 数据库与文件目录                                           |
| `DEMO_MEDIA_DIR`                | 应用内嵌 `media/`               | 示例与曲库目录；开发时回退到 `shared/public/media`         |
| `PROVIDER_LLM1`                 | `siliconflow`                   | 创作、对话、分镜与 SVG 供应商                              |
| `PROVIDER_LLM3`                 | `procedural`                    | 本地程序化配乐；新项目默认启用                             |
| `PROVIDER_LLM4`                 | `qwen`                          | 配音供应商                                                 |
| `PROVIDER_RENDERER`             | `quercus`                       | 剪辑与导出                                                 |
| `AUTH_MODE`                     | `dev`                           | `dev`：没有令牌时视为 `local_user`；`strict`：必须携带令牌 |
| `MEDIA_SIGNING_SECRET`          | 随机                            | 媒体签名密钥；不设置时，重启后旧链接失效                   |
| `MEDIA_URL_TTL_SECONDS`         | `1800`                          | 播放 / 下载地址有效期                                      |
| `MAX_CONCURRENT_JOBS`           | `4`                             | 同时执行的任务数                                           |
| `QUESTION_TTL_SECONDS`          | `86400`                         | 追问等待时长                                               |
| `FFPROBE_PATH`                  | `ffprobe`                       | 素材解析工具                                               |
| `LLM1_API_KEY` / `LLM1_MODEL`   | 空 / `zai-org/GLM-5.3`          | llm1 密钥与模型（硅基流动）                                |
| `LLM1_API_KEY` / `LLM1_MODEL`   | 空 / 按供应商选择模型           | 创作与 SVG 共用的模型                                      |
| `LLM3_API_KEY` / `LLM3_MODEL`   | 空 / `MiniMaxAI/MiniMax-H3`     | llm3 密钥与模型                                            |
| `SILICONFLOW_BASE_URL`          | `https://api.siliconflow.cn/v1` | 硅基流动接口地址                                           |
| `COMFYUI_URL`                   | `http://127.0.0.1:8188`         | 本地 ComfyUI（breezeTTS2）                                 |
| `COMFYUI_DIR`                   | 空（自动探测）                  | ComfyUI 安装目录；模型下载到其 `models/breezetts2/`         |
| `BREEZE_MODEL_ENDPOINT`         | `https://huggingface.co`        | 模型下载端点，可改用镜像地址                               |
| `BREEZE_MODEL`                  | `int8 hybrid (recommended)`     | breezeTTS2 检查点                                          |
| `BREEZE_VOICE_MALE` / `_FEMALE` | 空                              | 参考音路径；“沉稳男声”用男声，其余用女声                   |

登录模块还没有接入。dev 模式下，不同的 Bearer 令牌会被当作不同用户，可以用来验证跨用户隔离。

## 测试

```powershell
./cargo.ps1 test                     # Rust 单元测试
```

前端单元测试、浏览器测试与静态官网检查从 `front/` 执行，完整命令见
[前端说明](../front/README.md)。

## 代码规范

- 格式：`cargo fmt`（rustfmt 默认规则，行宽 100）。
- 注释：每个模块用 `//!` 说明职责；每个函数、结构体、枚举、字段、常量都用 `///`
  写文档注释，说明作用、约束和错误情况；关键分支、并发与一致性处理用 `//` 写明原因。
- 检查：`main.rs` 开启了 `clippy::missing_docs_in_private_items`，执行
  `./cargo.ps1 clippy --all-targets` 时，缺少文档注释的条目会报警告。提交前应保持零警告。
- 注释和对外错误信息使用中文；对外错误信息必须能直接展示给用户，不包含堆栈、SQL 或供应商原始报错。

## 尚未实现

- 调色：`color_preset_id` 目前只写进剪辑方案，还没有对应的 Quercus 调色操作。
- 运镜：静态图片按原样铺满镜头时长，还没有推拉摇移动画。
- breezeTTS2 没有语速参数，“语速”设置暂不生效；剪辑时按实际配音时长延长镜头，保证旁白不被截断。
- MiniMax-H3 生成的音轨约 5 秒，剪辑时循环铺满整片。
- 登录与令牌校验：只按令牌区分用户，没有校验令牌本身。
- 1080p 等更多导出规格：`/capabilities` 只开放 720p（复用预览文件）。
- 消息逐字流式：现在按句推送 `message.delta`；前端收到任何事件都会重新获取快照。
