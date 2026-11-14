# 灵镜后端（Rust）

按 [业务接口文档](../front/docs/API.md) 实现的 Rust 后端：Tauri
commands 处理工作台调用，SQLite 持久化，后台 worker
编排视频制作流程。四个模型与剪辑渲染器都可以独立切换：

它是根目录 Cargo workspace 中唯一的 crate。Tauri 桌面壳与全部业务实现共用
`lingjing_desktop_lib`，后端只在桌面应用进程内运行，没有独立 Web 服务入口。

| 环节                   | 当前默认行为                                            |
| ---------------------- | ------------------------------------------------------- |
| 创作方案 / 文案 / 分镜 | 配置的创作 API；每个产物后暂停等待确认                  |
| 画面                   | 同一创作模型生成 SVG/SMIL 或离线 HTML/CSS；本地输出镜头 |
| 音频                   | 用户选择 API、breezeTTS2 或纯 BGM；超时使用 BGM         |
| 初版输出               | `direct`：FFmpeg 排列镜头、混音与字幕，不调用剪辑库     |
| 可选剪辑               | 成片可播放后，`post_edit=quercus` 才调用 Quercus        |
| 交付                   | 再次确认后结束；保留初版及可选剪辑版本                  |

应用内规则与版本在 `skills/`；可执行确认边界在 `src/skills.rs` 和 `src/pipeline.rs`。
这不是安装在 Codex 或 Claude 客户端中的个人 skill。
[完整设计、来源与验证](../front/docs/GUIDED_PRODUCTION.md)。

## 快速开始

需要 Node.js、Rust、Tauri 平台依赖、FFmpeg 与 Quercus 编译环境。macOS 可设置
`FFMPEG_DIR=/opt/homebrew`，并通过 `brew install ffmpeg mpg123 zstd` 安装编解码库；Linux 还需要
`libmpg123-dev` 和 `libzstd-dev`。Quercus 固定为 0.7.2，其 Unix 链接配置位于根目录
`.cargo/config.toml`。
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

1. **配置**：复制 `.env.example` 为 `.env`，填写硅基流动密钥和配音参考音。`.env` 已加入
   `.gitignore`，
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

## 制作流程与执行边界

```text
主题 → 风格 / 时长 / 画面 / 配音 / 音乐选择
     → 方案确认 → 文案确认 → 分镜确认 → 制作确认
     → SVG 或 HTML 镜头 + 音频 → 输出确认 → FFmpeg 基础成片
     → 是否使用 Quercus → 交付确认
```

12 个确认步骤保存在 SQLite：`style / duration / visual / voice / music / plan / script / storyboard / production / render / post_edit / delivery`。
额外缺失项（例如 Breeze 安装、上传的音乐）同样使用持久化问题。
`question_id` 含任务 ID；回答须匹配当前未过期问题和有效选项。
旧问题、重复点击、跨项目提交不能推进其他任务。

| 阶段 key            | 当前执行                                       |
| ------------------- | ---------------------------------------------- |
| `planning`          | 理解主题 / 修改意见，生成方案后等待确认        |
| `script`            | 写文案后等待确认                               |
| `storyboard`        | 分镜后等待确认，随后另行确认开始制作           |
| `visuals` / `audio` | 并行准备画面和声音，各操作写入 `Job.logs`      |
| `edit`              | 确定性安排镜头顺序，不调用模型剪辑工具         |
| `finishing`         | 整理字幕与输出检查；可选转场等候后续剪辑       |
| `render`            | 默认 `DirectRenderer`；后续 Quercus 为显式选择 |

`DirectRenderer` 按镜头顺序硬切，使用 resvg 字幕图层与全部旁白混音，背景音乐循环到片尾。
不使用 Quercus / Oak 工具，也不把未执行的转场、调色显示成已完成。
初版存入历史版本后，任务仍是 `waiting_input`，保持当前项目活动任务。
选择 `skip` 后等待交付；选择 `quercus` 才用相同时间线生成第二版，然后等待交付。
确认交付只结束任务，不再次渲染。取消保留已提交成片，阻止迟到结果发布。

Quercus 仍随当前 crate 编译，Windows 编译依赖照常需要；**编译链接不代表运行时已调用**。
兼容旧配置 `PROVIDER_RENDERER=quercus` 时，基础输出仍走 direct，不能绕过用户选择。
只有明确请求包含工程文件的导出，才另外调用 Quercus 保存 OVE / OTIO / FCPXML。

