# ExplainCraft 多主题创作工作台

视觉改版日期：2026-11-13。当前主要形态是 Tauri 桌面应用，工作台连接 Rust 后端；
纯前端状态机只保留给浏览器开发测试。数据契约见 [API.md](./API.md)。

## 布局与组件

`/` 为独立打包的官网，使用基于官方源码的 Background Beams 和 Bento Grid，创作入口统一打开 Releases；
工作台是另一个入口 `/studio.html`，只构建给 Tauri，`#/workspace`
保留打开最近项目的入口。官方源码与适配见
[Aceternity 说明](aceternity/README.md)，三轮调整及终验见
[视觉审查记录](aceternity/VISUAL_REVIEW.md)。

- `LandingPage.vue`：产品 Hero、Bento 功能展示、原生示例播放、配乐试听与下载入口。
- `components/aceternity/`：官方组件的 Vue 移植，完整路径数据来自保存的 registry 源码。
- `aceternity.css`：首页与 Sidebar 的布局和动画，对接现有四套主题。

- `App.vue`：全宽品牌与项目栏、未保存编辑的撤销/重做入口、导出和项目管理弹窗，继续使用 `useStudio`
  协调业务。
- `Sidebar.vue`：工具窄栏、当前分镜缩略图、上传媒体、项目搜索与创作历史。点击镜头定位当前成片，点击上传素材展开素材详情。
- `PreviewPanel.vue`：中央画布、实际比例、版本选择、播放/暂停、静音、进度与全屏。脚本、素材、音频和设置由工具栏展开为可收起详情面板。
- `VideoTimeline.vue`：依据当前版本快照显示视频、音频和字幕轨道，支持点击定位、键盘调整播放位置及 100%～300% 缩放。播放头与真实
  `video` 时间同步。
- `Conversation.vue`：右侧 AI
  Copilot，保留讨论、生成、修改、阶段进度、追问、取消、恢复、附件及参数入口。
- `ThemeSwitcher.vue`：顶部外观入口，四套主题小样、即时应用、恢复默认及键盘选择。
- `useTheme.ts`：独立保存外观偏好，刷新恢复、跨窗口同步和存储异常回退，不触碰项目数据。
- `Modal.vue` / `Icon.vue`：统一弹窗焦点管理和 SVG 图标系统。

桌面左侧展开 300px、收起 60px，支持悬停与键盘展开，Copilot
330px，中央自适应；大屏适当增加侧栏宽度。768～1050px 在左侧素材区旁切换对话与画布；手机使用“创作对话 / 我的作品”分区，项目素材通过抽屉打开。各栏独立滚动，输入和主要操作保持可见。

## 设计 Tokens

布局与控件样式位于 `src/style.css`，全局配色及派生色位于
`src/themes.css`。默认银灰工作室，通过文档根节点的 `data-theme`
应用外观，挂到 body 的弹窗也同步变化。无远程字体或随机图片依赖。

| 主题                   | 基础背景  | 强调色    | 风格                                   |
| ---------------------- | --------- | --------- | -------------------------------------- |
| 银灰工作室 `silver`    | `#eef0f4` | `#7653b3` | 银灰分层、石墨文字、紫色重点，默认推荐 |
| 浅丁香创意空间 `lilac` | `#f3eff9` | `#8657b5` | 浅丁香与粉紫，轻盈创意                 |
| 暖纸编辑室 `paper`     | `#f4f0e8` | `#9b6043` | 米白、暖灰与陶土色，温暖纸感           |
| 雾蓝创作台 `mist`      | `#edf2f8` | `#3f6cc0` | 蓝灰、冷白与钴蓝，清爽理性             |

公共 Tokens 包括
`--bg`、`--surface`、`--surface-raised`、`--surface-hover`、`--surface-input`、`--canvas-bg`、`--text`、`--secondary`、`--muted`、`--line`、`--accent`、`--accent-soft`、`--accent-secondary`
等。卡片、聊天、时间轴、表单、通知和弹窗统一读取这些变量；照片上的播放控件与文字覆盖层保留必要的深底白字。

