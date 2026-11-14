# 2026-11-14 拉取 main 后的兼容说明

拉取前的工作树已与 `origin/main`（`ca5d11a`）合并。本地改动先
`git stash -u`，再 fast-forward 拉取，最后
`git stash pop`，**没有产生冲突**（两边改的代码行不重叠）。

## 已确认兼容的点

| 区域                              | 结论                                                                               |
| --------------------------------- | ---------------------------------------------------------------------------------- |
| `providers/mod.rs`                | 本地只改了 `visual` 分支；上游只改了模块声明和 `renderer` 分支，已同时存在。       |
| `providers/siliconflow.rs`        | 本地只新增 `with_base` / `with_access`；上游只删了 `#[cfg(feature = "quercus")]`。 |
| `lib.rs`                          | 已同时包含上游的 `breeze_model` 与本地三处 breeze 命令注册。                       |
| `cargo check -p lingjing-desktop` | 通过（含 quercus 必选之后的完整依赖）。                                            |

## 与另外两个对话相关的上游修正

1. **quercus 不再可选**：上游去掉了 `--features quercus`，`desktop:dev:windows` /
   `desktop:build:windows`
   仍会传这个 feature。已在 `src-tauri/Cargo.toml` 补一个空的 `quercus = []`
   feature，旧命令照常可用。
2. **构建脚本 PATH**（三条约束互相牵制，缺一条都会失败）：
   - `cl.exe` 来自 vcvars，VS 目录必须保留；
   - `libclang.dll` 的依赖 DLL 只能从 MSYS2 加载，MSYS2 目录必须排在 Git
     MinGW 之前，并且不能从 PATH 删除；
   - `D:\ProgramData\CMake\bin` 旁边的 `ld.exe` 会被 CMake 4.x 当成链接器，构建时必须从 PATH 移除。
     两个脚本现在都以 vcvars 设置后的 PATH 为基准（`cmd /v:on` 读取 `!PATH!`），把 MSYS2 与
     `FFMPEG_DIR\bin` 提到最前，再过滤掉 `CMake\bin`。
3. **`vendor/ffmpeg-sys-next`**：构建脚本增加了 `FFMPEG_SYS_NO_THREAD`
   排查开关，并把进度打到 stderr。

## 需要另外两个对话注意

- **`feat/narration-driven-shots`**：它修改了 `VisualRequest`（新增 `duration_ms` / `cut_index` /
  `cut_count`）
  和 `providers/mod.rs` 的注册段。这次新增的 `GlmScript::with_access` 与 `config.visual_access()` 在
  `visual` 分支内，合进来时需要保证构造 `VisualRequest` 时同步补这三个字段。
- **`feat/website-copy`**：只动 `website/`，与工作台和 `src-tauri/` 没有交集，可以独立合并。
- 运行配置 `src-tauri/.env` 的 `LLM1_MODEL` 已改为
  `deepseek-ai/DeepSeek-V4.1-Flash`，与代码默认值一致。

## Vibe 知识大赏新增字段

- `AudioSettings.narration_enabled`（后端 `models.rs` + 前端 `types.ts`）：默认
  `true`，旧草稿缺字段时按“有配音”处理。
  关闭后 `pipeline.rs` 不调用 llm4、不把历史配音放进剪辑方案，`need_audio` 也不再检查台词指纹。
- 前端「声音」面板的配音卡新增开关；`emptyDraft()` 补了默认值。
- 「按这个流程开始」会下载固定 BGM、上传到当前项目并设为背景音乐，同时关闭配音；失败时只留下提示词，不阻断。
- `feat/narration-driven-shots` 如果也改了 `AudioSettings` 或「声音」面板，合并时以这里的
  `narration_enabled` 为准保留。