HTML 输出使用 Node.js 24+、系统 Chrome 与 FFmpeg，不新增 Playwright/Remotion 依赖。
模型页面禁止脚本、事件、外部资源与文件访问，浏览器禁网，CSS/SMIL 由可信驱动定帧。
每镜头不超过 60 秒、最多 720 张采样帧；浏览器并发数为 2，取消会清理本次进程组。
缺少依赖时返回可操作错误，不返回示例成片。

## 视觉、音乐、配音与 50 个工具

- `PROVIDER_LLM1=openai|aliyun|deepseek|minimax|siliconflow|glm`：创作与画面共用 llm1，输出受约束的
  SVG/SMIL 或离线 HTML/CSS，后端验证后输出镜头 MP4，不使用文生视频。
- `LLM1_BASE_URL` 可覆盖六家预设；`LLM1_API_KEY` 和 `LLM1_MODEL` 配置密钥与模型。
- `PROVIDER_LLM1=custom` 使用用户自定义 OpenAI Compatible Endpoint，此时必须设置 `LLM1_BASE_URL`。
- `PROVIDER_LLM3=minimax` 使用 MiniMax `music_generation`，`stable_audio` 使用 Stability AI，
  `siliconflow` 保留 MiniMax-H3 音轨方案。
- `PROVIDER_LLM4=qwen` 使用阿里云 Qwen-TTS，`indextts` 使用本地 OpenAI `audio/speech`
  兼容接口，`breeze` 保留 ComfyUI 语音克隆。
- `PROVIDER_LLM4=siliconflow` 使用硅基流动 TTS，独立配置 `SILICONFLOW_TTS_*`。
- `list_models` command 通过 OpenAI Compatible `/models` 获取模型下拉候选，支持手动覆盖模型名。
- `TTS_VOICES` 配置可选声线，作品设置允许每次重新选择或手填；不再做供应商之间的声线名称猜测。
- 时长在对话 skill 中选择，支持 15～300 秒，自动模式展示方案建议后再确认。
- 引导模式的 BGM 使用程序化音乐或明确选择的上传 /
  Vibe 示例音频；云端音乐适配器保留，但不会暗中替换选择。
- 配音超时默认 API 45 秒、本地 90 秒；`VOICE_TIMEOUT_SECONDS`
  可设 1～600 秒。超时持久化回退纯 BGM；取消不视为超时。
- `src/oak/` 包含参考项目的 50 个 Quercus 工具及 GPL 声明。工具适配器保留；
  引导制作默认不调用这些工具，后续剪辑必须由当前任务的明确答案授权。

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
| `src/pipeline.rs` | 十二个确认步骤与八个执行阶段、检查点、取消与失败处理                     |
| `src/providers/`  | 模型与渲染器接口及真实供应商实现                                         |
| `src/media.rs`    | 文件存储、HMAC 签名地址、文件头识别、ffprobe 解析                        |
| `src/catalog.rs`  | `/capabilities` 内容与新项目默认草稿                                     |

### 检查点与复用

每个阶段完成后，产物会写回草稿（`draft_revision + 1`），并从 `invalidated_stages` 中移除。

- 失败重试、对话修改、手动保存设置后更新，都只重跑仍然失效的阶段，其余显示为“沿用已有结果”。例如“只改配音”会跳过画面阶段；“第二个镜头换一下”只重新生成那一个镜头。
- 配音按“来源 + 旁白 + 音色 + 语速 + 时长”生成指纹，内容没变就复用。

### 数据与文件

- 数据库：系统应用数据目录下的 `explaincraft.sqlite3`
- 上传、生成和渲染的文件：系统应用数据目录下的 `storage/<project_id>/`

可通过 `DATA_DIR` 或单独的路径环境变量覆盖。删除对应应用数据目录即可重置。

## 配置（环境变量）

