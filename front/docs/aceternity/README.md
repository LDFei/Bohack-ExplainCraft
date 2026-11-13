# Aceternity 官方组件来源与 Vue 适配

读取与获取日期：2026-11-13。作者：Manu Arora / Aceternity
UI。只使用公开免费组件，没有使用 Pro 付费内容。

## 已实际读取的官方来源

| 组件             | 页面与安装入口                                                                                                          | 完整源码快照                                                                                                                              |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Bento Grid       | [官方页面](https://ui.aceternity.com/components/bento-grid)，`npx shadcn@latest add @aceternity/bento-grid`             | [官方 registry](https://ui.aceternity.com/registry/bento-grid.json)，本地 `upstream/bento-grid.json` / `bento-grid.tsx`                   |
| Background Beams | [官方页面](https://ui.aceternity.com/components/background-beams)，`npx shadcn@latest add @aceternity/background-beams` | [官方 registry](https://ui.aceternity.com/registry/background-beams.json)，本地 `upstream/background-beams.json` / `background-beams.tsx` |
| Sidebar          | [官方页面](https://ui.aceternity.com/components/sidebar)，`npx shadcn@latest add @aceternity/sidebar`                   | [官方 registry](https://ui.aceternity.com/registry/sidebar.json)，本地 `upstream/sidebar.json` / `sidebar.tsx`                            |

[官方安装说明](https://ui.aceternity.com/installation)要求 React、Tailwind CSS，以及 Motion / `cn`
工具。registry 实际声明 Bento Grid 依赖 `@tabler/icons-react`，Background Beams 依赖
`motion`，Sidebar 依赖二者；源码另外引用 React、`@/lib/utils`，Sidebar 使用 `motion/react`
的 AnimatePresence。

当前工程是 Vue 3 + TypeScript + Vite，已有 vue-router、SVG 图标与设计变量，没有 React /
Tailwind。按任务要求不运行会注入 React
TSX 的 shadcn 安装，不迁移框架，不安装 React 动画依赖。改为读取官方源码后进行 Vue 移植，保留完整来源用于核对。

## 原始 registry 校验值

原始 JSON 及提取出的 TSX 存于 `upstream/`；不参与应用编译，也不自动格式化。

- `bento-grid.json`：`9d988ea63b4209b65e386aa26dba37005d67667e122184c8905b6d31eda32482`
- `background-beams.json`：`88af2973c676fabaee374fae69964ca7ce5fab84ea69fab8f2e0eb0146faf492`
- `sidebar.json`：`df9cbde825e234d19b781c9595c63037decd86714a28ce7a2e3eb6403146877b`

普通 Python 下载请求最初被站点 403 拒绝；官方网页和 registry 已通过浏览工具读取，之后使用浏览器 User-Agent 的 curl 成功取得以上完整快照。

## 逐项适配对应

### Background Beams

对应 `src/components/aceternity/BackgroundBeams.vue` 和 `beamPaths.ts`。

- 从官方源码原样提取 50 条动态路径，以及包含额外背景路径的基础 path，保留 `viewBox="0 0 696 316"`。
- 保留每条路径独立的线性渐变、0.4 光束不透明度、0.5 描边，以及青色 / 紫色 / 紫红色渐变停止点。
- 官方 `motion.linearGradient` 在 x1/x2/y1/y2 上进行 0% 到目标百分比的无限缓动。Vue 使用 SVG
  `<animate>` 对相同属性进行无限缓动，时长仍在 10～20 秒，y2 仍在 93%～101% 范围。
- 用确定性的索引分布替代每次渲染 Math.random，初始相位提前，使首次显示即有运动；每个实例使用唯一渐变 ID，避免冲突。
- 基础路径在浅色主题上将描边不透明度从 0.05 调整为 0.12；Hero 外层做边缘淡出，正文不随光束移动。
- 支持系统减少动态效果；离开视口暂停 SVG，组件卸载清理监听与观察器。没有用背景截图代替光束。

### Bento Grid

对应 `BentoGrid.vue` / `BentoGridItem.vue` 和 `src/aceternity.css`。

- 保留单列到中屏三列、16px 网格间距、18rem 默认行高、最大宽度和跨列项目。由于本项目根字号为 13px，首页单独用 400px 行高容纳产品示意，防止官方默认 rem 在此环境缩小后裁切内容。
- 保留 header / icon / title /
  description 内容层级、12px 圆角、细边框、200ms 阴影过渡和悬停时内容 translateX(8px)。
- React children / header / icon 转为 Vue 插槽；`cn` 和 Tailwind utilities 转为等价的局部 CSS 类。
- 用工作台现有主题变量替代固定白黑配色。卡片内容使用真实 DOM、本地视频、配乐试听及创作入口；功能说明明确真实 AI
  / TTS 尚未接入。

### Sidebar

对应现有 `src/components/Sidebar.vue` 和 `src/aceternity.css`。

- 对应官方 controlled open
  state；展开 300px，收起 60px，悬停临时展开，离开恢复，标签有渐隐与 4px 位移。
- Motion 宽度动画转换为 CSS Grid 列宽过渡，移动端保留从 -100% 滑入到 0 的 0.3 秒 ease-in-out 动画。
- 在原交互基础上提供固定展开和键盘焦点展开；默认固定展开，便于持续使用素材与项目列表。
- 移动端继续使用现有导航按钮、关闭和遮罩，与作品 / 对话分区配合。
- 保留项目搜索、历史、上传、模板、镜头定位及项目管理，不更改服务层或 API。

## 本轮文件改动

| 文件                                                             | 改动                                                |
| ---------------------------------------------------------------- | --------------------------------------------------- |
| `src/components/LandingPage.vue`                                 | 新增产品 Hero、功能展示、示例播放器与试听           |
| `src/components/aceternity/BackgroundBeams.vue` / `beamPaths.ts` | 官方路径和渐变属性动画的 Vue 移植                   |
| `src/components/aceternity/BentoGrid.vue` / `BentoGridItem.vue`  | 官方网格与卡片结构的 Vue 插槽实现                   |
| `src/components/Sidebar.vue`                                     | 增加受控收缩、悬停、固定展开和键盘支持              |
| `src/components/Icon.vue`                                        | 增加首页图标                                        |
| `src/App.vue` / `src/studio/main.ts` / `src/cover/main.ts`       | 首页与工作台分区、最近项目入口、侧栏列宽联动        |
| `src/aceternity.css`                                             | 统一布局、动画、响应式与现有主题 Tokens             |
| `tests/e2e/aceternity.spec.ts`                                   | 新增 6 项交互与真实动画检查                         |
| 原有 `studio.spec.ts` / `themes.spec.ts`                         | 将原首页最近项目测试入口改为 `/workspace`，保留断言 |
| `scripts/review-aceternity.mjs`                                  | 可复现的真实浏览器截图与布局指标                    |
| `.prettierignore`                                                | 排除原始上游快照，保留下载字节用于来源核对          |
| `docs/aceternity/` / `docs/screenshots/aceternity/`              | 来源快照、适配说明、三轮调整和终验材料              |
| `README.md` / 设计与验证文档                                     | 更新路由、启动方式与实际结果                        |

`src/services/`、`src/domain/`、`useStudio.ts`、依赖清单和后端均未因本轮改造而修改。

## 路由

`/` 为独立打包的官网，创作入口现统一下载桌面应用；`/studio.html#/workspace`
承接原首页“打开最近项目”的行为，空列表或读取失败回到
`#/create`。`#/create` 与 `#/projects/:projectId`
路由保持有效，工作台只随 Tauri 构建，不再发布到 Pages。工作台品牌或侧栏入口可返回线上官网。

## 本地快照与验证

本轮修改前的前端快照位于
`/private/tmp/bohack-before-aceternity-20261113.tar.gz`。原始提交与此前备份保留，不提交或推送 GitHub。

视觉审查通过
`node scripts/review-aceternity.mjs 1`（再以 2、3 重复）在真实 Chrome 中生成页面截图和 metrics；审查记录见
`VISUAL_REVIEW.md`。