主题小样在局部直接复用相同的 `data-theme` 与 Tokens，避免预览和实际颜色不一致。偏好仅存储在
`explaincraft.ui-theme`，未知值回退默认主题；无法保存时仍即时应用，并在选择面板说明。切换不重新挂载播放器、不清空输入、不重启生成任务。不提供主题增删改查。

使用系统无衬线字体与等宽时间码，6～9px 控件圆角。动效包括缩略图轻微放大、详情展开、弹窗和通知过渡，遵循系统减少动态效果设置。

## 可自定义布局与工作台设置

2026-11-14 增加桌面三栏换位、调宽与时间轴调高。布局通过 CSS
Grid 重排，保留组件 DOM 和业务状态；窄屏沿用原有分区。左下角改为设置，帮助问号仅保留顶部一个入口。

设置包括通用主题、发送快捷键、布局记忆与恢复默认，以及四类 API 的五个独立配置槽。配置预留不发请求，密钥仅当前页面内存保留，详细结构与截图见
[布局与设置](WORKSPACE_SETTINGS.md)。

## 功能边界

- 生成、失败重试、补充信息、取消、刷新恢复、项目隔离、上传和导出仍使用原服务层与数据类型。
- 播放器与时间轴读取不可变成片快照，表单编辑当前草稿；保存后需更新视频才会改变成片。
- 历史版本只读，不能把历史快照保存为当前草稿；导出绑定当前查看版本。
- 撤销/重做仅作用于当前未保存表单，最多 50 步；保存、切换项目或版本后清空。
- 时间轴为成片轨道预览，波形明确标注“示意”，不支持逐轨剪辑或独立音轨混音。播放区静音控制整个成片。
- 比例工具修改草稿并打开设置；保存与生成前不会拉伸或替换已有视频。
- 官网示例和默认后端演示文件为本地 30 秒、三种比例、720p MP4；真实供应商能力由后端配置决定。

## 本地预览与回撤

```bash
cd /Users/suckerhgz/Desktop/bohack/front
npm run dev
```

打开终端显示的本机地址，默认 [http://127.0.0.1:5173](http://127.0.0.1:5173)。改版分支为
`codex/dark-video-workbench`，原始起点为 `82365b1`，没有提交或推送改版。

原版 `front/` 源码备份位于
`/private/tmp/bohack-before-redesign-20261113.tar.gz`，临时目录重启后可能清理；Git 原始提交仍可恢复。本次未修改后端及
`v0.1.0` 标签。

放弃本次改版时，在仓库根目录运行以下命令会丢弃 `front/` 中当前改动，请先保存之后希望保留的工作：

```bash
cd /Users/suckerhgz/Desktop/bohack
git restore --source=82365b1 --worktree -- front
rm front/src/components/VideoTimeline.vue front/src/components/ThemeSwitcher.vue front/src/composables/useTheme.ts front/src/themes.css front/tests/e2e/themes.spec.ts front/src/components/LandingPage.vue front/src/aceternity.css front/tests/e2e/aceternity.spec.ts front/scripts/review-aceternity.mjs front/src/components/PanelDragHandle.vue front/src/components/SettingsPanel.vue front/src/composables/useAppSettings.ts front/src/composables/useWorkspaceLayout.ts front/src/workspace-layout.css front/tests/e2e/layout-settings.spec.ts front/docs/WORKSPACE_SETTINGS.md
rm -r front/src/components/aceternity front/docs/aceternity front/docs/screenshots/aceternity front/docs/screenshots/layout-settings
```

增加主题前的前端快照另存于
`/private/tmp/bohack-before-themes-20261113.tar.gz`，可用于回到暗色评审版；临时目录重启后可能清理。

不需要清空浏览器存储。原有项目、上传和 API 结构保持兼容。

本次 Aceternity 调整前的前端快照另存于
`/private/tmp/bohack-before-aceternity-20261113.tar.gz`，用于回到多主题评审版。所有临时快照均可能在系统清理后删除，Git 原始提交仍保留。