| 变量                            | 默认值                                 | 说明                                                       |
| ------------------------------- | -------------------------------------- | ---------------------------------------------------------- |
| `DATA_DIR`                      | 系统应用数据目录                       | 数据库与文件目录                                           |
| `DEMO_MEDIA_DIR`                | 应用内嵌 `media/`                      | 示例与曲库目录；开发时回退到 `shared/public/media`         |
| `PROVIDER_LLM1`                 | `siliconflow`                          | 创作、对话、分镜与 SVG 供应商                              |
| `PROVIDER_LLM3`                 | `procedural`                           | 本地程序化配乐；新项目默认启用                             |
| `PROVIDER_LLM4`                 | `qwen`                                 | 配音供应商                                                 |
| `PROVIDER_RENDERER`             | `direct`                               | 基础输出；后续剪辑由确认控制                               |
| `AUTH_MODE`                     | `dev`                                  | `dev`：没有令牌时视为 `local_user`；`strict`：必须携带令牌 |
| `MEDIA_SIGNING_SECRET`          | 随机                                   | 媒体签名密钥；不设置时，重启后旧链接失效                   |
| `MEDIA_URL_TTL_SECONDS`         | `1800`                                 | 播放 / 下载地址有效期                                      |
| `MAX_CONCURRENT_JOBS`           | `4`                                    | 同时执行的任务数                                           |
| `QUESTION_TTL_SECONDS`          | `86400`                                | 追问等待时长                                               |
| `VOICE_TIMEOUT_SECONDS`         | API 45 / 本地 90                       | 配音超时秒数（1～600），超时降级 BGM                       |
| `CHROME_PATH`                   | 系统 Chrome                            | HTML 离线输出浏览器，可显式覆盖                            |
| `NODE_PATH_BIN`                 | `node`                                 | HTML 驱动，需 Node 24+                                     |
| `FFPROBE_PATH`                  | `ffprobe`                              | 素材解析工具                                               |
| `LLM1_API_KEY` / `LLM1_MODEL`   | 空 / `deepseek-ai/DeepSeek-V4.1-Flash` | llm1 密钥与模型（硅基流动）                                |
| `LLM1_API_KEY` / `LLM1_MODEL`   | 空 / 按供应商选择模型                  | 创作与 SVG 共用的模型                                      |
| `LLM3_API_KEY` / `LLM3_MODEL`   | 空 / `MiniMaxAI/MiniMax-H3`            | llm3 密钥与模型                                            |
| `SILICONFLOW_BASE_URL`          | `https://api.siliconflow.cn/v1`        | 硅基流动接口地址                                           |
| `COMFYUI_URL`                   | `http://127.0.0.1:8188`                | 本地 ComfyUI（breezeTTS2）                                 |
| `COMFYUI_DIR`                   | 空（自动探测）                         | ComfyUI 安装目录；模型下载到其 `models/breezetts2/`        |
| `BREEZE_MODEL_ENDPOINT`         | `https://huggingface.co`               | 模型下载端点，可改用镜像地址                               |
| `BREEZE_MODEL`                  | `int8 hybrid (recommended)`            | breezeTTS2 检查点                                          |
| `BREEZE_VOICE_MALE` / `_FEMALE` | 空                                     | 参考音路径；“沉稳男声”用男声，其余用女声                   |

登录模块还没有接入。dev 模式下，不同的 Bearer 令牌会被当作不同用户，可以用来验证跨用户隔离。

## 测试

```powershell
./cargo.ps1 test                     # Rust 单元测试
```

前端类型检查、构建与格式检查从 `front/` 执行；静态官网检查从 `website/` 执行。完整命令见
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
- breezeTTS2 依赖本地 ComfyUI、自定义节点和参考音；本轮未下载约 5.2GB 模型，未验证真实大模型推理，已验证检测、同意边界与通用超时回退。
- MiniMax-H3 生成的音轨约 5 秒，剪辑时循环铺满整片。
- 登录与令牌校验：只按令牌区分用户，没有校验令牌本身。
- 1080p 等更多导出规格：`/capabilities` 只开放 720p（复用预览文件）。
- 消息逐字流式：现在按句推送 `message.delta`；前端收到任何事件都会重新获取快照。

## 生成用量

每支视频生成成功后，对话中的成片卡片会显示本轮端到端耗时和所有实际模型请求已报告的 Token 总量。用量明细仅在本地数据库内保留，不在界面中分模型展示。

最终成片会同时出现在左侧“素材库”，点击即可打开对应版本预览；SVG 镜头、旁白等中间文件不会混入素材库列表。
